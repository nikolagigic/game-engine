//! GE-001: a headless GPU context can be created on the developer machine.

use engine::gpu::GpuContext;

#[test]
fn headless_context_has_nonzero_limits() -> anyhow::Result<()> {
    let ctx = GpuContext::new(None)?;
    let limits = ctx.device.limits();
    assert!(limits.max_texture_dimension_2d > 0);
    assert!(limits.max_bind_groups > 0);
    assert!(limits.max_buffer_size > 0);
    Ok(())
}
