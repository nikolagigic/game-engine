//! A lit cube spinning around +Y at 1 rad/s. The camera sits at `(0, 2, 5)`
//! looking at the origin; a white light travels along `(1, -2, -1)`, coming
//! from above-front-left. Press Escape to quit.
//!
//! Run with `cargo run --example spinning_cube` (add `RUST_LOG=wgpu=warn`
//! to see any wgpu warnings).
//!
//! The renderer is created on the first frame, once the surface format is
//! known (`view.texture().format()`); the depth view comes from the loop.

use engine::app::{self, AppConfig, Game};
use engine::asset;
use engine::gpu::GpuContext;
use engine::math::{Camera, Projection, Quat, Transform, Vec3};
use engine::render::{GpuMesh, InstanceRaw, LightUniform, MeshRenderer, begin_mesh_pass};

/// Spin speed around +Y, in radians per second.
const SPIN_SPEED: f64 = 1.0;
/// Linear RGBA colour of the cube.
const CUBE_COLOR: [f32; 4] = [0.8, 0.3, 0.2, 1.0];
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
}

struct SpinningCube {
    camera: Camera,
    /// Angle after the previous and the latest fixed step, for interpolation.
    previous_angle: f64,
    angle: f64,
    gpu: Option<Gpu>,
}

impl SpinningCube {
    fn new() -> Self {
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
            previous_angle: 0.0,
            angle: 0.0,
            gpu: None,
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
        let texture = view.texture();
        let gpu = self.gpu.get_or_insert_with(|| {
            let renderer = MeshRenderer::new(&ctx.device, texture.format());
            renderer.set_light(
                &ctx.queue,
                &LightUniform {
                    direction: [1.0, -2.0, -1.0, 0.0],
                    color: [1.0, 1.0, 1.0, 1.0],
                    ambient: [0.1, 0.1, 0.1, 1.0],
                },
            );
            Gpu {
                renderer,
                mesh: GpuMesh::upload(&ctx.device, &asset::cube()),
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
            gpu.renderer.draw(&mut pass, &gpu.mesh, 0..1);
        }
        ctx.queue.submit([encoder.finish()]);
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let config = AppConfig {
        title: "spinning_cube".to_string(),
        clear_color: CLEAR,
        ..Default::default()
    };
    app::run(config, SpinningCube::new())
}
