use crate::selection::SelectionEdge;
use eframe::egui;

use super::super::EguiApp;
use super::super::input::InputSnapshot;
use super::consume_keypress;
use super::update_prompt::FocusFlags;

impl EguiApp {
    pub(super) fn handle_arrow_keys(
        &mut self,
        ctx: &egui::Context,
        focus: &FocusFlags,
        input: &InputSnapshot,
    ) {
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

    fn handle_arrow_down(
        &mut self,
        ctx: &egui::Context,
        focus: &FocusFlags,
        input: &InputSnapshot,
    ) {
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
        if input.alt {
            let step = if move_right { 1 } else { -1 };
            self.controller.nudge_selection_range(step, input.shift);
            return;
        }
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
