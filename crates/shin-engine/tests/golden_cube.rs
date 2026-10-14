//! GE-006: a lit cube rendered offscreen matches `golden/cube.png`.
//!
//! The cube is turned 30° about +Y so the camera at `(0, 2, 5)` sees three
//! faces: top, front-left (-X), and front-right (+Z). With the light
//! travelling along `(1, -2, -1)` their diffuse terms are about 0.82,
//! 0.56, and 0.15, so the top is brightest and all three differ.

mod common;

use common::{GoldenOpts, assert_golden, render_lit_mesh};
use shin::math::{Camera, Projection, Quat, Transform, Vec3};

const COLOR: [f32; 4] = [0.8, 0.3, 0.2, 1.0];

const SIZE: u32 = 128;

/// Projects the centre of the cube face with local normal `n` (cube turned
/// by `rotation`) to pixel coordinates in the shared test camera.
fn face_centre_pixel(rotation: Quat, n: Vec3) -> (u32, u32) {
    let mut camera = Camera {
        transform: Transform::from_translation(Vec3::new(0.0, 2.0, 5.0)),
        projection: Projection::Perspective {
            fov_y_radians: 45f32.to_radians(),
            near: 0.1,
            far: 100.0,
        },
    };
    camera.transform.looking_at(Vec3::ZERO, Vec3::Y);
    let ndc = camera
        .view_projection(1.0)
        .project_point3(rotation * (0.5 * n));
    let to_px = |v: f32| ((v * 0.5 + 0.5) * SIZE as f32) as u32;
    (to_px(ndc.x), to_px(-ndc.y))
}

/// Sum of the RGB channels; enough to rank faces of one colour.
fn brightness(p: &image::Rgba<u8>) -> u32 {
    p.0[..3].iter().map(|&c| u32::from(c)).sum()
}

#[test]
fn cube_matches_golden() -> anyhow::Result<()> {
    let rotation = Quat::from_rotation_y(30f32.to_radians());
    let transform = Transform::from_trs(Vec3::ZERO, rotation, Vec3::ONE);
    let image = render_lit_mesh(&shin::asset::cube(), transform, COLOR, SIZE)?;

    let sample = |n: Vec3| {
        let (x, y) = face_centre_pixel(rotation, n);
        let pixel = *image.get_pixel(x, y);
        eprintln!("face {n} centre at ({x}, {y}): {pixel:?}");
        pixel
    };
    let top = sample(Vec3::Y);
    let left = sample(Vec3::NEG_X);
    let right = sample(Vec3::Z);
    assert!(
        brightness(&top) > brightness(&left),
        "top must be brightest"
    );
    assert!(
        brightness(&left) > brightness(&right),
        "three distinct levels"
    );

    assert_golden("cube", &image, GoldenOpts::default());
    Ok(())
}
