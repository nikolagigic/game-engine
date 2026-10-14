//! A lit cube spinning around +Y at 1 rad/s, textured with the test checker
//! (`assets/test/checker.png`: red cells in the top half, blue in the
//! bottom, so each upright face shows red above blue). The camera sits at
//! `(0, 2, 5)` looking at the origin; a white light travels along
//! `(1, -2, -1)`, coming from above-front-left. Once a second the title
//! shows the frame-time summary. Press Escape to quit.
//!
//! Run with `cargo run --example spinning_cube` (add `RUST_LOG=wgpu=warn`
//! to see any wgpu warnings).
//!
//! The renderer is created on the first frame, once the surface format is
//! known (`view.texture().format()`); the depth view comes from the loop.

use std::time::Instant;

use shin::app::{self, AppConfig, Game};
use shin::asset::{self, ImageData};
use shin::gpu::GpuContext;
use shin::math::{Camera, Projection, Quat, Transform, Vec3};
use shin::render::{GpuMesh, GpuTexture, InstanceRaw, LightUniform, MeshRenderer, begin_mesh_pass};
use shin::stats::FrameStats;

/// Window title before the first frame-time summary.
const TITLE: &str = "spinning_cube";

/// Spin speed around +Y, in radians per second.
const SPIN_SPEED: f64 = 1.0;
/// Linear RGBA colour of the cube, multiplied into the texture.
const CUBE_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// The base-colour texture, relative to this crate.
const CHECKER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/test/checker.png");
/// Linear clear colour (near-black).
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.02,
    g: 0.02,
    b: 0.02,
    a: 1.0,
};

/// GPU state, created on the first frame once the surface format is known.
struct Gpu {
    renderer: MeshRenderer,
    mesh: GpuMesh,
    texture: GpuTexture,
}

struct SpinningCube {
    camera: Camera,
    /// The checker, decoded at startup and uploaded with the renderer.
    checker: ImageData,
    /// Angle after the previous and the latest fixed step, for interpolation.
    previous_angle: f64,
    angle: f64,
    gpu: Option<Gpu>,
    /// Frame times, measured between `render` calls.
    stats: FrameStats,
    last_frame: Option<Instant>,
    /// When `title` was last rebuilt.
    title_updated: Instant,
    title: String,
}

impl SpinningCube {
    fn new(checker: ImageData) -> Self {
        let mut transform = Transform::from_translation(Vec3::new(0.0, 2.0, 5.0));
        transform.looking_at(Vec3::ZERO, Vec3::Y);
        Self {
            camera: Camera {
                transform,
                projection: Projection::Perspective {
                    fov_y_radians: 45f32.to_radians(),
                    near: 0.1,
                    far: 100.0,
                },
            },
            checker,
            previous_angle: 0.0,
            angle: 0.0,
            gpu: None,
            stats: FrameStats::default(),
            last_frame: None,
            title_updated: Instant::now(),
            title: TITLE.to_string(),
        }
    }
}

impl Game for SpinningCube {
    fn fixed_update(&mut self, dt: f64) {
        self.previous_angle = self.angle;
        self.angle += SPIN_SPEED * dt;
    }

    fn render(
        &mut self,
        ctx: &GpuContext,
        view: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        alpha: f64,
    ) {
        let now = Instant::now();
        if let Some(last) = self.last_frame.replace(now) {
            self.stats.push((now - last).as_secs_f64());
        }
        if (now - self.title_updated).as_secs_f64() >= 1.0 {
            self.title = format!("{TITLE} | {}", self.stats.summary());
            self.title_updated = now;
        }

        let texture = view.texture();
        let gpu = self.gpu.get_or_insert_with(|| {
            let renderer = MeshRenderer::new(&ctx.device, &ctx.queue, texture.format());
            renderer.set_light(
                &ctx.queue,
                &LightUniform {
                    direction: [1.0, -2.0, -1.0, 0.0],
                    color: [1.0, 1.0, 1.0, 1.0],
                    ambient: [0.1, 0.1, 0.1, 1.0],
                },
            );
            let texture = GpuTexture::upload(&ctx.device, &ctx.queue, &renderer, &self.checker);
            Gpu {
                renderer,
                mesh: GpuMesh::upload(&ctx.device, &asset::cube()),
                texture,
            }
        });

        let aspect = texture.width() as f32 / texture.height() as f32;
        gpu.renderer.set_camera(&ctx.queue, &self.camera, aspect);
        let angle = self.previous_angle + (self.angle - self.previous_angle) * alpha;
        let transform =
            Transform::from_trs(Vec3::ZERO, Quat::from_rotation_y(angle as f32), Vec3::ONE);
        let instance = InstanceRaw::from_transform(&transform, CUBE_COLOR);
        gpu.renderer
            .upload_instances(&ctx.device, &ctx.queue, &[instance]);

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("spinning_cube encoder"),
            });
        {
            let mut pass = begin_mesh_pass(&mut encoder, view, depth, CLEAR);
            gpu.renderer
                .draw_textured(&mut pass, &gpu.mesh, &gpu.texture, 0..1);
        }
        ctx.queue.submit([encoder.finish()]);
    }

    fn title(&self) -> Option<String> {
        Some(self.title.clone())
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let config = AppConfig {
        title: TITLE.to_string(),
        clear_color: CLEAR,
        ..Default::default()
    };
    let checker = asset::load_image(CHECKER)?;
    app::run(config, SpinningCube::new(checker))
}
