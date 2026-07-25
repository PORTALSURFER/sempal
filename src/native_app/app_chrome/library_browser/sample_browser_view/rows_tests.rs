use super::super::cells::{
    COMPACT_COLUMN_CONTENT_TRAILING_GUTTER, LOCKED_KEEP_RATING_COLOR,
    LOCKED_KEEP_RATING_MARKER_SIDE, RATING_MARKER_SIDE, SIMILARITY_ASPECT_DISABLED_TRACK,
    SIMILARITY_SCORE_FILL, muted_sample_file_cell, sample_collection_cell, sample_file_cell,
    sample_harvest_badge_cell, sample_playback_type_cell, sample_rating_cell,
    sample_similarity_cell, selected_sample_name_cell_for_tests,
};
use super::super::row_widgets::RatingIndicator;
use super::super::similarity_aspect_color;
use super::*;
use crate::native_app::sample_library::folder_browser::{FolderBrowserState, model::FileEntry};
use radiant::{layout::Vector2, prelude::IntoView, runtime::PaintPrimitive, theme::ThemeTokens};
use wavecrate::sample_sources::{Rating, SampleCollection};

/// Builds a representative file entry for row rendering tests.
fn file_entry() -> FileEntry {
    FileEntry {
        id: String::from("C:\\Samples\\portal_SS_kick_003.wav"),
        name: String::from("portal_SS_kick_003.wav"),
        stem: String::from("portal_SS_kick_003"),
        extension: String::from("wav"),
        kind: String::from("Audio"),
        size: String::from("1 KB"),
        size_bytes: 1024,
        modified: String::from("today"),
        modified_rank: 1,
        rating: Rating::NEUTRAL,
        rating_locked: false,
        last_curated_at: None,
        collection: None,
        collections: Vec::new(),
    }
}

fn fill_rects_with_colors(
    frame: &radiant::runtime::SurfaceFrame,
) -> Vec<(radiant::prelude::Rect, radiant::prelude::Rgba8)> {
    let mut rects = frame
        .paint_plan
        .fill_rects()
        .map(|fill| (fill.rect, fill.color))
        .collect::<Vec<_>>();
    for primitive in &frame.paint_plan.primitives {
        if let PaintPrimitive::FillRectBatch(batch) = primitive {
            rects.extend(batch.rects.iter().copied().map(|rect| (rect, batch.color)));
        }
    }
    rects
}

fn fill_rects(frame: &radiant::runtime::SurfaceFrame) -> Vec<radiant::prelude::Rect> {
    fill_rects_with_colors(frame)
        .into_iter()
        .map(|(rect, _)| rect)
        .collect()
}

#[test]
fn active_similarity_anchor_keeps_sample_selection_and_focus_chrome() {
    let frame = sample_browser_row(
        SampleRowDisplay {
            file_id: "anchor.wav",
            selected: true,
            focused: true,
            focus_alpha: u8::MAX,
            selection_flash: false,
            copy_flash: false,
            protected_source_error_flash: false,
            cut_pending: false,
            drag_active: false,
            drag_source: false,
            cached: false,
            missing: false,
            similarity_anchor: true,
            similarity_strength: Some(1.0),
            columns: Vec::new(),
        },
        false,
    )
    .view_frame_at_size_with_default_theme(Vector2::new(320.0, SAMPLE_BROWSER_ROW_HEIGHT));

    assert!(
        frame
            .paint_plan
            .fill_rects()
            .any(|fill| fill.color == crate::native_app::app_chrome::palette::SELECTED_ROW_FILL),
        "an active anchor must not suppress persistent sample selection"
    );
    let focus = crate::native_app::app_chrome::palette::focused_row_marker();
    assert!(
        frame
            .paint_plan
            .fill_rects()
            .any(|fill| fill.color == focus.color && fill.rect.width() == focus.parts.width),
        "an active anchor must not suppress keyboard focus chrome"
    );
}

#[test]
/// Verifies rating strength maps to the visible indicator count.
fn rating_indicator_count_reflects_rating_strength() {
    assert_eq!(RatingIndicator::new(Rating::NEUTRAL, false).count(), 0);
    assert_eq!(RatingIndicator::new(Rating::KEEP_1, false).count(), 1);
    assert_eq!(RatingIndicator::new(Rating::new(2), false).count(), 2);
    assert_eq!(RatingIndicator::new(Rating::TRASH_3, false).count(), 3);
    assert_eq!(RatingIndicator::new(Rating::KEEP_3, true).count(), 3);
}

#[test]
/// Verifies locked keep ratings use the dedicated keep-marker affordance.
fn locked_keep_rating_uses_locked_keep_marker() {
    assert!(RatingIndicator::new(Rating::KEEP_3, true).shows_locked_keep_marker());
    assert!(!RatingIndicator::new(Rating::KEEP_3, false).shows_locked_keep_marker());
    assert!(!RatingIndicator::new(Rating::TRASH_3, true).shows_locked_keep_marker());
}

#[test]
/// Verifies locked keep rows paint a golden marker instead of a text label.
fn locked_keep_rating_cell_paints_gold_marker_without_text() {
    let mut file = file_entry();
    file.rating = Rating::KEEP_3;
    file.rating_locked = true;
    let theme = ThemeTokens::default();
    let frame = sample_rating_cell(RatingIndicator::new(file.rating, file.rating_locked), 64.0)
        .view_frame_at_size(Vector2::new(64.0, 20.0), &theme);

    assert!(
        !frame.paint_plan.text_runs().any(|run| run.text == "KEEP"),
        "locked keep ratings should no longer paint the KEEP text label"
    );

    let marker = frame
        .paint_plan
        .fill_rects()
        .find(|fill| fill.color == LOCKED_KEEP_RATING_COLOR)
        .expect("locked keep ratings should paint the golden marker");

    assert_eq!(marker.rect.width(), LOCKED_KEEP_RATING_MARKER_SIDE as f32);
    assert_eq!(marker.rect.height(), LOCKED_KEEP_RATING_MARKER_SIDE as f32);
    assert!(
        marker.rect.width() > RATING_MARKER_SIDE as f32,
        "locked keep marker should be larger than normal rating markers"
    );
}

#[test]
/// Verifies available sample names use primary text color without cache-state styling.
fn sample_text_uses_primary_theme_color() {
    let theme = ThemeTokens::default();
    let frame = sample_file_cell(String::from("kick_deep"), 120.0)
        .view_frame_at_size(Vector2::new(120.0, 20.0), &theme);

    assert!(
        frame
            .paint_plan
            .text_runs()
            .any(|run| run.text == "kick_deep" && run.color == theme.text_primary),
        "sample rows should not express loaded/cache state through text color"
    );
}

#[test]
fn selected_sample_name_uses_global_accent_color() {
    let theme = ThemeTokens::default();
    let frame = selected_sample_name_cell_for_tests(String::from("kick_deep"), 120.0)
        .view_frame_at_size(Vector2::new(120.0, 20.0), &theme);

    assert!(
        frame.paint_plan.text_runs().any(|run| {
            run.text == "kick_deep" && run.color == crate::native_app::app_chrome::palette::ACCENT
        }),
        "selected sample names should use the same coral accent as selected source and folder labels"
    );
}

#[test]
/// Verifies long sample names end before the header divider gutter.
fn sample_text_keeps_long_label_left_of_header_divider_gutter() {
    let theme = ThemeTokens::default();
    let column_width = 240.0;
    let frame = sample_file_cell(
        String::from("KAB1_0_AmenBreak_Original_FullStem"),
        column_width,
    )
    .view_frame_at_size(Vector2::new(column_width, 20.0), &theme);

    let text = frame
        .paint_plan
        .text_runs()
        .find(|run| run.text == "KAB1_0_AmenBreak_Original_FullStem")
        .expect("sample name text should paint");

    assert!(
        text.rect.max.x <= column_width - COMPACT_COLUMN_CONTENT_TRAILING_GUTTER,
        "sample name text should end left of the header divider gutter: rect={:?}",
        text.rect
    );
}

#[test]
/// Verifies finished harvest rows can demote passive text without heavy row coloring.
fn muted_sample_file_cell_uses_muted_theme_color() {
    let theme = ThemeTokens::default();
    let frame = muted_sample_file_cell(String::from("done_kick"), 120.0)
        .view_frame_at_size(Vector2::new(120.0, 20.0), &theme);

    assert!(
        frame
            .paint_plan
            .text_runs()
            .any(|run| run.text == "done_kick" && run.color == theme.text_muted),
        "done/ignored harvest rows should be able to use subtle muted text styling"
    );
}

#[test]
/// Verifies the empty folder message starts where the item rows would start.
fn empty_folder_message_paints_at_top_of_list_body() {
    let theme = ThemeTokens::default();
    let frame =
        empty_sample_browser_rows(false).view_frame_at_size(Vector2::new(480.0, 240.0), &theme);

    let message = frame
        .paint_plan
        .text_runs()
        .find(|run| run.text == "No audio files in selected folder")
        .expect("empty folder message should paint");

    assert!(
        message.rect.max.y <= SAMPLE_BROWSER_ROW_HEIGHT + 1.0,
        "empty folder message should stay in the first list row, rect={:?}",
        message.rect
    );
}

#[test]
/// Verifies curation mode uses a completion-oriented empty list message.
fn empty_curation_message_says_no_files_are_left_to_curate() {
    let theme = ThemeTokens::default();
    let frame =
        empty_sample_browser_rows(true).view_frame_at_size(Vector2::new(480.0, 240.0), &theme);

    assert!(
        frame
            .paint_plan
            .text_runs()
            .any(|run| run.text == "No files left to curate"),
        "curation empty state should explain that the current curation scope is complete"
    );
}

#[test]
/// Verifies the collection column paints one marker for each collection membership.
fn collection_cell_paints_each_collection_membership_color() {
    let first = SampleCollection::new(0).expect("collection");
    let third = SampleCollection::new(2).expect("collection");
    let mut file = file_entry();
    file.collections = vec![third, first];
    let theme = ThemeTokens::default();
    let folder_browser = FolderBrowserState::load_default();
    let collection_colors = file
        .collection_memberships()
        .into_iter()
        .filter_map(|collection| folder_browser.collection_color(collection))
        .collect::<Vec<_>>();
    let frame = sample_collection_cell(collection_colors, 64.0)
        .view_frame_at_size(Vector2::new(64.0, 20.0), &theme);

    let colors = frame
        .paint_plan
        .fill_rects()
        .map(|fill| fill.color)
        .collect::<Vec<_>>();

    assert!(
        colors.contains(
            &folder_browser
                .collection_color(first)
                .expect("first collection color")
        ),
        "collection column should paint the first collection color"
    );
    assert!(
        colors.contains(
            &folder_browser
                .collection_color(third)
                .expect("third collection color")
        ),
        "collection column should paint the third collection color"
    );
}

#[test]
/// Verifies collection markers remain clipped to their own column cell.
fn collection_cell_keeps_markers_inside_narrow_column_bounds() {
    let collections = [
        SampleCollection::new(0).expect("collection"),
        SampleCollection::new(1).expect("collection"),
        SampleCollection::new(2).expect("collection"),
    ];
    let theme = ThemeTokens::default();
    let folder_browser = FolderBrowserState::load_default();
    let collection_colors = collections
        .into_iter()
        .filter_map(|collection| folder_browser.collection_color(collection))
        .collect::<Vec<_>>();
    let column_width = 24.0;
    let frame = sample_collection_cell(collection_colors.clone(), column_width)
        .view_frame_at_size(Vector2::new(column_width, 20.0), &theme);

    let marker_rects = frame
        .paint_plan
        .fill_rects()
        .filter(|fill| collection_colors.contains(&fill.color))
        .map(|fill| fill.rect)
        .collect::<Vec<_>>();

    assert!(
        !marker_rects.is_empty(),
        "collection cell should paint visible collection markers"
    );
    assert!(
        marker_rects.iter().all(|rect| {
            rect.min.x >= 0.0
                && rect.max.x <= column_width
                && rect.min.y >= 0.0
                && rect.max.y <= 20.0
        }),
        "collection markers should stay inside the collection column bounds: {marker_rects:?}"
    );
}

#[test]
/// Verifies collection markers reserve the header divider gutter.
fn collection_cell_keeps_markers_left_of_header_divider_gutter() {
    let collections = [
        SampleCollection::new(0).expect("collection"),
        SampleCollection::new(1).expect("collection"),
        SampleCollection::new(2).expect("collection"),
    ];
    let theme = ThemeTokens::default();
    let folder_browser = FolderBrowserState::load_default();
    let collection_colors = collections
        .into_iter()
        .filter_map(|collection| folder_browser.collection_color(collection))
        .collect::<Vec<_>>();
    let column_width = 58.0;
    let frame = sample_collection_cell(collection_colors.clone(), column_width)
        .view_frame_at_size(Vector2::new(column_width, 20.0), &theme);

    let max_marker_x = frame
        .paint_plan
        .fill_rects()
        .filter(|fill| collection_colors.contains(&fill.color))
        .map(|fill| fill.rect.max.x)
        .max_by(f32::total_cmp)
        .expect("collection cell should paint visible collection markers");

    assert!(
        max_marker_x <= column_width - COMPACT_COLUMN_CONTENT_TRAILING_GUTTER,
        "collection markers should end left of the header divider gutter: max_marker_x={max_marker_x}"
    );
}

#[test]
/// Verifies rating markers reserve the same divider gutter as collection markers.
fn rating_cell_keeps_markers_left_of_header_divider_gutter() {
    let theme = ThemeTokens::default();
    let column_width = 68.0;
    let frame = sample_rating_cell(RatingIndicator::new(Rating::KEEP_3, false), column_width)
        .view_frame_at_size(Vector2::new(column_width, 20.0), &theme);

    let marker_rects = fill_rects(&frame);

    assert!(
        !marker_rects.is_empty(),
        "rating cell should paint visible rating markers"
    );
    assert!(
        marker_rects.iter().all(|rect| {
            rect.min.x >= 0.0
                && rect.max.x <= column_width - COMPACT_COLUMN_CONTENT_TRAILING_GUTTER
                && rect.min.y >= 0.0
                && rect.max.y <= 20.0
        }),
        "rating markers should stay inside the rating column content bounds: {marker_rects:?}"
    );
}

#[test]
/// Verifies Harvest badges reserve the shared divider gutter.
fn harvest_cell_keeps_badges_left_of_header_divider_gutter() {
    let theme = ThemeTokens::default();
    let column_width = 74.0;
    let frame = sample_harvest_badge_cell(
        vec![String::from("touch"), String::from("D3")],
        column_width,
    )
    .view_frame_at_size(Vector2::new(column_width, 20.0), &theme);

    let text_rects = frame
        .paint_plan
        .text_runs()
        .filter(|run| run.text == "touch" || run.text == "D3")
        .map(|run| run.rect)
        .collect::<Vec<_>>();

    assert!(
        !text_rects.is_empty(),
        "harvest cell should paint visible Harvest badge text"
    );
    assert!(
        text_rects.iter().all(|rect| {
            rect.min.x >= 0.0
                && rect.max.x <= column_width - COMPACT_COLUMN_CONTENT_TRAILING_GUTTER
                && rect.min.y >= 0.0
                && rect.max.y <= 20.0
        }),
        "Harvest badge text should stay inside the Harvest column content bounds: {text_rects:?}"
    );
}

#[test]
/// Verifies compact content does not bleed across neighboring columns.
fn compact_column_content_stays_inside_adjacent_column_boundaries() {
    let theme = ThemeTokens::default();
    let folder_browser = FolderBrowserState::load_default();
    let collection = SampleCollection::new(0).expect("collection");
    let collection_color = folder_browser
        .collection_color(collection)
        .expect("collection color");
    let rating = RatingIndicator::new(Rating::KEEP_3, false);
    let rating_color = rating.color().expect("rating marker color");

    let name_width = 240.0;
    let rating_width = 68.0;
    let harvest_width = 74.0;
    let collection_width = 58.0;
    let row_padding = 8.0;
    let column_spacing = 10.0;
    let name_start = row_padding;
    let rating_start = name_start + name_width + column_spacing;
    let harvest_start = rating_start + rating_width + column_spacing;
    let collection_start = harvest_start + harvest_width + column_spacing;
    let row_width = collection_start + collection_width + row_padding;
    let frame = radiant::application::compact_details_row([
        sample_file_cell(
            String::from("KAB1_0_AmenBreak_Original_FullStem"),
            name_width,
        ),
        sample_rating_cell(rating, rating_width),
        sample_harvest_badge_cell(
            vec![String::from("touch"), String::from("D3")],
            harvest_width,
        ),
        sample_collection_cell(vec![collection_color], collection_width),
    ])
    .view_frame_at_size(Vector2::new(row_width, 22.0), &theme);

    let fills = fill_rects_with_colors(&frame);
    let rating_rects = fills
        .iter()
        .filter_map(|(rect, color)| (*color == rating_color).then_some(*rect))
        .collect::<Vec<_>>();
    let collection_rects = fills
        .iter()
        .filter_map(|(rect, color)| (*color == collection_color).then_some(*rect))
        .collect::<Vec<_>>();
    let harvest_text_rects = frame
        .paint_plan
        .text_runs()
        .filter(|run| run.text == "touch" || run.text == "D3")
        .map(|run| run.rect)
        .collect::<Vec<_>>();
    let name_text_rects = frame
        .paint_plan
        .text_runs()
        .filter(|run| run.text == "KAB1_0_AmenBreak_Original_FullStem")
        .map(|run| run.rect)
        .collect::<Vec<_>>();

    assert!(
        !name_text_rects.is_empty(),
        "adjacent row should paint sample name text"
    );
    assert!(
        !rating_rects.is_empty(),
        "adjacent row should paint rating markers"
    );
    assert!(
        !harvest_text_rects.is_empty(),
        "adjacent row should paint Harvest badge text"
    );
    assert!(
        !collection_rects.is_empty(),
        "adjacent row should paint collection markers"
    );
    assert!(
        name_text_rects
            .iter()
            .all(|rect| rect.max.x
                <= name_start + name_width - COMPACT_COLUMN_CONTENT_TRAILING_GUTTER),
        "sample name text should not bleed into the rating column: {name_text_rects:?}"
    );
    assert!(
        rating_rects.iter().all(|rect| rect.max.x
            <= rating_start + rating_width - COMPACT_COLUMN_CONTENT_TRAILING_GUTTER),
        "rating markers should not bleed into the Harvest column: {rating_rects:?}"
    );
    assert!(
        harvest_text_rects.iter().all(|rect| rect.max.x
            <= harvest_start + harvest_width - COMPACT_COLUMN_CONTENT_TRAILING_GUTTER),
        "Harvest badges should not bleed into the collection column: {harvest_text_rects:?}"
    );
    assert!(
        collection_rects.iter().all(|rect| rect.max.x
            <= collection_start + collection_width - COMPACT_COLUMN_CONTENT_TRAILING_GUTTER),
        "collection markers should not bleed past their column: {collection_rects:?}"
    );
}

#[test]
/// Verifies playback-type cells paint compact type labels.
fn playback_type_cell_paints_loop_label() {
    let theme = ThemeTokens::default();
    let frame = sample_playback_type_cell(Some("Loop"), 76.0)
        .view_frame_at_size(Vector2::new(76.0, 20.0), &theme);

    assert!(
        frame.paint_plan.text_runs().any(|run| run.text == "Loop"),
        "loop playback type should paint a compact Loop label"
    );
}

#[test]
/// Verifies unknown playback type stays visually quiet.
fn missing_playback_type_cell_paints_muted_dash() {
    let theme = ThemeTokens::default();
    let frame =
        sample_playback_type_cell(None, 76.0).view_frame_at_size(Vector2::new(76.0, 20.0), &theme);

    assert!(
        frame
            .paint_plan
            .text_runs()
            .any(|run| run.text == "-" && run.color == theme.text_muted),
        "unknown playback type should paint a muted dash"
    );
}

#[test]
/// Verifies similarity scores render as compact progress bars.
fn similarity_score_cell_paints_progress_fill() {
    let theme = ThemeTokens::default();
    let mut aspects =
        crate::native_app::sample_library::folder_browser::model::EMPTY_SIMILARITY_ASPECT_STRENGTHS;
    aspects[wavecrate_analysis::aspects::SimilarityAspect::Spectrum.index()] = Some(0.8);
    let frame = sample_similarity_cell(
        Some(0.65),
        aspects,
        [true; wavecrate_analysis::aspects::ASPECT_COUNT],
        190.0,
    )
    .view_frame_at_size(Vector2::new(190.0, 20.0), &theme);

    assert!(
        frame
            .paint_plan
            .fill_rects()
            .any(|fill| fill.color == SIMILARITY_SCORE_FILL && fill.rect.width() > 20.0),
        "available similarity scores should paint a progress fill"
    );
    assert!(
        frame.paint_plan.fill_rects().any(|fill| fill.color
            == similarity_aspect_color(wavecrate_analysis::aspects::SimilarityAspect::Spectrum)
            && fill.rect.width() > 4.0),
        "available aspect scores should paint their compact aspect fill"
    );
}

#[test]
/// Verifies unavailable similarity scores are explicit instead of showing a zero bar.
fn missing_similarity_score_cell_paints_na_label() {
    let theme = ThemeTokens::default();
    let frame = sample_similarity_cell(
        None,
        crate::native_app::sample_library::folder_browser::model::EMPTY_SIMILARITY_ASPECT_STRENGTHS,
        [true; wavecrate_analysis::aspects::ASPECT_COUNT],
        190.0,
    )
    .view_frame_at_size(Vector2::new(190.0, 20.0), &theme);

    assert!(
        frame
            .paint_plan
            .text_runs()
            .any(|run| run.text == "N/A" && run.color == theme.text_muted),
        "missing similarity scores should paint a muted N/A label"
    );
}

#[test]
/// Verifies disabled aspects use the explicit disabled track color instead of an active fill.
fn disabled_similarity_aspect_paints_disabled_track() {
    let theme = ThemeTokens::default();
    let mut aspects =
        crate::native_app::sample_library::folder_browser::model::EMPTY_SIMILARITY_ASPECT_STRENGTHS;
    aspects[wavecrate_analysis::aspects::SimilarityAspect::Pitch.index()] = Some(0.9);
    let mut enabled = [true; wavecrate_analysis::aspects::ASPECT_COUNT];
    enabled[wavecrate_analysis::aspects::SimilarityAspect::Pitch.index()] = false;

    let frame = sample_similarity_cell(Some(0.65), aspects, enabled, 190.0)
        .view_frame_at_size(Vector2::new(190.0, 20.0), &theme);

    assert!(
        frame
            .paint_plan
            .fill_rects()
            .any(|fill| fill.color == SIMILARITY_ASPECT_DISABLED_TRACK),
        "disabled aspects should paint the disabled track"
    );
    assert!(
        !frame.paint_plan.fill_rects().any(|fill| fill.color
            == similarity_aspect_color(wavecrate_analysis::aspects::SimilarityAspect::Pitch)
            && fill.rect.width() > 4.0),
        "disabled aspects should not paint their active fill"
    );
}
