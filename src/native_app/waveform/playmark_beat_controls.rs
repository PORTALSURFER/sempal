use radiant::{
    gui::automation::AutomationRole,
    prelude as ui,
    widgets::{
        ButtonWidget, Widget, WidgetCapabilities, WidgetCommon, WidgetInput, WidgetKey,
        WidgetOutput, WidgetSemantics, WidgetSizing,
    },
};
use std::sync::{Arc, Mutex};

use crate::native_app::{
    app::GuiMessage,
    app_chrome::toolbar::{
        BeatGuideCountInputMessage, BeatGuideCountInputWidget, beat_guide_count_input_message,
    },
};

use super::{WaveformState, WaveformWidget, playmark_label};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaymarkBeatToggleMessage {
    Toggle,
}

#[derive(Clone, Debug)]
struct PlaymarkBeatToggleWidget {
    geometry_source: WaveformWidget,
    label: String,
    button: ButtonWidget,
    checked: bool,
    painted_bounds: Arc<Mutex<Option<ui::Rect>>>,
}

impl PlaymarkBeatToggleWidget {
    fn new(
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
        let mut button = ButtonWidget::new(
            id,
            "G",
            WidgetSizing::fixed(ui::Vector2::new(
                playmark_label::PLAYMARK_BEAT_TOGGLE_WIDTH,
                playmark_label::PLAYMARK_LABEL_HEIGHT,
            )),
        );
        button.common.state.active = beat_guides_enabled;
        button.common.paint.paints_focus = true;
        let mut geometry_source = geometry_source;
        geometry_source.common.focus = radiant::widgets::FocusBehavior::Keyboard;
        Some(Self {
            geometry_source,
            label,
            button,
            checked: beat_guides_enabled,
            painted_bounds: Arc::new(Mutex::new(None)),
        })
    }

    fn control_rect(&self, bounds: ui::Rect) -> Option<ui::Rect> {
        playmark_control_layout(&self.geometry_source, &self.label, bounds)
            .map(|layout| layout.beat_toggle_rect)
    }
}

impl Widget for PlaymarkBeatToggleWidget {
    fn common(&self) -> &WidgetCommon {
        self.geometry_source.common()
    }

    fn common_mut(&mut self) -> &mut WidgetCommon {
        self.geometry_source.common_mut()
    }

    fn handle_input(&mut self, bounds: ui::Rect, input: WidgetInput) -> Option<WidgetOutput> {
        let rect = self.control_rect(bounds)?;
        self.button
            .handle_input(rect, input)
            .map(|_| WidgetOutput::typed(PlaymarkBeatToggleMessage::Toggle))
    }

    fn synchronize_from_previous(&mut self, previous: &dyn Widget) {
        let Some(previous) = previous.as_any().downcast_ref::<Self>() else {
            return;
        };
        self.button.synchronize_from_previous(&previous.button);
        self.button.common.state.active = self.checked;
        self.geometry_source.common.state = previous.geometry_source.common.state;
        self.painted_bounds = Arc::clone(&previous.painted_bounds);
    }

    fn accepts_pointer_move(&self) -> bool {
        true
    }

    fn accepts_pointer_input(&self, input: &WidgetInput) -> bool {
        pointer_is_inside_control(input, &self.painted_bounds, |bounds| {
            self.control_rect(bounds)
        })
    }

    fn capabilities(&self) -> WidgetCapabilities<'_> {
        WidgetCapabilities::new().semantics(self)
    }

    fn append_paint(
        &self,
        primitives: &mut Vec<radiant::runtime::PaintPrimitive>,
        bounds: ui::Rect,
        layout: &ui::LayoutOutput,
        theme: &ui::ThemeTokens,
    ) {
        remember_painted_bounds(&self.painted_bounds, bounds);
        if let Some(rect) = self.control_rect(bounds) {
            self.button.append_paint(primitives, rect, layout, theme);
        }
    }
}

impl WidgetSemantics for PlaymarkBeatToggleWidget {
    fn automation_role(&self) -> AutomationRole {
        AutomationRole::Toggle
    }

    fn automation_label(&self) -> Option<String> {
        Some(String::from("Playmark beat grid"))
    }

    fn automation_description(&self) -> Option<String> {
        Some(String::from("Show beat guide lines inside the playmark"))
    }

    fn automation_checked(&self) -> Option<bool> {
        Some(self.checked)
    }
}

#[derive(Clone, Debug)]
struct PlaymarkBeatCountWidget {
    geometry_source: WaveformWidget,
    label: String,
    input: BeatGuideCountInputWidget,
    painted_bounds: Arc<Mutex<Option<ui::Rect>>>,
}

impl PlaymarkBeatCountWidget {
    fn new(
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
        let mut geometry_source = geometry_source;
        geometry_source.common.focus = radiant::widgets::FocusBehavior::Keyboard;
        Some(Self {
            geometry_source,
            label,
            input: BeatGuideCountInputWidget::new(
                id,
                beat_guide_count,
                playmark_label::PLAYMARK_BEAT_COUNT_WIDTH,
                playmark_label::PLAYMARK_LABEL_HEIGHT,
            ),
            painted_bounds: Arc::new(Mutex::new(None)),
        })
    }

    fn control_rect(&self, bounds: ui::Rect) -> Option<ui::Rect> {
        playmark_control_layout(&self.geometry_source, &self.label, bounds)
            .map(|layout| layout.beat_count_rect)
    }
}

impl Widget for PlaymarkBeatCountWidget {
    fn common(&self) -> &WidgetCommon {
        self.geometry_source.common()
    }

    fn common_mut(&mut self) -> &mut WidgetCommon {
        self.geometry_source.common_mut()
    }

    fn handle_input(&mut self, bounds: ui::Rect, input: WidgetInput) -> Option<WidgetOutput> {
        self.input.handle_input(self.control_rect(bounds)?, input)
    }

    fn synchronize_from_previous(&mut self, previous: &dyn Widget) {
        let Some(previous) = previous.as_any().downcast_ref::<Self>() else {
            return;
        };
        self.input.synchronize_from_previous(&previous.input);
        self.geometry_source.common.state = previous.geometry_source.common.state;
        self.painted_bounds = Arc::clone(&previous.painted_bounds);
    }

    fn accepts_text_input(&self) -> bool {
        self.input.accepts_text_input()
    }

    fn preempts_host_shortcut_key(&self, key: WidgetKey) -> bool {
        self.input.preempts_host_shortcut_key(key)
    }

    fn accepts_pointer_move(&self) -> bool {
        self.input.accepts_pointer_move()
    }

    fn accepts_wheel_input(&self) -> bool {
        self.input.accepts_wheel_input()
    }

    fn accepts_pointer_input(&self, input: &WidgetInput) -> bool {
        self.input.accepts_pointer_input(input)
            && pointer_is_inside_control(input, &self.painted_bounds, |bounds| {
                self.control_rect(bounds)
            })
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
        remember_painted_bounds(&self.painted_bounds, bounds);
        if let Some(rect) = self.control_rect(bounds) {
            self.input.append_paint(primitives, rect, layout, theme);
        }
    }
}

impl WidgetSemantics for PlaymarkBeatCountWidget {
    fn automation_role(&self) -> AutomationRole {
        self.input.automation_role()
    }

    fn automation_label(&self) -> Option<String> {
        Some(String::from("Playmark beat count"))
    }

    fn automation_value_text(&self) -> Option<String> {
        self.input.automation_value_text()
    }
}

pub(super) fn playmark_beat_control_views(
    toggle_id: u64,
    count_id: u64,
    geometry_source: WaveformWidget,
    state: &WaveformState,
    beat_guides_enabled: bool,
    beat_guide_count: u8,
) -> [Option<ui::View<GuiMessage>>; 2] {
    let toggle = PlaymarkBeatToggleWidget::new(
        toggle_id,
        geometry_source.clone(),
        state,
        beat_guides_enabled,
        beat_guide_count,
    )
    .map(|widget| {
        ui::custom_widget(widget, |output| {
            output
                .typed_copied::<PlaymarkBeatToggleMessage>()
                .map(|_| GuiMessage::ToggleBeatGuides)
        })
        .id(toggle_id)
        .size(super::WAVEFORM_WIDTH as f32, super::WAVEFORM_HEIGHT as f32)
    });
    let count = beat_guides_enabled
        .then(|| {
            PlaymarkBeatCountWidget::new(
                count_id,
                geometry_source,
                state,
                beat_guides_enabled,
                beat_guide_count,
            )
        })
        .flatten()
        .map(|widget| {
            ui::custom_widget(widget, |output| {
                output
                    .typed_cloned::<BeatGuideCountInputMessage>()
                    .map(beat_guide_count_input_message)
            })
            .id(count_id)
            .size(super::WAVEFORM_WIDTH as f32, super::WAVEFORM_HEIGHT as f32)
        });
    [toggle, count]
}

fn playmark_control_layout(
    geometry_source: &WaveformWidget,
    label: &str,
    bounds: ui::Rect,
) -> Option<playmark_label::PlaymarkLabelLayout> {
    let selection = geometry_source.play_selection?;
    let geometry = geometry_source.selection_geometry(bounds, Some(selection))?;
    playmark_label::playmark_label_layout(bounds, geometry.rect, label.len())
}

fn pointer_is_inside_control(
    input: &WidgetInput,
    painted_bounds: &Arc<Mutex<Option<ui::Rect>>>,
    rect: impl FnOnce(ui::Rect) -> Option<ui::Rect>,
) -> bool {
    input.pointer_position().is_none_or(|position| {
        painted_bounds
            .lock()
            .ok()
            .and_then(|bounds| *bounds)
            .and_then(rect)
            .is_some_and(|bounds| bounds.contains(position))
    })
}

fn remember_painted_bounds(painted_bounds: &Arc<Mutex<Option<ui::Rect>>>, bounds: ui::Rect) {
    if let Ok(mut painted_bounds) = painted_bounds.lock() {
        *painted_bounds = Some(bounds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radiant::widgets::{PointerButton, PointerModifiers};

    fn rect(left: f32, top: f32, width: f32, height: f32) -> ui::Rect {
        ui::Rect::from_min_size(ui::Point::new(left, top), ui::Vector2::new(width, height))
    }

    #[test]
    fn local_control_hit_testing_only_claims_its_own_painted_rect() {
        let mut state = WaveformState::synthetic_for_tests();
        state.set_play_selection_range(0.4, 0.6);
        let geometry_source = WaveformWidget::new(super::super::WaveformWidgetProps::from_state(
            &state, false, false, 4,
        ));
        let toggle = PlaymarkBeatToggleWidget::new(15, geometry_source, &state, false, 4)
            .expect("toggle widget");
        let bounds = rect(0.0, 0.0, 400.0, 80.0);
        toggle.append_paint(
            &mut Vec::new(),
            bounds,
            &Default::default(),
            &Default::default(),
        );
        let toggle_rect = toggle.control_rect(bounds).expect("toggle rect");

        assert!(toggle.accepts_pointer_input(&WidgetInput::PointerPress {
            position: toggle_rect.center(),
            button: PointerButton::Primary,
            modifiers: PointerModifiers::default(),
        }));
        assert!(!toggle.accepts_pointer_input(&WidgetInput::PointerPress {
            position: ui::Point::new(bounds.min.x + 2.0, bounds.min.y + 2.0),
            button: PointerButton::Primary,
            modifiers: PointerModifiers::default(),
        }));
    }

    #[test]
    fn local_toggle_exposes_checked_semantics_and_activates() {
        let mut state = WaveformState::synthetic_for_tests();
        state.set_play_selection_range(0.4, 0.6);
        let geometry_source = WaveformWidget::new(super::super::WaveformWidgetProps::from_state(
            &state, true, false, 4,
        ));
        let mut toggle = PlaymarkBeatToggleWidget::new(15, geometry_source, &state, true, 4)
            .expect("toggle widget");
        let bounds = rect(0.0, 0.0, 400.0, 80.0);
        let control = toggle.control_rect(bounds).expect("toggle rect");
        let modifiers = PointerModifiers::default();

        assert_eq!(toggle.automation_role(), AutomationRole::Toggle);
        assert_eq!(
            toggle.automation_label().as_deref(),
            Some("Playmark beat grid")
        );
        assert_eq!(toggle.automation_checked(), Some(true));
        assert!(
            toggle
                .handle_input(
                    bounds,
                    WidgetInput::PointerPress {
                        position: control.center(),
                        button: PointerButton::Primary,
                        modifiers,
                    },
                )
                .is_none()
        );
        let output = toggle
            .handle_input(
                bounds,
                WidgetInput::PointerRelease {
                    position: control.center(),
                    button: PointerButton::Primary,
                    modifiers,
                },
            )
            .expect("toggle activation");
        assert_eq!(
            output.typed_copied::<PlaymarkBeatToggleMessage>(),
            Some(PlaymarkBeatToggleMessage::Toggle)
        );
    }

    #[test]
    fn local_count_reuses_focused_bounded_step_contract() {
        let mut state = WaveformState::synthetic_for_tests();
        state.set_play_selection_range(0.4, 0.6);
        let geometry_source = WaveformWidget::new(super::super::WaveformWidgetProps::from_state(
            &state, false, false, 4,
        ));
        let mut count = PlaymarkBeatCountWidget::new(16, geometry_source, &state, false, 4)
            .expect("count widget");
        let bounds = rect(0.0, 0.0, 400.0, 80.0);

        assert_eq!(count.automation_role(), AutomationRole::TextInput);
        assert_eq!(
            count.automation_label().as_deref(),
            Some("Playmark beat count")
        );
        assert_eq!(count.automation_value_text().as_deref(), Some("4"));
        assert!(
            count
                .handle_input(bounds, WidgetInput::FocusChanged(true))
                .is_none()
        );
        assert!(count.preempts_host_shortcut_key(WidgetKey::ArrowUp));
        let output = count
            .handle_input(bounds, WidgetInput::KeyPress(WidgetKey::ArrowUp))
            .expect("focused up step");
        assert_eq!(
            output.typed_cloned::<BeatGuideCountInputMessage>(),
            Some(BeatGuideCountInputMessage::Set(5))
        );
    }
}
