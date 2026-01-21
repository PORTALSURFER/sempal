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
        if focus.browser {
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
            if self.controller.random_navigation_mode_enabled() {
                self.controller.play_random_visible_sample();
            } else if input.shift {
                self.controller.grow_selection(1);
            } else {
                self.controller.nudge_selection(1);
            }
        } else if focus.sources {
            self.controller.nudge_source_selection(1);
        }
        consume_keypress(ctx, input, egui::Key::ArrowDown);
    }

    fn handle_arrow_up(&mut self, ctx: &egui::Context, focus: &FocusFlags, input: &InputSnapshot) {
        if !input.arrow_up {
            return;
        }
        if focus.browser {
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
            if self.controller.random_navigation_mode_enabled() {
                self.controller.play_previous_random_sample();
            } else if input.shift {
                self.controller.grow_selection(-1);
            } else {
                self.controller.nudge_selection(-1);
            }
        } else if focus.sources {
            self.controller.nudge_source_selection(-1);
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
        let mut handled = false;
        if focus.waveform {
            self.handle_waveform_arrow(input, true);
            handled = true;
        } else if focus.folder {
            self.controller.expand_focused_folder();
            handled = true;
        } else if ctrl_or_command && focus.browser && browser_has_selection {
            self.controller.move_selection_column(1);
            handled = true;
        }
        if handled {
            consume_keypress(ctx, input, egui::Key::ArrowRight);
        }
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
        let mut handled = false;
        if focus.waveform {
            self.handle_waveform_arrow(input, false);
            handled = true;
        } else if focus.folder {
            self.controller.collapse_focused_folder();
            handled = true;
        } else if ctrl_or_command && focus.browser && browser_has_selection {
            self.controller.move_selection_column(-1);
            handled = true;
        }
        if handled {
            consume_keypress(ctx, input, egui::Key::ArrowLeft);
        }
    }

    fn handle_waveform_arrow(&mut self, input: &InputSnapshot, move_right: bool) {
        if input.alt {
            let step = if move_right { 1 } else { -1 };
            self.controller.nudge_selection_range(step, input.shift);
            return;
        }
        let step = if move_right { 1 } else { -1 };
        self.controller.slide_selection_range(step);
    }
}
