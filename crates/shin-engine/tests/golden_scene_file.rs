//! GE-021: the GE-015 tool scene, loaded from
//! `assets/scenes/tool_scene.scene.ron`, renders the same frame as building
//! it through the tools (`tests/golden/tool_scene.png`).

mod common;

use std::path::Path;

use common::{GoldenOpts, assert_golden};
use shin::headless::Headless;

#[test]
fn tool_scene_from_file_matches_golden() -> anyhow::Result<()> {
    let mut engine = Headless::new(128, 128)?;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/scenes/tool_scene.scene.ron");
    let spawned = shin::scene::load(
        &mut engine.world,
        &mut engine.resources,
        &engine.gpu.device,
        &path,
    )?;
    assert_eq!(spawned, 3);
    let frame = engine.render()?;
    assert_golden("tool_scene", &frame, GoldenOpts::default());
    Ok(())
}
