mod beat_count_input;
mod icons;
mod identity;
mod projection;

use radiant::{prelude as ui, widgets::ButtonMessage};

use crate::native_app::app::GuiMessage;
use crate::native_app::app_chrome::view_models::toolbar::MainToolbarViewModel;
pub(in crate::native_app) use beat_count_input::{
    BeatGuideCountInputMessage, BeatGuideCountInputWidget,
};

pub(in crate::native_app) use icons::{ToolbarIcon, toolbar_icon_color, toolbar_icon_glyph};
pub(in crate::native_app) use projection::{
    ToolbarControlProjection, ToolbarIconButtonProjection, ToolbarProjection,
};

pub(in crate::native_app) const TOOLBAR_FOCUS_LOADED_ID: u64 = identity::TOOLBAR_FOCUS_LOADED_ID;
pub(in crate::native_app) const TOOLBAR_APPLY_EDIT_MARK_EDITS_ID: u64 =
    identity::TOOLBAR_APPLY_EDIT_MARK_EDITS_ID;
pub(in crate::native_app) const TOOLBAR_SIMILAR_SECTIONS_ID: u64 =
    identity::TOOLBAR_SIMILAR_SECTIONS_ID;
pub(in crate::native_app) const TOOLBAR_STOP_ID: u64 = identity::TOOLBAR_STOP_ID;
pub(in crate::native_app) const TOOLBAR_RANDOM_ID: u64 = identity::TOOLBAR_RANDOM_ID;
pub(in crate::native_app) const TOOLBAR_ZERO_CROSSING_SNAP_ID: u64 =
    identity::TOOLBAR_ZERO_CROSSING_SNAP_ID;

#[cfg(test)]
pub(in crate::native_app) fn main_toolbar(model: MainToolbarViewModel) -> ui::View<GuiMessage> {
    radiant::application::toolbar_from_parts(
        radiant::application::ToolbarParts::new(main_toolbar_controls(model))
            .alignment(ui::ToolbarAlignment::End),
    )
}

pub(in crate::native_app) fn main_toolbar_controls(
    model: MainToolbarViewModel,
) -> Vec<ui::View<GuiMessage>> {
    let projection = ToolbarProjection::from_model(model);
    let mut utility_controls = Vec::with_capacity(projection.controls.len());
    let mut playback_controls = Vec::with_capacity(4);
    for control in &projection.controls {
        let view = toolbar_control(*control, projection.help_tooltips_enabled);
        if control_is_playback(*control) {
            playback_controls.push(view);
        } else {
            utility_controls.push(view);
        }
    }
    if !playback_controls.is_empty() {
        utility_controls.push(ui::spacer().width(6.0).height(24.0));
        utility_controls.push(
            ui::row(playback_controls)
                .spacing(2.0)
                .height(24.0)
                .intrinsic(),
        );
    }
    utility_controls
}

fn control_is_playback(control: ToolbarControlProjection) -> bool {
    matches!(
        control,
        ToolbarControlProjection::Icon(button)
            if matches!(
                button.icon,
                ToolbarIcon::Play | ToolbarIcon::Stop | ToolbarIcon::Random | ToolbarIcon::Loop
            )
    )
}

fn toolbar_control(
    control: ToolbarControlProjection,
    help_tooltips_enabled: bool,
) -> ui::View<GuiMessage> {
    match control {
        ToolbarControlProjection::Icon(button) => toolbar_icon_button_from_projection(button)
            .tooltip_if(help_tooltips_enabled, button.tooltip),
        ToolbarControlProjection::BeatGuideCountField {
            count,
            id,
            key,
            tooltip,
        } => beat_guide_count_field(count, id, key).tooltip_if(help_tooltips_enabled, tooltip),
        ToolbarControlProjection::ApplyEditMarkEdits { id, tooltip } => {
            apply_edit_mark_edits_button(id).tooltip_if(help_tooltips_enabled, tooltip)
        }
    }
}

fn beat_guide_count_field(count: u8, id: u64, key: &'static str) -> ui::View<GuiMessage> {
    ui::custom_widget_mapped(
        BeatGuideCountInputWidget::new(id, count, 34.0, 24.0),
        beat_guide_count_input_message,
    )
    .key(key)
    .id(id)
    .size(34.0, 24.0)
}

pub(in crate::native_app) fn beat_guide_count_input_message(
    message: BeatGuideCountInputMessage,
) -> GuiMessage {
    match message {
        BeatGuideCountInputMessage::Changed(value) => GuiMessage::ChangeBeatGuideCountInput(value),
        BeatGuideCountInputMessage::Committed(value) => {
            GuiMessage::CommitBeatGuideCountInput(value)
        }
        BeatGuideCountInputMessage::Set(count) => GuiMessage::SetBeatGuideCount(count),
    }
}

fn apply_edit_mark_edits_button(id: u64) -> ui::View<GuiMessage> {
    ui::button("Apply")
        .style(ui::WidgetStyle::strong(ui::WidgetTone::Accent))
        .message(GuiMessage::RequestApplyEditSelectionEffects)
        .id(id)
        .size(58.0, 24.0)
}

#[cfg(test)]
pub(in crate::native_app) fn toolbar_icon_button(
    id: u64,
    icon: ToolbarIcon,
    enabled: bool,
    active: bool,
) -> ui::View<GuiMessage> {
    toolbar_icon_button_from_projection(ToolbarIconButtonProjection {
        id,
        icon,
        enabled,
        icon_enabled: enabled,
        active,
        tooltip: "",
    })
}

fn toolbar_icon_button_from_projection(
    button: ToolbarIconButtonProjection,
) -> ui::View<GuiMessage> {
    ui::icon_button(toolbar_icon_glyph(
        button.icon,
        button.icon_enabled,
        button.active,
    ))
    .enabled(button.enabled)
    .active(button.active)
    .mapped(move |message| toolbar_button_message(button.icon, message))
    .id(button.id)
    .size(28.0, 24.0)
}

fn toolbar_button_message(icon: ToolbarIcon, message: ButtonMessage) -> GuiMessage {
    match icon {
        ToolbarIcon::FocusLoaded => GuiMessage::FocusLoadedFile,
        ToolbarIcon::Loop => GuiMessage::ToggleLoopPlayback,
        ToolbarIcon::Random
            if message
                .activation_modifiers()
                .is_some_and(|modifiers| modifiers.command) =>
        {
            GuiMessage::ToggleStickyRandomSampleRangePlayback
        }
        ToolbarIcon::Random
            if message
                .activation_modifiers()
                .is_some_and(|modifiers| modifiers.shift) =>
        {
            GuiMessage::PlayRandomListedSampleRange
        }
        ToolbarIcon::Random => GuiMessage::PlayRandomSampleRange,
        ToolbarIcon::SimilarSections => GuiMessage::ToggleSimilarSections,
        ToolbarIcon::ZeroCrossingSnap => GuiMessage::ToggleZeroCrossingSnap,
        ToolbarIcon::BpmSnap => GuiMessage::ToggleBpmSnap,
        ToolbarIcon::BeatGuides => GuiMessage::ToggleBeatGuides,
        ToolbarIcon::Metronome => GuiMessage::ToggleMetronome,
        ToolbarIcon::Play => GuiMessage::PlaySelectedSample,
        ToolbarIcon::Stop => GuiMessage::StopPlayback,
    }
}
