//! GE-006: a lit UV sphere rendered offscreen matches `golden/sphere.png`.

mod common;

use common::{GoldenOpts, assert_golden, render_lit_mesh};
use shin::math::Transform;

#[test]
fn sphere_matches_golden() -> anyhow::Result<()> {
    let image = render_lit_mesh(
        &shin::asset::uv_sphere(32, 16),
        Transform::IDENTITY,
        [0.8, 0.3, 0.2, 1.0],
        128,
    )?;
    eprintln!("centre pixel: {:?}", image.get_pixel(64, 64));
    assert_golden("sphere", &image, GoldenOpts::default());
    Ok(())
}
