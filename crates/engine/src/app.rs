//! Window, event loop, and fixed-timestep game loop. Owned by GE-001.
//!
//! User code implements [`Game`] and hands it to [`run`]. The loop ticks the
//! simulation at a fixed `dt` ([`FixedTimestep`]) and renders once per
//! frame, clearing the surface and depth buffer before calling
//! [`Game::render`].

use std::sync::Arc;
use std::time::Instant;

use anyhow::Context as _;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::gpu::{GpuContext, SurfaceTarget};

/// Longest frame time fed to the simulation, in seconds. Longer frames (a
/// breakpoint, a dragged window) are clamped so the loop never tries to
/// catch up with hundreds of steps.
pub const MAX_FRAME_TIME: f64 = 0.25;

/// Window and loop settings for [`run`].
#[derive(Clone, Debug)]
pub struct AppConfig {
    /// Window title.
    pub title: String,
    /// Initial window width in logical pixels.
    pub width: u32,
    /// Initial window height in logical pixels.
    pub height: u32,
    /// Simulation step in seconds; default `1.0 / 60.0`.
    pub fixed_dt: f64,
    /// Colour the surface is cleared to at the start of every frame, in
    /// linear RGB; the sRGB surface encodes it for display.
    pub clear_color: wgpu::Color,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            title: "engine".to_string(),
            width: 1280,
            height: 720,
            fixed_dt: 1.0 / 60.0,
            clear_color: wgpu::Color::BLACK,
        }
    }
}

/// User code implements this. [`run`] owns the loop and calls these hooks.
pub trait Game {
    /// Called once after the GPU context exists.
    fn init(&mut self, ctx: &GpuContext) -> anyhow::Result<()> {
        let _ = ctx;
        Ok(())
    }

    /// Fixed-timestep simulation step. Called 0..n times per frame.
    fn fixed_update(&mut self, dt: f64) {
        let _ = dt;
    }

    /// Once per frame, after the surface has been cleared. `alpha` in 0..1
    /// is the fraction of a fixed step left in the accumulator, for
    /// interpolating between the last two simulation states.
    fn render(&mut self, ctx: &GpuContext, view: &wgpu::TextureView, alpha: f64) {
        let _ = (ctx, view, alpha);
    }

    /// Every window event, before the engine handles it.
    fn on_window_event(&mut self, event: &winit::event::WindowEvent) {
        let _ = event;
    }

    /// Polled once per frame; returning `Some` sets the window title.
    fn title(&self) -> Option<String> {
        None
    }
}

/// Result of [`FixedTimestep::advance`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepResult {
    /// How many fixed steps to run this frame.
    pub steps: u32,
    /// Leftover fraction of a step, in `[0, 1)`.
    pub alpha: f64,
}

/// Accumulator for a fixed-timestep simulation with variable-rate rendering.
#[derive(Clone, Debug)]
pub struct FixedTimestep {
    fixed_dt: f64,
    accumulator: f64,
}

impl FixedTimestep {
    /// Tolerance so a frame that is an exact multiple of `dt` is not short a
    /// step because of floating-point rounding in the accumulator.
    const EPSILON: f64 = 1e-9;

    /// Creates an accumulator stepping at `fixed_dt` seconds.
    ///
    /// # Panics
    ///
    /// If `fixed_dt` is not positive.
    pub fn new(fixed_dt: f64) -> Self {
        assert!(fixed_dt > 0.0, "fixed_dt must be positive, got {fixed_dt}");
        Self {
            fixed_dt,
            accumulator: 0.0,
        }
    }

    /// The fixed step in seconds.
    pub fn fixed_dt(&self) -> f64 {
        self.fixed_dt
    }

    /// Adds `frame_time` seconds (clamped to `0..=`[`MAX_FRAME_TIME`]) and
    /// returns how many fixed steps are due and the interpolation alpha.
    pub fn advance(&mut self, frame_time: f64) -> StepResult {
        self.accumulator += frame_time.clamp(0.0, MAX_FRAME_TIME);
        let mut steps = 0;
        while self.accumulator + Self::EPSILON >= self.fixed_dt {
            self.accumulator -= self.fixed_dt;
            steps += 1;
        }
        self.accumulator = self.accumulator.max(0.0);
        let alpha = self.accumulator / self.fixed_dt;
        StepResult { steps, alpha }
    }
}

/// Opens a window and runs `game` until the window is closed or Escape is
/// pressed.
pub fn run<G: Game + 'static>(config: AppConfig, game: G) -> anyhow::Result<()> {
    let event_loop = EventLoop::new().context("failed to create event loop")?;
    let mut runner = Runner {
        timestep: FixedTimestep::new(config.fixed_dt),
        config,
        game,
        state: None,
        last_frame: None,
        title: None,
        error: None,
    };
    event_loop
        .run_app(&mut runner)
        .context("event loop failed")?;
    match runner.error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Everything that only exists while a window is open.
struct Running {
    window: Arc<Window>,
    gpu: GpuContext,
    target: SurfaceTarget<'static>,
}

struct Runner<G> {
    config: AppConfig,
    game: G,
    timestep: FixedTimestep,
    state: Option<Running>,
    last_frame: Option<Instant>,
    title: Option<String>,
    error: Option<anyhow::Error>,
}

impl<G: Game> Runner<G> {
    fn start(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        let attributes = Window::default_attributes()
            .with_title(self.config.title.clone())
            .with_inner_size(LogicalSize::new(self.config.width, self.config.height))
            .with_resizable(true);
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .context("failed to create window")?,
        );

        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(
                Box::new(event_loop.owned_display_handle()),
            ));
        let surface = instance
            .create_surface(window.clone())
            .context("failed to create surface")?;
        let gpu = GpuContext::with_instance(instance, Some(&surface))?;
        let size = window.inner_size();
        let target = SurfaceTarget::new(&gpu, surface, size.width, size.height)?;

        self.game.init(&gpu)?;
        window.request_redraw();
        self.state = Some(Running {
            window,
            gpu,
            target,
        });
        Ok(())
    }

    fn frame(&mut self) -> anyhow::Result<()> {
        let Some(state) = &mut self.state else {
            return Ok(());
        };

        let now = Instant::now();
        let frame_time = self
            .last_frame
            .map_or(0.0, |last| (now - last).as_secs_f64());
        self.last_frame = Some(now);

        let step = self.timestep.advance(frame_time);
        for _ in 0..step.steps {
            self.game.fixed_update(self.timestep.fixed_dt());
        }

        let (surface_texture, suboptimal) = match state.target.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => (t, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => (t, true),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                state.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                let (w, h) = (state.target.config.width, state.target.config.height);
                state.target.resize(&state.gpu.device, w, h);
                state.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                log::warn!("surface lost; recreating it");
                let surface = state
                    .gpu
                    .instance
                    .create_surface(state.window.clone())
                    .context("failed to recreate surface")?;
                let size = state.window.inner_size();
                state.target = SurfaceTarget::new(&state.gpu, surface, size.width, size.height)?;
                state.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("validation error while acquiring the surface texture")
            }
        };
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        clear(
            &state.gpu,
            &view,
            &state.target.depth_view,
            self.config.clear_color,
        );
        self.game.render(&state.gpu, &view, step.alpha);

        state.window.pre_present_notify();
        state.gpu.queue.present(surface_texture);
        if suboptimal {
            let (w, h) = (state.target.config.width, state.target.config.height);
            state.target.resize(&state.gpu.device, w, h);
        }

        if let Some(title) = self.game.title()
            && self.title.as_ref() != Some(&title)
        {
            state.window.set_title(&title);
            self.title = Some(title);
        }
        state.window.request_redraw();
        Ok(())
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, err: anyhow::Error) {
        log::error!("{err:#}");
        self.error = Some(err);
        event_loop.exit();
    }
}

impl<G: Game> ApplicationHandler for Runner<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none()
            && let Err(err) = self.start(event_loop)
        {
            self.fail(event_loop, err);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        self.game.on_window_event(&event);
        match event {
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(state) = &mut self.state {
                    state
                        .target
                        .resize(&state.gpu.device, size.width, size.height);
                    state.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(err) = self.frame() {
                    self.fail(event_loop, err);
                }
            }
            _ => {}
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.state = None;
    }
}

/// Clears `color` to `clear_color` and `depth` to 1.0 (the far plane).
fn clear(
    gpu: &GpuContext,
    color: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    clear_color: wgpu::Color,
) {
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("clear encoder"),
        });
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("clear pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear_color),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
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
    gpu.queue.submit([encoder.finish()]);
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f64 = 1.0 / 60.0;

    #[test]
    fn two_steps_for_double_dt() {
        let mut ts = FixedTimestep::new(DT);
        let r = ts.advance(1.0 / 30.0);
        assert_eq!(r.steps, 2);
        assert!(r.alpha.abs() < 1e-6, "alpha = {}", r.alpha);
    }

    #[test]
    fn long_frames_are_clamped() {
        let mut ts = FixedTimestep::new(DT);
        let r = ts.advance(0.4);
        assert_eq!(r.steps, 15);
        assert!((0.0..1.0).contains(&r.alpha), "alpha = {}", r.alpha);
    }

    #[test]
    fn partial_steps_accumulate() {
        let mut ts = FixedTimestep::new(DT);
        let r = ts.advance(DT / 2.0);
        assert_eq!(r.steps, 0);
        assert!((r.alpha - 0.5).abs() < 1e-9);
        let r = ts.advance(DT / 2.0);
        assert_eq!(r.steps, 1);
    }

    #[test]
    fn negative_frame_time_is_ignored() {
        let mut ts = FixedTimestep::new(DT);
        assert_eq!(ts.advance(-1.0).steps, 0);
    }

    #[test]
    fn alpha_always_in_unit_range() {
        let mut ts = FixedTimestep::new(DT);
        let frame_times = [0.0, 0.001, 0.007, 1.0 / 144.0, DT, 0.0213, 0.05, 0.3, 1.0];
        for i in 0..1000 {
            let r = ts.advance(frame_times[i % frame_times.len()]);
            assert!(
                (0.0..1.0).contains(&r.alpha),
                "alpha {} out of range",
                r.alpha
            );
        }
    }
}
