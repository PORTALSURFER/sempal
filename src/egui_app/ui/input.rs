use eframe::egui;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct InputSnapshot {
    pub escape: bool,
    pub space: bool,
    pub arrow_down: bool,
    pub arrow_up: bool,
    pub arrow_left: bool,
    pub arrow_right: bool,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub command: bool,
}

impl InputSnapshot {
    pub(super) fn capture(ctx: &egui::Context) -> Self {
        ctx.input(|i| Self {
            escape: i.key_pressed(egui::Key::Escape),
            space: i.key_pressed(egui::Key::Space),
            arrow_down: i.key_pressed(egui::Key::ArrowDown),
            arrow_up: i.key_pressed(egui::Key::ArrowUp),
            arrow_left: i.key_pressed(egui::Key::ArrowLeft),
            arrow_right: i.key_pressed(egui::Key::ArrowRight),
            shift: i.modifiers.shift,
            alt: i.modifiers.alt,
            ctrl: i.modifiers.ctrl,
            command: i.modifiers.command,
        })
    }

    pub(super) fn ctrl_or_command(&self) -> bool {
        self.ctrl || self.command
    }
}

#[inline]
pub(super) fn copy_shortcut_pressed(ctx: &egui::Context) -> bool {
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

pub(super) fn user_activity_detected(ctx: &egui::Context) -> bool {
    ctx.input(|i| {
        !i.events.is_empty()
            || i.pointer.any_down()
            || i.pointer.any_pressed()
            || i.pointer.any_released()
            || i.raw_scroll_delta != egui::Vec2::ZERO
    })
}
