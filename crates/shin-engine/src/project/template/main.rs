//! A Shin starter game: a lit cube on a ground slab, viewed through the
//! free-fly camera. The window title and size come from `shin.toml`.
//!
//! Controls: W/S forward and back, A/D strafe, E/Q up and down; hold the
//! right mouse button to look around; Escape quits.

use std::path::Path;

use shin::app::{self, AppConfig, EcsGame};
use shin::asset::{self, MeshData};
use shin::ecs::{Resources, Schedule, World};
use shin::input::{FlyCamera, fly_camera_system};
use shin::math::{Camera, Projection, Quat, Transform, Vec3};
use shin::project::Project;
use shin::render::{LightUniform, MeshInstance, RenderState, render_world};

/// The project's root directory, where `shin.toml` and `assets/` live.
const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// Linear clear colour: a dusky sky blue.
const SKY: wgpu::Color = wgpu::Color {
    r: 0.05,
    g: 0.08,
    b: 0.15,
    a: 1.0,
};

/// Loads the first mesh of `assets/cube.gltf`.
fn load_cube() -> anyhow::Result<MeshData> {
    let mut scene = asset::load_gltf(Path::new(ROOT).join("assets/cube.gltf"))?;
    anyhow::ensure!(!scene.meshes.is_empty(), "assets/cube.gltf has no mesh");
    Ok(scene.meshes.swap_remove(0))
}

/// Builds the scene: the renderer with a directional light, a ground slab,
/// and a cube resting on it.
fn setup(
    world: &mut World,
    res: &mut Resources,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    cube: &MeshData,
) {
    let mut state = RenderState::new(device, queue, format);
    state.light = LightUniform {
        direction: [-0.6, -1.0, -0.4, 0.0],
        color: [1.0, 0.95, 0.85, 1.0],
        ambient: [0.12, 0.13, 0.16, 1.0],
    };
    let cube = state.registry.add(device, cube);
    res.insert(state);

    // Ground: the cube flattened into a slab whose top face is y = 0.
    world.spawn((
        Transform::from_trs(
            Vec3::new(0.0, -0.25, 0.0),
            Quat::IDENTITY,
            Vec3::new(20.0, 0.5, 20.0),
        ),
        MeshInstance {
            mesh: cube,
            color: [0.18, 0.2, 0.17, 1.0],
            texture: None,
        },
    ));

    // The cube, turned a little so the light catches three faces.
    world.spawn((
        Transform::from_trs(
            Vec3::new(0.0, 0.5, 0.0),
            Quat::from_rotation_y(30f32.to_radians()),
            Vec3::ONE,
        ),
        MeshInstance {
            mesh: cube,
            color: [0.8, 0.25, 0.1, 1.0],
            texture: None,
        },
    ));
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let project = Project::load(Path::new(ROOT))?;
    let cube = load_cube()?;

    let mut schedule = Schedule::new();
    schedule.add_frame("fly_camera", fly_camera_system);
    let mut game = EcsGame::new(World::new(), schedule);
    game.title = project.window.title.clone();

    // Start above and in front of the cube, looking down at it.
    let fly = FlyCamera {
        pitch: -20f32.to_radians(),
        ..Default::default()
    };
    game.resources.insert(Camera {
        transform: Transform::from_trs(Vec3::new(0.0, 2.0, 5.0), fly.rotation(), Vec3::ONE),
        projection: Projection::Perspective {
            fov_y_radians: 60f32.to_radians(),
            near: 0.1,
            far: 200.0,
        },
    });
    game.resources.insert(fly);

    game.render_fn = Some(Box::new(move |world, res, ctx, color, depth| {
        let texture = color.texture();
        if res.get::<RenderState>().is_none() {
            setup(world, res, &ctx.device, &ctx.queue, texture.format(), &cube);
        }
        let aspect = texture.width() as f32 / texture.height() as f32;
        render_world(world, res, ctx, color, depth, aspect);
    }));

    let config = AppConfig {
        title: project.window.title,
        width: project.window.width,
        height: project.window.height,
        clear_color: SKY,
        ..Default::default()
    };
    app::run(config, game)
}
