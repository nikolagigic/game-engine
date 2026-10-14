//! `shin-mcp`: an MCP server over stdio that exposes every tool in
//! [`shin::tool::ToolRegistry`] (GE-014).
//!
//! ```text
//! shin-mcp [--project DIR] [--width W] [--height H] [--log-level L]
//! ```
//!
//! With `--project` (GE-026) the engine opens the project in `DIR`
//! ([`Headless::open_project`]): assets and scene paths are rooted there,
//! its entry scene is loaded, and frames default to its window size from
//! `shin.toml`. Without it the engine is empty and frames are 512x288.
//! `--width` and `--height` override either default.
//!
//! stdout carries only JSON-RPC; logs go to stderr. `--log-level` takes an
//! `env_logger` filter (e.g. `debug`, `engine=trace`) and overrides
//! `RUST_LOG`; the default is `info`.

mod engine_thread;
mod server;

use std::path::PathBuf;

use anyhow::{Context as _, bail};
use rmcp::ServiceExt as _;
use shin::headless::Headless;
use shin::tool::ToolRegistry;

use crate::engine_thread::EngineHandle;
use crate::server::EngineServer;

/// Frame size without `--project`, `--width`, or `--height`.
const DEFAULT_SIZE: (u32, u32) = (512, 288);

/// Command-line options.
#[derive(Debug, Default, PartialEq)]
struct Options {
    /// Project directory to open, if any.
    project: Option<PathBuf>,
    /// Frame width override.
    width: Option<u32>,
    /// Frame height override.
    height: Option<u32>,
    log_level: Option<String>,
}

const USAGE: &str = "usage: shin-mcp [--project DIR] [--width W] [--height H] [--log-level L]";

/// Parses the arguments after the program name.
fn parse_args(args: impl IntoIterator<Item = String>) -> anyhow::Result<Options> {
    let mut options = Options::default();
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--project" => options.project = Some(value(&mut args, &flag)?.into()),
            "--width" => options.width = Some(parse_size(&flag, &value(&mut args, &flag)?)?),
            "--height" => options.height = Some(parse_size(&flag, &value(&mut args, &flag)?)?),
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

/// Creates the engine `options` describe: the project opened, or an empty
/// engine, then resized to any `--width`/`--height` override.
fn create_engine(options: &Options) -> anyhow::Result<Headless> {
    let mut engine = match &options.project {
        Some(dir) => Headless::open_project(dir)?,
        None => Headless::new(DEFAULT_SIZE.0, DEFAULT_SIZE.1)?,
    };
    let width = options.width.unwrap_or(engine.target.width);
    let height = options.height.unwrap_or(engine.target.height);
    engine.resize(width, height);
    Ok(engine)
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
    let project = options.project.clone();
    let engine = EngineHandle::spawn(registry, move || create_engine(&options))?;
    log::info!(
        "shin-mcp: headless engine{}, {} tools, serving on stdio",
        project
            .map(|dir| format!(" on project {}", dir.display()))
            .unwrap_or_default(),
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
        log::info!("shin-mcp: stopped ({reason:?})");
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
    fn defaults_to_no_project_and_no_overrides() {
        assert_eq!(parse_args(args(&[])).unwrap(), Options::default());
    }

    #[test]
    fn parses_every_flag() {
        let options = parse_args(args(&[
            "--project",
            "games/demo",
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
                project: Some(PathBuf::from("games/demo")),
                width: Some(64),
                height: Some(32),
                log_level: Some("debug".into()),
            }
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse_args(args(&["--width", "0"])).is_err());
        assert!(parse_args(args(&["--width"])).is_err());
        assert!(parse_args(args(&["--project"])).is_err());
        assert!(parse_args(args(&["--frobnicate"])).is_err());
    }

    #[test]
    fn engine_without_project_is_512_by_288_unless_overridden() -> anyhow::Result<()> {
        let engine = create_engine(&Options::default())?;
        assert_eq!((engine.target.width, engine.target.height), DEFAULT_SIZE);
        let engine = create_engine(&Options {
            height: Some(10),
            ..Default::default()
        })?;
        assert_eq!((engine.target.width, engine.target.height), (512, 10));
        Ok(())
    }
}
