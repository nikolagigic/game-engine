//! `engine-mcp`: an MCP server over stdio that exposes every tool in
//! [`engine::tool::ToolRegistry`] (GE-014).
//!
//! ```text
//! engine-mcp [--width W] [--height H] [--log-level L]
//! ```
//!
//! stdout carries only JSON-RPC; logs go to stderr. `--log-level` takes an
//! `env_logger` filter (e.g. `debug`, `engine=trace`) and overrides
//! `RUST_LOG`; the default is `info`.

mod engine_thread;
mod server;

use anyhow::{Context as _, bail};
use engine::tool::ToolRegistry;
use rmcp::ServiceExt as _;

use crate::engine_thread::EngineHandle;
use crate::server::EngineServer;

/// Command-line options.
#[derive(Debug, PartialEq)]
struct Options {
    width: u32,
    height: u32,
    log_level: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            width: 512,
            height: 288,
            log_level: None,
        }
    }
}

const USAGE: &str = "usage: engine-mcp [--width W] [--height H] [--log-level L]";

/// Parses the arguments after the program name.
fn parse_args(args: impl IntoIterator<Item = String>) -> anyhow::Result<Options> {
    let mut options = Options::default();
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--width" => options.width = parse_size(&flag, &value(&mut args, &flag)?)?,
            "--height" => options.height = parse_size(&flag, &value(&mut args, &flag)?)?,
            "--log-level" => options.log_level = Some(value(&mut args, &flag)?),
            "-h" | "--help" => bail!("{USAGE}"),
            other => bail!("unknown argument `{other}`\n{USAGE}"),
        }
    }
    Ok(options)
}

/// Takes the value that follows `flag`.
fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> anyhow::Result<String> {
    args.next()
        .with_context(|| format!("{flag} needs a value\n{USAGE}"))
}

/// Parses a positive pixel size.
fn parse_size(flag: &str, value: &str) -> anyhow::Result<u32> {
    match value.parse::<u32>() {
        Ok(size) if size > 0 => Ok(size),
        _ => bail!("{flag} must be a positive integer, got `{value}`"),
    }
}

fn main() -> anyhow::Result<()> {
    let options = parse_args(std::env::args().skip(1))?;

    // stdout is the protocol channel, so logs must go to stderr.
    let mut logger =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));
    if let Some(level) = &options.log_level {
        logger.parse_filters(level);
    }
    logger.target(env_logger::Target::Stderr).init();

    let registry = ToolRegistry::with_engine_tools();
    let tools = registry.list();
    let engine = EngineHandle::spawn(registry, options.width, options.height)?;
    log::info!(
        "engine-mcp: {}x{} headless engine, {} tools, serving on stdio",
        options.width,
        options.height,
        tools.len()
    );

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start the tokio runtime")?;
    runtime.block_on(async {
        let service = EngineServer::new(engine, &tools)
            .serve(rmcp::transport::stdio())
            .await
            .context("MCP handshake failed")?;
        let reason = service.waiting().await?;
        log::info!("engine-mcp: stopped ({reason:?})");
        anyhow::Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults_to_512_by_288() {
        assert_eq!(parse_args(args(&[])).unwrap(), Options::default());
    }

    #[test]
    fn parses_every_flag() {
        let options = parse_args(args(&[
            "--width",
            "64",
            "--height",
            "32",
            "--log-level",
            "debug",
        ]))
        .unwrap();
        assert_eq!(
            options,
            Options {
                width: 64,
                height: 32,
                log_level: Some("debug".into()),
            }
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse_args(args(&["--width", "0"])).is_err());
        assert!(parse_args(args(&["--width"])).is_err());
        assert!(parse_args(args(&["--frobnicate"])).is_err());
    }
}
