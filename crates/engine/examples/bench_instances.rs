//! Benchmark: renders `N` cubes from the ECS with one instanced draw call
//! and a fixed camera for `SECONDS` seconds, prints the
//! [`FrameStats::summary`] line, and exits with code 0.
//!
//! Run with `cargo run --release --example bench_instances -- [N] [SECONDS]`
//! (defaults: 10000 cubes, 5 seconds). The summary covers the last
//! [`engine::stats::DEFAULT_CAPACITY`] frames.

use engine::app::{self, AppConfig, EcsGame};
use engine::asset;
use engine::ecs::{ExitRequest, Resources, Schedule, Time, World};
use engine::gpu::GpuContext;
use engine::math::{Camera, Projection, Transform, Vec3};
use engine::render::{LightUniform, MeshInstance, RenderState, render_world};
use engine::stats::FrameStats;

/// Cubes drawn when no argument is given.
const DEFAULT_COUNT: usize = 10_000;
/// Seconds to run when no argument is given.
const DEFAULT_SECONDS: f64 = 5.0;
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

/// Benchmark settings and progress.
struct Bench {
    /// Number of cubes to spawn.
    count: usize,
    /// Wall-clock seconds to run for.
    duration: f64,
    /// Wall-clock seconds run so far (sum of frame times).
    elapsed: f64,
}

/// Cubes per side of the smallest square grid holding `count` cubes.
fn grid_side(count: usize) -> usize {
    (count as f64).sqrt().ceil().max(1.0) as usize
}

/// Frame system: once `duration` has passed, prints the summary and sets
/// [`ExitRequest`], so the window closes and `app::run` returns.
fn finish_after_duration(_: &mut World, res: &mut Resources) {
    if res.expect::<ExitRequest>().0 {
        return;
    }
    let frame_dt = res.expect::<Time>().frame_dt;
    let bench = res.expect_mut::<Bench>();
    bench.elapsed += frame_dt;
    if bench.elapsed < bench.duration {
        return;
    }
    // This runs before `frame_stats_system` (always last), so the summary
    // covers every frame but this one.
    println!("{}", res.expect::<FrameStats>().summary());
    res.insert(ExitRequest(true));
}

/// First frame only: builds the [`RenderState`] for the surface format and
/// spawns the cubes in a square grid centred on the origin.
fn setup(world: &mut World, res: &mut Resources, ctx: &GpuContext, format: wgpu::TextureFormat) {
    let mut state = RenderState::new(&ctx.device, format);
    state.light = LightUniform {
        direction: [1.0, -2.0, -1.0, 0.0],
        color: [1.0, 1.0, 1.0, 1.0],
        ambient: [0.1, 0.1, 0.1, 1.0],
    };
    let cube = state.registry.add(&ctx.device, &asset::cube());
    res.insert(state);

    let count = res.expect::<Bench>().count;
    let side = grid_side(count);
    let offset = (side as f32 - 1.0) / 2.0;
    world.spawn_batch((0..count).map(|k| {
        let (i, j) = (k % side, k / side);
        let position = Vec3::new(
            (i as f32 - offset) * SPACING,
            0.0,
            (j as f32 - offset) * SPACING,
        );
        let color = if (i + j) % 2 == 0 { ORANGE } else { BLUE };
        (
            Transform::from_translation(position),
            MeshInstance { mesh: cube, color },
        )
    }));
    log::info!("spawned {count} cubes");
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let mut args = std::env::args().skip(1);
    let count = match args.next() {
        Some(arg) => arg.parse()?,
        None => DEFAULT_COUNT,
    };
    let duration = match args.next() {
        Some(arg) => arg.parse()?,
        None => DEFAULT_SECONDS,
    };

    // A fixed camera (no fly controller) far enough back to see the whole
    // grid, pitched 45° down at the origin.
    let distance = grid_side(count) as f32 * SPACING * 1.2;
    let mut transform = Transform::from_translation(Vec3::new(0.0, distance, distance));
    transform.looking_at(Vec3::ZERO, Vec3::Y);

    let mut schedule = Schedule::new();
    schedule.add_frame("finish_after_duration", finish_after_duration);
    let mut game = EcsGame::new(World::new(), schedule);
    game.resources.insert(Bench {
        count,
        duration,
        elapsed: 0.0,
    });
    game.resources.insert(Camera {
        transform,
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
        title: format!("bench_instances ({count} cubes, {duration} s)"),
        clear_color: CLEAR,
        ..Default::default()
    };
    game.title = config.title.clone();
    app::run(config, game)
}
