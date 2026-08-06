use super::identity::{AUTOMATION_SOURCE_ADD_BUTTON_ID, retained_source_row_input_id};
use super::rows::{
    SOURCE_ADD_BUTTON_HEIGHT, SOURCE_ADD_BUTTON_WIDTH, SOURCE_ROW_HEIGHT, SOURCE_ROW_INSET_X,
    SOURCE_ROW_LABEL_PADDING_X, source_acceptance_fill_for_tests, source_add_button,
    source_add_button_tooltip_for_tests, source_missing_color_for_tests,
    source_protected_error_icon_color_for_tests, source_role_icon_color_for_source_for_tests,
    source_role_icon_color_for_tests, source_row, source_row_label_for_tests,
    source_selected_fill_for_tests, source_selected_marker_color_for_tests,
};
use super::source_selector;
use crate::native_app::app::{GuiMessage, SourceProcessingHealth, SourceProcessingHealthStatus};
use crate::native_app::app_chrome::library_browser::library_sidebar::sidebar_row::{
    sidebar_row_hover_fill_for_tests, sidebar_row_palette_for_tests,
    sidebar_row_selected_fill_for_tests,
};
use crate::native_app::app_chrome::palette::SELECTED_ROW_MARKER_WIDTH;
use crate::native_app::app_chrome::view_models::library_sidebar::{
    LibrarySidebarViewModel, SourceRowViewModel, SourceSelectorViewModel,
};
use crate::native_app::sample_library::folder_browser::commands::FolderBrowserMessage;
use crate::native_app::sample_library::folder_browser::{FolderBrowserState, model::SourceEntry};
use crate::native_app::test_support::state::{FolderScanProgress, NativeAppStateFixture};
use radiant::prelude as ui;
use radiant::prelude::IntoView;
use radiant::widgets::ButtonMessage;
use std::time::{Duration, Instant};
use wavecrate::sample_sources::readiness::{ReadinessStage, ReadinessStageCounts};
use wavecrate::sample_sources::{SampleSource, SourceId, SourceRole};

fn test_source(id: &str) -> SourceEntry {
    SourceEntry::new(id, "Source", std::path::PathBuf::from("C:/samples"))
}

fn test_source_row_with_health(
    health_label: Option<&str>,
    health_warning: bool,
    scanning: bool,
) -> SourceRowViewModel {
    SourceRowViewModel {
        id: String::from("source-health"),
        label: String::from("Source"),
        role: SourceRole::Normal,
        selected: false,
        focused: false,
        focus_alpha: 0,
        reorder_enabled: false,
        reorder_drag_active: false,
        reorder_drag_source: false,
        reorder_drop_target: false,
        reorder_drop_after: false,
        scanning,
        health_label: health_label.map(str::to_string),
        health_detail: Some(String::from("Readiness: diagnostics")),
        unsupported_count: 0,
        health_warning,
        missing: false,
        protected_source_error_flash: false,
        primary_source_acceptance_flash: false,
        drag_active: false,
        drop_candidate: false,
        drop_target: false,
        drop_target_active: false,
    }
}

fn source_model_with_terminal_counts(counts: ReadinessStageCounts) -> LibrarySidebarViewModel {
    let root = tempfile::tempdir().expect("source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("terminal-source"),
        root.path().to_path_buf(),
    );
    let mut state = NativeAppStateFixture::default()
        .with_folder_browser(FolderBrowserState::from_sample_sources(
            std::slice::from_ref(&source),
        ))
        .build();
    let mut stage_counts = std::collections::BTreeMap::new();
    stage_counts.insert(ReadinessStage::AnalysisFeatures, counts);
    state.background.source_processing_health.insert(
        source.id.as_str().to_string(),
        SourceProcessingHealth {
            source_id: source.id.as_str().to_string(),
            lifecycle_generation: 1,
            status: SourceProcessingHealthStatus::DegradedTerminal,
            source_generation: 4,
            readiness_revision: 5,
            stage_counts,
            retry_at: None,
            failure_codes: vec![String::from("terminal_diagnostic")],
        },
    );
    LibrarySidebarViewModel::from_app_state(&state)
}

macro_rules! assert_no_left_source_marker {
    ($frame:expr) => {
        assert!(
            !$frame.paint_plan.fill_rects().any(|fill| {
                fill.rect.min.x <= SOURCE_ROW_LABEL_PADDING_X + 12.0
                    && fill.rect.width() <= 10.0
                    && fill.rect.height() <= 10.0
            }),
            "source rows should not paint a separate left color marker"
        );
    };
}

#[test]
fn source_add_button_routes_add_source_message() {
    assert_eq!(
        source_add_button(false).view_dispatch_widget_output(
            AUTOMATION_SOURCE_ADD_BUTTON_ID,
            ui::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        Some(GuiMessage::FolderBrowser(FolderBrowserMessage::AddSource))
    );
}

#[test]
fn source_add_button_uses_regular_icon_button_chrome() {
    let frame = source_add_button(false).view_frame_at_size_with_default_theme(ui::Vector2::new(
        SOURCE_ADD_BUTTON_WIDTH,
        SOURCE_ADD_BUTTON_HEIGHT,
    ));
    let icon_rect = frame
        .paint_plan
        .first_svg_rect_for_widget(AUTOMATION_SOURCE_ADD_BUTTON_ID)
        .expect("source add button should paint a plus icon");

    assert!(
        !frame.paint_plan.contains_text("+"),
        "source add should not render as a text button"
    );
    assert!(icon_rect.width() <= SOURCE_ADD_BUTTON_WIDTH);
    assert!(icon_rect.height() <= SOURCE_ADD_BUTTON_HEIGHT);
}

#[test]
fn source_add_button_exposes_tooltip_when_help_tooltips_are_enabled() {
    let surface = source_add_button(true).into_surface();
    let tooltip = surface
        .find_widget(AUTOMATION_SOURCE_ADD_BUTTON_ID)
        .and_then(|widget| widget.widget_object().common().tooltip.as_deref());

    assert_eq!(tooltip, Some(source_add_button_tooltip_for_tests()));
}

#[test]
fn source_add_button_omits_tooltip_when_help_tooltips_are_disabled() {
    let surface = source_add_button(false).into_surface();
    let tooltip = surface
        .find_widget(AUTOMATION_SOURCE_ADD_BUTTON_ID)
        .and_then(|widget| widget.widget_object().common().tooltip.as_deref());

    assert_eq!(tooltip, None);
}

#[test]
fn source_selector_threads_help_tooltips_to_add_button() {
    let source = test_source("source-with-tooltip");
    let state = FolderBrowserState::from_sources_deferred(vec![source.clone()], source.id.clone());
    let model = SourceSelectorViewModel::from_folder_browser(&state, true);
    let surface = source_selector(&model).into_surface();
    let tooltip = surface
        .find_widget(AUTOMATION_SOURCE_ADD_BUTTON_ID)
        .and_then(|widget| widget.widget_object().common().tooltip.as_deref());

    assert_eq!(tooltip, Some(source_add_button_tooltip_for_tests()));
}

#[test]
fn source_row_routes_primary_activation_through_interactive_row() {
    let source = test_source("source-a");
    let state = FolderBrowserState::from_sources_deferred(vec![source.clone()], source.id.clone());
    let model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let row = model.rows.first().expect("source row");

    assert_eq!(
        source_row(row).view_dispatch_widget_output(
            retained_source_row_input_id(source.id.as_str()),
            ui::WidgetOutput::typed(ui::InteractiveRowMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::SelectSource(source.id.clone())
        ))
    );
}

#[test]
fn source_row_routes_secondary_activation_to_context_menu() {
    let source = test_source("source-b");
    let state = FolderBrowserState::from_sources_deferred(vec![source.clone()], source.id.clone());
    let position = ui::Point::new(12.0, 20.0);
    let model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let row = model.rows.first().expect("source row");

    assert_eq!(
        source_row(row).view_dispatch_widget_output(
            retained_source_row_input_id(source.id.as_str()),
            ui::WidgetOutput::typed(ui::InteractiveRowMessage::SecondaryActivate { position }),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::OpenSourceContextMenu(source.id.clone(), position)
        ))
    );
}

#[test]
fn source_row_routes_drop_to_source_root() {
    let source = SourceRowViewModel {
        id: String::from("drop-source"),
        label: String::from("Drop Source"),
        role: SourceRole::Normal,
        selected: false,
        focused: false,
        focus_alpha: 0,
        reorder_enabled: true,
        reorder_drag_active: false,
        reorder_drag_source: false,
        reorder_drop_target: false,
        reorder_drop_after: false,
        scanning: false,
        health_label: None,
        health_detail: None,
        unsupported_count: 0,
        health_warning: false,
        missing: false,
        protected_source_error_flash: false,
        primary_source_acceptance_flash: false,
        drag_active: true,
        drop_candidate: true,
        drop_target: false,
        drop_target_active: false,
    };

    assert_eq!(
        source_row(&source).view_dispatch_widget_output(
            retained_source_row_input_id(source.id.as_str()),
            ui::WidgetOutput::typed(ui::InteractiveRowMessage::Drop),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::DropOnSource(source.id.clone())
        ))
    );
}

#[test]
fn source_row_routes_drag_lifecycle_by_stable_source_id() {
    let first = test_source("source-drag-a");
    let second = test_source("source-drag-b");
    let state =
        FolderBrowserState::from_sources_deferred(vec![first.clone(), second], first.id.clone());
    let model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let row = model.rows.first().expect("source row");
    let drag = ui::DragHandleMessage::started(ui::Point::new(12.0, 32.0));

    assert_eq!(
        source_row(row).view_dispatch_widget_output(
            retained_source_row_input_id(first.id.as_str()),
            ui::WidgetOutput::typed(ui::InteractiveRowMessage::Drag(drag.clone())),
        ),
        Some(GuiMessage::FolderBrowser(FolderBrowserMessage::DragSource(
            first.id.clone(),
            drag
        )))
    );
}

#[test]
fn selected_source_row_uses_flat_highlight_with_left_active_marker() {
    let source = test_source("source-active");
    let state = FolderBrowserState::from_sources_deferred(vec![source.clone()], source.id.clone());
    let model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let row = model.rows.first().expect("source row");
    let frame = source_row(row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(180.0, SOURCE_ROW_HEIGHT));
    let selected_fill = source_selected_fill_for_tests();

    assert!(
        frame
            .paint_plan
            .fill_rects()
            .any(|fill| fill.color == selected_fill),
        "selected source should paint the restrained accent tint"
    );
    assert!(
        frame.paint_plan.fill_rects().any(|fill| {
            fill.color == source_selected_marker_color_for_tests()
                && (fill.rect.width() - SELECTED_ROW_MARKER_WIDTH).abs() < 0.5
                && (fill.rect.min.x - SOURCE_ROW_INSET_X).abs() < 0.5
        }),
        "selected source should paint an inset left active marker"
    );
    assert!(
        frame.paint_plan.fill_rects().all(|fill| {
            fill.color != source_selected_marker_color_for_tests()
                || (fill.rect.max.x - 180.0).abs() >= 0.5
        }),
        "selected source should keep the reference's single leading selection rail"
    );
    assert_eq!(
        frame.paint_plan.first_text_color("Source"),
        Some(source_selected_marker_color_for_tests()),
        "selected source label should use the accent color"
    );
}

#[test]
fn keyboard_navigation_layers_focus_over_active_source_selection() {
    let source = test_source("source-focused");
    let mut state =
        FolderBrowserState::from_sources_deferred(vec![source.clone()], source.id.clone());
    state.focus_selected_source_for_keyboard();
    let model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let row = model.rows.first().expect("source row");
    let frame = source_row(row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(180.0, SOURCE_ROW_HEIGHT));
    let focus = crate::native_app::app_chrome::palette::focused_row_marker();

    assert!(row.selected);
    assert!(row.focused);
    assert!(
        frame
            .paint_plan
            .fill_rects()
            .any(|fill| fill.color == focus.color && fill.rect.width() == focus.parts.width)
    );
}

#[test]
fn source_focus_fade_overlays_the_fixed_selected_rail_without_a_horizontal_jump() {
    let source = test_source("source-focused-fade");
    let mut state =
        FolderBrowserState::from_sources_deferred(vec![source.clone()], source.id.clone());
    state.focus_selected_source_for_keyboard();
    let mut model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let row = model.rows.first_mut().expect("source row");
    row.focus_alpha = 128;
    let frame = source_row(row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(180.0, SOURCE_ROW_HEIGHT));
    let selected_color = source_selected_marker_color_for_tests();
    let focus_color = crate::native_app::app_chrome::palette::PALE_MARKER.with_alpha(128);
    let selected = frame
        .paint_plan
        .fill_rects()
        .find(|fill| {
            fill.color == selected_color
                && (fill.rect.width() - SELECTED_ROW_MARKER_WIDTH).abs() < 0.5
        })
        .expect("selected rail");
    let focus = frame
        .paint_plan
        .fill_rects()
        .find(|fill| {
            fill.color == focus_color
                && (fill.rect.width()
                    - crate::native_app::app_chrome::palette::FOCUSED_ROW_MARKER_WIDTH)
                    .abs()
                    < 0.5
        })
        .expect("fading focus rail");

    assert_eq!(selected.rect.min.x, focus.rect.min.x);
    assert_eq!(selected.rect.min.x, SOURCE_ROW_INSET_X);
}

#[test]
fn source_row_keeps_actions_enabled_while_processing_feedback_is_overlay_owned() {
    let source = test_source("source-processing");
    let state = FolderBrowserState::from_sources_deferred(vec![source.clone()], source.id.clone());
    let mut model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let row = model.rows.first_mut().expect("source row");
    assert_eq!(
        source_row(row).view_dispatch_widget_output(
            retained_source_row_input_id(source.id.as_str()),
            ui::WidgetOutput::typed(ui::InteractiveRowMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::SelectSource(source.id.clone())
        )),
        "processing must never lock source interaction"
    );
}

#[test]
fn source_rows_use_slim_flat_item_chrome() {
    let source = test_source("source-bordered");
    let state = FolderBrowserState::from_sources_deferred(vec![source.clone()], source.id.clone());
    let model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let row = model.rows.first().expect("source row");
    let frame = source_row(row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(180.0, SOURCE_ROW_HEIGHT));
    assert_eq!(
        SOURCE_ROW_HEIGHT, 22.0,
        "source rows should stay slimmer than the old 24px baseline"
    );
    assert!(
        frame
            .paint_plan
            .stroke_rects_for_widget(retained_source_row_input_id(source.id.as_str()))
            .next()
            .is_none(),
        "source item chrome should not draw a boxed outline"
    );
}

#[test]
fn inactive_source_row_does_not_paint_active_marker() {
    let source = test_source("source-inactive");
    let selected = test_source("source-active");
    let state = FolderBrowserState::from_sources_deferred(
        vec![source.clone(), selected.clone()],
        selected.id.clone(),
    );
    let model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let row = model.rows.first().expect("source row");
    let frame = source_row(row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(180.0, SOURCE_ROW_HEIGHT));

    assert!(
        !frame
            .paint_plan
            .fill_rects()
            .any(|fill| fill.color == sidebar_row_selected_fill_for_tests()),
        "inactive sources should stay visually quiet"
    );
}

#[test]
fn source_row_label_keeps_left_breathing_room() {
    let source = test_source("source-padded");
    let state = FolderBrowserState::from_sources_deferred(vec![source.clone()], source.id.clone());
    let model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let row = model.rows.first().expect("source row");
    let frame = source_row(row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(180.0, SOURCE_ROW_HEIGHT));
    let label_rect = frame
        .paint_plan
        .first_text_rect("Source")
        .expect("source label");

    assert!(
        label_rect.min.x >= SOURCE_ROW_LABEL_PADDING_X,
        "source label should be inset from the sidebar edge: {label_rect:?}"
    );
}

#[test]
fn source_row_hides_routine_processing_suffixes() {
    let scanning = test_source_row_with_health(Some("processing"), false, true);
    let processing = test_source_row_with_health(Some("processing"), false, false);

    assert_eq!(source_row_label_for_tests(&scanning), "Source");
    assert_eq!(source_row_label_for_tests(&processing), "Source");
}

#[test]
fn source_row_exposes_unsupported_file_count_action() {
    let mut source = test_source_row_with_health(Some("limited"), true, false);
    source.unsupported_count = 3;

    let frame = source_row(&source)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(240.0, SOURCE_ROW_HEIGHT));

    assert!(frame.paint_plan.contains_text("3 unsupported"));
}

#[test]
fn scanning_source_projection_renders_base_label_without_suffix() {
    let root = tempfile::tempdir().expect("source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("scanning-source"),
        root.path().to_path_buf(),
    );
    let mut state = NativeAppStateFixture::default()
        .with_folder_browser(FolderBrowserState::from_sample_sources(
            std::slice::from_ref(&source),
        ))
        .build();
    let request = state
        .library
        .begin_source_scan(source.id.as_str().to_string(), 17)
        .expect("begin source scan");
    state.library.start_folder_scan(&request);
    assert!(
        state
            .library
            .apply_folder_scan_progress(FolderScanProgress::new(
            request.task_id,
            request.source_id.clone(),
            request.label,
            crate::native_app::sample_library::folder_browser::scan::FolderScanLifecycle::Scanning,
            1,
            10,
            String::from("kick.wav"),
        ))
    );

    let model = LibrarySidebarViewModel::from_app_state(&state);
    let row = model
        .source_selector
        .rows
        .first()
        .expect("scanning source row");
    assert!(row.scanning);
    let frame = source_row(row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(200.0, SOURCE_ROW_HEIGHT));

    assert!(frame.paint_plan.contains_text(row.label.as_str()));
    assert!(!frame.paint_plan.contains_text("(scanning)"));
}

#[test]
fn unsupported_only_source_row_uses_base_label_and_neutral_text() {
    let neutral = test_source_row_with_health(None, false, false);
    let unsupported = test_source_row_with_health(Some("limited"), false, false);
    let neutral_frame = source_row(&neutral)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(200.0, SOURCE_ROW_HEIGHT));
    let unsupported_frame = source_row(&unsupported)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(200.0, SOURCE_ROW_HEIGHT));

    assert_eq!(source_row_label_for_tests(&unsupported), "Source");
    assert_eq!(
        unsupported_frame.paint_plan.first_text_color("Source"),
        neutral_frame.paint_plan.first_text_color("Source"),
        "unsupported-only health should retain neutral source-row text"
    );
    assert!(!unsupported_frame.paint_plan.contains_text("(limited)"));
}

#[test]
fn mixed_terminal_source_row_retains_limited_warning_presentation() {
    let mixed = test_source_row_with_health(Some("limited"), true, false);
    let frame = source_row(&mixed)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(200.0, SOURCE_ROW_HEIGHT));

    assert_eq!(source_row_label_for_tests(&mixed), "Source (limited)");
    assert_eq!(
        frame.paint_plan.first_text_color("Source (limited)"),
        Some(source_missing_color_for_tests()),
        "mixed terminal health should retain warning text"
    );
}

#[test]
fn unsupported_plus_stale_projects_limited_warning_row() {
    let model = source_model_with_terminal_counts(ReadinessStageCounts {
        unsupported: 2,
        stale: 1,
        ..ReadinessStageCounts::default()
    });
    let row = model
        .source_selector
        .rows
        .first()
        .expect("stale terminal source row");
    assert_eq!(row.unsupported_count, 2);
    assert!(row.health_warning);
    let frame = source_row(row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(200.0, SOURCE_ROW_HEIGHT));

    let expected_label = format!("{} (limited)", row.label);
    assert_eq!(source_row_label_for_tests(row), expected_label);
    assert_eq!(
        frame.paint_plan.first_text_color(&expected_label),
        Some(source_missing_color_for_tests())
    );
}

#[test]
fn unsupported_plus_deleted_projects_limited_warning_row() {
    let model = source_model_with_terminal_counts(ReadinessStageCounts {
        unsupported: 2,
        deleted: 1,
        ..ReadinessStageCounts::default()
    });
    let row = model
        .source_selector
        .rows
        .first()
        .expect("deleted terminal source row");
    assert!(row.health_warning);
    let frame = source_row(row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(200.0, SOURCE_ROW_HEIGHT));

    let expected_label = format!("{} (limited)", row.label);
    assert_eq!(source_row_label_for_tests(row), expected_label);
    assert_eq!(
        frame.paint_plan.first_text_color(&expected_label),
        Some(source_missing_color_for_tests())
    );
}

#[test]
fn count_empty_terminal_source_row_retains_limited_warning_suffix() {
    let empty = test_source_row_with_health(Some("limited"), true, false);

    assert_eq!(source_row_label_for_tests(&empty), "Source (limited)");
}

#[test]
fn source_row_retains_genuine_availability_error_suffix() {
    let offline = test_source_row_with_health(Some("offline"), true, false);
    let scanning_offline = test_source_row_with_health(Some("offline"), true, true);

    assert_eq!(source_row_label_for_tests(&offline), "Source (offline)");
    assert_eq!(
        source_row_label_for_tests(&scanning_offline),
        "Source (offline)"
    );
}

#[test]
fn source_rows_use_shared_grey_sidebar_hover_fill() {
    assert_eq!(
        sidebar_row_palette_for_tests().hovered,
        Some(sidebar_row_hover_fill_for_tests())
    );
}

#[test]
fn missing_source_row_paints_missing_badge_without_left_marker() {
    let mut source = test_source("source-missing");
    source.mark_missing_for_tests();
    let state = FolderBrowserState::from_sources_deferred(vec![source.clone()], source.id.clone());
    let model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let row = model.rows.first().expect("source row");
    let frame = source_row(row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(200.0, SOURCE_ROW_HEIGHT));

    assert!(
        frame.paint_plan.contains_text("MISSING"),
        "missing sources should get an explicit source-list badge"
    );
    assert_eq!(
        frame.paint_plan.first_text_color("Source"),
        Some(source_missing_color_for_tests()),
        "missing source labels should use warning text"
    );
    assert_eq!(
        frame.paint_plan.first_text_color("MISSING"),
        Some(source_missing_color_for_tests()),
        "missing source badges should use warning text"
    );
    assert_no_left_source_marker!(frame);
}

#[test]
fn primary_source_row_uses_role_icon_instead_of_text_badge() {
    let mut source = test_source("source-primary");
    source.role = SourceRole::Primary;
    let state = FolderBrowserState::from_sources_deferred(vec![source.clone()], source.id.clone());
    let model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let row = model.rows.first().expect("source row");
    let frame = source_row(row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(200.0, SOURCE_ROW_HEIGHT));
    let icon_rect = frame
        .paint_plan
        .svgs()
        .next()
        .expect("primary source icon")
        .rect;

    assert!(icon_rect.height() <= SOURCE_ROW_HEIGHT);
    assert_eq!(
        source_role_icon_color_for_tests(),
        ui::Rgba8::new(216, 215, 211, 255),
        "source role icons should use the warm primary tint"
    );
    assert!(
        !frame.paint_plan.contains_text("PRI"),
        "primary sources should not render the old text badge"
    );
    assert_no_left_source_marker!(frame);
}

#[test]
fn primary_source_acceptance_flash_projects_paints_and_expires_after_one_second() {
    let mut primary = test_source("source-primary-acceptance");
    primary.role = SourceRole::Primary;
    let normal = test_source("source-normal-acceptance");
    let mut state = FolderBrowserState::from_sources_deferred(
        vec![primary.clone(), normal],
        primary.id.clone(),
    );

    let started_at = Instant::now();
    state.set_primary_source_acceptance_flash_time_for_tests(started_at);
    let model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let primary_row = model
        .rows
        .iter()
        .find(|row| row.id == primary.id)
        .expect("primary source row");
    let normal_row = model
        .rows
        .iter()
        .find(|row| row.role == SourceRole::Normal)
        .expect("normal source row");
    let frame = source_row(primary_row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(200.0, SOURCE_ROW_HEIGHT));

    assert!(primary_row.primary_source_acceptance_flash);
    assert!(!normal_row.primary_source_acceptance_flash);
    assert!(
        frame
            .paint_plan
            .fill_rects()
            .any(|fill| fill.color == source_acceptance_fill_for_tests()),
        "accepted extraction should tint the Primary source row green"
    );
    assert!(state.primary_source_acceptance_flash_active_at_for_tests(
        started_at + Duration::from_millis(999)
    ));
    assert!(
        !state.primary_source_acceptance_flash_active_at_for_tests(
            started_at + Duration::from_secs(1)
        )
    );

    let restarted_at = Instant::now();
    state.set_primary_source_acceptance_flash_time_for_tests(restarted_at);
    assert!(
        state.primary_source_acceptance_flash_active_at_for_tests(
            restarted_at + Duration::from_millis(999)
        ),
        "a later extraction should restart the one-second interval"
    );
    state.advance_primary_source_acceptance_flash_time_for_tests(
        restarted_at + Duration::from_secs(1),
    );
    assert!(!state.primary_source_acceptance_flash_active());
}

#[test]
fn protected_source_row_uses_role_icon_instead_of_text_badge() {
    let mut source = test_source("source-protected");
    source.role = SourceRole::Protected;
    let state = FolderBrowserState::from_sources_deferred(vec![source.clone()], source.id.clone());
    let model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let row = model.rows.first().expect("source row");
    let frame = source_row(row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(200.0, SOURCE_ROW_HEIGHT));
    let icon_rect = frame
        .paint_plan
        .svgs()
        .next()
        .expect("protected source icon")
        .rect;

    assert!(icon_rect.height() <= SOURCE_ROW_HEIGHT);
    assert_eq!(
        source_role_icon_color_for_tests(),
        ui::Rgba8::new(216, 215, 211, 255),
        "source role icons should use the warm primary tint"
    );
    assert!(
        !frame.paint_plan.contains_text("PRO") && !frame.paint_plan.contains_text("PROT"),
        "protected sources should not render the old text badge"
    );
    assert_no_left_source_marker!(frame);
}

#[test]
fn protected_source_error_flash_tints_lock_icon_red() {
    let mut source = test_source("source-protected-flash");
    source.role = SourceRole::Protected;
    let mut state =
        FolderBrowserState::from_sources_deferred(vec![source.clone()], source.id.clone());
    state.flash_protected_source_error_paths([std::path::PathBuf::from("C:/samples/kick.wav")]);
    let model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let row = model.rows.first().expect("source row");
    let frame = source_row(row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(200.0, SOURCE_ROW_HEIGHT));

    assert!(
        row.protected_source_error_flash,
        "protected source flash should reach the source row view model"
    );
    assert!(
        frame.paint_plan.svgs().next().is_some(),
        "protected source should still paint its lock icon during the flash"
    );
    assert_eq!(
        source_role_icon_color_for_source_for_tests(row),
        source_protected_error_icon_color_for_tests(),
        "protected source lock icon should flash with the red error tint"
    );
}

#[test]
fn normal_source_row_keeps_role_slot_neutral() {
    let source = test_source("source-normal");
    let state = FolderBrowserState::from_sources_deferred(vec![source.clone()], source.id.clone());
    let model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let row = model.rows.first().expect("source row");
    let frame = source_row(row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(200.0, SOURCE_ROW_HEIGHT));

    assert_eq!(
        frame.paint_plan.svgs().count(),
        0,
        "normal sources should not paint a role icon"
    );
    assert!(
        !frame.paint_plan.contains_text("PRI")
            && !frame.paint_plan.contains_text("PRO")
            && !frame.paint_plan.contains_text("PROT"),
        "normal sources should stay free of source-role text badges"
    );
    assert_no_left_source_marker!(frame);
}

#[test]
fn source_selector_header_reports_missing_sources() {
    let mut missing = test_source("source-missing");
    missing.mark_missing_for_tests();
    let present = SourceEntry::new(
        "source-present",
        "Present",
        std::path::PathBuf::from("C:/present"),
    );
    let state = FolderBrowserState::from_sources_deferred(
        vec![missing.clone(), present],
        missing.id.clone(),
    );
    let model = SourceSelectorViewModel::from_folder_browser(&state, false);
    let frame = source_selector(&model)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(220.0, 76.0));

    assert!(
        frame.paint_plan.contains_text("Sources (1 missing)"),
        "source header should expose missing source count"
    );
    assert_eq!(
        frame.paint_plan.first_text_color("Sources (1 missing)"),
        Some(source_missing_color_for_tests()),
        "source header should use warning text when any source is missing"
    );
}
