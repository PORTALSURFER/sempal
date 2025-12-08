//! egui renderer for the application UI.

mod chrome;
mod collections_panel;
mod drag_overlay;
mod helpers;
mod hotkey_overlay;
mod sample_browser_panel;
mod sample_menus;
mod sources_panel;
pub mod style;
mod waveform_view;

/// Default viewport sizes used when creating or restoring the window.
pub const DEFAULT_VIEWPORT_SIZE: [f32; 2] = [960.0, 560.0];
pub const MIN_VIEWPORT_SIZE: [f32; 2] = [640.0, 400.0];

use crate::{
    audio::AudioPlayer,
    egui_app::controller::{EguiController, hotkeys},
    egui_app::state::{FocusContext, TriageFlagColumn},
    sample_sources::SampleTag,
    waveform::WaveformRenderer,
};
use eframe::egui;
use eframe::egui::{TextureHandle, Ui, UiBuilder};
#[cfg(target_os = "windows")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;

/// Renders the egui UI using the shared controller state.
pub struct EguiApp {
    controller: EguiController,
    visuals_set: bool,
    waveform_tex: Option<TextureHandle>,
    last_viewport_log: Option<(u32, u32, u32, u32, &'static str)>,
    sources_panel_rect: Option<egui::Rect>,
    sources_panel_drop_hovered: bool,
    sources_panel_drop_armed: bool,
}

#[inline]
fn copy_shortcut_pressed(ctx: &egui::Context) -> bool {
    let events = ctx.input(|i| i.events.clone());
    events.into_iter().any(|event| match event {
        egui::Event::Copy => true,
        egui::Event::Key {
            key: egui::Key::C,
            pressed: true,
            repeat: false,
            modifiers,
            ..
        } if (modifiers.command || modifiers.ctrl) && !modifiers.alt => true,
        _ => false,
    })
}

fn hotkey_triggered(ctx: &egui::Context, gesture: &hotkeys::HotkeyGesture) -> bool {
    ctx.input(|input| input.key_pressed(gesture.key) && modifiers_match(&input.modifiers, gesture))
}

fn modifiers_match(modifiers: &egui::Modifiers, gesture: &hotkeys::HotkeyGesture) -> bool {
    let command = modifiers.command;
    command == gesture.command && modifiers.shift == gesture.shift && modifiers.alt == gesture.alt
}

impl EguiApp {
    /// Create a new egui app, loading persisted configuration.
    pub fn new(
        renderer: WaveformRenderer,
        player: Option<std::rc::Rc<std::cell::RefCell<AudioPlayer>>>,
    ) -> Result<Self, String> {
        let mut controller = EguiController::new(renderer, player);
        controller
            .load_configuration()
            .map_err(|err| format!("Failed to load config: {err}"))?;
        controller.select_first_source();
        Ok(Self {
            controller,
            visuals_set: false,
            waveform_tex: None,
            last_viewport_log: None,
            sources_panel_rect: None,
            sources_panel_drop_hovered: false,
            sources_panel_drop_armed: false,
        })
    }

    fn apply_visuals(&mut self, ctx: &egui::Context) {
        if self.visuals_set {
            return;
        }
        let mut visuals = egui::Visuals::dark();
        style::apply_visuals(&mut visuals);
        ctx.set_visuals(visuals);
        self.visuals_set = true;
    }

    fn render_center(&mut self, ui: &mut Ui) {
        ui.set_min_height(ui.available_height());
        ui.vertical(|ui| {
            self.render_waveform(ui);
            ui.add_space(8.0);
            let browser_top = ui.cursor().min.y;
            let browser_rect = egui::Rect::from_min_max(
                egui::pos2(ui.max_rect().left(), browser_top),
                ui.max_rect().max,
            );
            if browser_rect.height() > 0.0 {
                let mut browser_ui = ui.new_child(
                    UiBuilder::new()
                        .id_salt("sample_browser_area")
                        .max_rect(browser_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                browser_ui.set_min_height(browser_ui.available_height());
                self.render_sample_browser(&mut browser_ui);
            }
        });
    }

    fn consume_source_panel_drops(&mut self, ctx: &egui::Context) {
        let panel_hit = if self.sources_panel_drop_hovered || self.sources_panel_drop_armed {
            true
        } else if let Some(rect) = self.sources_panel_rect {
            ctx.input(|i| {
                i.pointer
                    .hover_pos()
                    .or_else(|| i.pointer.interact_pos())
                    .map_or(false, |pos| rect.contains(pos))
            })
        } else {
            false
        };
        if !panel_hit {
            return;
        }
        let dropped_files = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped_files.is_empty() {
            return;
        }
        let mut handled_directory = false;
        for file in dropped_files {
            let Some(path) = file.path else {
                continue;
            };
            if !path.is_dir() {
                continue;
            }
            handled_directory = true;
            if let Err(err) = self.controller.add_source_from_path(path) {
                self.controller.set_status(err, style::StatusTone::Error);
            }
        }
        if !handled_directory {
            self.controller.set_status(
                "Drop a folder onto Sources to add it",
                style::StatusTone::Warning,
            );
        }
        self.sources_panel_drop_armed = false;
    }

    fn process_hotkeys(&mut self, ctx: &egui::Context, focus: FocusContext) {
        let overlay_open = self.controller.ui.hotkeys.overlay_visible;
        for action in hotkeys::iter_actions() {
            if overlay_open && !action.is_global() {
                continue;
            }
            if !action.is_active(focus) {
                continue;
            }
            if hotkey_triggered(ctx, &action.gesture) {
                self.controller.handle_hotkey(action, focus);
            }
        }
    }
}

impl eframe::App for EguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_visuals(ctx);
        #[cfg(target_os = "windows")]
        {
            let frame = _frame;
            self.controller.set_drag_hwnd(hwnd_from_frame(frame));
        }
        self.controller.tick_playhead();
        if let Some(pos) = ctx.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
        {
            self.controller.refresh_drag_position(pos);
        }
        #[cfg(target_os = "windows")]
        {
            let (pointer_outside, pointer_left) = ctx.input(|i| {
                let outside = i.pointer.primary_down() && i.pointer.hover_pos().is_none();
                let left = i
                    .events
                    .iter()
                    .any(|e| matches!(e, egui::Event::PointerGone));
                (outside, left)
            });
            self.controller
                .maybe_launch_external_drag(pointer_outside || pointer_left);
        }
        if self.controller.ui.drag.payload.is_some() && !ctx.input(|i| i.pointer.primary_down()) {
            self.controller.finish_active_drag();
        }
        let focus_context = self.controller.ui.focus.context;
        let collection_focus = matches!(focus_context, FocusContext::CollectionSample);
        let browser_has_selection = self.controller.ui.browser.selected.is_some();
        if collection_focus {
            self.controller.ui.browser.autoscroll = false;
            self.controller.ui.browser.selected = None;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.controller.toggle_play_pause();
        }
        if copy_shortcut_pressed(ctx) {
            self.controller.copy_selection_to_clipboard();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if !self.controller.ui.browser.selected_paths.is_empty() {
                self.controller.clear_browser_selection();
            }
        }
        if let Some(new_maximized) = ctx.input(|i| {
            if i.key_pressed(egui::Key::F11) {
                Some(!i.viewport().maximized.unwrap_or(false))
            } else {
                None
            }
        }) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(new_maximized));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown) && i.modifiers.shift) {
            if collection_focus {
                self.controller.nudge_collection_sample(1);
            } else {
                self.controller.grow_selection(1);
            }
        } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if collection_focus {
                self.controller.nudge_collection_sample(1);
            } else {
                self.controller.nudge_selection(1);
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp) && i.modifiers.shift) {
            if collection_focus {
                self.controller.nudge_collection_sample(-1);
            } else {
                self.controller.grow_selection(-1);
            }
        } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            if collection_focus {
                self.controller.nudge_collection_sample(-1);
            } else {
                self.controller.nudge_selection(-1);
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            if ctx.input(|i| i.modifiers.ctrl) {
                if browser_has_selection {
                    self.controller.move_selection_column(1);
                }
            } else if collection_focus {
                self.controller
                    .tag_selected_collection_sample(SampleTag::Keep);
            } else if browser_has_selection {
                let col = self.controller.ui.browser.selected.map(|t| t.column);
                let target = if matches!(col, Some(TriageFlagColumn::Trash)) {
                    crate::sample_sources::SampleTag::Neutral
                } else {
                    crate::sample_sources::SampleTag::Keep
                };
                self.controller.tag_selected(target);
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            if ctx.input(|i| i.modifiers.ctrl) {
                if browser_has_selection {
                    self.controller.move_selection_column(-1);
                }
            } else if collection_focus {
                self.controller.tag_selected_collection_left();
            } else if browser_has_selection {
                self.controller.tag_selected_left();
            }
        }
        self.process_hotkeys(ctx, focus_context);
        self.render_status(ctx);
        egui::SidePanel::left("sources")
            .resizable(false)
            .min_width(220.0)
            .max_width(240.0)
            .show(ctx, |ui| self.render_sources_panel(ui));
        self.consume_source_panel_drops(ctx);
        egui::SidePanel::right("collections")
            .resizable(false)
            .min_width(240.0)
            .max_width(280.0)
            .show(ctx, |ui| self.render_collections_panel(ui));
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.set_min_height(ui.available_height());
            self.render_center(ui);
        });
        self.render_drag_overlay(ctx);
        if self.controller.ui.hotkeys.overlay_visible {
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.controller.ui.hotkeys.overlay_visible = false;
            }
            let focus_actions = hotkeys::focused_actions(focus_context);
            let global_actions = hotkeys::global_actions();
            hotkey_overlay::render_hotkey_overlay(
                ctx,
                focus_context,
                &focus_actions,
                &global_actions,
                &mut self.controller.ui.hotkeys.overlay_visible,
            );
        }
        ctx.request_repaint();
    }
}

#[cfg(target_os = "windows")]
fn hwnd_from_frame(frame: &eframe::Frame) -> Option<HWND> {
    let handle = frame.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win) => Some(HWND(win.hwnd.get() as *mut _)),
        _ => None,
    }
}
