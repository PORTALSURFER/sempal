use crate::egui_app::state::{FocusContext, TriageFlagColumn};
use crate::sample_sources::SampleTag;
use crate::selection::SelectionEdge;
use eframe::egui;

use super::EguiApp;
use super::input::{InputSnapshot, copy_shortcut_pressed};
#[cfg(target_os = "windows")]
use super::platform;

struct FocusFlags {
    browser: bool,
    folder: bool,
    waveform: bool,
    sources: bool,
    collection_sample: bool,
    collections_list: bool,
}

impl FocusFlags {
    fn from_context(context: FocusContext) -> Self {
        Self {
            browser: matches!(context, FocusContext::SampleBrowser | FocusContext::None),
            folder: matches!(context, FocusContext::SourceFolders),
            waveform: matches!(context, FocusContext::Waveform),
            sources: matches!(context, FocusContext::SourcesList),
            collection_sample: matches!(context, FocusContext::CollectionSample),
            collections_list: matches!(context, FocusContext::CollectionsList),
        }
    }
}

impl eframe::App for EguiApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.prepare_frame(ctx, frame);
        let focus_context = self.controller.ui.focus.context;
        let focus_flags = FocusFlags::from_context(focus_context);
        self.handle_focus_side_effects(&focus_flags);
        let input = InputSnapshot::capture(ctx);
        self.handle_space_shortcut(ctx, &input);
        self.handle_copy_shortcut(ctx);
        self.handle_escape_shortcut(ctx, &input);
        self.handle_window_shortcuts(ctx);
        self.handle_arrow_keys(ctx, &focus_flags, &input);
        self.process_hotkeys(ctx, focus_context);
        self.render_ui(ctx, &input, focus_context);
    }
}

impl EguiApp {
    fn prepare_frame(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_visuals(ctx);
        self.ensure_initial_focus(ctx);
        #[cfg(target_os = "windows")]
        self.controller
            .set_drag_hwnd(platform::hwnd_from_frame(_frame));
        self.controller.tick_playhead();
        if let Some(pos) = ctx.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
        {
            let shift_down = ctx.input(|i| i.modifiers.shift);
            self.controller.refresh_drag_position(pos, shift_down);
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
    }

    fn handle_focus_side_effects(&mut self, focus: &FocusFlags) {
        if !focus.browser {
            self.controller.blur_browser_focus();
        }
        if focus.collection_sample {
            self.controller.ui.browser.autoscroll = false;
        }
    }

    fn handle_space_shortcut(&mut self, ctx: &egui::Context, input: &InputSnapshot) {
        if !input.space {
            return;
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        let ctrl_or_command = input.ctrl_or_command();
        if ctrl_or_command {
            let handled = self.controller.play_from_cursor();
            if !handled {
                self.controller.toggle_play_pause();
            }
        } else if input.shift {
            let handled = self.controller.replay_from_last_start();
            if !handled {
                self.controller.toggle_play_pause();
            }
        } else {
            self.controller.toggle_play_pause();
        }
        consume_keypress(ctx, input, egui::Key::Space);
    }

    fn handle_copy_shortcut(&mut self, ctx: &egui::Context) {
        if copy_shortcut_pressed(ctx) {
            self.controller.copy_selection_to_clipboard();
        }
    }

    fn handle_escape_shortcut(&mut self, ctx: &egui::Context, input: &InputSnapshot) {
        if !input.escape {
            return;
        }
        if self.controller.ui.progress.visible {
            self.controller.request_progress_cancel();
        }
        self.controller.handle_escape();
        if self.controller.ui.hotkeys.overlay_visible {
            self.controller.ui.hotkeys.overlay_visible = false;
            ctx.input_mut(|state| state.consume_key(egui::Modifiers::default(), egui::Key::Escape));
        }
        ctx.input_mut(|state| state.consume_key(egui::Modifiers::default(), egui::Key::Escape));
    }

    fn handle_window_shortcuts(&self, ctx: &egui::Context) {
        if let Some(new_maximized) = ctx.input(|i| {
            if i.key_pressed(egui::Key::F11) {
                Some(!i.viewport().maximized.unwrap_or(false))
            } else {
                None
            }
        }) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(new_maximized));
        }
    }

    fn handle_arrow_keys(&mut self, ctx: &egui::Context, focus: &FocusFlags, input: &InputSnapshot) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let browser_has_selection = self.controller.ui.browser.selected.is_some();
        let ctrl_or_command = input.ctrl_or_command();
        self.handle_arrow_down(ctx, focus, input);
        self.handle_arrow_up(ctx, focus, input);
        self.handle_arrow_right(ctx, focus, input, browser_has_selection, ctrl_or_command);
        self.handle_arrow_left(ctx, focus, input, browser_has_selection, ctrl_or_command);
    }

    fn handle_arrow_down(&mut self, ctx: &egui::Context, focus: &FocusFlags, input: &InputSnapshot) {
        if !input.arrow_down {
            return;
        }
        if focus.collection_sample {
            self.controller.nudge_collection_sample(1);
        } else if focus.browser {
            if self.controller.random_navigation_mode_enabled() {
                self.controller.play_random_visible_sample();
            } else if input.shift {
                self.controller.grow_selection(1);
            } else {
                self.controller.nudge_selection(1);
            }
        } else if focus.folder {
            self.controller.nudge_folder_selection(1, input.shift);
        } else if focus.waveform {
            self.controller.zoom_waveform(false);
        } else if focus.sources {
            self.controller.nudge_source_selection(1);
        } else if focus.collections_list {
            self.controller.nudge_collection_row(1);
        }
        consume_keypress(ctx, input, egui::Key::ArrowDown);
    }

    fn handle_arrow_up(&mut self, ctx: &egui::Context, focus: &FocusFlags, input: &InputSnapshot) {
        if !input.arrow_up {
            return;
        }
        if focus.collection_sample {
            self.controller.nudge_collection_sample(-1);
        } else if focus.browser {
            if self.controller.random_navigation_mode_enabled() {
                self.controller.play_previous_random_sample();
            } else if input.shift {
                self.controller.grow_selection(-1);
            } else {
                self.controller.nudge_selection(-1);
            }
        } else if focus.folder {
            self.controller.nudge_folder_selection(-1, input.shift);
        } else if focus.waveform {
            self.controller.zoom_waveform(true);
        } else if focus.sources {
            self.controller.nudge_source_selection(-1);
        } else if focus.collections_list {
            self.controller.nudge_collection_row(-1);
        }
        consume_keypress(ctx, input, egui::Key::ArrowUp);
    }

    fn handle_arrow_right(
        &mut self,
        ctx: &egui::Context,
        focus: &FocusFlags,
        input: &InputSnapshot,
        browser_has_selection: bool,
        ctrl_or_command: bool,
    ) {
        if !input.arrow_right {
            return;
        }
        if focus.waveform {
            self.handle_waveform_arrow(input, ctrl_or_command, true);
        } else if focus.folder {
            self.controller.expand_focused_folder();
        } else if ctrl_or_command && focus.browser && browser_has_selection {
            self.controller.move_selection_column(1);
        } else if focus.collection_sample {
            self.controller
                .tag_selected_collection_sample(SampleTag::Keep);
        } else if focus.browser && browser_has_selection {
            let col = self.controller.ui.browser.selected.map(|t| t.column);
            let target = if matches!(col, Some(TriageFlagColumn::Trash)) {
                SampleTag::Neutral
            } else {
                SampleTag::Keep
            };
            self.controller.tag_selected(target);
        }
        consume_keypress(ctx, input, egui::Key::ArrowRight);
    }

    fn handle_arrow_left(
        &mut self,
        ctx: &egui::Context,
        focus: &FocusFlags,
        input: &InputSnapshot,
        browser_has_selection: bool,
        ctrl_or_command: bool,
    ) {
        if !input.arrow_left {
            return;
        }
        if focus.waveform {
            self.handle_waveform_arrow(input, ctrl_or_command, false);
        } else if focus.folder {
            self.controller.collapse_focused_folder();
        } else if ctrl_or_command && focus.browser && browser_has_selection {
            self.controller.move_selection_column(-1);
        } else if focus.collection_sample {
            self.controller.tag_selected_collection_left();
        } else if focus.browser && browser_has_selection {
            self.controller.tag_selected_left();
        }
        consume_keypress(ctx, input, egui::Key::ArrowLeft);
    }

    fn handle_waveform_arrow(
        &mut self,
        input: &InputSnapshot,
        ctrl_or_command: bool,
        move_right: bool,
    ) {
        let was_playing = self.controller.is_playing();
        let has_selection = self.controller.ui.waveform.selection.is_some();
        if input.shift {
            self.adjust_waveform_selection(
                input,
                ctrl_or_command,
                move_right,
                was_playing,
                has_selection,
            );
            return;
        }
        self.move_waveform_playhead(input, move_right, was_playing);
    }

    fn adjust_waveform_selection(
        &mut self,
        input: &InputSnapshot,
        ctrl_or_command: bool,
        move_right: bool,
        was_playing: bool,
        has_selection: bool,
    ) {
        if ctrl_or_command {
            self.handle_waveform_chord_selection(input, move_right, was_playing, has_selection);
            return;
        }
        self.handle_waveform_shift_selection(input, move_right, was_playing, has_selection);
    }

    fn handle_waveform_chord_selection(
        &mut self,
        input: &InputSnapshot,
        move_right: bool,
        was_playing: bool,
        has_selection: bool,
    ) {
        let start_direction_is_left = !move_right;
        if has_selection {
            self.controller.nudge_selection_edge(
                SelectionEdge::Start,
                start_direction_is_left,
                input.alt,
            );
        } else {
            self.controller.create_selection_from_playhead(
                start_direction_is_left,
                was_playing,
                input.alt,
            );
        }
    }

    fn handle_waveform_shift_selection(
        &mut self,
        input: &InputSnapshot,
        move_right: bool,
        was_playing: bool,
        has_selection: bool,
    ) {
        let start_direction_is_left = !move_right;
        if has_selection {
            self.controller
                .nudge_selection_edge(SelectionEdge::End, move_right, input.alt);
            return;
        }
        self.controller.create_selection_from_playhead(
            start_direction_is_left,
            was_playing,
            input.alt,
        );
    }

    fn move_waveform_playhead(
        &mut self,
        input: &InputSnapshot,
        move_right: bool,
        was_playing: bool,
    ) {
        let step = if move_right { 1 } else { -1 };
        self.controller
            .move_playhead_steps(step, input.alt, was_playing);
    }
}

fn consume_keypress(ctx: &egui::Context, input: &InputSnapshot, key: egui::Key) {
    let mut modifiers = egui::Modifiers::default();
    modifiers.shift = input.shift;
    modifiers.alt = input.alt;
    modifiers.ctrl = input.ctrl;
    modifiers.command = input.command;
    ctx.input_mut(|state| state.consume_key(modifiers, key));
}
