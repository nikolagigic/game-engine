//! GE-013: the `Headless` engine handle steps time and renders offscreen.

use image::Rgba;
use shin::ecs::Time;
use shin::headless::Headless;

#[test]
fn step_render_and_png_round_trip() -> anyhow::Result<()> {
    let mut engine = Headless::new(64, 64)?;
    // Linear 0 and 1 encode to exactly 0 and 255 in the sRGB target.
    engine.clear_color = wgpu::Color {
        r: 1.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    };

    engine.step(3);
    let time = engine.resources.expect::<Time>();
    assert_eq!(time.fixed_steps, 3);
    assert!((time.elapsed - 3.0 * engine.fixed_dt).abs() < 1e-12);

    let image = engine.render()?;
    assert_eq!(image.dimensions(), (64, 64));
    let expected = Rgba([255, 0, 255, 255]);
    for (x, y, pixel) in image.enumerate_pixels() {
        assert_eq!(*pixel, expected, "pixel ({x}, {y})");
    }

    let png = engine.render_png()?;
    let decoded = image::load_from_memory_with_format(&png, image::ImageFormat::Png)?.to_rgba8();
    assert_eq!(decoded, image);
    Ok(())
}
