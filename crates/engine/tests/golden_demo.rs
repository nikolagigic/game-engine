//! GE-012: the demo scene's initial frame.
//!
//! Includes `examples/demo.rs` so the test renders exactly the scene the
//! example builds (its `main` is unused here), from the example's starting
//! camera, at 256x144 through `Headless` + `render_world`.

mod common;

#[path = "../examples/demo.rs"]
#[allow(dead_code)]
mod demo;

use common::{GoldenOpts, assert_golden};
use engine::ecs::Schedule;
use engine::gpu::OFFSCREEN_COLOR_FORMAT;
use engine::headless::Headless;
use engine::math::{Camera, Vec3};
use engine::render::{RenderState, render_world};
use image::Rgba;

const WIDTH: u32 = 256;
const HEIGHT: u32 = 144;

/// A headless engine holding the demo scene, the example's starting camera,
/// and the orbit system on the fixed schedule.
fn demo_engine() -> anyhow::Result<Headless> {
    let mut engine = Headless::new(WIDTH, HEIGHT)?;
    engine.clear_color = demo::CLEAR;
    let gltf_cube = demo::load_gltf_cube()?;
    demo::setup(
        &mut engine.world,
        &mut engine.resources,
        &engine.gpu.device,
        OFFSCREEN_COLOR_FORMAT,
        &gltf_cube,
    );
    let (camera, _) = demo::initial_camera();
    engine.resources.insert(camera);
    let mut schedule = Schedule::new();
    schedule.add_fixed("orbit", demo::orbit_system);
    engine.schedule = schedule;
    engine.render_fn = Some(Box::new(|world, res, ctx, color, depth| {
        let size = color.texture().size();
        let aspect = size.width as f32 / size.height as f32;
        render_world(world, res, ctx, color, depth, aspect);
    }));
    Ok(engine)
}

/// Pixel that world point `p` projects to.
fn to_pixel(engine: &Headless, p: Vec3) -> (u32, u32) {
    let aspect = WIDTH as f32 / HEIGHT as f32;
    let ndc = engine
        .resources
        .expect::<Camera>()
        .view_projection(aspect)
        .project_point3(p);
    let x = (ndc.x * 0.5 + 0.5) * WIDTH as f32;
    let y = (-ndc.y * 0.5 + 0.5) * HEIGHT as f32;
    (x as u32, y as u32)
}

#[test]
fn demo_initial_frame_matches_golden() -> anyhow::Result<()> {
    let mut engine = demo_engine()?;
    let image = engine.render()?;

    // Three meshes (built-in cube, built-in sphere, glTF cube): one
    // instanced draw call each.
    let state = engine.resources.expect::<RenderState>();
    assert_eq!(state.last_frame_draw_calls, 3);
    assert_eq!(state.registry.len(), 3);
    // Ground + 396 field objects + pedestal + 6 satellites.
    let field = demo::FIELD_SIDE * demo::FIELD_SIDE - 4;
    assert_eq!(engine.world.len() as usize, 1 + field + 1 + 6);

    // The white glTF pedestal is in view and lit (bright, near-grey).
    let (x, y) = to_pixel(&engine, Vec3::new(0.0, 1.5, 1.6));
    let Rgba([r, g, b, _]) = *image.get_pixel(x, y);
    assert!(
        r > 120 && g > 120 && b > 120 && r.abs_diff(b) < 60,
        "pedestal at ({x}, {y}) is {r},{g},{b}"
    );
    // The top-left corner is above the horizon: sky (clear colour).
    assert_eq!(*image.get_pixel(0, 0), Rgba([63, 80, 108, 255]));

    assert_golden("demo", &image, GoldenOpts::default());
    Ok(())
}

#[test]
fn satellites_move_with_fixed_steps() -> anyhow::Result<()> {
    let mut engine = demo_engine()?;
    let before = engine.render()?;
    engine.step(30);
    let after = engine.render()?;
    assert_ne!(before, after, "half a second of orbiting changes the frame");

    // Every satellite is where its orbit says after 30 steps.
    let t = 30.0 / 60.0;
    for (transform, orbit) in engine
        .world
        .query_mut::<(&engine::math::Transform, &demo::Orbit)>()
    {
        let expected = orbit.position(t);
        assert!(
            transform.translation.distance(expected) < 1e-4,
            "satellite at {:?}, expected {expected:?}",
            transform.translation
        );
    }
    Ok(())
}
