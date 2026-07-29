use radiant::{gui::paint::BorderSides, prelude as ui, widgets::TextInputMessage};

mod projection;

use self::projection::{
    CurationFilterOptionProjection, CurationFilterRowProjection, HarvestFilterOptionProjection,
    HarvestFilterRowProjection, PlaybackTypeFilterRowProjection,
    PlaybackTypeFilterToggleProjection, RatingFilterRowProjection, RatingFilterToggleProjection,
    TextFilterField, TextFilterRowProjection, filter_rows_projection,
};
use crate::native_app::app::GuiMessage;
use crate::native_app::app_chrome::library_browser::library_sidebar::sidebar_row::sidebar_row_underlay;
use crate::native_app::app_chrome::palette::ListItemState;
use crate::native_app::app_chrome::view_models::library_sidebar::FilterSectionViewModel;
use crate::native_app::sample_library::folder_browser::commands::{
    FilterFamily, FolderBrowserMessage,
};
use crate::native_app::sample_library::folder_browser::model::{
    BrowserCurationScope, PlaybackTypeFilter,
};
pub(super) use crate::native_app::sample_library::folder_browser::view_contract::{
    FILTER_CONTROLS_CONTENT_HEIGHT, FILTER_ROW_CONTROL_HEIGHT, FILTER_ROW_HEIGHT,
    FILTER_ROW_SPACING,
};
use crate::native_app::ui::ids as widget_ids;

pub(super) const FILTER_ROW_VERTICAL_INSET: f32 =
    (FILTER_ROW_HEIGHT - FILTER_ROW_CONTROL_HEIGHT) * 0.5;
pub(super) const FILTER_LABEL_VERTICAL_SHIFT: f32 = 1.0;
pub(super) const FILTER_LABEL_WIDTH: f32 = 82.0;
const FILTER_CONTROL_SPACING: f32 = 4.0;
pub(super) const FILTER_LABEL_CONTROL_SPACING: f32 = 1.0;
pub(super) const FILTER_CONTROL_HORIZONTAL_PADDING: f32 = 8.0;
const PLAYBACK_TYPE_FILTER_TOGGLE_WIDTH: f32 = 56.0;
const RATING_FILTER_TOGGLE_WIDTH: f32 = 14.0;
pub(super) const RATING_FILTER_SWATCH_SIZE: u8 = 10;
const RATING_FILTER_TRASH_COLOR: ui::Rgba8 = ui::Rgba8 {
    r: 238,
    g: 77,
    b: 67,
    a: 235,
};
const RATING_FILTER_UNRATED_ACTIVE_COLOR: ui::Rgba8 = ui::Rgba8 {
    r: 184,
    g: 188,
    b: 194,
    a: 235,
};
const RATING_FILTER_UNRATED_INACTIVE_COLOR: ui::Rgba8 = ui::Rgba8 {
    r: 138,
    g: 142,
    b: 148,
    a: 220,
};
const RATING_FILTER_KEEP_COLOR: ui::Rgba8 = ui::Rgba8 {
    r: 122,
    g: 226,
    b: 96,
    a: 235,
};
/// Golden marker used for the top (locked) keep rating in the sample browser.
const RATING_FILTER_TOP_KEEP_COLOR: ui::Rgba8 = ui::Rgba8 {
    r: 232,
    g: 188,
    b: 56,
    a: 245,
};
pub(super) const NAME_FILTER_INPUT_ID: u64 = widget_ids::NAME_FILTER_INPUT_ID;
pub(super) const TAG_FILTER_INPUT_ID: u64 = widget_ids::TAG_FILTER_INPUT_ID;
/// Scope for automation-facing playback-type filter toggle ids.
const AUTOMATION_PLAYBACK_TYPE_FILTER_TOGGLE_SCOPE: u64 =
    widget_ids::AUTOMATION_PLAYBACK_TYPE_FILTER_TOGGLE_SCOPE;
const AUTOMATION_CURATION_FILTER_DROPDOWN_OPTION_SCOPE: u64 =
    widget_ids::AUTOMATION_CURATION_FILTER_DROPDOWN_OPTION_SCOPE;
pub(super) const CURATION_FILTER_DROPDOWN_TRIGGER_ID: u64 =
    widget_ids::CURATION_FILTER_DROPDOWN_TRIGGER_ID;
pub(super) const HARVEST_FILTER_DROPDOWN_TRIGGER_ID: u64 =
    widget_ids::HARVEST_FILTER_DROPDOWN_TRIGGER_ID;
/// Scope for automation-facing harvest dropdown option ids.
const AUTOMATION_HARVEST_FILTER_DROPDOWN_OPTION_SCOPE: u64 =
    widget_ids::AUTOMATION_HARVEST_FILTER_DROPDOWN_OPTION_SCOPE;
/// Scope for automation-facing rating filter toggle ids.
const AUTOMATION_RATING_FILTER_TOGGLE_SCOPE: u64 =
    widget_ids::AUTOMATION_RATING_FILTER_TOGGLE_SCOPE;
/// Scope for automation-facing filter family label toggles.
const AUTOMATION_FILTER_FAMILY_LABEL_TOGGLE_SCOPE: u64 =
    widget_ids::AUTOMATION_FILTER_FAMILY_LABEL_TOGGLE_SCOPE;

pub(super) fn filter_rows(model: &FilterSectionViewModel) -> [ui::View<GuiMessage>; 6] {
    let projection = filter_rows_projection(model);
    [
        text_filter_row(projection.name_filter),
        text_filter_row(projection.tag_filter),
        curation_filter_row(projection.curation),
        harvest_filter_row(projection.harvest),
        playback_type_filter_row(projection.playback_type),
        rating_filter_row(projection.rating),
    ]
}

#[cfg(test)]
pub(super) fn name_filter_clear_button_id() -> u64 {
    ui::text_input_clear_button_id(NAME_FILTER_INPUT_ID)
}

#[cfg(test)]
pub(super) fn tag_filter_clear_button_id() -> u64 {
    ui::text_input_clear_button_id(TAG_FILTER_INPUT_ID)
}

fn text_filter_row(projection: TextFilterRowProjection) -> ui::View<GuiMessage> {
    filter_input_row(
        filter_row_label(
            projection.label,
            projection.family,
            projection.enabled,
            projection.help_tooltips_enabled,
        ),
        filter_text_input(
            projection.value,
            projection.placeholder,
            text_filter_input_id(projection.field),
            text_filter_message_mapper(projection.field),
            text_filter_tooltip(projection.field),
            projection.help_tooltips_enabled,
        ),
        text_filter_row_key(projection.field),
        projection.enabled,
    )
}

fn filter_text_input(
    value: String,
    placeholder: &'static str,
    input_id: u64,
    map_message: fn(TextInputMessage) -> FolderBrowserMessage,
    tooltip: &'static str,
    help_tooltips_enabled: bool,
) -> ui::View<GuiMessage> {
    ui::text_input(value)
        .placeholder(placeholder.to_ascii_uppercase())
        .underline()
        .subtle()
        .clear_button(GuiMessage::FolderBrowser(map_message(
            empty_filter_message(),
        )))
        .id(input_id)
        .message_event(move |message| GuiMessage::FolderBrowser(map_message(message)))
        .tooltip_if(help_tooltips_enabled, tooltip)
}

fn text_filter_input_id(field: TextFilterField) -> u64 {
    match field {
        TextFilterField::Name => NAME_FILTER_INPUT_ID,
        TextFilterField::Tags => TAG_FILTER_INPUT_ID,
    }
}

fn text_filter_message_mapper(
    field: TextFilterField,
) -> fn(TextInputMessage) -> FolderBrowserMessage {
    match field {
        TextFilterField::Name => FolderBrowserMessage::NameFilterInput,
        TextFilterField::Tags => FolderBrowserMessage::TagFilterInput,
    }
}

fn text_filter_row_key(field: TextFilterField) -> &'static str {
    match field {
        TextFilterField::Name => "filter-name-row",
        TextFilterField::Tags => "filter-tags-row",
    }
}

fn text_filter_tooltip(field: TextFilterField) -> &'static str {
    match field {
        TextFilterField::Name => "Show samples whose filename contains this text.",
        TextFilterField::Tags => "Show samples with matching tags.",
    }
}

fn playback_type_filter_row(row: PlaybackTypeFilterRowProjection) -> ui::View<GuiMessage> {
    let help_tooltips_enabled = row.help_tooltips_enabled;
    let label = filter_row_label(row.label, row.family, row.enabled, help_tooltips_enabled);
    filter_labeled_control_row(
        label,
        ui::row(
            row.toggles
                .iter()
                .map(|toggle| playback_type_filter_toggle(toggle, help_tooltips_enabled))
                .collect::<Vec<_>>(),
        )
        .spacing(FILTER_CONTROL_SPACING)
        .fill_width()
        .height(FILTER_ROW_CONTROL_HEIGHT),
        "filter-type-row",
        row.enabled,
    )
}

fn curation_filter_row(row: CurationFilterRowProjection) -> ui::View<GuiMessage> {
    let help_tooltips_enabled = row.help_tooltips_enabled;
    filter_labeled_control_row(
        filter_row_label(row.label, row.family, row.enabled, help_tooltips_enabled),
        ui::dropdown_trigger(row.selected_label.to_ascii_uppercase(), row.dropdown_open)
            .toggle_message(GuiMessage::ToggleCurationFilterDropdown)
            .build()
            .id(CURATION_FILTER_DROPDOWN_TRIGGER_ID)
            .tooltip_if(help_tooltips_enabled, "Choose the curation queue to show.")
            .fill_width()
            .height(FILTER_ROW_CONTROL_HEIGHT),
        "filter-curation-row",
        row.enabled,
    )
}

pub(super) fn curation_filter_dropdown_menu(
    model: &FilterSectionViewModel,
) -> Option<(ui::View<GuiMessage>, ui::Vector2)> {
    let row = filter_rows_projection(model).curation;
    if row.dropdown_open {
        let size = ui::Vector2::new(
            f32::from(row.menu_width).max(1.0),
            ui::dropdown_menu_height(row.options.len()),
        );
        Some((
            curation_filter_dropdown_menu_from_options(
                &row.options,
                row.help_tooltips_enabled,
                size.x,
            ),
            size,
        ))
    } else {
        None
    }
}

fn curation_filter_dropdown_menu_from_options(
    options: &[CurationFilterOptionProjection],
    help_tooltips_enabled: bool,
    menu_width: f32,
) -> ui::View<GuiMessage> {
    let option_count = options.len();
    ui::column(
        options
            .iter()
            .map(|option| curation_filter_dropdown_option(option, help_tooltips_enabled)),
    )
    .key("curation-filter-dropdown-menu")
    .style(ui::WidgetStyle {
        tone: ui::WidgetTone::Neutral,
        prominence: ui::WidgetProminence::Strong,
    })
    .padding(4.0)
    .spacing(3.0)
    .width(menu_width.max(1.0))
    .height(ui::dropdown_menu_height(option_count))
}

fn curation_filter_dropdown_option(
    option: &CurationFilterOptionProjection,
    help_tooltips_enabled: bool,
) -> ui::View<GuiMessage> {
    let scope = option.scope;
    ui::button(option.label.to_ascii_uppercase())
        .message(GuiMessage::FolderBrowser(
            FolderBrowserMessage::SetCurationScope(scope, true),
        ))
        .id(automation_curation_filter_dropdown_option_id(option.label))
        .style(ui::WidgetStyle {
            tone: if option.selected {
                ui::WidgetTone::Accent
            } else {
                ui::WidgetTone::Neutral
            },
            prominence: if option.selected {
                ui::WidgetProminence::Strong
            } else {
                ui::WidgetProminence::Subtle
            },
        })
        .fill_width()
        .height(22.0)
        .tooltip_if(help_tooltips_enabled, curation_scope_tooltip(scope))
}

/// Automation-facing id for a curation dropdown option.
pub(super) fn automation_curation_filter_dropdown_option_id(label: &str) -> u64 {
    ui::stable_widget_id(AUTOMATION_CURATION_FILTER_DROPDOWN_OPTION_SCOPE, label)
}

fn harvest_filter_row(row: HarvestFilterRowProjection) -> ui::View<GuiMessage> {
    let help_tooltips_enabled = row.help_tooltips_enabled;
    filter_labeled_control_row(
        filter_row_label(row.label, row.family, row.enabled, help_tooltips_enabled),
        ui::dropdown_trigger(row.selected_label.to_ascii_uppercase(), row.dropdown_open)
            .toggle_message(GuiMessage::ToggleHarvestFilterDropdown)
            .build()
            .id(HARVEST_FILTER_DROPDOWN_TRIGGER_ID)
            .tooltip_if(help_tooltips_enabled, "Choose the Harvest queue to show.")
            .fill_width()
            .height(FILTER_ROW_CONTROL_HEIGHT),
        "filter-harvest-row",
        row.enabled,
    )
}

pub(super) fn harvest_filter_dropdown_menu(
    model: &FilterSectionViewModel,
) -> Option<(ui::View<GuiMessage>, ui::Vector2)> {
    let row = filter_rows_projection(model).harvest;
    if row.dropdown_open {
        let size = ui::Vector2::new(
            f32::from(row.menu_width).max(1.0),
            ui::dropdown_menu_height(row.options.len()),
        );
        Some((
            harvest_filter_dropdown_menu_from_options(
                &row.options,
                row.help_tooltips_enabled,
                size.x,
            ),
            size,
        ))
    } else {
        None
    }
}

fn harvest_filter_dropdown_menu_from_options(
    options: &[HarvestFilterOptionProjection],
    help_tooltips_enabled: bool,
    menu_width: f32,
) -> ui::View<GuiMessage> {
    let option_count = options.len();
    ui::column(
        options
            .iter()
            .map(|option| harvest_filter_dropdown_option(option, help_tooltips_enabled)),
    )
    .key("harvest-filter-dropdown-menu")
    .style(ui::WidgetStyle {
        tone: ui::WidgetTone::Neutral,
        prominence: ui::WidgetProminence::Strong,
    })
    .padding(4.0)
    .spacing(3.0)
    .width(menu_width.max(1.0))
    .height(ui::dropdown_menu_height(option_count))
}

fn harvest_filter_dropdown_option(
    option: &HarvestFilterOptionProjection,
    help_tooltips_enabled: bool,
) -> ui::View<GuiMessage> {
    let filter = option.filter;
    ui::button(option.label.to_ascii_uppercase())
        .message(GuiMessage::FolderBrowser(
            FolderBrowserMessage::SetHarvestFilter(filter, true),
        ))
        .id(automation_harvest_filter_dropdown_option_id(option.label))
        .style(ui::WidgetStyle {
            tone: if option.selected {
                ui::WidgetTone::Accent
            } else {
                ui::WidgetTone::Neutral
            },
            prominence: if option.selected {
                ui::WidgetProminence::Strong
            } else {
                ui::WidgetProminence::Subtle
            },
        })
        .fill_width()
        .height(22.0)
        .tooltip_if(help_tooltips_enabled, option.tooltip)
}

/// Automation-facing id for a harvest dropdown option.
pub(super) fn automation_harvest_filter_dropdown_option_id(label: &str) -> u64 {
    ui::stable_widget_id(AUTOMATION_HARVEST_FILTER_DROPDOWN_OPTION_SCOPE, label)
}

fn playback_type_filter_toggle(
    toggle: &PlaybackTypeFilterToggleProjection,
    help_tooltips_enabled: bool,
) -> ui::View<GuiMessage> {
    let filter = toggle.filter;
    let label = if filter == PlaybackTypeFilter::OneShot {
        format!("{},", toggle.label.to_ascii_uppercase())
    } else {
        toggle.label.to_ascii_uppercase()
    };
    let visible = ui::text_line(label, FILTER_ROW_CONTROL_HEIGHT)
        .text_color(if toggle.active {
            ui::TextColorRole::Custom(crate::native_app::app_chrome::palette::ACCENT)
        } else {
            ui::TextColorRole::Primary
        })
        .size(PLAYBACK_TYPE_FILTER_TOGGLE_WIDTH, FILTER_ROW_CONTROL_HEIGHT);
    let input = ui::button("")
        .message(GuiMessage::FolderBrowser(
            FolderBrowserMessage::TogglePlaybackTypeFilter(filter, !toggle.active),
        ))
        .id(automation_playback_type_filter_toggle_id(toggle.label))
        .size(PLAYBACK_TYPE_FILTER_TOGGLE_WIDTH, FILTER_ROW_CONTROL_HEIGHT)
        .tooltip_if(help_tooltips_enabled, playback_type_filter_tooltip(filter));
    ui::input_underlay(visible, input.input_only())
}

/// Automation-facing id for a playback-type filter toggle.
pub(super) fn automation_playback_type_filter_toggle_id(label: &str) -> u64 {
    ui::stable_widget_id(AUTOMATION_PLAYBACK_TYPE_FILTER_TOGGLE_SCOPE, label)
}

fn rating_filter_row(row: RatingFilterRowProjection) -> ui::View<GuiMessage> {
    let help_tooltips_enabled = row.help_tooltips_enabled;
    let label = filter_row_label(row.label, row.family, row.enabled, help_tooltips_enabled);
    filter_labeled_control_row(
        label,
        ui::row(
            row.toggles
                .iter()
                .map(|toggle| rating_filter_toggle(toggle, help_tooltips_enabled))
                .collect::<Vec<_>>(),
        )
        .spacing(1.0)
        .fill_width()
        .height(FILTER_ROW_CONTROL_HEIGHT),
        "filter-rating-row",
        row.enabled,
    )
}

fn rating_filter_toggle(
    toggle: &RatingFilterToggleProjection,
    help_tooltips_enabled: bool,
) -> ui::View<GuiMessage> {
    let level = toggle.level;
    let visible = ui::color_marker(Some(rating_filter_swatch_color(level, toggle.active)))
        .side(RATING_FILTER_SWATCH_SIZE)
        .inset(0)
        .align(ui::ColorMarkerAlign::Center)
        .view()
        .size(RATING_FILTER_TOGGLE_WIDTH, FILTER_ROW_CONTROL_HEIGHT);
    let input = ui::button("")
        .active(toggle.active)
        .message(GuiMessage::FolderBrowser(
            FolderBrowserMessage::ToggleRatingFilter(level, !toggle.active),
        ))
        .id(automation_rating_filter_toggle_id(toggle.label))
        .size(RATING_FILTER_TOGGLE_WIDTH, FILTER_ROW_CONTROL_HEIGHT)
        .tooltip_if(help_tooltips_enabled, rating_filter_tooltip(level));
    ui::input_underlay(visible, input.input_only())
}

pub(super) fn rating_filter_swatch_color(level: i8, active: bool) -> ui::Rgba8 {
    let color = if level == 4 {
        RATING_FILTER_TOP_KEEP_COLOR
    } else if level < 0 {
        RATING_FILTER_TRASH_COLOR
    } else if level > 0 {
        RATING_FILTER_KEEP_COLOR
    } else if active {
        RATING_FILTER_UNRATED_ACTIVE_COLOR
    } else {
        RATING_FILTER_UNRATED_INACTIVE_COLOR
    };
    if active {
        color
    } else {
        color.with_alpha(color.a.saturating_sub(128))
    }
}

/// Automation-facing id for a rating filter toggle.
pub(super) fn automation_rating_filter_toggle_id(label: &str) -> u64 {
    ui::stable_widget_id(AUTOMATION_RATING_FILTER_TOGGLE_SCOPE, label)
}

fn filter_input_row(
    label: ui::View<GuiMessage>,
    control: ui::View<GuiMessage>,
    key: &'static str,
    selected: bool,
) -> ui::View<GuiMessage> {
    filter_labeled_control_row(label, control, key, selected)
}

fn filter_row_label(
    label: &'static str,
    family: FilterFamily,
    enabled: bool,
    help_tooltips_enabled: bool,
) -> ui::View<GuiMessage> {
    let visible = ui::text_line(label.to_ascii_uppercase(), FILTER_ROW_CONTROL_HEIGHT)
        .text_color(ui::TextColorRole::Custom(
            crate::native_app::app_chrome::palette::ACCENT,
        ))
        .size(FILTER_LABEL_WIDTH, FILTER_ROW_CONTROL_HEIGHT);
    let input = ui::button("")
        .message(GuiMessage::FolderBrowser(
            FolderBrowserMessage::SetFilterFamilyEnabled(family, !enabled),
        ))
        .id(automation_filter_family_label_toggle_id(label))
        .size(FILTER_LABEL_WIDTH, FILTER_ROW_CONTROL_HEIGHT)
        .tooltip_if(help_tooltips_enabled, filter_family_label_tooltip(family));
    ui::input_underlay(visible, input.input_only())
}

fn filter_family_label_tooltip(family: FilterFamily) -> &'static str {
    match family {
        FilterFamily::Name => "Turn filename filtering on or off.",
        FilterFamily::Tags => "Turn tag text filtering on or off.",
        FilterFamily::Curation => "Turn curation queue filtering on or off.",
        FilterFamily::Harvest => "Turn Harvest queue filtering on or off.",
        FilterFamily::PlaybackType => "Turn playback-type filtering on or off.",
        FilterFamily::Rating => "Turn rating filtering on or off.",
    }
}

fn curation_scope_tooltip(scope: BrowserCurationScope) -> &'static str {
    match scope {
        BrowserCurationScope::All => "Show all curation work.",
        BrowserCurationScope::Ratings => "Show samples that need rating decisions.",
        BrowserCurationScope::Tags => "Show samples that need tag cleanup.",
    }
}

fn playback_type_filter_tooltip(filter: PlaybackTypeFilter) -> &'static str {
    match filter {
        PlaybackTypeFilter::OneShot => "Show samples tagged as one-shots.",
        PlaybackTypeFilter::Loop => "Show samples tagged as loops.",
    }
}

fn rating_filter_tooltip(level: i8) -> &'static str {
    match level {
        -3 => "Show T3 trash-rated samples.",
        -2 => "Show T2 trash-rated samples.",
        -1 => "Show T1 trash-rated samples.",
        0 => "Show unrated samples.",
        1 => "Show K1 keep-rated samples.",
        2 => "Show K2 keep-rated samples.",
        3 => "Show K3 keep-rated samples.",
        4 => "Show locked K4 samples.",
        _ => "Show samples with this rating.",
    }
}

/// Automation-facing id for a filter family label toggle.
pub(super) fn automation_filter_family_label_toggle_id(label: &str) -> u64 {
    ui::stable_widget_id(AUTOMATION_FILTER_FAMILY_LABEL_TOGGLE_SCOPE, label)
}

fn filter_labeled_control_row(
    label: ui::View<GuiMessage>,
    control: ui::View<GuiMessage>,
    key: &'static str,
    selected: bool,
) -> ui::View<GuiMessage> {
    let label_cell = ui::stack([
        centered_filter_label_cell(label),
        ui::feedback_overlay()
            .edge(
                ui::ThemeTokens::default().grid_soft,
                FILTER_LABEL_CONTROL_SPACING,
                BorderSides {
                    top: false,
                    bottom: false,
                    left: false,
                    right: true,
                },
            )
            .view()
            .fill(),
    ])
    .width(FILTER_LABEL_WIDTH)
    .height(FILTER_ROW_HEIGHT);
    let content = ui::row([label_cell, centered_filter_control_cell(control)])
        .fill_width()
        .height(FILTER_ROW_HEIGHT)
        .spacing(0.0);
    let divider = ui::feedback_overlay()
        .edge(
            ui::ThemeTokens::default().grid_soft,
            1.0,
            BorderSides {
                top: false,
                bottom: true,
                left: false,
                right: false,
            },
        )
        .view()
        .fill();
    let content = ui::stack([content, divider])
        .fill_width()
        .height(FILTER_ROW_HEIGHT);
    sidebar_row_underlay(content, ListItemState::new(selected, false))
        .input_key(format!("filter-row-state-{key}"))
        .selected(selected)
        .actions(ui::row_actions())
        .key(format!("filter-row-{key}"))
        .fill_width()
        .height(FILTER_ROW_HEIGHT)
}

fn centered_filter_label_cell(label: ui::View<GuiMessage>) -> ui::View<GuiMessage> {
    ui::column([
        ui::spacer()
            .fill_width()
            .height((FILTER_ROW_VERTICAL_INSET - FILTER_LABEL_VERTICAL_SHIFT).max(0.0)),
        label
            .width(FILTER_LABEL_WIDTH)
            .height(FILTER_ROW_CONTROL_HEIGHT),
        ui::spacer()
            .fill_width()
            .height(FILTER_ROW_VERTICAL_INSET + FILTER_LABEL_VERTICAL_SHIFT),
    ])
    .width(FILTER_LABEL_WIDTH)
    .height(FILTER_ROW_HEIGHT)
}

fn centered_filter_control_cell(control: ui::View<GuiMessage>) -> ui::View<GuiMessage> {
    ui::column([
        ui::spacer().fill_width().height(FILTER_ROW_VERTICAL_INSET),
        control
            .fill_width()
            .height(FILTER_ROW_CONTROL_HEIGHT)
            .padding_x(FILTER_CONTROL_HORIZONTAL_PADDING),
        ui::spacer().fill_width().height(FILTER_ROW_VERTICAL_INSET),
    ])
    .fill_width()
    .height(FILTER_ROW_HEIGHT)
}

pub(super) fn empty_filter_message() -> TextInputMessage {
    TextInputMessage::Changed {
        value: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_app::app_chrome::view_models::library_sidebar::{
        CurationFilterOptionViewModel, CurationFilterViewModel, FilterSectionViewModel,
        HarvestFilterOptionViewModel, HarvestFilterViewModel, PlaybackTypeFilterToggleViewModel,
        RatingFilterToggleViewModel,
    };
    use crate::native_app::sample_library::folder_browser::model::{
        BrowserCurationScope, HarvestFilter, PlaybackTypeFilter,
    };
    use radiant::prelude::IntoView;
    use radiant::runtime::UiSurface;

    #[test]
    fn harvest_filter_dropdown_options_expose_full_help_tooltips() {
        let surface = ui::column(filter_rows(&filter_model(true))).into_surface();
        let tooltip = surface
            .find_widget(HARVEST_FILTER_DROPDOWN_TRIGGER_ID)
            .and_then(|widget| widget.widget_object().common().tooltip.as_deref());

        assert_eq!(tooltip, Some("Choose the Harvest queue to show."));

        let menu = harvest_filter_dropdown_menu(&filter_model(true))
            .expect("open harvest dropdown menu")
            .0
            .into_surface();
        let option_tooltip = menu
            .find_widget(automation_harvest_filter_dropdown_option_id("Needs Review"))
            .and_then(|widget| widget.widget_object().common().tooltip.as_deref());

        assert_eq!(
            option_tooltip,
            Some("Files not done or ignored that do not have derivatives yet.")
        );
    }

    fn filter_model(help_tooltips_enabled: bool) -> FilterSectionViewModel {
        FilterSectionViewModel {
            sidebar_width: 240.0,
            help_tooltips_enabled,
            name_filter: String::new(),
            name_filter_enabled: false,
            tag_filter: String::new(),
            tag_filter_enabled: false,
            curation: CurationFilterViewModel {
                enabled: false,
                dropdown_open: false,
                selected_scope: BrowserCurationScope::All,
                options: vec![CurationFilterOptionViewModel {
                    scope: BrowserCurationScope::All,
                    label: "All",
                }],
                help_tooltips_enabled,
            },
            harvest: HarvestFilterViewModel {
                enabled: false,
                dropdown_open: true,
                selected_filter: Some(HarvestFilter::NeedsReview),
                options: vec![HarvestFilterOptionViewModel {
                    filter: HarvestFilter::NeedsReview,
                    label: "Needs Review",
                }],
                family_available: false,
                family_open: false,
                help_tooltips_enabled,
            },
            playback_type_enabled: false,
            playback_type_filters: vec![PlaybackTypeFilterToggleViewModel {
                filter: PlaybackTypeFilter::OneShot,
                label: "1-Shot",
                active: false,
            }],
            rating_enabled: false,
            rating_filters: vec![RatingFilterToggleViewModel {
                level: 0,
                label: "U",
                active: false,
            }],
            panel_height: 120.0,
        }
    }

    #[test]
    fn filter_controls_expose_help_tooltips_on_interactive_targets() {
        let surface = ui::column(filter_rows(&filter_model(true))).into_surface();

        assert_eq!(
            widget_tooltip(&surface, automation_filter_family_label_toggle_id("Name")).as_deref(),
            Some("Turn filename filtering on or off.")
        );
        assert_eq!(
            widget_tooltip(&surface, CURATION_FILTER_DROPDOWN_TRIGGER_ID).as_deref(),
            Some("Choose the curation queue to show.")
        );
        assert_eq!(
            widget_tooltip(
                &surface,
                automation_playback_type_filter_toggle_id("1-Shot")
            )
            .as_deref(),
            Some("Show samples tagged as one-shots.")
        );
        assert_eq!(
            widget_tooltip(&surface, automation_rating_filter_toggle_id("U")).as_deref(),
            Some("Show unrated samples.")
        );
    }

    #[test]
    fn curation_dropdown_options_expose_help_tooltips() {
        let mut model = filter_model(true);
        model.curation.dropdown_open = true;
        model.curation.options = vec![CurationFilterOptionViewModel {
            scope: BrowserCurationScope::Ratings,
            label: "Rate",
        }];
        let menu = curation_filter_dropdown_menu(&model)
            .expect("open curation dropdown menu")
            .0
            .into_surface();

        assert_eq!(
            widget_tooltip(&menu, automation_curation_filter_dropdown_option_id("Rate")).as_deref(),
            Some("Show samples that need rating decisions.")
        );
    }

    #[test]
    fn filter_controls_omit_help_tooltips_when_help_is_disabled() {
        let surface = ui::column(filter_rows(&filter_model(false))).into_surface();

        assert_eq!(
            widget_tooltip(&surface, automation_filter_family_label_toggle_id("Name")),
            None
        );
        assert_eq!(
            widget_tooltip(&surface, CURATION_FILTER_DROPDOWN_TRIGGER_ID),
            None
        );
        assert_eq!(
            widget_tooltip(&surface, automation_rating_filter_toggle_id("U")),
            None
        );
    }

    fn widget_tooltip(surface: &UiSurface<GuiMessage>, widget_id: u64) -> Option<String> {
        surface
            .find_widget(widget_id)
            .and_then(|widget| widget.widget_object().common().tooltip.as_deref())
            .map(str::to_string)
    }
}
