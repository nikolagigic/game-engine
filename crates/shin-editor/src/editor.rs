//! The editor frame: run the egui UI, render the scene into the viewport's
//! [`OffscreenTarget`], and paint the UI over the output view.
//!
//! [`EditorCore`] does one frame given egui input and an output view, so the
//! same code serves the window ([`EditorApp`], a [`Game`] for
//! [`shin::app::run`]) and `--screenshot` ([`screenshot`], which paints into
//! an offscreen target instead of the surface).

use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use egui_dock::DockState;
use shin::app::{AppConfig, Game};
use shin::ecs::{Resources, World};
use shin::gpu::{GpuContext, OFFSCREEN_COLOR_FORMAT, OFFSCREEN_GAMMA_VIEW_FORMAT, OffscreenTarget};
use shin::math::Camera;
use shin::project::Project;
use shin::render::{begin_mesh_pass, render_world};
use winit::window::Window;

use crate::log_sink::LogBuffer;
use crate::orbit::OrbitCamera;
use crate::scene;
use crate::ui::{self, EditorUi, Tab, Viewport};

/// Editor window size in logical pixels, and the `--screenshot` size in
/// pixels (screenshots render at one pixel per point).
pub const WINDOW_SIZE: [u32; 2] = [1600, 900];

/// The scene's offscreen target and the egui texture id that shows it.
struct ViewportTarget {
    target: OffscreenTarget,
    texture: egui::TextureId,
}

/// Editor state that does not depend on having a window.
pub struct EditorCore {
    egui: egui::Context,
    /// Created on the first frame, when the output format is known.
    renderer: Option<egui_wgpu::Renderer>,
    dock: DockState<Tab>,
    world: World,
    resources: Resources,
    orbit: OrbitCamera,
    log: LogBuffer,
    viewport: Viewport,
    viewport_target: Option<ViewportTarget>,
    quit: bool,
}

impl EditorCore {
    /// An editor showing the starter scene of the project in `project_dir`,
    /// drawing its UI with `egui` and its Log panel from `log`.
    pub fn new(
        gpu: &GpuContext,
        egui: egui::Context,
        project_dir: &Path,
        log: LogBuffer,
    ) -> anyhow::Result<Self> {
        let mut world = World::new();
        let mut resources = Resources::new();
        scene::build(&mut world, &mut resources, gpu, project_dir)?;
        Ok(Self {
            egui,
            renderer: None,
            dock: ui::default_layout(),
            world,
            resources,
            orbit: OrbitCamera::default(),
            log,
            viewport: Viewport::default(),
            viewport_target: None,
            quit: false,
        })
    }

    /// Runs one editor frame: the UI pass on `input`, then the scene into
    /// the viewport target (resized to the panel first), then the UI painted
    /// over `output` (loaded, not cleared), whose size is `size_px` and
    /// colour format `format`. Returns what egui wants the platform to do
    /// (cursor icon, clipboard, ...).
    pub fn frame(
        &mut self,
        gpu: &GpuContext,
        input: egui::RawInput,
        output: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        size_px: [u32; 2],
    ) -> egui::PlatformOutput {
        let renderer = self.renderer.get_or_insert_with(|| {
            egui_wgpu::Renderer::new(&gpu.device, format, egui_wgpu::RendererOptions::default())
        });

        let egui_ctx = self.egui.clone();
        let mut full = egui_ctx.run_ui(input, |ui| {
            EditorUi {
                world: &self.world,
                log: &self.log,
                orbit: &mut self.orbit,
                viewport: &mut self.viewport,
                quit: &mut self.quit,
            }
            .show(ui, &mut self.dock);
        });

        sync_viewport_target(&mut self.viewport_target, &mut self.viewport, renderer, gpu);
        if let Some(viewport) = &self.viewport_target {
            self.orbit.apply(self.resources.expect_mut::<Camera>());
            render_scene(&mut self.world, &mut self.resources, gpu, &viewport.target);
        }

        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: size_px,
            pixels_per_point: full.pixels_per_point,
        };
        let jobs = egui_ctx.tessellate(full.shapes, full.pixels_per_point);
        // Drained, not iterated: a `TexturesDelta` dropped with entries left
        // in it asserts in debug builds.
        let mut textures = std::mem::take(&mut full.textures_delta);
        for (id, deltas) in textures.set.drain() {
            for delta in &deltas {
                renderer.update_texture(&gpu.device, &gpu.queue, id, delta);
            }
        }
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("egui encoder"),
            });
        let mut commands =
            renderer.update_buffers(&gpu.device, &gpu.queue, &mut encoder, &jobs, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
        }
        commands.push(encoder.finish());
        gpu.queue.submit(commands);
        for id in textures.free.drain() {
            renderer.free_texture(&id);
        }
        full.platform_output
    }

    /// Whether File > Quit was clicked.
    pub fn quit_requested(&self) -> bool {
        self.quit
    }
}

/// Makes the viewport target match the size the Viewport panel asked for:
/// creates it on first use, recreates it when the panel was resized, and
/// points the panel's egui texture id at its colour texture. Does nothing
/// while the panel has no area (before its first layout, or collapsed).
fn sync_viewport_target(
    slot: &mut Option<ViewportTarget>,
    viewport: &mut Viewport,
    renderer: &mut egui_wgpu::Renderer,
    gpu: &GpuContext,
) {
    let [width, height] = viewport.size_px;
    if width == 0 || height == 0 {
        return;
    }
    if slot
        .as_ref()
        .is_some_and(|v| (v.target.width, v.target.height) == (width, height))
    {
        return;
    }

    let target = OffscreenTarget::new(&gpu.device, width, height);
    // egui samples textures as gamma-encoded bytes: view the sRGB colour
    // texture as plain Rgba8Unorm so the stored bytes reach egui unchanged.
    let view = target.color.create_view(&wgpu::TextureViewDescriptor {
        format: Some(OFFSCREEN_GAMMA_VIEW_FORMAT),
        ..Default::default()
    });
    let filter = wgpu::FilterMode::Linear;
    let texture = match slot.take() {
        Some(old) => {
            renderer.update_egui_texture_from_wgpu_texture(&gpu.device, &view, filter, old.texture);
            old.texture
        }
        None => renderer.register_native_texture(&gpu.device, &view, filter),
    };
    log::debug!("viewport target resized to {width}x{height}");
    viewport.texture = Some(texture);
    *slot = Some(ViewportTarget { target, texture });
}

/// Clears `target` to [`scene::SKY`] and draws the world into it.
fn render_scene(
    world: &mut World,
    resources: &mut Resources,
    gpu: &GpuContext,
    target: &OffscreenTarget,
) {
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("viewport clear encoder"),
        });
    drop(begin_mesh_pass(
        &mut encoder,
        &target.color_view,
        &target.depth_view,
        scene::SKY,
    ));
    gpu.queue.submit([encoder.finish()]);
    let aspect = target.width as f32 / target.height as f32;
    render_world(
        world,
        resources,
        gpu,
        &target.color_view,
        &target.depth_view,
        aspect,
    );
}

/// The window title for `project`.
fn title(project: &Project) -> String {
    format!("{} — Shin Editor", project.window.title)
}

/// The windowed editor: an [`EditorCore`] fed by egui-winit.
pub struct EditorApp {
    project_dir: std::path::PathBuf,
    log: LogBuffer,
    egui: egui::Context,
    window: Option<Arc<Window>>,
    egui_winit: Option<egui_winit::State>,
    core: Option<EditorCore>,
}

impl EditorApp {
    /// An editor for the project in `project_dir`; the window, GPU, and
    /// scene are created when [`shin::app::run`] starts it.
    pub fn new(project_dir: &Path, log: LogBuffer) -> Self {
        Self {
            project_dir: project_dir.to_path_buf(),
            log,
            egui: egui::Context::default(),
            window: None,
            egui_winit: None,
            core: None,
        }
    }
}

impl Game for EditorApp {
    fn window_created(&mut self, window: &Arc<Window>) {
        self.egui_winit = Some(egui_winit::State::new(
            self.egui.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            window.theme(),
            None,
        ));
        self.window = Some(window.clone());
    }

    fn init(&mut self, ctx: &GpuContext) -> anyhow::Result<()> {
        if let Some(state) = &mut self.egui_winit {
            state.set_max_texture_side(ctx.device.limits().max_texture_dimension_2d as usize);
        }
        self.core = Some(EditorCore::new(
            ctx,
            self.egui.clone(),
            &self.project_dir,
            self.log.clone(),
        )?);
        Ok(())
    }

    fn render(
        &mut self,
        ctx: &GpuContext,
        view: &wgpu::TextureView,
        _depth: &wgpu::TextureView,
        _alpha: f64,
    ) {
        let (Some(window), Some(state), Some(core)) =
            (&self.window, &mut self.egui_winit, &mut self.core)
        else {
            return;
        };
        let input = state.take_egui_input(window);
        let texture = view.texture();
        let size = [texture.width(), texture.height()];
        let platform = core.frame(ctx, input, view, texture.format(), size);
        state.handle_platform_output(window, platform);
    }

    fn on_window_event(&mut self, event: &winit::event::WindowEvent) {
        if let (Some(window), Some(state)) = (&self.window, &mut self.egui_winit) {
            // The editor repaints every frame, so the repaint hint is unused;
            // whether egui consumed the event is decided per widget in the UI.
            let _ = state.on_window_event(window, event);
        }
    }

    fn exit_requested(&self) -> bool {
        self.core.as_ref().is_some_and(EditorCore::quit_requested)
    }
}

/// Opens the editor window on the project in `project_dir` and runs until
/// it is closed.
pub fn run(project_dir: &Path, log: LogBuffer) -> anyhow::Result<()> {
    let project = Project::load(project_dir)?;
    let config = AppConfig {
        title: title(&project),
        width: WINDOW_SIZE[0],
        height: WINDOW_SIZE[1],
        ..Default::default()
    };
    shin::app::run(config, EditorApp::new(project_dir, log))
}

/// Renders one full editor frame of the project in `project_dir` without a
/// window, [`WINDOW_SIZE`] pixels at one pixel per point, and saves it as a
/// PNG at `out`.
///
/// The frame is run a few times first so egui and the dock settle their
/// layout and the viewport target exists at its final size.
pub fn screenshot(project_dir: &Path, out: &Path, log: LogBuffer) -> anyhow::Result<()> {
    /// Frames run before the one that is saved.
    const WARM_UP_FRAMES: u32 = 3;

    let project = Project::load(project_dir)?;
    log::info!("rendering a screenshot of `{}`", title(&project));
    let gpu = GpuContext::new(None)?;
    let mut core = EditorCore::new(&gpu, egui::Context::default(), project_dir, log)?;
    let [width, height] = WINDOW_SIZE;
    let target = OffscreenTarget::new(&gpu.device, width, height);
    let screen_rect =
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width as f32, height as f32));
    for frame in 0..=WARM_UP_FRAMES {
        let input = egui::RawInput {
            screen_rect: Some(screen_rect),
            time: Some(f64::from(frame) / 60.0),
            max_texture_side: Some(gpu.device.limits().max_texture_dimension_2d as usize),
            ..Default::default()
        };
        // Like the window loop, start every frame from black.
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("screenshot clear encoder"),
            });
        drop(begin_mesh_pass(
            &mut encoder,
            &target.color_view,
            &target.depth_view,
            wgpu::Color::BLACK,
        ));
        gpu.queue.submit([encoder.finish()]);
        core.frame(
            &gpu,
            input,
            &target.color_view,
            OFFSCREEN_COLOR_FORMAT,
            WINDOW_SIZE,
        );
    }
    target
        .read_pixels(&gpu)?
        .save(out)
        .with_context(|| format!("writing {}", out.display()))?;
    log::info!("screenshot written to {}", out.display());
    Ok(())
}
