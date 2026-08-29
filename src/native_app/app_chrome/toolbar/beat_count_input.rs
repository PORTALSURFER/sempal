use radiant::{
    gui::automation::AutomationRole,
    prelude as ui,
    widgets::{
        TextEditCommand, TextInputMessage, TextInputWidget, Widget, WidgetCapabilities,
        WidgetCommon, WidgetInput, WidgetKey, WidgetOutput, WidgetSemantics, WidgetSizing,
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum BeatGuideCountInputMessage {
    Changed(String),
    Committed(String),
    Set(u8),
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct BeatGuideCountInputWidget {
    input: TextInputWidget,
}

impl BeatGuideCountInputWidget {
    pub(in crate::native_app) fn new(id: u64, count: u8, width: f32, height: f32) -> Self {
        let mut input = TextInputWidget::new(
            id,
            count.to_string(),
            WidgetSizing::fixed(ui::Vector2::new(width, height)),
        );
        input.props.character_limit = Some(3);
        Self { input }
    }

    fn value(&self) -> &str {
        self.input.state.value.as_str()
    }

    fn select_all(&mut self) {
        self.input.state.selection_anchor = 0;
        self.input.state.caret = self.input.state.char_len();
    }

    fn step_value(&self, delta: i8) -> u8 {
        let current = committed_count_from_text(self.value());
        let stepped = i16::from(current) + i16::from(delta);
        stepped.clamp(
            i16::from(crate::native_app::app::MIN_BEAT_GUIDE_COUNT),
            i16::from(crate::native_app::app::MAX_BEAT_GUIDE_COUNT),
        ) as u8
    }

    fn accepts_editing_input(&self) -> bool {
        self.input.common.state.focused
            && !self.input.common.state.disabled
            && !self.input.common.state.read_only
    }

    fn handle_text_input(
        &mut self,
        bounds: ui::Rect,
        input: WidgetInput,
    ) -> Option<BeatGuideCountInputMessage> {
        match input {
            WidgetInput::Character { character, .. } if !character.is_ascii_digit() => None,
            WidgetInput::TextEdit {
                command: TextEditCommand::InsertText(text),
                timestamp,
            } => {
                let digits = text
                    .chars()
                    .filter(char::is_ascii_digit)
                    .collect::<String>();
                if digits.is_empty() {
                    return None;
                }
                self.input
                    .handle_input(
                        bounds,
                        WidgetInput::TextEdit {
                            command: TextEditCommand::InsertText(digits),
                            timestamp,
                        },
                    )
                    .and_then(input_message)
            }
            WidgetInput::KeyPress {
                key: WidgetKey::ArrowUp,
                ..
            } if self.accepts_editing_input() => {
                Some(BeatGuideCountInputMessage::Set(self.step_value(1)))
            }
            WidgetInput::KeyPress {
                key: WidgetKey::ArrowDown,
                ..
            } if self.accepts_editing_input() => {
                Some(BeatGuideCountInputMessage::Set(self.step_value(-1)))
            }
            WidgetInput::Wheel { delta, .. } if delta.y < 0.0 => {
                Some(BeatGuideCountInputMessage::Set(self.step_value(1)))
            }
            WidgetInput::Wheel { delta, .. } if delta.y > 0.0 => {
                Some(BeatGuideCountInputMessage::Set(self.step_value(-1)))
            }
            WidgetInput::FocusChanged(false) => {
                let message = Some(BeatGuideCountInputMessage::Committed(
                    self.value().to_owned(),
                ));
                let _ = self
                    .input
                    .handle_input(bounds, WidgetInput::FocusChanged(false));
                message
            }
            WidgetInput::FocusChanged(true) => {
                let _ = self
                    .input
                    .handle_input(bounds, WidgetInput::FocusChanged(true));
                None
            }
            WidgetInput::PointerPress { position, .. } if bounds.contains(position) => {
                let _ = self.input.handle_input(bounds, input);
                self.select_all();
                None
            }
            input => self
                .input
                .handle_input(bounds, input)
                .and_then(input_message),
        }
    }
}

impl Widget for BeatGuideCountInputWidget {
    fn common(&self) -> &WidgetCommon {
        self.input.common()
    }

    fn common_mut(&mut self) -> &mut WidgetCommon {
        self.input.common_mut()
    }

    fn handle_input(&mut self, bounds: ui::Rect, input: WidgetInput) -> Option<WidgetOutput> {
        self.handle_text_input(bounds, input)
            .map(WidgetOutput::typed)
    }

    fn synchronize_from_previous(&mut self, previous: &dyn Widget) {
        if let Some(previous) = previous.as_any().downcast_ref::<Self>() {
            self.input.synchronize_from_previous(&previous.input);
        }
    }

    fn accepts_text_input(&self) -> bool {
        self.input.accepts_text_input()
    }

    fn preempts_host_shortcut_key(&self, key: WidgetKey) -> bool {
        matches!(key, WidgetKey::ArrowUp | WidgetKey::ArrowDown) && self.accepts_editing_input()
    }

    fn accepts_pointer_move(&self) -> bool {
        self.input.accepts_pointer_move()
    }

    fn accepts_pointer_input(&self, input: &WidgetInput) -> bool {
        match input {
            WidgetInput::Wheel { delta, .. } => {
                delta.y.abs() >= delta.x.abs() && delta.y.abs() > f32::EPSILON
            }
            _ => self.input.accepts_pointer_input(input),
        }
    }

    fn accepts_wheel_input(&self) -> bool {
        true
    }

    fn capabilities(&self) -> WidgetCapabilities<'_> {
        WidgetCapabilities::new().semantics(self)
    }

    fn selected_text_slice(&self) -> Option<&str> {
        self.input.selected_text_slice()
    }

    fn selected_text(&self) -> Option<String> {
        self.input.selected_text()
    }

    fn append_paint(
        &self,
        primitives: &mut Vec<radiant::runtime::PaintPrimitive>,
        bounds: ui::Rect,
        layout: &ui::LayoutOutput,
        theme: &ui::ThemeTokens,
    ) {
        self.input.append_paint(primitives, bounds, layout, theme);
    }
}

impl WidgetSemantics for BeatGuideCountInputWidget {
    fn automation_role(&self) -> AutomationRole {
        self.input.automation_role()
    }

    fn automation_label(&self) -> Option<String> {
        Some(String::from("Beat guide count"))
    }

    fn automation_value_text(&self) -> Option<String> {
        self.input.automation_value_text()
    }
}

fn input_message(message: TextInputMessage) -> Option<BeatGuideCountInputMessage> {
    match message {
        TextInputMessage::Changed { value } if is_in_range_count_text(&value) => {
            Some(BeatGuideCountInputMessage::Changed(value))
        }
        TextInputMessage::Changed { .. } => None,
        TextInputMessage::Submitted { value } | TextInputMessage::CompletionRequested { value } => {
            Some(BeatGuideCountInputMessage::Committed(value))
        }
    }
}

fn is_in_range_count_text(value: &str) -> bool {
    value.parse::<u16>().ok().is_some_and(|count| {
        (u16::from(crate::native_app::app::MIN_BEAT_GUIDE_COUNT)
            ..=u16::from(crate::native_app::app::MAX_BEAT_GUIDE_COUNT))
            .contains(&count)
    })
}

fn committed_count_from_text(value: &str) -> u8 {
    value
        .parse::<u16>()
        .ok()
        .map(clamp_beat_guide_count)
        .unwrap_or(crate::native_app::app::MIN_BEAT_GUIDE_COUNT)
}

fn clamp_beat_guide_count(value: u16) -> u8 {
    value.clamp(
        u16::from(crate::native_app::app::MIN_BEAT_GUIDE_COUNT),
        u16::from(crate::native_app::app::MAX_BEAT_GUIDE_COUNT),
    ) as u8
}
