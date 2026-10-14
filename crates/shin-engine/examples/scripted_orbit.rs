//! Cubes orbiting the origin, driven by a Rhai script (GE-011): every cube
//! carries a `Script` pointing at `examples/scripts/orbit.rhai`, and
//! `script_system` runs its `update(entity, dt)` each fixed step.
//!
//! Run with `cargo run --example scripted_orbit`, then edit
//! `crates/shin-engine/examples/scripts/orbit.rhai` and save: the cubes
//! change behaviour within a second, without restarting. A syntax error is
//! logged as `scripts/orbit.rhai:<line>: <message>` and the cubes keep
//! running the last version that compiled. Escape quits.

use std::path::Path;

use shin::app::{self, AppConfig, EcsGame};
use shin::asset::{self, Assets};
use shin::ecs::{Name, Resources, Schedule, World};
use shin::gpu::GpuContext;
use shin::math::{Camera, Projection, Transform, Vec3};
use shin::render::{LightUniform, MeshInstance, RenderState, render_world};
use shin::script::{Script, script_system};

/// Number of cubes on the circle.
const CUBES: usize = 8;
/// The script every cube runs, relative to the `Assets` root.
const SCRIPT: &str = "scripts/orbit.rhai";

/// First frame only: builds the [`RenderState`] for the surface format and
/// spawns the cubes on a circle, each with the orbit script.
fn setup(world: &mut World, res: &mut Resources, ctx: &GpuContext, format: wgpu::TextureFormat) {
    let mut state = RenderState::new(&ctx.device, &ctx.queue, format);
    state.light = LightUniform {
        direction: [1.0, -2.0, -1.0, 0.0],
        color: [1.0, 1.0, 1.0, 1.0],
        ambient: [0.15, 0.15, 0.15, 1.0],
    };
    let cube = state.registry.add(&ctx.device, &asset::cube());
    res.insert(state);

    for i in 0..CUBES {
        let angle = i as f32 / CUBES as f32 * std::f32::consts::TAU;
        let radius = if i % 2 == 0 { 2.5 } else { 1.5 };
        let hue = i as f32 / CUBES as f32;
        let mut transform =
            Transform::from_translation(Vec3::new(angle.cos(), 0.0, angle.sin()) * radius);
        transform.scale = Vec3::splat(0.5);
        world.spawn((
            transform,
            MeshInstance {
                mesh: cube,
                color: [0.2 + 0.7 * hue, 0.3, 0.9 - 0.7 * hue, 1.0],
                texture: None,
            },
            Name(format!("cube {i}")),
            Script::new(SCRIPT),
        ));
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut schedule = Schedule::new();
    schedule.add_fixed("script", script_system);
    let mut game = EcsGame::new(World::new(), schedule);
    // Script paths resolve against the Assets root.
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    game.resources.insert(Assets::new(examples));
    let mut camera = Camera {
        transform: Transform::from_translation(Vec3::new(0.0, 4.0, 7.0)),
        projection: Projection::Perspective {
            fov_y_radians: 45f32.to_radians(),
            near: 0.1,
            far: 100.0,
        },
    };
    camera.transform.looking_at(Vec3::ZERO, Vec3::Y);
    game.resources.insert(camera);
    game.render_fn = Some(Box::new(|world, res, ctx, color, depth| {
        let texture = color.texture();
        if res.get::<RenderState>().is_none() {
            setup(world, res, ctx, texture.format());
        }
        let aspect = texture.width() as f32 / texture.height() as f32;
        render_world(world, res, ctx, color, depth, aspect);
    }));

    let config = AppConfig {
        title: format!("scripted_orbit (edit examples/{SCRIPT} while it runs)"),
        clear_color: wgpu::Color {
            r: 0.02,
            g: 0.02,
            b: 0.03,
            a: 1.0,
        },
        ..Default::default()
    };
    game.title = config.title.clone();
    app::run(config, game)
}
