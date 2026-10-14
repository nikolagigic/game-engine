//! A grid of cubes in an alternating checkerboard of two colours, drawn
//! from the ECS with one instanced draw call: the scene of the
//! `golden_instanced` test, but interactive and 100x100 (10,000 entities)
//! by default.
//!
//! Run with `cargo run --release --example instanced_grid` for the
//! 10,000-cube stress test, or pass the side length:
//! `cargo run --example instanced_grid -- 10` is the golden-test scene.
//! The average frame time, entity count and draw-call count are logged
//! once a second.
//!
//! Controls: W/S forward and back, A/D strafe, E/Q up and down; hold the
//! right mouse button to grab the cursor and look around; Escape quits.

use shin::app::{self, AppConfig, EcsGame};
use shin::asset;
use shin::ecs::{Resources, Schedule, Time, World};
use shin::gpu::GpuContext;
use shin::input::{FlyCamera, fly_camera_system};
use shin::math::{Camera, Projection, Transform, Vec3};
use shin::render::{LightUniform, MeshInstance, RenderState, render_world};

/// Cubes per side when no argument is given.
const DEFAULT_SIDE: usize = 100;
/// Distance between neighbouring cube centres.
const SPACING: f32 = 1.5;
/// Linear RGBA checkerboard colours.
const ORANGE: [f32; 4] = [0.8, 0.3, 0.1, 1.0];
const BLUE: [f32; 4] = [0.1, 0.3, 0.8, 1.0];
/// Linear clear colour (near-black).
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.02,
    g: 0.02,
    b: 0.02,
    a: 1.0,
};

/// Number of cubes per side of the grid.
struct GridSide(usize);

/// Frame-time accumulator for the once-a-second log line.
#[derive(Default)]
struct FrameLog {
    elapsed: f64,
    frames: u32,
}

/// Frame system: logs the average frame time over the last second.
fn log_frame_time(world: &mut World, res: &mut Resources) {
    let frame_dt = res.expect::<Time>().frame_dt;
    let draw_calls = res
        .get::<RenderState>()
        .map_or(0, |state| state.last_frame_draw_calls);
    let log = res.expect_mut::<FrameLog>();
    log.elapsed += frame_dt;
    log.frames += 1;
    if log.elapsed < 1.0 {
        return;
    }
    let ms = 1000.0 * log.elapsed / f64::from(log.frames);
    log::info!(
        "frame time {ms:.2} ms ({:.0} fps), {} entities, {draw_calls} draw call(s)",
        1000.0 / ms,
        world.len(),
    );
    *log = FrameLog::default();
}

/// First frame only: builds the [`RenderState`] for the surface format and
/// spawns the grid of cube entities.
fn setup(world: &mut World, res: &mut Resources, ctx: &GpuContext, format: wgpu::TextureFormat) {
    let mut state = RenderState::new(&ctx.device, &ctx.queue, format);
    state.light = LightUniform {
        direction: [1.0, -2.0, -1.0, 0.0],
        color: [1.0, 1.0, 1.0, 1.0],
        ambient: [0.1, 0.1, 0.1, 1.0],
    };
    let cube = state.registry.add(&ctx.device, &asset::cube());
    res.insert(state);

    let side = res.expect::<GridSide>().0;
    let offset = (side as f32 - 1.0) / 2.0;
    world.spawn_batch((0..side * side).map(|k| {
        let (i, j) = (k % side, k / side);
        let position = Vec3::new(
            (i as f32 - offset) * SPACING,
            0.0,
            (j as f32 - offset) * SPACING,
        );
        let color = if (i + j) % 2 == 0 { ORANGE } else { BLUE };
        (
            Transform::from_translation(position),
            MeshInstance {
                mesh: cube,
                color,
                texture: None,
            },
        )
    }));
    log::info!("spawned {} cubes", side * side);
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let side = match std::env::args().nth(1) {
        Some(arg) => arg.parse()?,
        None => DEFAULT_SIDE,
    };
    // Back far enough to see the whole grid, pitched 45° down; for a side
    // of 10 this is the golden test's camera at (0, 18, 18).
    let distance = side as f32 * SPACING * 1.2;

    let mut schedule = Schedule::new();
    schedule
        .add_frame("fly_camera", fly_camera_system)
        .add_frame("log_frame_time", log_frame_time);
    let mut game = EcsGame::new(World::new(), schedule);
    game.resources.insert(GridSide(side));
    game.resources.insert(FrameLog::default());
    game.resources.insert(FlyCamera {
        speed: distance / 3.0,
        pitch: -45f32.to_radians(),
        ..Default::default()
    });
    game.resources.insert(Camera {
        transform: Transform::from_translation(Vec3::new(0.0, distance, distance)),
        projection: Projection::Perspective {
            fov_y_radians: 45f32.to_radians(),
            near: 0.1,
            far: distance * 4.0,
        },
    });
    game.render_fn = Some(Box::new(|world, res, ctx, color, depth| {
        let texture = color.texture();
        if res.get::<RenderState>().is_none() {
            setup(world, res, ctx, texture.format());
        }
        let aspect = texture.width() as f32 / texture.height() as f32;
        render_world(world, res, ctx, color, depth, aspect);
    }));

    let config = AppConfig {
        title: format!(
            "instanced_grid ({side}x{side} cubes; WASD + E/Q, hold right mouse to look)"
        ),
        clear_color: CLEAR,
        ..Default::default()
    };
    game.title = config.title.clone();
    app::run(config, game)
}
