//! The editor's UI: a File menu stub above an `egui_dock` area with four
//! panels: Viewport (centre), Scene (left), Inspector (right), and Log
//! (below the viewport).
//!
//! The panels are placeholders that later tickets fill; the Viewport shows
//! the scene texture and turns pointer input into orbit-camera moves.

use egui::{Color32, PointerButton, Rect, Sense, TextureId, Ui, WidgetText, pos2};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use shin::ecs::World;
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
}

/// The initial layout: Scene takes the left 18%, Inspector the right 22%
/// of what remains, and Log the bottom quarter under the Viewport.
///
/// An `egui_dock` split fraction is the share of the left (or top) child,
/// whichever side the new tabs go on.
pub fn default_layout() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Viewport]);
    let surface = dock.main_surface_mut();
    let [centre, _scene] = surface.split_left(NodeIndex::root(), 0.18, vec![Tab::Scene]);
    let [centre, _inspector] = surface.split_right(centre, 0.78, vec![Tab::Inspector]);
    let [_viewport, _log] = surface.split_below(centre, 0.75, vec![Tab::Log]);
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
    /// The world the Scene panel lists.
    pub world: &'a World,
    /// The lines the Log panel shows.
    pub log: &'a LogBuffer,
    /// The camera viewport input drives.
    pub orbit: &'a mut OrbitCamera,
    /// The viewport's texture and requested size.
    pub viewport: &'a mut Viewport,
    /// Set when File > Quit is clicked.
    pub quit: &'a mut bool,
}

impl EditorUi<'_> {
    /// Draws the whole editor into `ui`, which covers the window.
    pub fn show(&mut self, ui: &mut Ui, dock: &mut DockState<Tab>) {
        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        *self.quit = true;
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
        for entity in self.world.iter() {
            ui.label(format!("Entity {}", entity.entity().id()));
        }
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
        }
    }

    /// No panel can be closed: there is no menu to bring one back yet.
    fn is_closeable(&self, _tab: &Tab) -> bool {
        false
    }

    /// The viewport fills its panel exactly (scroll bars would let it grow
    /// and would swallow the zoom wheel); the Log scrolls itself so it can
    /// stick to the newest line.
    fn scroll_bars(&self, tab: &Tab) -> [bool; 2] {
        match tab {
            Tab::Viewport | Tab::Log => [false, false],
            Tab::Scene | Tab::Inspector => [true, true],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_has_all_four_panels() {
        let dock = default_layout();
        for tab in [Tab::Viewport, Tab::Scene, Tab::Inspector, Tab::Log] {
            assert!(dock.find_tab(&tab).is_some(), "{tab:?} is docked");
        }
    }
}
