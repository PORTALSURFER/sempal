use radiant::{
    gui::automation::AutomationRole,
    prelude as ui,
    runtime::WidgetPaint,
    widgets::{
        FocusBehavior, PointerButton, TextInputMessage, TextInputWidget, Widget,
        WidgetCapabilities, WidgetCommon, WidgetInput, WidgetKey, WidgetOutput, WidgetSemantics,
        WidgetSizing,
    },
};
use std::sync::{Arc, Mutex};

use super::{PlaymarkLabelMessage, WaveformState, WaveformWidget, playmark_label};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct PlaymarkLabelEditorState {
    pub(super) draft: String,
    pub(super) original_draft: String,
    pub(super) error: Option<String>,
}

impl WaveformState {
    pub(in crate::native_app) fn playmark_label_editor_active(&self) -> bool {
        self.playmark_label_editor.is_some()
    }

    pub(in crate::native_app) fn begin_playmark_label_edit(
        &mut self,
        beat_guides_enabled: bool,
        beat_guide_count: u8,
    ) -> bool {
        let Some(selection) = self.play_selection else {
            return false;
        };
        let Some(draft) = playmark_label::playmark_selection_label(
            selection,
            self.frames(),
            self.sample_rate(),
            beat_guides_enabled,
            beat_guide_count,
        ) else {
            return false;
        };
        self.playmark_label_editor = Some(PlaymarkLabelEditorState {
            original_draft: draft.clone(),
            draft,
            error: None,
        });
        true
    }

    pub(in crate::native_app) fn update_playmark_label_draft(&mut self, draft: String) {
        if let Some(editor) = self.playmark_label_editor.as_mut() {
            editor.draft = draft;
            editor.error = None;
        }
    }

    pub(in crate::native_app) fn set_playmark_label_error(&mut self, error: String) {
        if let Some(editor) = self.playmark_label_editor.as_mut() {
            editor.error = Some(error);
        }
    }

    pub(in crate::native_app) fn close_playmark_label_editor(&mut self) {
        self.playmark_label_editor = None;
    }

    pub(in crate::native_app) fn playmark_label_commit_is_unchanged(&self, draft: &str) -> bool {
        self.playmark_label_editor
            .as_ref()
            .is_some_and(|editor| editor.original_draft == draft)
    }

    pub(in crate::native_app) fn playmark_frame_range_from_text(
        &self,
        text: &str,
        beat_count: u8,
    ) -> Result<(usize, usize), String> {
        let current = self
            .play_selection
            .ok_or_else(|| String::from("Set a playmark selection first"))?;
        parsed_playmark_frame_range(text, self.sample_rate(), self.frames(), beat_count, current)
    }
}

#[derive(Clone, Debug)]
pub(super) struct PlaymarkLabelWidget {
    geometry_source: WaveformWidget,
    label: String,
    input: Option<TextInputWidget>,
    error: Option<String>,
    painted_bounds: Arc<Mutex<Option<ui::Rect>>>,
}

impl PlaymarkLabelWidget {
    pub(super) fn new(
        id: u64,
        geometry_source: WaveformWidget,
        state: &WaveformState,
        beat_guides_enabled: bool,
        beat_guide_count: u8,
    ) -> Option<Self> {
        let selection = state.play_selection()?;
        let label = playmark_label::playmark_selection_label(
            selection,
            state.frames(),
            state.sample_rate(),
            beat_guides_enabled,
            beat_guide_count,
        )?;
        let (input, error) = state
            .playmark_label_editor
            .as_ref()
            .map(|editor| {
                let mut input = TextInputWidget::new(
                    id,
                    editor.draft.clone(),
                    WidgetSizing::fixed(ui::Vector2::new(150.0, 18.0)),
                );
                input.props.character_limit = Some(64);
                input.state.selection_anchor = 0;
                input.state.caret = input.state.char_len();
                (Some(input), editor.error.clone())
            })
            .unwrap_or((None, None));
        let mut geometry_source = geometry_source;
        geometry_source.common.focus = FocusBehavior::Keyboard;
        Some(Self {
            geometry_source,
            label,
            input,
            error,
            painted_bounds: Arc::new(Mutex::new(None)),
        })
    }

    fn label_rect(&self, bounds: ui::Rect) -> Option<ui::Rect> {
        let selection = self.geometry_source.play_selection?;
        let geometry = self
            .geometry_source
            .selection_geometry(bounds, Some(selection))?;
        playmark_label::playmark_label_layout(bounds, geometry.rect, self.label.len())
            .map(|layout| layout.rect)
    }

    fn editing(&self) -> bool {
        self.input.is_some()
    }
}

impl Widget for PlaymarkLabelWidget {
    fn common(&self) -> &WidgetCommon {
        self.geometry_source.common()
    }

    fn common_mut(&mut self) -> &mut WidgetCommon {
        self.geometry_source.common_mut()
    }

    fn handle_input(&mut self, bounds: ui::Rect, input: WidgetInput) -> Option<WidgetOutput> {
        let label_rect = self.label_rect(bounds)?;
        if let Some(text_input) = self.input.as_mut() {
            if matches!(input, WidgetInput::FocusChanged(false)) {
                let value = text_input.state.value.clone();
                let _ = text_input.handle_input(label_rect, input);
                return Some(WidgetOutput::typed(PlaymarkLabelMessage::Commit(value)));
            }
            return text_input
                .handle_input(label_rect, input)
                .map(|message| match message {
                    TextInputMessage::Changed { value } => PlaymarkLabelMessage::Changed(value),
                    TextInputMessage::Submitted { value }
                    | TextInputMessage::CompletionRequested { value } => {
                        PlaymarkLabelMessage::Commit(value)
                    }
                })
                .map(WidgetOutput::typed);
        }
        (matches!(input, WidgetInput::PointerPress { position, button: PointerButton::Primary, .. } if label_rect.contains(position))
            || matches!(input, WidgetInput::KeyPress { key: WidgetKey::Enter | WidgetKey::Space, .. }))
        .then(|| WidgetOutput::typed(PlaymarkLabelMessage::BeginEdit))
    }

    fn synchronize_from_previous(&mut self, previous: &dyn Widget) {
        let Some(previous) = previous.as_any().downcast_ref::<Self>() else {
            return;
        };
        if let (Some(input), Some(previous_input)) = (&mut self.input, &previous.input) {
            input.synchronize_from_previous(previous_input);
        }
        self.geometry_source.common.state = previous.geometry_source.common.state;
        self.painted_bounds = Arc::clone(&previous.painted_bounds);
    }

    fn accepts_text_input(&self) -> bool {
        self.input.as_ref().is_some_and(Widget::accepts_text_input)
    }

    fn accepts_pointer_move(&self) -> bool {
        false
    }

    fn accepts_pointer_input(&self, input: &WidgetInput) -> bool {
        let accepts_event = self.editing()
            || matches!(
                input,
                WidgetInput::PointerPress {
                    button: PointerButton::Primary,
                    ..
                }
            );
        accepts_event
            && input.pointer_position().is_none_or(|position| {
                self.painted_bounds
                    .lock()
                    .ok()
                    .and_then(|bounds| *bounds)
                    .and_then(|bounds| self.label_rect(bounds))
                    .is_some_and(|rect| rect.contains(position))
            })
    }

    fn capabilities(&self) -> WidgetCapabilities<'_> {
        WidgetCapabilities::new().semantics(self)
    }

    fn selected_text_slice(&self) -> Option<&str> {
        self.input.as_ref().and_then(Widget::selected_text_slice)
    }

    fn selected_text(&self) -> Option<String> {
        self.input.as_ref().and_then(Widget::selected_text)
    }

    fn append_paint(
        &self,
        primitives: &mut Vec<radiant::runtime::PaintPrimitive>,
        bounds: ui::Rect,
        layout: &ui::LayoutOutput,
        theme: &ui::ThemeTokens,
    ) {
        self.append_paint_impl(primitives, bounds, layout, theme);
    }
}

impl WidgetSemantics for PlaymarkLabelWidget {
    fn automation_role(&self) -> AutomationRole {
        if self.editing() {
            AutomationRole::TextInput
        } else {
            AutomationRole::Button
        }
    }

    fn automation_label(&self) -> Option<String> {
        Some(String::from("Playmark length"))
    }

    fn automation_description(&self) -> Option<String> {
        self.error.clone().or_else(|| {
            (!self.editing()).then(|| String::from("Click to edit the playmark length"))
        })
    }

    fn automation_value_text(&self) -> Option<String> {
        self.input
            .as_ref()
            .map(|input| input.state.value.clone())
            .or_else(|| Some(self.label.clone()))
    }
}

impl PlaymarkLabelWidget {
    fn append_paint_impl(
        &self,
        primitives: &mut Vec<radiant::runtime::PaintPrimitive>,
        bounds: ui::Rect,
        layout: &ui::LayoutOutput,
        theme: &ui::ThemeTokens,
    ) {
        if let Ok(mut painted_bounds) = self.painted_bounds.lock() {
            *painted_bounds = Some(bounds);
        }
        let Some(rect) = self.label_rect(bounds) else {
            return;
        };
        if let Some(input) = &self.input {
            input.append_paint(primitives, rect, layout, theme);
            return;
        }
        if matches!(
            self.geometry_source.active_drag_kind,
            Some(
                super::WaveformActiveDragKind::Selection(super::WaveformSelectionKind::Play)
                    | super::WaveformActiveDragKind::SelectionMove(
                        super::WaveformSelectionKind::Play
                    )
                    | super::WaveformActiveDragKind::SelectionResize(
                        super::WaveformSelectionKind::Play,
                        _
                    )
            )
        ) {
            return;
        }
        let Some(selection) = self.geometry_source.play_selection else {
            return;
        };
        let Some(geometry) = self
            .geometry_source
            .selection_geometry(bounds, Some(selection))
        else {
            return;
        };
        let mut paint = WidgetPaint::new(primitives, self.common().id);
        self.geometry_source
            .append_playmark_label_paint(&mut paint, bounds, geometry, selection);
    }
}

pub(super) fn parsed_playmark_frame_range(
    text: &str,
    sample_rate: u32,
    total_frames: usize,
    beat_count: u8,
    current: wavecrate::selection::SelectionRange,
) -> Result<(usize, usize), String> {
    if total_frames == 0 {
        return Err(String::from("The loaded sample has no audio frames"));
    }
    let seconds = parse_length_seconds(text, beat_count)?;
    let requested_frames = (seconds * f64::from(sample_rate.max(1))).round();
    if !requested_frames.is_finite() || requested_frames < 1.0 {
        return Err(String::from(
            "Playmark length must be at least one sample frame",
        ));
    }
    let requested_frames = requested_frames as usize;
    let current = current.frame_bounds(total_frames);
    // Duration edits are anchored to the existing start edge.  The end edge
    // grows or shrinks in place, and the waveform bound remains the final
    // clamp when the requested duration extends past the sample.
    let start = current.start_frame;
    let end = start.saturating_add(requested_frames).min(total_frames);
    Ok((start, end))
}

fn parse_length_seconds(text: &str, beat_count: u8) -> Result<f64, String> {
    let compact = text
        .chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    if let Some(number) = compact.strip_suffix("bpm") {
        let bpm = positive_number(number)?;
        if beat_count == 0 {
            return Err(String::from(
                "Set a positive beat count before entering BPM",
            ));
        }
        return Ok(f64::from(beat_count) * 60.0 / bpm);
    }

    let mut remaining = compact.as_str();
    let mut seconds = 0.0;
    let mut components = 0;
    while !remaining.is_empty() {
        let number_end = remaining
            .char_indices()
            .take_while(|(_, character)| character.is_ascii_digit() || *character == '.')
            .map(|(index, character)| index + character.len_utf8())
            .last()
            .unwrap_or(0);
        if number_end == 0 {
            return Err(String::from(
                "Use a value such as 1000ms, 1.5s, 1min, or 142bpm",
            ));
        }
        let number = non_negative_number(&remaining[..number_end])?;
        remaining = &remaining[number_end..];
        let (factor, unit_len) = if remaining.starts_with("min") {
            (60.0, 3)
        } else if remaining.starts_with("ms") {
            (0.001, 2)
        } else if remaining.starts_with('m') {
            (60.0, 1)
        } else if remaining.starts_with('s') {
            (1.0, 1)
        } else {
            return Err(String::from(
                "Use a value such as 1000ms, 1.5s, 1min, or 142bpm",
            ));
        };
        seconds += number * factor;
        components += 1;
        remaining = &remaining[unit_len..];
    }
    if components == 0 || !seconds.is_finite() || seconds <= 0.0 {
        return Err(String::from("Playmark length must be positive and finite"));
    }
    Ok(seconds)
}

fn positive_number(text: &str) -> Result<f64, String> {
    text.parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| String::from("Playmark length must be positive and finite"))
}

fn non_negative_number(text: &str) -> Result<f64, String> {
    text.parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| String::from("Playmark length must be positive and finite"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_app::waveform::WaveformWidgetProps;
    use crate::native_app::waveform::{WaveformInteraction, WaveformSelectionKind};
    #[test]
    fn parses_supported_time_and_bpm_forms_case_insensitively() {
        assert_eq!(parse_length_seconds(" 1000 MS ", 4), Ok(1.0));
        assert_eq!(parse_length_seconds("1.5s", 4), Ok(1.5));
        assert_eq!(parse_length_seconds("1 MIN", 4), Ok(60.0));
        assert_eq!(parse_length_seconds("2m 05.00s", 4), Ok(125.0));
        assert_eq!(parse_length_seconds("2m 00s", 4), Ok(120.0));
        assert_eq!(parse_length_seconds("120 bpm", 4), Ok(2.0));
    }

    #[test]
    fn duration_edits_keep_start_edge_and_resize_end_edge() {
        let current = wavecrate::selection::SelectionRange::from_frame_bounds(1_000, 250, 950);
        assert_eq!(
            parsed_playmark_frame_range("0.4s", 1_000, 1_000, 4, current),
            Ok((250, 650))
        );
    }

    #[test]
    fn duration_edits_clamp_end_without_moving_start() {
        let current = wavecrate::selection::SelectionRange::from_frame_bounds(1_000, 850, 950);
        assert_eq!(
            parsed_playmark_frame_range("0.4s", 1_000, 1_000, 4, current),
            Ok((850, 1_000))
        );
    }

    #[test]
    fn duration_edits_longer_than_sample_clamp_end_without_moving_start() {
        let current = wavecrate::selection::SelectionRange::from_frame_bounds(1_000, 250, 400);
        assert_eq!(
            parsed_playmark_frame_range("2s", 1_000, 1_000, 4, current),
            Ok((250, 1_000))
        );
    }

    #[test]
    fn rejects_invalid_too_short_and_too_long_lengths() {
        let current = wavecrate::selection::SelectionRange::from_frame_bounds(100, 20, 40);
        assert!(parse_length_seconds("NaNs", 4).is_err());
        assert!(parsed_playmark_frame_range("0.0001s", 1_000, 100, 4, current).is_err());
        assert_eq!(
            parsed_playmark_frame_range("1s", 1_000, 100, 4, current),
            Ok((20, 100))
        );
    }

    #[test]
    fn label_widget_only_claims_pointer_input_inside_its_painted_label() {
        let mut state = WaveformState::synthetic_for_tests();
        state.set_play_selection_range(0.25, 0.75);
        let geometry_source =
            WaveformWidget::new(WaveformWidgetProps::from_state(&state, false, false, 4));
        let mut widget = PlaymarkLabelWidget::new(14, geometry_source, &state, false, 4)
            .expect("playmark label widget");
        let bounds =
            ui::Rect::from_min_max(ui::Point::new(20.0, 40.0), ui::Point::new(1_220.0, 360.0));
        *widget.painted_bounds.lock().expect("painted bounds") = Some(bounds);
        let label_rect = widget.label_rect(bounds).expect("label rect");
        assert_eq!(
            widget.automation_description().as_deref(),
            Some("Click to edit the playmark length")
        );

        let inside = WidgetInput::primary_press(ui::Point::new(
            label_rect.min.x + 1.0,
            label_rect.min.y + 1.0,
        ));
        let outside =
            WidgetInput::primary_press(ui::Point::new(bounds.min.x + 1.0, bounds.min.y + 1.0));

        assert!(widget.accepts_pointer_input(&inside));
        assert!(widget.handle_input(bounds, inside).is_some());
        assert!(!widget.accepts_pointer_input(&outside));
    }

    #[test]
    fn new_play_selection_discards_stale_label_editor_draft() {
        let mut state = WaveformState::synthetic_for_tests();
        state.set_play_selection_range(0.25, 0.75);
        assert!(state.begin_playmark_label_edit(false, 4));
        state.update_playmark_label_draft(String::from("stale value"));

        state.apply_interaction(WaveformInteraction::BeginSelection {
            kind: WaveformSelectionKind::Play,
            visible_ratio: 0.1,
        });
        assert!(!state.playmark_label_editor_active());
        state.apply_interaction(WaveformInteraction::UpdateSelection { visible_ratio: 0.3 });
        state.apply_interaction(WaveformInteraction::FinishSelection { visible_ratio: 0.3 });

        assert!(state.begin_playmark_label_edit(false, 4));
        let editor = state.playmark_label_editor.as_ref().expect("fresh editor");
        assert_ne!(editor.draft, "stale value");
        assert_eq!(editor.draft, editor.original_draft);
    }
}
