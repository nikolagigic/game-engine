//! Opening the project's scene in the editor's engine and the paths File >
//! Open and Save use.

use std::path::Path;

use shin::headless::Headless;
use shin::project::{Project, STARTER_SCENE};

/// Linear clear colour of the viewport: the starter game's dusky sky blue.
pub const SKY: wgpu::Color = wgpu::Color {
    r: 0.05,
    g: 0.08,
    b: 0.15,
    a: 1.0,
};

/// Opens the project in `project_dir` on `engine` ([`Headless::load_project`]:
/// assets and scene paths rooted at the project, its entry scene loaded if
/// it has one) with the [`SKY`] clear colour, and returns its manifest.
pub fn open(engine: &mut Headless, project_dir: &Path) -> anyhow::Result<Project> {
    engine.clear_color = SKY;
    engine.load_project(project_dir)
}

/// The scene file File > Open loads and File > Save writes at first,
/// relative to the project: its `entry_scene`, or [`STARTER_SCENE`] when it
/// has none.
pub fn default_path(project: &Project) -> String {
    project
        .entry_scene
        .as_deref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| STARTER_SCENE.to_string())
}
