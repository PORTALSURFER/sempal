use super::folder_tree_guide_rows;
use super::identity::retained_folder_row_input_id;
use super::rows::{
    FOLDER_LABEL_INSET_X, FOLDER_TREE_EMPTY_LABEL, folder_row, folder_tree_label_color,
    folder_tree_palette_for_tests,
};
use crate::native_app::app::GuiMessage;
use crate::native_app::app_chrome::library_browser::library_sidebar::sidebar_row::sidebar_row_full_palette;
use crate::native_app::app_chrome::palette::{ACCENT, selected_row_marker};
use crate::native_app::sample_library::folder_browser::commands::FolderBrowserMessage;
use crate::native_app::sample_library::folder_browser::model::VisibleFolder;
use crate::native_app::sample_library::folder_browser::view_contract::TREE_ROW_HEIGHT;
use radiant::prelude as ui;
use radiant::prelude::IntoView;
use radiant::runtime::{DeclarativeOwnedRuntimeBridge, SurfaceRuntime};
use radiant::widgets::TextInputMessage;

#[test]
fn folder_tree_uses_shared_grey_sidebar_hover_fill() {
    let theme = ui::ThemeTokens::default();
    let palette = folder_tree_palette_for_tests(&theme);
    let expected = sidebar_row_full_palette(&theme);

    assert_eq!(palette.hovered, expected.hovered);
    assert_eq!(palette.candidate_hovered, expected.candidate_hovered);
    assert_eq!(palette.selected, expected.selected);
    assert_eq!(palette.selected_hovered, expected.selected_hovered);
}

#[test]
fn folders_with_visible_children_paint_disclosure_arrow() {
    let mut folder = visible_folder_for_tests(false);
    folder.has_children = true;

    let collapsed = folder_row(&folder)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(220.0, TREE_ROW_HEIGHT));
    assert_eq!(
        collapsed.paint_plan.svgs().count(),
        1,
        "collapsible folders should paint a closed disclosure arrow"
    );

    folder.expanded = true;
    let expanded = folder_row(&folder)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(220.0, TREE_ROW_HEIGHT));
    assert_eq!(
        expanded.paint_plan.svgs().count(),
        1,
        "expanded folders should paint an open disclosure arrow"
    );
}

#[test]
fn leaf_folders_do_not_paint_disclosure_arrow() {
    let folder = visible_folder_for_tests(false);
    let frame = folder_row(&folder)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(220.0, TREE_ROW_HEIGHT));

    assert_eq!(
        frame.paint_plan.svgs().count(),
        0,
        "leaf folders should keep the disclosure slot visually empty"
    );
}

#[test]
fn source_root_stays_expanded_without_painting_a_disclosure_arrow() {
    let mut folder = visible_folder_for_tests(false);
    folder.is_source_root = true;
    folder.has_children = true;
    folder.expanded = true;

    let frame = folder_row(&folder)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(220.0, TREE_ROW_HEIGHT));

    assert_eq!(
        frame.paint_plan.svgs().count(),
        0,
        "the permanent source root should not offer a collapse control"
    );
}

#[test]
fn source_root_does_not_start_a_tree_guide_through_its_disclosure_slot() {
    let mut root = visible_folder_for_tests(false);
    root.is_source_root = true;
    root.has_children = true;
    root.expanded = true;
    let mut child = visible_folder_for_tests(false);
    child.id = String::from("child");
    child.depth = 1;

    let rows = folder_tree_guide_rows(&[root, child]);

    assert!(!rows[0].starts_descendant_group);
}

#[test]
fn empty_folder_rows_use_subdued_idle_label_color() {
    let folder = visible_folder_for_tests(true);

    assert_eq!(
        folder_tree_label_color(&folder),
        Some(FOLDER_TREE_EMPTY_LABEL)
    );
}

#[test]
fn non_empty_folder_rows_use_default_idle_label_color() {
    let folder = visible_folder_for_tests(false);

    assert_eq!(folder_tree_label_color(&folder), None);
}

#[test]
fn selected_empty_folder_rows_use_global_accent_label_color() {
    let mut folder = visible_folder_for_tests(true);
    folder.selected = true;

    let frame = folder_row(&folder)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(220.0, TREE_ROW_HEIGHT));

    assert_eq!(frame.paint_plan.first_text_color("Folder"), Some(ACCENT));
}

#[test]
fn selected_non_empty_folder_rows_use_global_accent_label_color() {
    let mut folder = visible_folder_for_tests(false);
    folder.selected = true;

    let frame = folder_row(&folder)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(220.0, TREE_ROW_HEIGHT));

    assert_eq!(frame.paint_plan.first_text_color("Folder"), Some(ACCENT));
}

#[test]
fn focused_unselected_folder_rows_use_global_accent_label_color() {
    let mut folder = visible_folder_for_tests(false);
    folder.focused = true;

    let frame = folder_row(&folder)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(220.0, TREE_ROW_HEIGHT));

    assert_eq!(frame.paint_plan.first_text_color("Folder"), Some(ACCENT));
}

#[test]
fn focused_unselected_folder_rows_paint_marker_without_selected_fill() {
    let mut folder = visible_folder_for_tests(false);
    folder.focused = true;

    let frame = folder_row(&folder)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(220.0, TREE_ROW_HEIGHT));
    let selected_fill = folder_tree_palette_for_tests(&ui::ThemeTokens::default())
        .selected
        .expect("folder tree selected fill");

    assert!(
        !frame
            .paint_plan
            .fill_rects()
            .any(|fill| fill.color == selected_fill)
    );
    let focus = crate::native_app::app_chrome::palette::focused_row_marker();
    assert!(
        frame
            .paint_plan
            .fill_rects()
            .any(|fill| fill.color == focus.color && fill.rect.width() == focus.parts.width)
    );
}

#[test]
fn selected_folder_rows_paint_global_fill_and_persistent_marker() {
    let mut folder = visible_folder_for_tests(false);
    folder.focused = true;
    folder.selected = true;

    let frame = folder_row(&folder)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(220.0, TREE_ROW_HEIGHT));
    let selected_fill = folder_tree_palette_for_tests(&ui::ThemeTokens::default())
        .selected
        .expect("folder tree selected fill");
    let marker = selected_row_marker();

    assert!(
        frame
            .paint_plan
            .fill_rects()
            .any(|fill| fill.color == selected_fill),
        "focused selected folder rows should paint the base selected fill"
    );
    assert!(
        frame
            .paint_plan
            .fill_rects()
            .any(|fill| fill.color == marker.color && fill.rect.width() == marker.parts.width),
        "selected folder rows should paint the shared leading marker without requiring hover"
    );
}

#[test]
fn x_marked_folder_rows_paint_the_selection_flash_fill() {
    let mut folder = visible_folder_for_tests(false);
    folder.selected = true;
    folder.selection_flash = true;
    let frame = folder_row(&folder)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(320.0, TREE_ROW_HEIGHT));

    assert!(
        frame.paint_plan.fill_rects().any(|fill| {
            fill.color == crate::native_app::app_chrome::palette::SELECTION_FLASH_FILL
        }),
        "x-marked folders should paint the transient accent flash"
    );
}

#[test]
fn folder_labels_keep_space_after_the_selection_rail_and_disclosure_slot() {
    let mut folder = visible_folder_for_tests(false);
    folder.selected = true;

    let frame = folder_row(&folder)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(220.0, TREE_ROW_HEIGHT));
    let label = frame
        .paint_plan
        .first_text_rect("Folder")
        .expect("folder label should paint");

    let marker = selected_row_marker();
    assert_eq!(label.min.x, FOLDER_LABEL_INSET_X + 2.0);
    assert!(label.min.x - marker.parts.width >= 10.0);
}

#[test]
fn rename_folder_rows_project_draft_into_stable_input() {
    let mut folder = visible_folder_for_tests(false);
    folder.rename_draft = Some(String::from("Renamed Folder"));
    folder.rename_input_id = Some(4_242);

    let frame = folder_row(&folder)
        .view_frame_at_size_with_default_theme(ui::Vector2::new(220.0, TREE_ROW_HEIGHT));
    let input = frame
        .paint_plan
        .first_text_input()
        .expect("rename row should project a text input");

    assert_eq!(input.widget_id, 4_242);
    assert_eq!(input.state.value, "Renamed Folder");
    assert_eq!(input.state.selection_anchor, 0);
    assert_eq!(input.state.caret, "Renamed Folder".chars().count());
    assert!(!frame.paint_plan.contains_text("Folder"));
}

#[test]
fn rename_folder_rows_route_input_messages_to_folder_browser() {
    let mut folder = visible_folder_for_tests(false);
    folder.rename_draft = Some(String::from("Folder"));
    folder.rename_input_id = Some(4_242);
    let message = TextInputMessage::Changed {
        value: String::from("Renamed Folder"),
    };

    assert_eq!(
        folder_row(&folder)
            .view_dispatch_widget_output(4_242, ui::WidgetOutput::typed(message.clone()),),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::RenameInput(message)
        ))
    );
}

#[test]
fn standard_folder_rows_derive_stable_input_id_from_row_key() {
    let folder = visible_folder_for_tests(false);
    let input_id = retained_folder_row_input_id(folder.id.as_str());
    let mut surface = folder_row(&folder).into_surface();
    let bounds = ui::Rect::from_size(220.0, TREE_ROW_HEIGHT);
    let position = ui::Point::new(8.0, 10.0);

    surface.dispatch_widget_input(
        input_id,
        bounds,
        ui::WidgetInput::PointerPress {
            position,
            button: ui::PointerButton::Primary,
            modifiers: Default::default(),
        },
    );
    let output = surface.dispatch_widget_input(
        input_id,
        bounds,
        ui::WidgetInput::PointerRelease {
            position,
            button: ui::PointerButton::Primary,
            modifiers: Default::default(),
        },
    );

    assert_eq!(
        output.and_then(|output| output.typed_cloned::<GuiMessage>()),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::ActivateFolder(folder.id.clone(), Default::default())
        ))
    );
}

#[test]
fn dragged_source_folder_row_clears_active_drop_target_on_hover() {
    let mut folder = visible_folder_for_tests(false);
    folder.drag_active = true;
    folder.drag_source = true;
    folder.drop_target_active = true;
    let folder_id = folder.id.clone();
    let position = ui::Point::new(8.0, 10.0);
    let bridge = DeclarativeOwnedRuntimeBridge::new(
        Vec::<GuiMessage>::new(),
        move |_| folder_row(&folder).fill_width().into_surface(),
        |state: &mut Vec<GuiMessage>, message| state.push(message),
    );
    let mut runtime = SurfaceRuntime::new(bridge, ui::Vector2::new(220.0, TREE_ROW_HEIGHT));

    runtime.dispatch_input_at(position, ui::WidgetInput::pointer_move(position));

    assert_eq!(
        runtime.bridge().state(),
        &[GuiMessage::FolderBrowser(
            FolderBrowserMessage::ClearDropTargetUnless(folder_id, position)
        )]
    );
}

fn visible_folder_for_tests(empty: bool) -> VisibleFolder {
    VisibleFolder {
        id: String::from("folder"),
        name: String::from("Folder"),
        depth: 0,
        is_source_root: false,
        has_children: false,
        empty,
        locked: false,
        lock_inherited: false,
        expanded: false,
        selected: false,
        focused: false,
        focus_alpha: u8::MAX,
        selection_flash: false,
        drag_active: false,
        drag_source: false,
        drop_candidate: false,
        drop_target: false,
        drop_target_active: false,
        rename_draft: None,
        rename_input_id: None,
    }
}
