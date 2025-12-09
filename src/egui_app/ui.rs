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
    egui_app::controller::{hotkeys, EguiController},
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
use std::time::{Duration, Instant};

/// Renders the egui UI using the shared controller state.
pub struct EguiApp {
    controller: EguiController,
    visuals_set: bool,
    waveform_tex: Option<TextureHandle>,
    last_viewport_log: Option<(u32, u32, u32, u32, &'static str)>,
    sources_panel_rect: Option<egui::Rect>,
    sources_panel_drop_hovered: bool,
    sources_panel_drop_armed: bool,
    selection_edge_offset: Option<f32>,
    pending_chord: Option<PendingChord>,
    key_feedback: KeyFeedback,
    requested_initial_focus: bool,
}

#[derive(Clone, Copy, Debug)]
struct PendingChord {
    first: hotkeys::KeyPress,
    started_at: Instant,
}

const CHORD_TIMEOUT: Duration = Duration::from_millis(900);

struct InputSnapshot {
    escape: bool,
    space: bool,
    arrow_down: bool,
    arrow_up: bool,
    arrow_left: bool,
    arrow_right: bool,
    bracket_left: bool,
    bracket_right: bool,
    shift: bool,
    alt: bool,
    ctrl: bool,
    command: bool,
}

#[derive(Default)]
struct KeyFeedback {
    last_key: Option<hotkeys::KeyPress>,
    pending_root: Option<hotkeys::KeyPress>,
    last_chord: Option<(hotkeys::KeyPress, hotkeys::KeyPress)>,
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

fn keypress_from_event(event: &egui::Event) -> Option<hotkeys::KeyPress> {
    match event {
        egui::Event::Key {
            key,
            pressed: true,
            repeat: false,
            modifiers,
            ..
        } => Some(hotkeys::KeyPress {
            key: *key,
            command: modifiers.command || modifiers.ctrl,
            shift: modifiers.shift,
            alt: modifiers.alt,
        }),
        _ => None,
    }
}

fn press_matches(press: &hotkeys::KeyPress, target: &hotkeys::KeyPress) -> bool {
    press.key == target.key
        && press.command == target.command
        && press.shift == target.shift
        && press.alt == target.alt
}

fn format_keypress(press: &Option<hotkeys::KeyPress>) -> String {
    press
        .as_ref()
        .map(hotkeys::format_keypress)
        .unwrap_or_else(|| "—".to_string())
}

fn consume_press(ctx: &egui::Context, press: hotkeys::KeyPress) {
    let mut modifiers = egui::Modifiers::default();
    modifiers.alt = press.alt;
    modifiers.shift = press.shift;
    modifiers.command = press.command;
    modifiers.ctrl = press.command;
    ctx.input_mut(|i| {
        i.consume_key(modifiers, press.key);
    });
}

impl EguiApp {
    fn ensure_initial_focus(&mut self, ctx: &egui::Context) {
        if self.requested_initial_focus {
            return;
        }
        let is_focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
        if is_focused {
            self.requested_initial_focus = true;
            return;
        }
        self.requested_initial_focus = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
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
            selection_edge_offset: None,
            pending_chord: None,
            key_feedback: KeyFeedback::default(),
            requested_initial_focus: false,
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
        let actions: Vec<_> = hotkeys::iter_actions()
            .filter(|action| (!overlay_open || action.is_global()) && action.is_active(focus))
            .collect();
        if actions.is_empty() {
            self.pending_chord = None;
            self.key_feedback.pending_root = None;
            return;
        }
        let now = Instant::now();
        if let Some(pending) = self.pending_chord {
            if now.saturating_duration_since(pending.started_at) > CHORD_TIMEOUT {
                self.pending_chord = None;
                self.key_feedback.pending_root = None;
            }
        }
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            let Some(press) = keypress_from_event(&event) else {
                continue;
            };
            self.key_feedback.last_key = Some(press);
            if self.try_handle_chord(ctx, &actions, press, focus, now) {
                continue;
            }
            if self.try_start_chord(&actions, press, now, ctx) {
                continue;
            }
            if let Some(action) = actions
                .iter()
                .find(|action| action.gesture.chord.is_none()
                    && press_matches(&press, &action.gesture.first))
                .copied()
            {
                self.controller.handle_hotkey(action, focus);
                consume_press(ctx, press);
                continue;
            }
            // No hotkey matched; let it fall through without consuming to avoid system beeps.
        }
    }

    fn try_handle_chord(
        &mut self,
        ctx: &egui::Context,
        actions: &[hotkeys::HotkeyAction],
        press: hotkeys::KeyPress,
        focus: FocusContext,
        now: Instant,
    ) -> bool {
        let Some(pending) = self.pending_chord else {
            return false;
        };
        if now.saturating_duration_since(pending.started_at) > CHORD_TIMEOUT {
            self.pending_chord = None;
            return false;
        }
        if let Some(action) = actions
            .iter()
            .find(|action| action
                .gesture
                .chord
                .is_some_and(|second| press_matches(&press, &second))
                && press_matches(&pending.first, &action.gesture.first))
            .copied()
        {
            self.pending_chord = None;
            self.key_feedback.last_chord = Some((pending.first, press));
            self.key_feedback.pending_root = None;
            consume_press(ctx, pending.first);
            consume_press(ctx, press);
            self.controller.handle_hotkey(action, focus);
            return true;
        }
        self.pending_chord = None;
        self.key_feedback.pending_root = None;
        false
    }

    fn try_start_chord(
        &mut self,
        actions: &[hotkeys::HotkeyAction],
        press: hotkeys::KeyPress,
        now: Instant,
        _ctx: &egui::Context,
    ) -> bool {
        let starts_chord = actions.iter().any(|action| {
            action
                .gesture
                .chord
                .is_some_and(|_| press_matches(&press, &action.gesture.first))
        });
        if starts_chord {
            self.pending_chord = Some(PendingChord {
                first: press,
                started_at: now,
            });
            self.key_feedback.pending_root = Some(press);
            return true;
        }
        false
    }
}

impl eframe::App for EguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_visuals(ctx);
        self.ensure_initial_focus(ctx);
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
        let input = ctx.input(|i| InputSnapshot {
            escape: i.key_pressed(egui::Key::Escape),
            space: i.key_pressed(egui::Key::Space),
            arrow_down: i.key_pressed(egui::Key::ArrowDown),
            arrow_up: i.key_pressed(egui::Key::ArrowUp),
            arrow_left: i.key_pressed(egui::Key::ArrowLeft),
            arrow_right: i.key_pressed(egui::Key::ArrowRight),
            bracket_left: i.key_pressed(egui::Key::OpenBracket),
            bracket_right: i.key_pressed(egui::Key::CloseBracket),
            shift: i.modifiers.shift,
            alt: i.modifiers.alt,
            ctrl: i.modifiers.ctrl,
            command: i.modifiers.command,
        });
        let ctrl_or_command = input.ctrl || input.command;
        let browser_focus = matches!(
            focus_context,
            FocusContext::SampleBrowser | FocusContext::None
        );
        let collection_focus = matches!(focus_context, FocusContext::CollectionSample);
        let waveform_focus = matches!(focus_context, FocusContext::Waveform);
        let sources_focus = matches!(focus_context, FocusContext::SourcesList);
        let collections_list_focus = matches!(focus_context, FocusContext::CollectionsList);
        let browser_has_selection = self.controller.ui.browser.selected.is_some();
        if collection_focus {
            self.controller.ui.browser.autoscroll = false;
            self.controller.ui.browser.selected = None;
        }
        if input.space {
            self.controller.toggle_play_pause();
        }
        if copy_shortcut_pressed(ctx) {
            self.controller.copy_selection_to_clipboard();
        }
        if input.escape {
            let _ = self.controller.stop_playback_if_active();
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
        if input.arrow_down {
            if collection_focus {
                self.controller.nudge_collection_sample(1);
            } else if browser_focus {
                if input.shift {
                    self.controller.grow_selection(1);
                } else {
                    self.controller.nudge_selection(1);
                }
            } else if waveform_focus {
                self.controller.zoom_waveform(false);
            } else if sources_focus {
                self.controller.nudge_source_selection(1);
            } else if collections_list_focus {
                self.controller.nudge_collection_row(1);
            }
        }
        if input.arrow_up {
            if collection_focus {
                self.controller.nudge_collection_sample(-1);
            } else if browser_focus {
                if input.shift {
                    self.controller.grow_selection(-1);
                } else {
                    self.controller.nudge_selection(-1);
                }
            } else if waveform_focus {
                self.controller.zoom_waveform(true);
            } else if sources_focus {
                self.controller.nudge_source_selection(-1);
            } else if collections_list_focus {
                self.controller.nudge_collection_row(-1);
            }
        }
        if input.arrow_right {
            if waveform_focus {
                let was_playing = self.controller.is_playing();
                let has_selection = self.controller.ui.waveform.selection.is_some();
                if input.shift && ctrl_or_command {
                    if has_selection {
                        self.controller.nudge_selection_edge(
                            crate::selection::SelectionEdge::Start,
                            false,
                            input.alt,
                        );
                    } else {
                        self.controller.create_selection_from_playhead(
                            false,
                            was_playing,
                            input.alt,
                        );
                    }
                } else if input.shift {
                    if has_selection {
                        self.controller.nudge_selection_edge(
                            crate::selection::SelectionEdge::End,
                            true,
                            input.alt,
                        );
                    } else {
                        self.controller.create_selection_from_playhead(
                            false,
                            was_playing,
                            input.alt,
                        );
                    }
                } else if input.alt {
                    self.controller.move_playhead_steps(1, true, was_playing);
                } else {
                    self.controller.move_playhead_steps(1, false, was_playing);
                }
            } else if ctrl_or_command && browser_focus && browser_has_selection {
                self.controller.move_selection_column(1);
            } else if collection_focus {
                self.controller
                    .tag_selected_collection_sample(SampleTag::Keep);
            } else if browser_focus && browser_has_selection {
                let col = self.controller.ui.browser.selected.map(|t| t.column);
                let target = if matches!(col, Some(TriageFlagColumn::Trash)) {
                    crate::sample_sources::SampleTag::Neutral
                } else {
                    crate::sample_sources::SampleTag::Keep
                };
                self.controller.tag_selected(target);
            }
        }
        if input.arrow_left {
            if waveform_focus {
                let was_playing = self.controller.is_playing();
                let has_selection = self.controller.ui.waveform.selection.is_some();
                if input.shift && ctrl_or_command {
                    if has_selection {
                        self.controller
                            .nudge_selection_edge(
                                crate::selection::SelectionEdge::Start,
                                true,
                                input.alt,
                            );
                    } else {
                        self.controller.create_selection_from_playhead(
                            true,
                            was_playing,
                            input.alt,
                        );
                    }
                } else if input.shift {
                    if has_selection {
                        self.controller
                            .nudge_selection_edge(
                                crate::selection::SelectionEdge::End,
                                false,
                                input.alt,
                            );
                    } else {
                        self.controller.create_selection_from_playhead(
                            true,
                            was_playing,
                            input.alt,
                        );
                    }
                } else if input.alt {
                    self.controller.move_playhead_steps(-1, true, was_playing);
                } else {
                    self.controller.move_playhead_steps(-1, false, was_playing);
                }
            } else if ctrl_or_command && browser_focus && browser_has_selection {
                self.controller.move_selection_column(-1);
            } else if collection_focus {
                self.controller.tag_selected_collection_left();
            } else if browser_focus && browser_has_selection {
                self.controller.tag_selected_left();
            }
        }
        if waveform_focus && input.bracket_left {
            self.controller
                .nudge_selection_edge(
                    crate::selection::SelectionEdge::Start,
                    !input.shift,
                    false,
                );
        }
        if waveform_focus && input.bracket_right {
            self.controller
                .nudge_selection_edge(
                    crate::selection::SelectionEdge::End,
                    !input.shift,
                    false,
                );
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
            if input.escape {
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
