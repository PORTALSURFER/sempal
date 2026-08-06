use radiant::prelude as ui;

use crate::native_app::app::GuiMessage;
use crate::native_app::app_chrome::view_models::library_sidebar::HarvestFamilyViewModel;
use crate::native_app::ui::ids as widget_ids;

pub(super) const HARVEST_FAMILY_SECTION_HEIGHT: f32 = 106.0;
const HARVEST_FAMILY_ROW_HEIGHT: f32 = 18.0;
const HARVEST_FAMILY_ACTION_HEIGHT: f32 = 20.0;
const HARVEST_FAMILY_LABEL_WIDTH: f32 = 50.0;
const HARVEST_FAMILY_COUNT_WIDTH: f32 = 42.0;
const HARVEST_FAMILY_BUTTON_WIDTH: f32 = 112.0;
const HARVEST_FAMILY_CONTENT_SPACING: f32 = 2.0;
const HARVEST_FAMILY_PANEL_PADDING: f32 = 6.0;

pub(super) fn harvest_family_section(model: &HarvestFamilyViewModel) -> ui::View<GuiMessage> {
    let content = ui::column([
        harvest_info_row("State", model.state_label.clone(), None),
        harvest_info_row(
            "Origin",
            model.origin_count_label.clone(),
            model.origin_detail.clone(),
        ),
        harvest_info_row(
            "Derivs",
            model.derivative_count_label.clone(),
            model.derivative_detail.clone(),
        ),
        harvest_action_row(model),
    ])
    .spacing(HARVEST_FAMILY_CONTENT_SPACING)
    .fill_width();

    let panel = ui::panel_section("Harvest", content, HARVEST_FAMILY_SECTION_HEIGHT)
        .padding(HARVEST_FAMILY_PANEL_PADDING)
        .fill_width();
    #[cfg(test)]
    {
        panel.id(widget_ids::HARVEST_FAMILY_SECTION_ID)
    }
    #[cfg(not(test))]
    {
        panel
    }
}

fn harvest_info_row(
    label: &'static str,
    value: String,
    detail: Option<String>,
) -> ui::View<GuiMessage> {
    let detail = detail.unwrap_or_default();
    ui::row([
        ui::text_line(label, HARVEST_FAMILY_ROW_HEIGHT)
            .muted_text()
            .width(HARVEST_FAMILY_LABEL_WIDTH),
        ui::text_line(value, HARVEST_FAMILY_ROW_HEIGHT).width(HARVEST_FAMILY_COUNT_WIDTH),
        ui::text_line(detail, HARVEST_FAMILY_ROW_HEIGHT)
            .muted_text()
            .fill_width(),
    ])
    .spacing(4.0)
    .fill_width()
    .height(HARVEST_FAMILY_ROW_HEIGHT)
}

fn harvest_action_row(model: &HarvestFamilyViewModel) -> ui::View<GuiMessage> {
    let mut buttons = Vec::with_capacity(3);
    if model.can_show_origin {
        buttons.push(harvest_action_button(
            "Show Origin",
            widget_ids::HARVEST_FAMILY_ORIGIN_BUTTON_ID,
            GuiMessage::ShowSelectedSampleHarvestOrigin,
        ));
    }
    if model.can_show_derivatives {
        buttons.push(harvest_action_button(
            "Show Derivs",
            widget_ids::HARVEST_FAMILY_DERIVATIVES_BUTTON_ID,
            GuiMessage::ShowSelectedSampleHarvestDerivatives,
        ));
    }
    if model.can_open_destination {
        buttons.push(harvest_action_button(
            "Open Dest",
            widget_ids::HARVEST_FAMILY_DESTINATION_BUTTON_ID,
            GuiMessage::OpenSelectedSampleHarvestDestination,
        ));
    }
    ui::row(buttons)
        .spacing(4.0)
        .fill_width()
        .height(HARVEST_FAMILY_ACTION_HEIGHT)
}

fn harvest_action_button(
    label: &'static str,
    id: u64,
    message: GuiMessage,
) -> ui::View<GuiMessage> {
    ui::button(label)
        .subtle()
        .message(message)
        .id(id)
        .size(HARVEST_FAMILY_BUTTON_WIDTH, HARVEST_FAMILY_ACTION_HEIGHT)
}

#[cfg(test)]
mod tests {
    use radiant::prelude::IntoView;
    use radiant::widgets::ButtonMessage;

    use super::*;

    fn harvest_family_model() -> HarvestFamilyViewModel {
        HarvestFamilyViewModel {
            state_label: String::from("Touched"),
            origin_count_label: String::from("1"),
            derivative_count_label: String::from("3"),
            origin_detail: Some(String::from("jam.wav")),
            derivative_detail: Some(String::from("jam chop 01.wav")),
            can_show_origin: true,
            can_show_derivatives: true,
            can_open_destination: true,
        }
    }

    #[test]
    fn harvest_family_buttons_route_selected_sample_actions() {
        assert_eq!(
            harvest_family_section(&harvest_family_model()).view_dispatch_widget_output(
                widget_ids::HARVEST_FAMILY_ORIGIN_BUTTON_ID,
                ui::WidgetOutput::typed(ButtonMessage::Activate {
                    provenance: ui::InteractionProvenance::Programmatic,
                }),
            ),
            Some(GuiMessage::ShowSelectedSampleHarvestOrigin)
        );
        assert_eq!(
            harvest_family_section(&harvest_family_model()).view_dispatch_widget_output(
                widget_ids::HARVEST_FAMILY_DERIVATIVES_BUTTON_ID,
                ui::WidgetOutput::typed(ButtonMessage::Activate {
                    provenance: ui::InteractionProvenance::Programmatic,
                }),
            ),
            Some(GuiMessage::ShowSelectedSampleHarvestDerivatives)
        );
        assert_eq!(
            harvest_family_section(&harvest_family_model()).view_dispatch_widget_output(
                widget_ids::HARVEST_FAMILY_DESTINATION_BUTTON_ID,
                ui::WidgetOutput::typed(ButtonMessage::Activate {
                    provenance: ui::InteractionProvenance::Programmatic,
                }),
            ),
            Some(GuiMessage::OpenSelectedSampleHarvestDestination)
        );
    }

    #[test]
    fn harvest_family_panel_paints_state_counts_and_examples() {
        let frame = harvest_family_section(&harvest_family_model())
            .view_frame_at_size_with_default_theme(ui::Vector2::new(260.0, 120.0));

        for text in [
            "Harvest",
            "Touched",
            "1",
            "3",
            "jam.wav",
            "jam chop 01.wav",
            "Show Origin",
            "Show Derivs",
            "Open Dest",
        ] {
            assert!(
                frame.paint_plan.contains_text(text),
                "harvest family panel should paint {text}"
            );
        }
    }
}
