//! Free-fly camera demo. There is nothing to draw yet (meshes arrive with
//! GE-006), so the screen is only cleared and the camera pose is logged
//! once per second; check the log to see that the controls work.
//!
//! Controls: W/S forward and back, A/D strafe, E/Q up and down; hold the
//! right mouse button to grab the cursor and look around; Escape quits and
//! restores the cursor.
//!
//! Run with `cargo run --example fly_camera` (logs at `info` unless
//! `RUST_LOG` says otherwise).

use shin::app::{self, AppConfig, EcsGame};
use shin::ecs::{Resources, Schedule, Time, World};
use shin::input::{FlyCamera, fly_camera_system};
use shin::math::{Camera, Projection, Transform, Vec3};

/// Wall-clock seconds until the next pose log line.
struct PoseLog {
    until_next: f64,
}

/// Frame system: logs the camera position and forward vector once per second.
fn log_pose(_: &mut World, res: &mut Resources) {
    let frame_dt = res.expect::<Time>().frame_dt;
    let transform = res.expect::<Camera>().transform;
    let fly = *res.expect::<FlyCamera>();
    let log = res.expect_mut::<PoseLog>();
    log.until_next -= frame_dt;
    if log.until_next > 0.0 {
        return;
    }
    log.until_next += 1.0;
    let p = transform.translation;
    let f = transform.forward();
    log::info!(
        "position ({:6.2}, {:6.2}, {:6.2})  forward ({:5.2}, {:5.2}, {:5.2})  yaw {:6.1}°  pitch {:5.1}°",
        p.x,
        p.y,
        p.z,
        f.x,
        f.y,
        f.z,
        fly.yaw.to_degrees(),
        fly.pitch.to_degrees(),
    );
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut schedule = Schedule::new();
    schedule
        .add_frame("fly_camera", fly_camera_system)
        .add_frame("log_pose", log_pose);

    let mut game = EcsGame::new(World::new(), schedule);
    game.resources.insert(FlyCamera::default());
    game.resources.insert(Camera {
        transform: Transform::from_translation(Vec3::new(0.0, 1.0, 5.0)),
        projection: Projection::Perspective {
            fov_y_radians: 60f32.to_radians(),
            near: 0.1,
            far: 100.0,
        },
    });
    game.resources.insert(PoseLog { until_next: 0.0 });

    let config = AppConfig {
        title: "fly_camera (WASD + E/Q, hold right mouse to look)".to_string(),
        // Cornflower blue #6495ED, converted to linear.
        clear_color: wgpu::Color {
            r: 0.127,
            g: 0.301,
            b: 0.846,
            a: 1.0,
        },
        ..Default::default()
    };
    game.title = config.title.clone();
    app::run(config, game)
}
