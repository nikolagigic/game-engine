//! GE-002: headless offscreen render + golden-image harness, proven with a
//! clear colour.

mod common;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use common::{GoldenOpts, assert_golden, assert_golden_in, clear, golden_dir};
use image::RgbaImage;
use shin::gpu::{GpuContext, OffscreenTarget};

/// The reference clear colour, in linear RGB.
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.2,
    g: 0.4,
    b: 0.6,
    a: 1.0,
};

/// Renders a 64x64 offscreen target cleared to `color` and reads it back.
fn render_clear(color: wgpu::Color) -> anyhow::Result<RgbaImage> {
    let ctx = GpuContext::new(None)?;
    let target = OffscreenTarget::new(&ctx.device, 64, 64);
    clear(&ctx, &target, color);
    target.read_pixels(&ctx)
}

#[test]
fn clear_matches_golden() -> anyhow::Result<()> {
    let image = render_clear(CLEAR)?;
    assert_eq!(image.dimensions(), (64, 64));
    eprintln!("centre pixel: {:?}", image.get_pixel(32, 32));
    assert_golden("clear", &image, GoldenOpts::default());
    Ok(())
}

/// A fresh, empty per-test directory under the system temp dir.
fn scratch_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ge-golden-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Runs `f`, expecting a panic, and returns the panic message.
fn panic_message(f: impl FnOnce()) -> String {
    let payload = catch_unwind(AssertUnwindSafe(f)).expect_err("expected a golden failure");
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

#[test]
fn missing_reference_writes_new_png_then_update_recreates() -> anyhow::Result<()> {
    let dir = scratch_dir("missing");
    let image = render_clear(CLEAR)?;

    let message =
        panic_message(|| assert_golden_in(&dir, "clear", &image, GoldenOpts::default(), false));
    let new_png = dir.join("_diff/clear.new.png");
    assert!(
        message.contains(&new_png.display().to_string()),
        "{message}"
    );
    assert!(new_png.exists());

    // With the update flag the reference is (re)created and later runs pass.
    assert_golden_in(&dir, "clear", &image, GoldenOpts::default(), true);
    assert!(dir.join("clear.png").exists());
    assert_golden_in(&dir, "clear", &image, GoldenOpts::default(), false);
    Ok(())
}

#[test]
fn clear_off_by_0_05_fails_and_writes_diff() -> anyhow::Result<()> {
    let dir = scratch_dir("mismatch");
    std::fs::copy(golden_dir().join("clear.png"), dir.join("clear.png"))?;
    let image = render_clear(wgpu::Color {
        r: CLEAR.r + 0.05,
        ..CLEAR
    })?;

    let message =
        panic_message(|| assert_golden_in(&dir, "clear", &image, GoldenOpts::default(), false));
    assert!(message.contains("4096 of 4096 pixels"), "{message}");
    assert!(dir.join("_diff/clear.actual.png").exists());
    assert!(dir.join("_diff/clear.diff.png").exists());
    Ok(())
}
