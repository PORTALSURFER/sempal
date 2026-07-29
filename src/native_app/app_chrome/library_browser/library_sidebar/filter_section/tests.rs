use super::rows::{
    CURATION_FILTER_DROPDOWN_TRIGGER_ID, FILTER_CONTROLS_CONTENT_HEIGHT,
    FILTER_LABEL_VERTICAL_SHIFT, FILTER_LABEL_WIDTH, FILTER_ROW_CONTROL_HEIGHT, FILTER_ROW_HEIGHT,
    FILTER_ROW_SPACING, FILTER_ROW_VERTICAL_INSET, HARVEST_FILTER_DROPDOWN_TRIGGER_ID,
    NAME_FILTER_INPUT_ID, RATING_FILTER_SWATCH_SIZE, TAG_FILTER_INPUT_ID,
    automation_curation_filter_dropdown_option_id, automation_filter_family_label_toggle_id,
    automation_harvest_filter_dropdown_option_id, automation_playback_type_filter_toggle_id,
    automation_rating_filter_toggle_id, empty_filter_message, name_filter_clear_button_id,
    rating_filter_swatch_color, tag_filter_clear_button_id,
};
use super::*;
use crate::native_app::app_chrome::library_browser::library_sidebar::sidebar_row::sidebar_row_selected_fill_for_tests;
use crate::native_app::sample_library::folder_browser::FolderBrowserState;
use crate::native_app::sample_library::folder_browser::commands::FilterFamily;
use crate::native_app::sample_library::folder_browser::model::{
    BrowserCurationScope, HarvestFilter, PlaybackTypeFilter,
};
use crate::native_app::ui::ids::RETIRED_HARVEST_FILTER_ARROW_TOGGLE_ID;
use radiant::gui::types::Rgba8;
use radiant::prelude::IntoView;
use radiant::runtime::{SurfaceFrame, SurfacePaintPlan};
use radiant::widgets::{ButtonMessage, TextInputMessage};

const FILTER_SECTION_TEST_FRAME_HEIGHT: f32 = 220.0;

mod layout;
mod row_projection;
