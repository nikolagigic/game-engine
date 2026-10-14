//! The editor's UI: File and Edit menus above an `egui_dock` area with six
//! panels: Viewport (centre), Scene (left), Inspector and History (right),
//! and Log and Console (below the viewport).
//!
//! The Viewport shows the scene texture and turns pointer input into
//! orbit-camera moves. Edits are not made here: the UI pushes [`Action`]s
//! (undo, redo, a console line) that [`crate::editor::EditorCore`] runs
//! after the UI pass. Ctrl+Z (Cmd+Z on macOS) undoes and Ctrl+Shift+Z
//! redoes, except while the console input holds text.

use egui::{
    Color32, Key, KeyboardShortcut, Modifiers, PointerButton, Rect, Sense, TextureId, Ui,
    WidgetText, pos2,
};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use serde_json::Value;
use shin::command::History;
use shin::headless::Headless;
use shin::math::Vec2;

use crate::log_sink::LogBuffer;
use crate::orbit::OrbitCamera;

/// One dockable panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    /// The live scene, rendered offscreen and shown as an image.
    Viewport,
    /// The entities in the world.
    Scene,
    /// Properties of the selected entity.
    Inspector,
    /// The most recent log lines.
    Log,
    /// The command history, oldest first (read-only).
    History,
    /// A one-line prompt that runs tools: `<tool.name> [json args]`.
    Console,
}

/// Something the UI asks [`crate::editor::EditorCore`] to do after the UI
/// pass, when the engine can be borrowed mutably.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Undo the most recent edit.
    Undo,
    /// Redo the most recently undone edit.
    Redo,
    /// Run a console line.
    Console(String),
}

/// The Console panel's state: the line being typed and past lines with
/// their results.
#[derive(Debug, Default)]
pub struct Console {
    /// The text in the input field.
    pub input: String,
    /// `(line, result)` of every line run, oldest first; `Err` holds the
    /// error message.
    pub transcript: Vec<(String, Result<String, String>)>,
}

impl Console {
    /// Appends a line that was run and its result to the transcript.
    pub fn record(&mut self, line: String, result: Result<String, String>) {
        self.transcript.push((line, result));
    }
}

/// Splits a console line into a tool name and its JSON arguments; with no
/// arguments the tool gets `{}`.
///
/// `entity.spawn {"mesh": 0}` gives `("entity.spawn", {"mesh": 0})`.
pub fn parse_console_line(line: &str) -> Result<(&str, Value), String> {
    let line = line.trim();
    let (tool, args) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    if tool.is_empty() {
        return Err("type a tool name, e.g. engine.list_tools".to_string());
    }
    let args = args.trim();
    let args = if args.is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_str(args).map_err(|err| format!("arguments are not JSON: {err}"))?
    };
    Ok((tool, args))
}

/// The initial layout: Scene takes the left 18%, Inspector the right 22%
/// of what remains with History under it, and Log and Console share the
/// bottom quarter under the Viewport side by side.
///
/// An `egui_dock` split fraction is the share of the left (or top) child,
/// whichever side the new tabs go on.
pub fn default_layout() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Viewport]);
    let surface = dock.main_surface_mut();
    let [centre, _scene] = surface.split_left(NodeIndex::root(), 0.18, vec![Tab::Scene]);
    let [centre, inspector] = surface.split_right(centre, 0.78, vec![Tab::Inspector]);
    let [_inspector, _history] = surface.split_below(inspector, 0.55, vec![Tab::History]);
    let [_viewport, log] = surface.split_below(centre, 0.75, vec![Tab::Log]);
    let [_log, _console] = surface.split_right(log, 0.5, vec![Tab::Console]);
    dock
}

/// What the Viewport panel shows and what it asks for.
#[derive(Debug, Default)]
pub struct Viewport {
    /// The scene texture to draw, once registered with the egui renderer.
    pub texture: Option<TextureId>,
    /// The panel's size in physical pixels, measured during the last UI
    /// pass; the offscreen target is resized to match. Zero before the
    /// panel has been laid out.
    pub size_px: [u32; 2],
}

/// Everything the panels read or change during one UI pass.
pub struct EditorUi<'a> {
    /// The engine whose world the Scene panel lists and whose command
    /// history the History panel shows.
    pub engine: &'a Headless,
    /// The lines the Log panel shows.
    pub log: &'a LogBuffer,
    /// The camera viewport input drives.
    pub orbit: &'a mut OrbitCamera,
    /// The viewport's texture and requested size.
    pub viewport: &'a mut Viewport,
    /// The Console panel's input and transcript.
    pub console: &'a mut Console,
    /// What to run after the UI pass.
    pub actions: &'a mut Vec<Action>,
    /// Set when File > Quit is clicked.
    pub quit: &'a mut bool,
}

/// Ctrl+Z (Cmd+Z on macOS).
const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);

/// Ctrl+Shift+Z (Cmd+Shift+Z on macOS).
const REDO: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);

impl EditorUi<'_> {
    /// Draws the whole editor into `ui`, which covers the window.
    pub fn show(&mut self, ui: &mut Ui, dock: &mut DockState<Tab>) {
        self.shortcuts(ui);
        let history = self.engine.resources.get::<History>();
        let can_undo = history.is_some_and(|h| h.undo_len() > 0);
        let can_redo = history.is_some_and(|h| h.redo_len() > 0);
        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        *self.quit = true;
                    }
                });
                ui.menu_button("Edit", |ui| {
                    let undo =
                        egui::Button::new("Undo").shortcut_text(ui.ctx().format_shortcut(&UNDO));
                    if ui.add_enabled(can_undo, undo).clicked() {
                        self.actions.push(Action::Undo);
                    }
                    let redo =
                        egui::Button::new("Redo").shortcut_text(ui.ctx().format_shortcut(&REDO));
                    if ui.add_enabled(can_redo, redo).clicked() {
                        self.actions.push(Action::Redo);
                    }
                });
            });
        });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                DockArea::new(dock)
                    .style(Style::from_egui(ui.style()))
                    .show_close_buttons(false)
                    .show_leaf_close_all_buttons(false)
                    .show_leaf_collapse_buttons(false)
                    .show_inside(ui, self);
            });
    }

    /// Undo and redo shortcuts. They are left to the text field while one
    /// has focus and holds text, so typing in the console keeps its own
    /// text undo; in an empty console (right after running a line) they
    /// undo and redo edits.
    fn shortcuts(&mut self, ui: &mut Ui) {
        if ui.ctx().egui_wants_keyboard_input() && !self.console.input.is_empty() {
            return;
        }
        // Redo first: consuming Ctrl+Z would also match Ctrl+Shift+Z.
        if ui.input_mut(|input| input.consume_shortcut(&REDO)) {
            self.actions.push(Action::Redo);
        } else if ui.input_mut(|input| input.consume_shortcut(&UNDO)) {
            self.actions.push(Action::Undo);
        }
    }

    fn viewport_ui(&mut self, ui: &mut Ui) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let ppp = ui.ctx().pixels_per_point();
        self.viewport.size_px = [
            (rect.width() * ppp).round().max(0.0) as u32,
            (rect.height() * ppp).round().max(0.0) as u32,
        ];
        if let Some(texture) = self.viewport.texture {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            ui.painter().image(texture, rect, uv, Color32::WHITE);
        }

        // Drags only count when they started on the viewport, and hover is
        // false while a menu, popup, or another panel is under the pointer,
        // so panel interaction never moves the camera.
        let drag = response.drag_delta();
        let drag = Vec2::new(drag.x, drag.y);
        if response.dragged_by(PointerButton::Primary) {
            self.orbit.orbit(drag);
        } else if response.dragged_by(PointerButton::Secondary) {
            self.orbit.pan(drag);
        }
        if response.hovered() {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.orbit.zoom(scroll);
            }
        }
    }

    fn scene_ui(&self, ui: &mut Ui) {
        for entity in self.engine.world.iter() {
            ui.label(format!("Entity {}", entity.entity().id()));
        }
    }

    /// The undoable edits, oldest first, then the undone ones greyed out.
    fn history_ui(&self, ui: &mut Ui) {
        let history = self.engine.resources.get::<History>();
        if history.is_none_or(|h| h.undo_len() + h.redo_len() == 0) {
            ui.weak("No edits yet");
        }
        let Some(history) = history else {
            return;
        };
        for (tool, args) in history.undo_entries() {
            ui.label(egui::RichText::new(format!("{tool} {args}")).monospace());
        }
        for (tool, args) in history.redo_entries().rev() {
            ui.weak(egui::RichText::new(format!("{tool} {args}")).monospace());
        }
    }

    /// Past lines with their results above a one-line input; Enter runs it.
    fn console_ui(&mut self, ui: &mut Ui) {
        egui::Panel::bottom("console_input")
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let input = egui::TextEdit::singleline(&mut self.console.input)
                    .hint_text("tool.name {\"json\": \"args\"}")
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY);
                let response = ui.add(input);
                if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    let line = std::mem::take(&mut self.console.input);
                    if !line.trim().is_empty() {
                        self.actions.push(Action::Console(line));
                    }
                    response.request_focus();
                }
            });
        egui::ScrollArea::both()
            .auto_shrink(false)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for (line, result) in &self.console.transcript {
                    ui.label(egui::RichText::new(format!("> {line}")).monospace());
                    let (text, color) = match result {
                        Ok(text) => (text, ui.visuals().text_color()),
                        Err(text) => (text, ui.visuals().error_fg_color),
                    };
                    ui.add(
                        egui::Label::new(egui::RichText::new(text).monospace().color(color))
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                }
            });
    }

    /// The log lines, scrolled to the newest unless the user scrolls up.
    fn log_ui(&self, ui: &mut Ui) {
        egui::ScrollArea::both()
            .auto_shrink(false)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in self.log.lines() {
                    ui.add(
                        egui::Label::new(egui::RichText::new(line).monospace())
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                }
            });
    }
}

impl TabViewer for EditorUi<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Tab) -> WidgetText {
        format!("{tab:?}").into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        match tab {
            Tab::Viewport => self.viewport_ui(ui),
            Tab::Scene => self.scene_ui(ui),
            Tab::Inspector => {
                ui.label("Select an entity");
            }
            Tab::Log => self.log_ui(ui),
            Tab::History => self.history_ui(ui),
            Tab::Console => self.console_ui(ui),
        }
    }

    /// No panel can be closed: there is no menu to bring one back yet.
    fn is_closeable(&self, _tab: &Tab) -> bool {
        false
    }

    /// The viewport fills its panel exactly (scroll bars would let it grow
    /// and would swallow the zoom wheel); the Log and Console scroll
    /// themselves so they can stick to the newest line.
    fn scroll_bars(&self, tab: &Tab) -> [bool; 2] {
        match tab {
            Tab::Viewport | Tab::Log | Tab::Console => [false, false],
            Tab::Scene | Tab::Inspector | Tab::History => [true, true],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_has_all_panels() {
        let dock = default_layout();
        for tab in [
            Tab::Viewport,
            Tab::Scene,
            Tab::Inspector,
            Tab::Log,
            Tab::History,
            Tab::Console,
        ] {
            assert!(dock.find_tab(&tab).is_some(), "{tab:?} is docked");
        }
    }

    #[test]
    fn console_lines_parse_into_tool_and_args() {
        assert_eq!(
            parse_console_line("  entity.spawn {\"mesh\": 0} "),
            Ok(("entity.spawn", serde_json::json!({ "mesh": 0 })))
        );
        assert_eq!(
            parse_console_line("history.undo"),
            Ok(("history.undo", serde_json::json!({})))
        );
        assert!(parse_console_line("   ").is_err());
        let err = parse_console_line("entity.spawn {mesh: 0}").unwrap_err();
        assert!(err.contains("not JSON"), "{err}");
    }
}
