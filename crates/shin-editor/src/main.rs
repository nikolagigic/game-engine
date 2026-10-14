//! `shin-editor`, the Shin editor: an egui shell around a live viewport.
//!
//! ```text
//! shin-editor <project-dir> [--screenshot <png>]
//! ```
//!
//! Opens the project in `<project-dir>` (its `shin.toml` names the window)
//! with docked Viewport, Scene, Inspector, and Log panels. In the viewport,
//! left-drag orbits, right-drag pans, and the wheel zooms.
//!
//! `--screenshot <png>` renders one full editor frame without a window,
//! writes it to `<png>`, and exits; tests use it.

mod editor;
mod log_sink;
mod orbit;
mod scene;
mod ui;

use std::path::PathBuf;

use clap::Parser;

use crate::log_sink::LogBuffer;

/// The Shin editor.
#[derive(Parser)]
#[command(name = "shin-editor", version, about)]
struct Cli {
    /// The project directory (the one containing `shin.toml`).
    project_dir: PathBuf,
    /// Render one editor frame offscreen to this PNG and exit.
    #[arg(long, value_name = "PNG")]
    screenshot: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let log = LogBuffer::default();
    log_sink::init(log.clone())?;

    match cli.screenshot {
        Some(out) => editor::screenshot(&cli.project_dir, &out, log),
        None => editor::run(&cli.project_dir, log),
    }
}
