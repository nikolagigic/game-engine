//! The milestone-1 showcase: every subsystem in one scene.
//!
//! A ground plane, a 20x20 field of cubes and spheres in varied colours and
//! sizes, a glTF-loaded pedestal in the middle with satellites orbiting it
//! (moved by a fixed-timestep system), a directional light, the free-fly
//! camera, and frame stats in the window title. Everything is drawn from
//! the ECS with one instanced draw call per mesh.
//!
//! Run with `cargo run --release --example demo`.
//!
//! Controls: W/S forward and back, A/D strafe, E/Q up and down; hold the
//! right mouse button to grab the cursor and look around; Escape quits.
//!
//! The scene is built by [`setup`], which `tests/golden_demo.rs` includes
//! with `#[path]` to render the initial frame headlessly.

use std::f32::consts::TAU;

use engine::app::{self, AppConfig, EcsGame, Game};
use engine::asset::{self, MeshData};
use engine::ecs::{Resources, Schedule, Time, World};
use engine::gpu::GpuContext;
use engine::input::{FlyCamera, fly_camera_system};
use engine::math::{Camera, Projection, Quat, Transform, Vec3};
use engine::render::{LightUniform, MeshInstance, RenderState, render_world};

/// Objects per side of the field.
pub const FIELD_SIDE: usize = 20;
/// Distance between neighbouring field cells.
const SPACING: f32 = 2.5;
/// Number of orbiting satellites.
const SATELLITES: usize = 6;
/// Linear clear colour: a dusky sky blue.
pub const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.05,
    g: 0.08,
    b: 0.15,
    a: 1.0,
};

/// Path of the glTF cube used for the ground and the central pedestal.
pub fn gltf_cube_path() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/assets/test/cube.gltf")
}

/// Loads the glTF test cube's single mesh, so the demo exercises the
/// loader and not just the built-in meshes.
pub fn load_gltf_cube() -> anyhow::Result<MeshData> {
    let mut scene = asset::load_gltf(gltf_cube_path())?;
    anyhow::ensure!(!scene.meshes.is_empty(), "the glTF cube has no mesh");
    Ok(scene.meshes.swap_remove(0))
}

/// A satellite circling the Y axis: position at simulation time `t` is
/// `(radius cos θ, height, radius sin θ)` with `θ = phase + speed · t`.
#[derive(Clone, Copy, Debug)]
pub struct Orbit {
    /// Distance from the Y axis.
    pub radius: f32,
    /// Height above the ground.
    pub height: f32,
    /// Angular speed in radians per second.
    pub speed: f32,
    /// Angle at `t = 0`, in radians.
    pub phase: f32,
}

impl Orbit {
    /// The satellite's position after `t` seconds of simulation.
    pub fn position(&self, t: f32) -> Vec3 {
        let theta = self.phase + self.speed * t;
        Vec3::new(
            self.radius * theta.cos(),
            self.height,
            self.radius * theta.sin(),
        )
    }
}

/// Fixed system: moves every [`Orbit`] entity to its position at
/// `Time::elapsed`.
pub fn orbit_system(world: &mut World, res: &mut Resources) {
    let t = res.expect::<Time>().elapsed as f32;
    for (transform, orbit) in world.query_mut::<(&mut Transform, &Orbit)>() {
        transform.translation = orbit.position(t);
    }
}

/// A deterministic pseudo-random value in `0..1` for cell `k` and stream
/// `salt` (an integer hash; the demo needs variety, not randomness).
fn hash01(k: u32, salt: u32) -> f32 {
    let mut x = k.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// A saturated linear RGB colour for hue `h` in `0..1`.
fn hue(h: f32) -> [f32; 4] {
    let channel = |offset: f32| {
        let x = ((h + offset).fract() * 6.0 - 3.0).abs() - 1.0;
        // Squared as a rough sRGB-to-linear step so colours do not wash out.
        x.clamp(0.0, 1.0).powi(2) * 0.8 + 0.02
    };
    [channel(0.0), channel(2.0 / 3.0), channel(1.0 / 3.0), 1.0]
}

/// The starting camera: above and behind the field's near edge, looking
/// toward the pedestal. The orientation comes from the returned
/// [`FlyCamera`] so the controller picks up exactly where the scene starts.
pub fn initial_camera() -> (Camera, FlyCamera) {
    let fly = FlyCamera {
        speed: 12.0,
        pitch: -25f32.to_radians(),
        ..Default::default()
    };
    let camera = Camera {
        transform: Transform::from_trs(Vec3::new(0.0, 14.0, 38.0), fly.rotation(), Vec3::ONE),
        projection: Projection::Perspective {
            fov_y_radians: 50f32.to_radians(),
            near: 0.1,
            far: 300.0,
        },
    };
    (camera, fly)
}

/// Builds the demo scene: inserts a [`RenderState`] for `format` (with the
/// directional light) and spawns the ground, the field, the pedestal, and
/// the satellites. `gltf_cube` comes from [`load_gltf_cube`]. The camera is
/// separate: see [`initial_camera`].
pub fn setup(
    world: &mut World,
    res: &mut Resources,
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    gltf_cube: &MeshData,
) {
    let mut state = RenderState::new(device, format);
    state.light = LightUniform {
        direction: [-0.6, -1.0, -0.4, 0.0],
        color: [1.0, 0.95, 0.85, 1.0],
        ambient: [0.12, 0.13, 0.16, 1.0],
    };
    let cube = state.registry.add(device, &asset::cube());
    let sphere = state.registry.add(device, &asset::uv_sphere(24, 12));
    let loaded = state.registry.add(device, gltf_cube);
    res.insert(state);

    // Ground: the glTF cube flattened into a slab whose top face is y = 0.
    let ground = FIELD_SIDE as f32 * SPACING + 10.0;
    world.spawn((
        Transform::from_trs(
            Vec3::new(0.0, -0.25, 0.0),
            Quat::IDENTITY,
            Vec3::new(ground, 0.5, ground),
        ),
        MeshInstance {
            mesh: loaded,
            color: [0.18, 0.2, 0.17, 1.0],
        },
    ));

    // Field: a cube or a sphere per cell, resting on the ground, with a
    // random colour, size, and (for cubes) heading. The four centre cells
    // are left empty for the pedestal.
    let offset = (FIELD_SIDE as f32 - 1.0) / 2.0;
    let centre = (FIELD_SIDE / 2 - 1)..=(FIELD_SIDE / 2);
    world.spawn_batch(
        (0..FIELD_SIDE * FIELD_SIDE)
            .filter(|k| !(centre.contains(&(k % FIELD_SIDE)) && centre.contains(&(k / FIELD_SIDE))))
            .map(|k| {
                let (i, j) = (k % FIELD_SIDE, k / FIELD_SIDE);
                let k = k as u32;
                let size = 0.5 + 0.9 * hash01(k, 1);
                let color = hue(hash01(k, 2));
                let x = (i as f32 - offset) * SPACING;
                let z = (j as f32 - offset) * SPACING;
                let (mesh, transform) = if hash01(k, 3) < 0.5 {
                    let heading = Quat::from_rotation_y(hash01(k, 4) * TAU);
                    let t = Vec3::new(x, size / 2.0, z);
                    (cube, Transform::from_trs(t, heading, Vec3::splat(size)))
                } else {
                    let r = size / 2.0;
                    let t = Vec3::new(x, r, z);
                    (
                        sphere,
                        Transform::from_trs(t, Quat::IDENTITY, Vec3::splat(r)),
                    )
                };
                (transform, MeshInstance { mesh, color })
            }),
    );

    // Pedestal: the glTF cube again, white, in the cleared centre.
    world.spawn((
        Transform::from_trs(
            Vec3::new(0.0, 1.5, 0.0),
            Quat::from_rotation_y(TAU / 8.0),
            Vec3::splat(3.0),
        ),
        MeshInstance {
            mesh: loaded,
            color: [0.9, 0.9, 0.9, 1.0],
        },
    ));

    // Satellites: glowing-yellow spheres circling the pedestal at two
    // heights, alternating direction. `orbit_system` moves them.
    world.spawn_batch((0..SATELLITES).map(|s| {
        let orbit = Orbit {
            radius: 5.0 + (s % 2) as f32 * 2.0,
            height: 4.0 + (s % 3) as f32,
            speed: if s % 2 == 0 { 0.8 } else { -0.6 },
            phase: s as f32 / SATELLITES as f32 * TAU,
        };
        let transform = Transform::from_trs(orbit.position(0.0), Quat::IDENTITY, Vec3::splat(0.6));
        let mesh = MeshInstance {
            mesh: sphere,
            color: [1.0, 0.75, 0.2, 1.0],
        };
        (transform, mesh, orbit)
    }));
}

/// Frame-time accumulator behind the window title. (The `stats` module is
/// still a stub, GE-009, so the demo averages frame times itself.)
#[derive(Default)]
struct FrameStats {
    elapsed: f64,
    frames: u32,
    /// The last title, refreshed twice a second.
    title: String,
}

/// Frame system: averages `frame_dt` over half a second into
/// [`FrameStats::title`].
fn frame_stats_system(world: &mut World, res: &mut Resources) {
    let frame_dt = res.expect::<Time>().frame_dt;
    let draw_calls = res
        .get::<RenderState>()
        .map_or(0, |state| state.last_frame_draw_calls);
    let stats = res.expect_mut::<FrameStats>();
    stats.elapsed += frame_dt;
    stats.frames += 1;
    if stats.elapsed < 0.5 {
        return;
    }
    let ms = 1000.0 * stats.elapsed / f64::from(stats.frames);
    stats.title = format!(
        "demo | {ms:.2} ms ({:.0} fps) | {} entities, {draw_calls} draw calls | WASD+E/Q, hold right mouse to look",
        1000.0 / ms,
        world.len(),
    );
    stats.elapsed = 0.0;
    stats.frames = 0;
}

/// [`EcsGame`] plus a window title taken from [`FrameStats`].
struct Demo(EcsGame);

impl Game for Demo {
    fn init(&mut self, ctx: &GpuContext) -> anyhow::Result<()> {
        self.0.init(ctx)
    }

    fn fixed_update(&mut self, dt: f64) {
        self.0.fixed_update(dt);
    }

    fn render(
        &mut self,
        ctx: &GpuContext,
        view: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        alpha: f64,
    ) {
        self.0.render(ctx, view, depth, alpha);
    }

    fn on_window_event(&mut self, event: &winit::event::WindowEvent) {
        self.0.on_window_event(event);
    }

    fn on_device_event(&mut self, event: &winit::event::DeviceEvent) {
        self.0.on_device_event(event);
    }

    fn title(&self) -> Option<String> {
        let title = &self.0.resources.get::<FrameStats>()?.title;
        (!title.is_empty()).then(|| title.clone())
    }

    fn cursor_grabbed(&self) -> bool {
        self.0.cursor_grabbed()
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let gltf_cube = load_gltf_cube()?;

    let mut schedule = Schedule::new();
    schedule
        .add_fixed("orbit", orbit_system)
        .add_frame("fly_camera", fly_camera_system)
        .add_frame("frame_stats", frame_stats_system);
    let mut game = EcsGame::new(World::new(), schedule);
    game.resources.insert(FrameStats::default());
    let (camera, fly) = initial_camera();
    game.resources.insert(camera);
    game.resources.insert(fly);
    game.render_fn = Some(Box::new(move |world, res, ctx, color, depth| {
        let texture = color.texture();
        if res.get::<RenderState>().is_none() {
            setup(world, res, &ctx.device, texture.format(), &gltf_cube);
        }
        let aspect = texture.width() as f32 / texture.height() as f32;
        render_world(world, res, ctx, color, depth, aspect);
    }));

    let config = AppConfig {
        title: "demo".to_string(),
        clear_color: CLEAR,
        ..Default::default()
    };
    app::run(config, Demo(game))
}
