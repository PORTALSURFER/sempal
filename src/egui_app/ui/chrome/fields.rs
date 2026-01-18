use eframe::egui;

/// A text input field designed for numeric values, supporting arrow key increments/decrements
/// and automatic select-all behavior on focus or adjustment.
pub struct NumericInput<'a> {
    value: &'a mut String,
    id: egui::Id,
    width: f32,
    hint: String,
}

impl<'a> NumericInput<'a> {
    pub fn new(value: &'a mut String, id: egui::Id) -> Self {
        Self {
            value,
            id,
            width: 64.0,
            hint: String::new(),
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }

    /// Display the input field. Returns the response and an optional adjustment factor
    /// (e.g., +1.0 for ArrowUp, -1.0 for ArrowDown).
    pub fn show(self, ui: &mut egui::Ui) -> (egui::Response, Option<f32>) {
        let mut adjust = 0.0;
        
        // Intercept arrow keys before TextEdit can see them
        if ui.memory(|m| m.has_focus(self.id)) {
            ui.input_mut(|i| {
                if i.key_pressed(egui::Key::ArrowUp) {
                    adjust = 1.0;
                } else if i.key_pressed(egui::Key::ArrowDown) {
                    adjust = -1.0;
                }

                if adjust != 0.0 {
                    // Aggressively consume the arrow keys to prevent TextEdit from seeing them
                    // and moving the cursor or jumping to start/end.
                    i.events.retain(|e| {
                        !matches!(
                            e,
                            egui::Event::Key {
                                key: egui::Key::ArrowUp | egui::Key::ArrowDown,
                                ..
                            }
                        )
                    });
                }
            });
        }

        let output = egui::TextEdit::singleline(self.value)
            .id(self.id)
            .desired_width(self.width)
            .hint_text(self.hint)
            .show(ui);

        let response = output.response;

        // Auto-select all on focus or after an arrow-key adjustment
        if (adjust != 0.0 || response.gained_focus()) && response.has_focus() {
            let mut state = output.state;
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::select_all(&output.galley)));
            state.store(ui.ctx(), response.id);
        }

        (response, if adjust != 0.0 { Some(adjust) } else { None })
    }
}
