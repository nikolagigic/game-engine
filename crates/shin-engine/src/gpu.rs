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

/// Colour format of every [`OffscreenTarget`]. sRGB, so shaders and clear
/// colours are linear and the hardware encodes on write; read-back bytes
/// are sRGB-encoded RGBA8, ready to save as PNG.
pub const OFFSCREEN_COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// The non-sRGB twin of [`OFFSCREEN_COLOR_FORMAT`], allowed as a view format
/// of [`OffscreenTarget::color`]. Sampling through such a view returns the
/// stored sRGB-encoded bytes unchanged, which is what UI renderers that
/// work in gamma space (egui) expect of a texture.
pub const OFFSCREEN_GAMMA_VIEW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A window-less render target: a colour texture that can be copied back to
/// the CPU, plus a matching depth texture. Used by golden-image tests,
/// headless rendering, and the editor viewport.
pub struct OffscreenTarget {
    /// Colour texture in [`OFFSCREEN_COLOR_FORMAT`], usable as a render
    /// attachment, a copy source, and a sampled texture (also through an
    /// [`OFFSCREEN_GAMMA_VIEW_FORMAT`] view).
    pub color: wgpu::Texture,
    /// View of [`Self::color`], used as the colour attachment.
    pub color_view: wgpu::TextureView,
    /// Depth texture in [`DEPTH_FORMAT`].
    pub depth: wgpu::Texture,
    /// View of [`Self::depth`], used as the depth attachment.
    pub depth_view: wgpu::TextureView,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl OffscreenTarget {
    /// Creates a `width` x `height` colour + depth target. Both dimensions
    /// must be non-zero.
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen color"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OFFSCREEN_COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[OFFSCREEN_GAMMA_VIEW_FORMAT],
        });
        let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let (depth, depth_view) = create_depth(device, width, height);
        Self {
            color,
            color_view,
            depth,
            depth_view,
            width,
            height,
        }
    }

    /// Copies the colour texture into a staging buffer, maps it, and returns
    /// tightly packed RGBA8 pixels (row padding removed).
    ///
    /// Blocks until all previously submitted GPU work has finished. The
    /// bytes are sRGB-encoded, exactly as stored in the texture.
    pub fn read_pixels(&self, ctx: &GpuContext) -> anyhow::Result<image::RgbaImage> {
        const BYTES_PER_PIXEL: u32 = 4;
        let unpadded_row = self.width * BYTES_PER_PIXEL;
        let padded_row = padded_bytes_per_row(unpadded_row);

        // wgpu requires each copied row to start on a 256-byte boundary, so
        // the staging buffer is padded per row; we strip it again below.
        let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("offscreen readback"),
            size: u64::from(padded_row) * u64::from(self.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("offscreen readback encoder"),
            });
        encoder.copy_texture_to_buffer(
            self.color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(self.height),
                },
            },
            self.color.size(),
        );
        ctx.queue.submit([encoder.finish()]);

        let (sender, receiver) = std::sync::mpsc::channel();
        staging.map_async(wgpu::MapMode::Read, .., move |result| {
            // The receiver outlives this callback; a send error is impossible.
            let _ = sender.send(result);
        });
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .context("waiting for the GPU to finish the readback copy")?;
        receiver
            .recv()
            .context("readback map callback was dropped")?
            .context("failed to map the readback buffer")?;

        let mut pixels = Vec::with_capacity((unpadded_row * self.height) as usize);
        {
            let mapped = staging
                .get_mapped_range(..)
                .context("failed to view the mapped readback buffer")?;
            for row in mapped.chunks_exact(padded_row as usize) {
                pixels.extend_from_slice(&row[..unpadded_row as usize]);
            }
        }
        staging.unmap();

        image::RgbaImage::from_raw(self.width, self.height, pixels)
            .context("readback size does not match the target dimensions")
    }
}

/// Rounds `unpadded` up to wgpu's `COPY_BYTES_PER_ROW_ALIGNMENT` (256).
fn padded_bytes_per_row(unpadded: u32) -> u32 {
    unpadded.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::TextureFormat as F;

    #[test]
    fn rows_are_padded_to_256_bytes() {
        assert_eq!(padded_bytes_per_row(64 * 4), 256);
        assert_eq!(padded_bytes_per_row(70 * 4), 512);
        assert_eq!(padded_bytes_per_row(1), 256);
        assert_eq!(padded_bytes_per_row(512), 512);
    }

    /// Clears `target` to a linear colour and submits the pass.
    fn clear_target(ctx: &GpuContext, target: &OffscreenTarget, color: wgpu::Color) {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("test clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        ctx.queue.submit([encoder.finish()]);
    }

    /// 70 px * 4 B = 280 B per row, which wgpu pads to 512; the readback
    /// must strip that padding.
    #[test]
    fn read_pixels_handles_unaligned_width() -> anyhow::Result<()> {
        let ctx = GpuContext::new(None)?;
        let target = OffscreenTarget::new(&ctx.device, 70, 50);
        // Linear 0 and 1 encode to exactly 0 and 255 in sRGB.
        clear_target(
            &ctx,
            &target,
            wgpu::Color {
                r: 1.0,
                g: 0.0,
                b: 1.0,
                a: 1.0,
            },
        );

        let image = target.read_pixels(&ctx)?;
        assert_eq!(image.dimensions(), (70, 50));
        let expected = image::Rgba([255, 0, 255, 255]);
        for (x, y, pixel) in image.enumerate_pixels() {
            assert_eq!(*pixel, expected, "pixel ({x}, {y})");
        }
        Ok(())
    }

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
