//! Spawns 100 entities with a `Transform`. A fixed-step system spins them
//! around +Y at 1 rad/s; a frame system logs the first entity's yaw once
//! per second. The screen is only cleared. Press Escape to quit.
//!
//! Run with `cargo run --example ecs_spin` (logs at `info` unless `RUST_LOG`
//! says otherwise).

use engine::app::{self, AppConfig, EcsGame};
use engine::ecs::{Entity, Resources, Schedule, Time, World};
use engine::math::{Quat, Transform, Vec3};

/// Spin speed around +Y, in radians per second.
const SPIN_SPEED: f64 = 1.0;

/// Which entity the logger watches and its yaw tracking state.
struct YawLog {
    entity: Entity,
    /// Simulation time at which to log next.
    next_log: f64,
    /// Yaw at the last log, in `-PI..=PI` as read from the quaternion.
    last_yaw: f32,
    /// Yaw with the ±PI wrap-around removed, so it keeps increasing.
    unwrapped_yaw: f32,
}

/// Fixed system: rotates every transform around +Y by `SPIN_SPEED * dt`.
fn spin(world: &mut World, res: &mut Resources) {
    let dt = res.expect::<Time>().fixed_dt;
    let step = Quat::from_rotation_y((SPIN_SPEED * dt) as f32);
    for transform in world.query_mut::<&mut Transform>() {
        transform.rotation = (step * transform.rotation).normalize();
    }
}

/// Frame system: logs the watched entity's yaw once per simulated second.
fn log_yaw(world: &mut World, res: &mut Resources) {
    let elapsed = res.expect::<Time>().elapsed;
    let log = res.expect_mut::<YawLog>();
    if elapsed < log.next_log {
        return;
    }
    let Ok(transform) = world.get::<&Transform>(log.entity) else {
        return;
    };
    // Yaw is the angle of the forward vector (-Z) around +Y.
    let forward = transform.forward();
    let yaw = (-forward.x).atan2(-forward.z);
    let mut delta = yaw - log.last_yaw;
    if delta < -std::f32::consts::PI {
        delta += std::f32::consts::TAU;
    } else if delta > std::f32::consts::PI {
        delta -= std::f32::consts::TAU;
    }
    log.unwrapped_yaw += delta;
    log.last_yaw = yaw;
    log.next_log += 1.0;
    log::info!("t = {elapsed:6.3} s  yaw = {:7.3} rad", log.unwrapped_yaw);
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut world = World::new();
    let mut first = None;
    for i in 0..100 {
        let position = Vec3::new((i % 10) as f32, 0.0, (i / 10) as f32);
        let entity = world.spawn((Transform::from_translation(position),));
        first.get_or_insert(entity);
    }

    let mut schedule = Schedule::new();
    schedule
        .add_fixed("spin", spin)
        .add_frame("log_yaw", log_yaw);

    let mut game = EcsGame::new(world, schedule);
    game.resources.insert(YawLog {
        entity: first.expect("spawned 100 entities"),
        next_log: 0.0,
        last_yaw: 0.0,
        unwrapped_yaw: 0.0,
    });

    let config = AppConfig {
        title: "ecs_spin".to_string(),
        ..Default::default()
    };
    game.title = config.title.clone();
    app::run(config, game)
}
