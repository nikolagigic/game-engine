//! Window-less engine handle: GPU context, world, schedule, and an offscreen
//! target, driven by tools and tests instead of an event loop. Owned by GE-013.
//!
//! [`Headless`] is what every [`crate::tool::Tool`] operates on. Simulation
//! is advanced explicitly with [`Headless::step`] (no wall clock), and
//! frames are produced on demand with [`Headless::render`], so results are
//! deterministic.

use anyhow::Context as _;

use crate::app::RenderFn;
use crate::ecs::{self, Resources, Schedule, Time, World};
use crate::gpu::{GpuContext, OffscreenTarget};
use crate::stats::{FrameStats, frame_stats_system};

/// A window-less engine: everything [`crate::app::EcsGame`] has, plus a GPU
/// context and an offscreen target instead of a window.
pub struct Headless {
    /// GPU context created without a surface.
    pub gpu: GpuContext,
    /// Entities and components.
    pub world: World,
    /// Singletons shared by systems; always contains a [`Time`].
    pub resources: Resources,
    /// Systems run by [`Self::step`] (fixed) and [`Self::render`] (frame).
    pub schedule: Schedule,
    /// The colour + depth target [`Self::render`] draws into.
    pub target: OffscreenTarget,
    /// Length of one simulation step in seconds; default `1.0 / 60.0`.
    pub fixed_dt: f64,
    /// Colour the target is cleared to before drawing, in linear RGB.
    /// Default black.
    pub clear_color: wgpu::Color,
    /// Optional drawing callback, called by [`Self::render`] after the
    /// clear with the target's colour and depth views. GE-015 installs the renderer
    /// here; with none installed a render is just the clear.
    pub render_fn: Option<RenderFn>,
}

impl Headless {
    /// Creates a GPU context without a surface and a `width` x `height`
    /// offscreen target, with an empty world, default [`Time`] and
    /// [`FrameStats`] resources, and a schedule holding only
    /// [`frame_stats_system`].
    ///
    /// Fails if no GPU adapter is available or a dimension is zero.
    pub fn new(width: u32, height: u32) -> anyhow::Result<Self> {
        anyhow::ensure!(
            width > 0 && height > 0,
            "headless target must be at least 1x1, got {width}x{height}"
        );
        let gpu = GpuContext::new(None)?;
        let target = OffscreenTarget::new(&gpu.device, width, height);
        let mut resources = Resources::new();
        resources.insert(Time::default());
        resources.insert(FrameStats::default());
        let mut schedule = Schedule::new();
        schedule.add_frame("frame_stats", frame_stats_system);
        Ok(Self {
            gpu,
            world: World::new(),
            resources,
            schedule,
            target,
            fixed_dt: 1.0 / 60.0,
            clear_color: wgpu::Color::BLACK,
            render_fn: None,
        })
    }

    /// Recreates the offscreen target at the new size. A zero width or
    /// height is ignored, as with a minimised window.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        if (width, height) != (self.target.width, self.target.height) {
            self.target = OffscreenTarget::new(&self.gpu.device, width, height);
        }
    }

    /// Runs `n` fixed steps of [`Self::fixed_dt`], advancing [`Time`]
    /// exactly as [`crate::app::EcsGame`] does (both use
    /// [`ecs::fixed_step`]).
    pub fn step(&mut self, n: u32) {
        for _ in 0..n {
            ecs::fixed_step(
                &mut self.world,
                &mut self.resources,
                &mut self.schedule,
                self.fixed_dt,
            );
        }
    }

    /// Renders one frame and reads it back.
    ///
    /// Sets `Time::frame_dt` and `Time::alpha` to zero (headless frames
    /// have no wall-clock duration and always land on a whole step), runs
    /// the frame systems, clears the target to [`Self::clear_color`] (depth
    /// to 1.0), then calls [`Self::render_fn`] if one is installed.
    pub fn render(&mut self) -> anyhow::Result<image::RgbaImage> {
        if self.resources.get::<Time>().is_none() {
            self.resources.insert(Time::default());
        }
        let time = self.resources.expect_mut::<Time>();
        time.frame_dt = 0.0;
        time.alpha = 0.0;
        self.schedule
            .run_frame(&mut self.world, &mut self.resources);

        self.clear();
        if let Some(render_fn) = &mut self.render_fn {
            render_fn(
                &mut self.world,
                &mut self.resources,
                &self.gpu,
                &self.target.color_view,
                &self.target.depth_view,
            );
        }
        self.target.read_pixels(&self.gpu)
    }

    /// [`Self::render`], encoded as a PNG.
    pub fn render_png(&mut self) -> anyhow::Result<Vec<u8>> {
        let image = self.render()?;
        let mut png = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .context("failed to encode the frame as PNG")?;
        Ok(png)
    }

    /// Clears the colour target to [`Self::clear_color`] and depth to 1.0.
    fn clear(&self) {
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("headless clear encoder"),
            });
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("headless clear pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.target.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(self.clear_color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.target.depth_view,
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
        self.gpu.queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_recreates_target_and_ignores_zero() -> anyhow::Result<()> {
        let mut engine = Headless::new(8, 8)?;
        engine.resize(16, 4);
        assert_eq!((engine.target.width, engine.target.height), (16, 4));
        engine.resize(0, 10);
        assert_eq!((engine.target.width, engine.target.height), (16, 4));
        assert_eq!(engine.render()?.dimensions(), (16, 4));
        Ok(())
    }

    #[test]
    fn new_rejects_zero_size() {
        assert!(Headless::new(0, 4).is_err());
    }

    fn mark_frame(_: &mut World, res: &mut Resources) {
        res.insert(true);
    }

    #[test]
    fn render_runs_frame_systems_and_render_fn() -> anyhow::Result<()> {
        let mut engine = Headless::new(4, 4)?;
        engine.schedule.add_frame("mark_frame", mark_frame);
        engine.render_fn = Some(Box::new(|_, res, _, _, _| res.insert(7_u8)));
        engine.render()?;
        assert_eq!(engine.resources.get::<bool>(), Some(&true));
        assert_eq!(engine.resources.get::<u8>(), Some(&7));
        Ok(())
    }
}
