//! The editor frame: run the egui UI, render the scene into the viewport's
//! [`OffscreenTarget`], and paint the UI over the output view.
//!
//! [`EditorCore`] owns a [`Headless`] engine (on a clone of the window's
//! GPU context) and a [`ToolRegistry`]; every edit (console line, undo,
//! redo) goes through the engine's command history ([`shin::command`]).
//! It does one frame given egui input and an output view, so the
//! same code serves the window ([`EditorApp`], a [`Game`] for
//! [`shin::app::run`]) and `--screenshot` ([`screenshot`], which paints into
//! an offscreen target instead of the surface).

use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use egui_dock::DockState;
use shin::app::{AppConfig, Game};
use shin::command;
use shin::gpu::{GpuContext, OFFSCREEN_COLOR_FORMAT, OFFSCREEN_GAMMA_VIEW_FORMAT, OffscreenTarget};
use shin::headless::Headless;
use shin::math::Camera;
use shin::project::Project;
use shin::render::begin_mesh_pass;
use shin::tool::{ToolOutput, ToolRegistry};
use winit::window::Window;

use crate::log_sink::LogBuffer;
use crate::orbit::OrbitCamera;
use crate::scene;
use crate::ui::{self, Action, Console, EditorUi, Tab, Viewport};

/// Editor window size in logical pixels, and the `--screenshot` size in
/// pixels (screenshots render at one pixel per point).
pub const WINDOW_SIZE: [u32; 2] = [1600, 900];

/// The egui texture showing the engine's offscreen target, and the target
/// size it was registered for.
struct ViewportTexture {
    size: [u32; 2],
    texture: egui::TextureId,
}

/// Editor state that does not depend on having a window.
pub struct EditorCore {
    egui: egui::Context,
    /// Created on the first frame, when the output format is known.
    renderer: Option<egui_wgpu::Renderer>,
    dock: DockState<Tab>,
    /// The scene: world, resources, command history, offscreen target.
    engine: Headless,
    /// The tools the console and menus call.
    registry: ToolRegistry,
    orbit: OrbitCamera,
    log: LogBuffer,
    viewport: Viewport,
    viewport_texture: Option<ViewportTexture>,
    console: Console,
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
        // 1x1 until the Viewport panel has been laid out and asks for a size.
        let mut engine = Headless::from_gpu(gpu.clone(), 1, 1)?;
        scene::build(&mut engine, project_dir)?;
        Ok(Self {
            egui,
            renderer: None,
            dock: ui::default_layout(),
            engine,
            registry: ToolRegistry::with_engine_tools(),
            orbit: OrbitCamera::default(),
            log,
            viewport: Viewport::default(),
            viewport_texture: None,
            console: Console::default(),
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
        let egui_ctx = self.egui.clone();
        let mut actions = Vec::new();
        let mut full = egui_ctx.run_ui(input, |ui| {
            EditorUi {
                engine: &self.engine,
                log: &self.log,
                orbit: &mut self.orbit,
                viewport: &mut self.viewport,
                console: &mut self.console,
                actions: &mut actions,
                quit: &mut self.quit,
            }
            .show(ui, &mut self.dock);
        });
        for action in actions {
            self.run(action);
        }

        let renderer = self.renderer.get_or_insert_with(|| {
            egui_wgpu::Renderer::new(&gpu.device, format, egui_wgpu::RendererOptions::default())
        });

        sync_viewport_texture(
            &mut self.viewport_texture,
            &mut self.viewport,
            &mut self.engine,
            renderer,
        );
        if self.viewport_texture.is_some() {
            self.orbit
                .apply(self.engine.resources.expect_mut::<Camera>());
            self.engine.render_to_target();
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

    /// Carries out what the UI asked for. Results and errors go to the log
    /// and, for console lines, to the console transcript.
    fn run(&mut self, action: Action) {
        match action {
            Action::Undo => match command::undo(&mut self.engine) {
                Ok(true) => log::info!("undo"),
                Ok(false) => log::info!("nothing to undo"),
                Err(err) => log::error!("{err:#}"),
            },
            Action::Redo => match command::redo(&mut self.engine) {
                Ok(true) => log::info!("redo"),
                Ok(false) => log::info!("nothing to redo"),
                Err(err) => log::error!("{err:#}"),
            },
            Action::Console(line) => {
                let reply = self.run_console_line(&line);
                match &reply {
                    Ok(text) => log::info!("{line} -> {text}"),
                    Err(text) => log::error!("{line} -> {text}"),
                }
                self.console.record(line, reply);
            }
        }
    }

    /// Runs `<tool.name> [json args]` through the command history and
    /// describes the result.
    fn run_console_line(&mut self, line: &str) -> Result<String, String> {
        let (tool, args) = ui::parse_console_line(line)?;
        let output = command::apply(&self.registry, &mut self.engine, tool, args)
            .map_err(|err| format!("{err:#}"))?;
        Ok(match output {
            ToolOutput::Text(text) => text,
            ToolOutput::Json(value) => value.to_string(),
            ToolOutput::Image { width, height, .. } => format!("<{width}x{height} PNG>"),
        })
    }

    /// Whether File > Quit was clicked.
    pub fn quit_requested(&self) -> bool {
        self.quit
    }
}

/// Makes the engine's offscreen target match the size the Viewport panel
/// asked for and points the panel's egui texture id at its colour texture,
/// re-registering it whenever the target was recreated. Does nothing while
/// the panel has no area (before its first layout, or collapsed).
fn sync_viewport_texture(
    slot: &mut Option<ViewportTexture>,
    viewport: &mut Viewport,
    engine: &mut Headless,
    renderer: &mut egui_wgpu::Renderer,
) {
    let [width, height] = viewport.size_px;
    if width == 0 || height == 0 {
        return;
    }
    if slot.as_ref().is_some_and(|v| v.size == [width, height]) {
        return;
    }

    engine.resize(width, height);
    let gpu = &engine.gpu;
    // egui samples textures as gamma-encoded bytes: view the sRGB colour
    // texture as plain Rgba8Unorm so the stored bytes reach egui unchanged.
    let view = engine
        .target
        .color
        .create_view(&wgpu::TextureViewDescriptor {
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
    *slot = Some(ViewportTexture {
        size: [width, height],
        texture,
    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use shin::project::{self, NewProjectOptions};

    /// One frame at [`WINDOW_SIZE`] with `events` as the input.
    fn frame(
        core: &mut EditorCore,
        gpu: &GpuContext,
        target: &OffscreenTarget,
        events: Vec<egui::Event>,
    ) {
        let [width, height] = WINDOW_SIZE;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width as f32, height as f32),
            )),
            events,
            ..Default::default()
        };
        core.frame(
            gpu,
            input,
            &target.color_view,
            OFFSCREEN_COLOR_FORMAT,
            WINDOW_SIZE,
        );
    }

    /// Ctrl+Z (Cmd+Z on macOS), with `shift` for redo.
    fn undo_key(shift: bool) -> egui::Event {
        let mut modifiers = egui::Modifiers::COMMAND;
        modifiers.shift = shift;
        egui::Event::Key {
            key: egui::Key::Z,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// The ticket's manual check, automated: `entity.spawn` from the console
    /// adds an entity, Ctrl+Z removes it, Ctrl+Shift+Z brings it back.
    #[test]
    fn ctrl_z_undoes_a_console_spawn() -> anyhow::Result<()> {
        let root =
            std::env::temp_dir().join(format!("shin-editor-undo-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let project_dir = root.join("demo");
        project::create(&NewProjectOptions {
            name: "demo".to_string(),
            dir: project_dir.clone(),
            engine: Default::default(),
        })?;

        let gpu = GpuContext::new(None)?;
        let mut core = EditorCore::new(
            &gpu,
            egui::Context::default(),
            &project_dir,
            LogBuffer::default(),
        )?;
        let [width, height] = WINDOW_SIZE;
        let target = OffscreenTarget::new(&gpu.device, width, height);
        frame(&mut core, &gpu, &target, Vec::new());
        let starter = core.engine.world.len();

        core.run(Action::Console(
            r#"entity.spawn {"mesh": 0, "translation": [0, 2, 0]}"#.to_string(),
        ));
        assert!(
            matches!(core.console.transcript.last(), Some((_, Ok(_)))),
            "{:?}",
            core.console.transcript
        );
        assert_eq!(core.engine.world.len(), starter + 1);

        frame(&mut core, &gpu, &target, vec![undo_key(false)]);
        assert_eq!(core.engine.world.len(), starter, "Ctrl+Z removes it");

        frame(&mut core, &gpu, &target, vec![undo_key(true)]);
        assert_eq!(
            core.engine.world.len(),
            starter + 1,
            "Ctrl+Shift+Z restores it"
        );

        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }
}
