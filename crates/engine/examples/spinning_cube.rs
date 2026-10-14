//! A lit cube spinning around +Y at 1 rad/s. The camera sits at `(0, 2, 5)`
//! looking at the origin; a white light travels along `(1, -2, -1)`, coming
//! from above-front-left. Press Escape to quit.
//!
//! Run with `cargo run --example spinning_cube` (add `RUST_LOG=wgpu=warn`
//! to see any wgpu warnings).
//!
//! `Game::render` only receives the colour view, so the example reads the
//! size and format from `view.texture()` and owns its own depth texture,
//! recreated when the window size changes.

use engine::app::{self, AppConfig, Game};
use engine::asset;
use engine::gpu::{DEPTH_FORMAT, GpuContext};
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
    /// Depth texture view and the size it was created for.
    depth: Option<(wgpu::TextureView, (u32, u32))>,
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

/// A depth view matching a `width` x `height` colour target.
fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("spinning_cube depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

impl Game for SpinningCube {
    fn fixed_update(&mut self, dt: f64) {
        self.previous_angle = self.angle;
        self.angle += SPIN_SPEED * dt;
    }

    fn render(&mut self, ctx: &GpuContext, view: &wgpu::TextureView, alpha: f64) {
        let texture = view.texture();
        let size = (texture.width(), texture.height());

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
                depth: None,
            }
        });
        if gpu.depth.as_ref().is_none_or(|(_, s)| *s != size) {
            gpu.depth = Some((create_depth_view(&ctx.device, size.0, size.1), size));
        }
        let Some((depth_view, _)) = &gpu.depth else {
            return;
        };

        gpu.renderer
            .set_camera(&ctx.queue, &self.camera, size.0 as f32 / size.1 as f32);
        let angle = self.previous_angle + (self.angle - self.previous_angle) * alpha;
        let transform =
            Transform::from_trs(Vec3::ZERO, Quat::from_rotation_y(angle as f32), Vec3::ONE);
        let instance = InstanceRaw::from_transform(&transform, CUBE_COLOR);

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("spinning_cube encoder"),
            });
        {
            let mut pass = begin_mesh_pass(&mut encoder, view, depth_view, CLEAR);
            gpu.renderer
                .draw(&ctx.device, &ctx.queue, &mut pass, &gpu.mesh, &[instance]);
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
