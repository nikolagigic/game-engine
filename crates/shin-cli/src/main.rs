//! `shin`, the Shin command-line tool.
//!
//! ```text
//! shin new <name> [--dir <path>] [--engine-path <path>]
//! ```
//!
//! `shin new` creates a runnable game project in `<dir>/<name>` (the
//! current directory by default) and prints every file it wrote, one per
//! line. It is the same operation as the engine's `project.new` tool.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use shin::project::{self, EngineDep, NewProjectOptions};

/// The Shin game engine's command-line tool.
#[derive(Parser)]
#[command(name = "shin", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new game project in `<dir>/<name>`.
    New {
        /// Project and Cargo package name (ASCII letters, digits, `-`, `_`;
        /// not starting with a digit).
        name: String,
        /// Parent directory of the new project.
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        /// Depend on a local `crates/shin-engine` instead of the engine's
        /// git repository.
        #[arg(long)]
        engine_path: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    match Cli::parse().command {
        Command::New {
            name,
            dir,
            engine_path,
        } => {
            let opts = NewProjectOptions {
                dir: dir.join(&name),
                name,
                engine: engine_path.map(EngineDep::Path).unwrap_or_default(),
            };
            for file in project::create(&opts)? {
                println!("{}", file.display());
            }
            eprintln!(
                "Created `{}`. Run it with:\n\n    cd {} && cargo run",
                opts.name,
                opts.dir.display()
            );
        }
    }
    Ok(())
}
