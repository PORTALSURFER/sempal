use eframe::egui;

use super::super::EguiApp;
#[cfg(target_os = "windows")]
use super::super::platform;

impl EguiApp {
    pub(super) fn prepare_frame(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_visuals(ctx);
        self.ensure_initial_focus(ctx);
        let feedback_modal_open = self.controller.ui.feedback_issue.open;
        #[cfg(target_os = "windows")]
        self.controller
            .set_drag_hwnd(platform::hwnd_from_frame(_frame));
        #[cfg(target_os = "windows")]
        if !feedback_modal_open {
            let pixels_per_point = ctx.pixels_per_point();
            self.controller.ui.drag.os_cursor_pos = platform::hwnd_from_frame(_frame)
                .and_then(|hwnd| platform::cursor_pos_in_client_points(hwnd, pixels_per_point));
            let left_mouse_down = platform::left_mouse_button_down();
            self.controller
                .ui
                .drag
                .update_os_mouse_state(left_mouse_down);
        }
        self.controller.tick_playhead();
        if !feedback_modal_open {
            if let Some(pos) =
                ctx.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
            {
                let shift_down = ctx.input(|i| i.modifiers.shift);
                self.controller.refresh_drag_position(pos, shift_down);
            }
        } else {
            self.controller.ui.drag.position = None;
            self.controller.ui.drag.label.clear();
            self.controller.ui.drag.clear_all_targets();
        }
        #[cfg(target_os = "windows")]
        if !feedback_modal_open {
            let window_focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
            if self.controller.ui.drag.payload.is_some() {
                ctx.request_repaint();
            }
            let window_inside = self
                .controller
                .ui
                .drag
                .payload
                .is_some()
                .then(|| platform::hwnd_from_frame(_frame))
                .flatten()
                .and_then(platform::cursor_inside_hwnd)
                .map(|inside| inside && window_focused);

            if self.controller.ui.drag.payload.is_some()
                && matches!(window_inside, Some(false))
                && !self.controller.ui.drag.external_started
            {
                // The pointer has left the window during an in-app drag. Hide all internal drag
                // visuals (ghost + hover highlights) so the only remaining user action is to drop
                // externally once the OS drag-out is triggered.
                self.controller.ui.drag.position = None;
                self.controller.ui.drag.label.clear();
                self.controller.ui.drag.clear_all_targets();
            }

            let (pointer_outside, pointer_left) = ctx.input(|i| {
                if self.controller.ui.drag.payload.is_some() {
                    if !i.viewport().focused.unwrap_or(true) {
                        self.controller.ui.drag.pointer_left_window = true;
                    }
                    if matches!(window_inside, Some(false)) {
                        // Once the pointer leaves the window during a drag, permanently disable
                        // in-app dragging until the gesture completes (external-only).
                        self.controller.ui.drag.pointer_left_window = true;
                    }
                    let pointer_gone = i
                        .events
                        .iter()
                        .any(|e| matches!(e, egui::Event::PointerGone));
                    if pointer_gone {
                        self.controller.ui.drag.pointer_left_window = true;
                    }
                } else {
                    self.controller.ui.drag.pointer_left_window = false;
                }

                // Prefer the OS cursor/window geometry check when available; it's robust even
                // when egui stops reporting pointer positions outside the window.
                let inside = window_inside.unwrap_or_else(|| {
                    let hover_pos = i.pointer.hover_pos();
                    let interact_pos = i.pointer.interact_pos();
                    hover_pos.is_some()
                        || (interact_pos.is_some() && !self.controller.ui.drag.pointer_left_window)
                });
                let outside = self.controller.ui.drag.payload.is_some() && !inside;
                let left = match window_inside {
                    Some(true) => false,
                    Some(false) => true,
                    None => self.controller.ui.drag.pointer_left_window,
                };
                (outside, left)
            });
            self.controller
                .maybe_launch_external_drag(pointer_outside, pointer_left);
            if self.controller.ui.drag.payload.is_some()
                && self.controller.ui.drag.pointer_left_window
                && self.controller.ui.drag.os_left_mouse_released
                && !self.controller.ui.drag.external_started
            {
                self.controller.cancel_active_drag();
            }
        }
    }
}
