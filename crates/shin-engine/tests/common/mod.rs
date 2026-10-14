//! Shared support for integration tests: offscreen clearing and golden-image
//! comparison. Not part of the engine library.
//!
//! Golden workflow:
//! - References live in `tests/golden/<name>.png` and are committed.
//! - Failures write `tests/golden/_diff/<name>.{new,actual,diff}.png`
//!   (gitignored) so you can look at what went wrong.
//! - `UPDATE_GOLDEN=1 cargo test ...` overwrites references with the current
//!   output. Only do that when the change in output is intended.

#![allow(dead_code)] // each test binary uses a different subset

use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};
use shin::gpu::{GpuContext, OffscreenTarget};

/// Tolerances for [`assert_golden`].
#[derive(Clone, Copy, Debug)]
pub struct GoldenOpts {
    /// A pixel differs if any channel differs by more than this.
    pub max_channel_diff: u8,
    /// The test fails if more than this fraction of pixels differ.
    pub max_diff_fraction: f64,
}

impl Default for GoldenOpts {
    fn default() -> Self {
        Self {
            max_channel_diff: 2,
            max_diff_fraction: 0.001,
        }
    }
}

/// `crates/shin-engine/tests/golden/`.
pub fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// Compares `actual` to `tests/golden/<name>.png`.
/// - If `UPDATE_GOLDEN=1`: writes the reference and passes.
/// - If the reference is missing: writes it to `tests/golden/_diff/<name>.new.png`
///   and fails with a clear message.
/// - On mismatch: writes actual + per-pixel diff to `tests/golden/_diff/` and
///   fails with counts.
pub fn assert_golden(name: &str, actual: &RgbaImage, opts: GoldenOpts) {
    let update = std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1");
    assert_golden_in(&golden_dir(), name, actual, opts, update);
}

/// [`assert_golden`] against an explicit golden directory and update flag,
/// so the harness itself can be tested without touching real references.
pub fn assert_golden_in(
    dir: &Path,
    name: &str,
    actual: &RgbaImage,
    opts: GoldenOpts,
    update: bool,
) {
    let reference_path = dir.join(format!("{name}.png"));
    let diff_dir = dir.join("_diff");

    if update {
        std::fs::create_dir_all(dir).expect("create golden dir");
        actual
            .save(&reference_path)
            .expect("write golden reference");
        eprintln!("UPDATE_GOLDEN: wrote {}", reference_path.display());
        return;
    }

    if !reference_path.exists() {
        let new_path = diff_dir.join(format!("{name}.new.png"));
        save(&new_path, actual);
        panic!(
            "golden reference {} is missing.\n\
             Wrote the current output to {}.\n\
             Inspect it, then re-run with UPDATE_GOLDEN=1 to accept it.",
            reference_path.display(),
            new_path.display(),
        );
    }

    let expected = image::open(&reference_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", reference_path.display()))
        .to_rgba8();
    let actual_path = diff_dir.join(format!("{name}.actual.png"));

    if expected.dimensions() != actual.dimensions() {
        save(&actual_path, actual);
        panic!(
            "golden '{name}': size mismatch, expected {:?}, got {:?}. Actual written to {}",
            expected.dimensions(),
            actual.dimensions(),
            actual_path.display(),
        );
    }

    let (diff, differing) = diff_image(&expected, actual, opts.max_channel_diff);
    let total = u64::from(actual.width()) * u64::from(actual.height());
    let fraction = differing as f64 / total as f64;
    if fraction > opts.max_diff_fraction {
        let diff_path = diff_dir.join(format!("{name}.diff.png"));
        save(&actual_path, actual);
        save(&diff_path, &diff);
        panic!(
            "golden '{name}' mismatch: {differing} of {total} pixels ({:.3}%) differ by more \
             than {} in some channel (allowed {:.3}%).\n\
             Actual: {}\nDiff:   {}",
            fraction * 100.0,
            opts.max_channel_diff,
            opts.max_diff_fraction * 100.0,
            actual_path.display(),
            diff_path.display(),
        );
    }
}

/// Builds a diff image and counts differing pixels. Differing pixels are
/// opaque red; matching pixels are the actual image at quarter brightness so
/// the scene stays recognisable.
fn diff_image(expected: &RgbaImage, actual: &RgbaImage, max_channel_diff: u8) -> (RgbaImage, u64) {
    let mut differing = 0;
    let diff = RgbaImage::from_fn(actual.width(), actual.height(), |x, y| {
        let e = expected.get_pixel(x, y);
        let a = actual.get_pixel(x, y);
        let differs =
            e.0.iter()
                .zip(a.0)
                .any(|(&e, a)| e.abs_diff(a) > max_channel_diff);
        if differs {
            differing += 1;
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([a[0] / 4, a[1] / 4, a[2] / 4, 255])
        }
    });
    (diff, differing)
}

fn save(path: &Path, image: &RgbaImage) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create golden diff dir");
    }
    image
        .save(path)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
}

/// Shared GE-006 scene: one instance of `mesh` at `transform`, viewed from
/// `(0, 2, 5)` looking at the origin (45° vertical FOV), lit by a white
/// light travelling along `(1, -2, -1)` (from above-front-left) with 0.1
/// ambient, over a near-black clear. Renders `size` x `size` pixels
/// offscreen and reads them back.
pub fn render_lit_mesh(
    mesh: &shin::asset::MeshData,
    transform: shin::math::Transform,
    color: [f32; 4],
    size: u32,
) -> anyhow::Result<RgbaImage> {
    use shin::gpu::OFFSCREEN_COLOR_FORMAT;
    use shin::math::{Camera, Projection, Transform, Vec3};
    use shin::render::{GpuMesh, InstanceRaw, LightUniform, MeshRenderer, begin_mesh_pass};

    let ctx = GpuContext::new(None)?;
    let target = OffscreenTarget::new(&ctx.device, size, size);
    let mut renderer = MeshRenderer::new(&ctx.device, &ctx.queue, OFFSCREEN_COLOR_FORMAT);

    let mut camera = Camera {
        transform: Transform::from_translation(Vec3::new(0.0, 2.0, 5.0)),
        projection: Projection::Perspective {
            fov_y_radians: 45f32.to_radians(),
            near: 0.1,
            far: 100.0,
        },
    };
    camera.transform.looking_at(Vec3::ZERO, Vec3::Y);
    renderer.set_camera(&ctx.queue, &camera, 1.0);
    renderer.set_light(
        &ctx.queue,
        &LightUniform {
            direction: [1.0, -2.0, -1.0, 0.0],
            color: [1.0, 1.0, 1.0, 1.0],
            ambient: [0.1, 0.1, 0.1, 1.0],
        },
    );

    let gpu_mesh = GpuMesh::upload(&ctx.device, mesh);
    let instance = InstanceRaw::from_transform(&transform, color);
    renderer.upload_instances(&ctx.device, &ctx.queue, &[instance]);
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("lit mesh encoder"),
        });
    {
        let mut pass = begin_mesh_pass(
            &mut encoder,
            &target.color_view,
            &target.depth_view,
            wgpu::Color {
                r: 0.02,
                g: 0.02,
                b: 0.02,
                a: 1.0,
            },
        );
        renderer.draw(&mut pass, &gpu_mesh, 0..1);
    }
    ctx.queue.submit([encoder.finish()]);
    target.read_pixels(&ctx)
}

/// Clears `target`'s colour to `color` (linear) and depth to 1.0.
pub fn clear(ctx: &GpuContext, target: &OffscreenTarget, color: wgpu::Color) {
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("test clear encoder"),
        });
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("test clear pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &target.color_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(color),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &target.depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    ctx.queue.submit([encoder.finish()]);
}
