//! wgpu device/queue/surface context and offscreen render targets.
//! Owned by GE-001 (context) and GE-002 (offscreen + readback).
//!
//! [`GpuContext`] holds the instance, adapter, device, and queue. A windowed
//! app also owns a [`SurfaceTarget`]: the swapchain surface, its
//! configuration, and a depth texture that is recreated on resize.

use anyhow::Context as _;

/// Depth buffer format used by every render pass that has a depth attachment.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The core wgpu objects every subsystem needs.
pub struct GpuContext {
    /// The wgpu instance the adapter (and any surface) was created from.
    pub instance: wgpu::Instance,
    /// The physical GPU in use.
    pub adapter: wgpu::Adapter,
    /// Logical device: creates buffers, textures, pipelines.
    pub device: wgpu::Device,
    /// Submits command buffers and presents surface textures.
    pub queue: wgpu::Queue,
}

impl GpuContext {
    /// Creates a context with a fresh instance. If `compatible_surface` is
    /// given, the adapter is chosen to be compatible with it. Headless callers
    /// (GE-002) pass `None`.
    ///
    /// A surface belongs to the instance that created it, so windowed callers
    /// should create the instance first and use [`GpuContext::with_instance`].
    pub fn new(compatible_surface: Option<&wgpu::Surface<'_>>) -> anyhow::Result<Self> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        Self::with_instance(instance, compatible_surface)
    }

    /// Creates a context on an existing `instance`, choosing an adapter
    /// compatible with `compatible_surface` when one is given. The surface
    /// must have been created from `instance`.
    pub fn with_instance(
        instance: wgpu::Instance,
        compatible_surface: Option<&wgpu::Surface<'_>>,
    ) -> anyhow::Result<Self> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface,
            ..Default::default()
        }))
        .context("no suitable GPU adapter found")?;
        log::info!("using adapter: {:?}", adapter.get_info());

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("engine device"),
            ..Default::default()
        }))
        .context("failed to create GPU device")?;

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }
}

/// A window surface, its configuration, and a matching depth texture.
pub struct SurfaceTarget<'w> {
    /// The presentable surface.
    pub surface: wgpu::Surface<'w>,
    /// Current swapchain configuration (format, size, present mode).
    pub config: wgpu::SurfaceConfiguration,
    /// Depth texture sized to the surface, in [`DEPTH_FORMAT`].
    pub depth_texture: wgpu::Texture,
    /// View of [`Self::depth_texture`], used as the depth attachment.
    pub depth_view: wgpu::TextureView,
}

impl<'w> SurfaceTarget<'w> {
    /// Configures `surface` for `ctx` at the given size (clamped to at least
    /// 1x1).
    ///
    /// The format is `Bgra8UnormSrgb` when supported, otherwise the first
    /// sRGB format, otherwise whatever the surface prefers. An sRGB format
    /// means shaders write linear colour and the hardware encodes it.
    pub fn new(
        ctx: &GpuContext,
        surface: wgpu::Surface<'w>,
        width: u32,
        height: u32,
    ) -> anyhow::Result<Self> {
        let caps = surface.get_capabilities(&ctx.adapter);
        let format = choose_surface_format(&caps.formats)
            .context("surface is not supported by the adapter")?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&ctx.device, &config);
        let (depth_texture, depth_view) = create_depth(&ctx.device, config.width, config.height);
        log::info!("surface format: {format:?}");

        Ok(Self {
            surface,
            config,
            depth_texture,
            depth_view,
        })
    }

    /// Reconfigures the surface and recreates the depth texture. A zero
    /// width or height (e.g. a minimised window) is ignored.
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(device, &self.config);
        let (depth_texture, depth_view) = create_depth(device, width, height);
        self.depth_texture = depth_texture;
        self.depth_view = depth_view;
    }
}

fn choose_surface_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    formats
        .iter()
        .copied()
        .find(|f| *f == wgpu::TextureFormat::Bgra8UnormSrgb)
        .or_else(|| formats.iter().copied().find(|f| f.is_srgb()))
        .or_else(|| formats.first().copied())
}

fn create_depth(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth texture"),
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
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::TextureFormat as F;

    #[test]
    fn prefers_bgra8_srgb() {
        let formats = [F::Rgba8UnormSrgb, F::Bgra8Unorm, F::Bgra8UnormSrgb];
        assert_eq!(choose_surface_format(&formats), Some(F::Bgra8UnormSrgb));
    }

    #[test]
    fn falls_back_to_any_srgb_then_first() {
        assert_eq!(
            choose_surface_format(&[F::Bgra8Unorm, F::Rgba8UnormSrgb]),
            Some(F::Rgba8UnormSrgb)
        );
        assert_eq!(
            choose_surface_format(&[F::Rgba16Float, F::Bgra8Unorm]),
            Some(F::Rgba16Float)
        );
        assert_eq!(choose_surface_format(&[]), None);
    }
}
