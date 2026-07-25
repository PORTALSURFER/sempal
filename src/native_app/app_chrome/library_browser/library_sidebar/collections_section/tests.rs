use super::identity::retained_collection_row_input_id;
use super::rows::{collection_count, collection_row};
use super::*;
use crate::native_app::app_chrome::library_browser::library_sidebar::sidebar_row::{
    sidebar_row_active_target_fill_for_tests, sidebar_row_hover_fill_for_tests,
    sidebar_row_palette_for_tests,
};
use crate::native_app::app_chrome::view_models::library_sidebar::CollectionRowViewModel;
use crate::native_app::sample_library::folder_browser::FolderBrowserState;
use crate::native_app::sample_library::folder_browser::view_contract::{
    COLLECTION_ROW_HEIGHT, SampleCollectionView,
};
use radiant::prelude::IntoView;
use wavecrate::sample_sources::SampleCollection;

/// Builds a minimal collection view for interaction tests.
fn collection_view(drag_active: bool, drop_target: bool) -> SampleCollectionView {
    let collection = SampleCollection::new(0).expect("valid collection");
    SampleCollectionView {
        collection,
        hotkey: '1',
        name: String::from("Collection 1"),
        color: ui::Rgba8 {
            r: 255,
            g: 86,
            b: 98,
            a: 240,
        },
        selected: false,
        focused: false,
        focus_alpha: u8::MAX,
        drop_target,
        drag_active,
        assigned_count: 0,
    }
}

#[test]
/// Verifies double-click routing through Radiant's generic row primitive.
fn collection_input_routes_double_click_to_rename() {
    let collection = collection_view(false, false);
    let collection_id = collection.collection;
    let row = CollectionRowViewModel {
        collection,
        rename: None,
    };

    assert!(matches!(
        collection_row(&row).view_dispatch_widget_output(
            retained_collection_row_input_id(collection_id),
            ui::WidgetOutput::typed(ui::InteractiveRowMessage::DoubleActivate),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::RenameCollection(collection)
        )) if collection == collection_id
    ));
}

#[test]
fn collection_input_routes_secondary_activation_to_context_menu() {
    let collection = collection_view(false, false);
    let collection_id = collection.collection;
    let row = CollectionRowViewModel {
        collection,
        rename: None,
    };
    let position = ui::Point::new(12.0, 20.0);

    assert!(matches!(
        collection_row(&row).view_dispatch_widget_output(
            retained_collection_row_input_id(collection_id),
            ui::WidgetOutput::typed(ui::InteractiveRowMessage::SecondaryActivate { position }),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::OpenCollectionContextMenu(collection, actual_position)
        )) if collection == collection_id && actual_position == position
    ));
}

#[test]
/// Verifies active drop targets keep a distinct visual state.
fn collection_input_paints_current_drop_target_fill() {
    let collection = collection_view(true, true);
    let row = CollectionRowViewModel {
        collection,
        rename: None,
    };
    let frame = collection_row(&row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(180.0, COLLECTION_ROW_HEIGHT));

    assert!(frame.paint_plan.fill_rects().any(|fill| {
        fill.color == sidebar_row_active_target_fill_for_tests()
            && fill.rect.width() == 180.0
            && fill.rect.height() == COLLECTION_ROW_HEIGHT
    }));
}

#[test]
fn collection_rows_use_shared_grey_sidebar_hover_fill() {
    assert_eq!(
        sidebar_row_palette_for_tests().hovered,
        Some(sidebar_row_hover_fill_for_tests())
    );
}

#[test]
fn focused_selected_collection_layers_global_selection_and_focus_chrome() {
    let mut collection = collection_view(false, false);
    collection.selected = true;
    collection.focused = true;
    let row = CollectionRowViewModel {
        collection,
        rename: None,
    };
    let frame = collection_row(&row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(180.0, COLLECTION_ROW_HEIGHT));
    let focus = crate::native_app::app_chrome::palette::focused_row_marker();

    assert_eq!(
        frame.paint_plan.first_text_color("1  COLLECTION 1"),
        Some(crate::native_app::app_chrome::palette::ACCENT)
    );
    assert!(
        frame
            .paint_plan
            .fill_rects()
            .any(|fill| fill.color == focus.color && fill.rect.width() == focus.parts.width)
    );
}

#[test]
fn collection_focus_fade_overlays_the_fixed_selected_rail_without_a_horizontal_jump() {
    let mut collection = collection_view(false, false);
    collection.selected = true;
    collection.focused = true;
    collection.focus_alpha = 128;
    let row = CollectionRowViewModel {
        collection,
        rename: None,
    };
    let frame = collection_row(&row)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(180.0, COLLECTION_ROW_HEIGHT));
    let selected = frame
        .paint_plan
        .fill_rects()
        .find(|fill| {
            fill.color == crate::native_app::app_chrome::palette::ACCENT
                && (fill.rect.width()
                    - crate::native_app::app_chrome::palette::SELECTED_ROW_MARKER_WIDTH)
                    .abs()
                    < 0.5
        })
        .expect("selected rail");
    let focus = frame
        .paint_plan
        .fill_rects()
        .find(|fill| {
            fill.color == crate::native_app::app_chrome::palette::PALE_MARKER.with_alpha(128)
                && (fill.rect.width()
                    - crate::native_app::app_chrome::palette::FOCUSED_ROW_MARKER_WIDTH)
                    .abs()
                    < 0.5
        })
        .expect("fading focus rail");

    assert_eq!(selected.rect.min.x, focus.rect.min.x);
}

#[test]
fn collection_drop_target_keeps_distinct_target_fill() {
    assert_eq!(
        sidebar_row_palette_for_tests().active_target,
        Some(sidebar_row_active_target_fill_for_tests())
    );
}

#[test]
/// Empty collections should not paint a count placeholder rectangle.
fn collection_count_hides_empty_placeholder() {
    let frame = collection_count(0)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(28.0, COLLECTION_ROW_HEIGHT));

    assert!(frame.paint_plan.fill_rects().next().is_none());
    assert!(frame.paint_plan.stroke_rects().next().is_none());
    assert!(frame.paint_plan.text_runs().next().is_none());
}

#[test]
/// Empty collections should not reserve trailing count-cell layout space.
fn collection_count_collapses_empty_placeholder_layout() {
    const EMPTY_COUNT_NODE_ID: u64 = widget_ids::EMPTY_COLLECTION_COUNT_NODE_ID;
    let layout = ui::row([
        ui::text_line("Collection 1", COLLECTION_ROW_HEIGHT),
        collection_count(0).id(EMPTY_COUNT_NODE_ID),
    ])
    .view_layout_at_size(ui::Vector2::new(240.0, COLLECTION_ROW_HEIGHT));
    let rect = layout
        .rects
        .get(&EMPTY_COUNT_NODE_ID)
        .expect("empty collection count layout rect");

    assert_eq!(rect.width(), 0.0);
}

#[test]
/// Non-empty collections still paint the numeric count.
fn collection_count_shows_assigned_count() {
    let frame = collection_count(3)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(28.0, COLLECTION_ROW_HEIGHT));

    assert!(frame.paint_plan.contains_text("3"));
}

#[test]
/// Verifies the section keeps its resized height in the parent layout slot.
fn collections_section_layout_uses_configured_height() {
    let mut state = FolderBrowserState::load_default();
    state.resize_collections_panel(ui::DragHandleMessage::started(ui::Point::new(0.0, 200.0)));
    state.resize_collections_panel(ui::DragHandleMessage::moved(ui::Point::new(0.0, 120.0)));

    let layout = ui::column([
        collections_section(&CollectionsSectionViewModel::from_folder_browser(&state)),
        ui::spacer().fill_width().fill_height(),
    ])
    .view_layout_at_size(ui::Vector2::new(240.0, 600.0));
    let section = layout
        .rects
        .get(&COLLECTIONS_SECTION_NODE_ID)
        .expect("collections section layout rect");

    assert_eq!(section.height(), state.collections_panel_height());
}

#[test]
fn collections_resize_header_uses_full_width_hit_target() {
    let state = FolderBrowserState::load_default();
    let model = CollectionsSectionViewModel::from_folder_browser(&state);
    let layout = collections_section(&model).view_layout_at_size(ui::Vector2::new(240.0, 180.0));
    let section = layout
        .rects
        .get(&COLLECTIONS_SECTION_NODE_ID)
        .expect("collections section layout rect");
    let header = layout
        .rects
        .get(&COLLECTIONS_RESIZE_HEADER_ID)
        .expect("collections resize header layout rect");
    let drag = ui::DragHandleMessage::started(ui::Point::new(header.center().x, header.center().y));

    assert_eq!(header.min.x, section.min.x);
    assert_eq!(header.max.x, section.max.x);
    assert_eq!(header.min.y, section.min.y);
    assert_eq!(
        collections_section(&model).view_dispatch_widget_output(
            COLLECTIONS_RESIZE_HEADER_ID,
            ui::WidgetOutput::typed(drag),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::ResizeCollectionsPanel(drag)
        ))
    );
}

#[test]
/// Verifies the collection list reserves exactly the useful row stack height.
fn collections_section_list_height_matches_visible_rows() {
    let state = FolderBrowserState::load_default();
    let model = CollectionsSectionViewModel::from_folder_browser(&state);
    let expected = COLLECTION_ROW_HEIGHT * model.rows.len() as f32
        + COLLECTION_ROW_SPACING * (model.rows.len() - 1) as f32;

    assert_eq!(model.list_height, expected);
}

#[test]
/// The useful maximum height should fit all collection rows without residual scroll.
fn collections_section_max_height_does_not_overflow_rows() {
    let state = FolderBrowserState::load_default();
    let layout = collections_section(&CollectionsSectionViewModel::from_folder_browser(&state))
        .view_layout_at_size(ui::Vector2::new(
            240.0,
            state.max_collections_panel_height(),
        ));

    assert!(
        !layout
            .overflow_flags
            .get(&COLLECTIONS_LIST_SCROLL_NODE_ID)
            .is_some_and(|overflow| overflow.y),
        "collections list should not have vertical scroll overflow at max height"
    );
}

#[test]
fn collections_overflow_fade_strength_tracks_the_clipped_height() {
    let full_height = 240.0;

    assert_eq!(collection_overflow_fade_alpha(full_height, full_height), 0);
    assert_eq!(collection_overflow_fade_alpha(250.0, full_height), 0);
    assert!(
        collection_overflow_fade_alpha(full_height - 1.0, full_height) > 50,
        "the first clipped pixel should already produce a visible cue"
    );
    assert!(
        collection_overflow_fade_alpha(full_height - 6.0, full_height) > 200,
        "the early clipping ramp should be visibly front-loaded"
    );
    assert_eq!(
        collection_overflow_fade_alpha(full_height - 12.0, full_height),
        u8::MAX
    );
}
