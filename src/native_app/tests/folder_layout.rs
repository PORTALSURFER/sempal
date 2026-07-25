use super::native_runtime_for_tests;
use crate::native_app::sample_library::folder_browser::view_contract::FOLDER_TREE_SELECTION_CONTEXT_ROWS;
use crate::native_app::test_support::{
    sample_browser::{DEFAULT_FOLDER_WIDTH, MAX_FOLDER_WIDTH, MIN_FOLDER_WIDTH},
    state::{FolderBrowserState, GuiMessage, NativeAppStateFixture},
};
use radiant::runtime::{Command, Event};
use radiant::{
    gui::types::{Point, Vector2},
    runtime::SurfaceFrame,
    widgets::DragHandleMessage,
};
use std::fs;

#[test]
fn folder_browser_splitter_resizes_and_clamps_width() {
    let mut state = NativeAppStateFixture::default()
        .with_synthetic_waveform()
        .with_sample_status("")
        .build();
    state.resize_folder_browser(DragHandleMessage::started(Point::new(100.0, 0.0)));
    state.resize_folder_browser(DragHandleMessage::moved(Point::new(160.0, 0.0)));

    assert_eq!(
        state.ui.chrome.folder_panel.size(),
        DEFAULT_FOLDER_WIDTH + 60.0
    );

    state.resize_folder_browser(DragHandleMessage::moved(Point::new(900.0, 0.0)));
    assert_eq!(state.ui.chrome.folder_panel.size(), MAX_FOLDER_WIDTH);

    state.resize_folder_browser(DragHandleMessage::ended(Point::new(-900.0, 0.0)));
    assert_eq!(state.ui.chrome.folder_panel.size(), MIN_FOLDER_WIDTH);
    assert!(!state.ui.chrome.folder_panel.is_resizing());
}

#[test]
fn folder_tree_and_sample_list_share_one_pixel_boundary() {
    let tempdir = tempfile::tempdir().expect("create temp root");
    let root = tempdir.path().join("wavecrate-pane-boundary");
    fs::create_dir_all(&root).expect("create source root");
    fs::write(root.join("sample.wav"), []).expect("write sample");
    let state = NativeAppStateFixture::default()
        .with_synthetic_waveform()
        .with_folder_browser(FolderBrowserState::from_root(root))
        .build();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let tree = runtime
        .layout()
        .rects
        .get(&crate::native_app::ui::ids::FOLDER_TREE_LIST_ID)
        .copied()
        .expect("folder tree list should be laid out");
    let samples = runtime
        .layout()
        .rects
        .get(&crate::native_app::sample_library::sample_list::SAMPLE_BROWSER_LIST_ID)
        .copied()
        .expect("sample list should be laid out");

    assert!(
        (samples.min.x - tree.max.x - 1.0).abs() < 0.01,
        "the resize divider should be the only column between panes: tree={tree:?}, samples={samples:?}"
    );
    let frame = runtime.frame(&radiant::prelude::ThemeTokens::default());
    let rail = frame
        .paint_plan
        .fill_rects()
        .find(|fill| {
            (fill.rect.min.x - tree.max.x).abs() < 0.01
                && (fill.rect.width() - 1.0).abs() < 0.01
                && fill.color == radiant::prelude::ThemeTokens::default().border_emphasis
        })
        .expect("the outer sidebar resize boundary should paint one continuous rail");
    assert!(rail.rect.min.y <= tree.min.y);
    assert!(rail.rect.max.y >= samples.max.y);
    assert!(
        frame.paint_plan.stroke_rects().all(|stroke| {
            ((stroke.rect.max.x - tree.max.x).abs() >= 0.01
                && (stroke.rect.min.x - samples.min.x).abs() >= 0.01)
                || stroke.rect.height() <= 40.0
        }),
        "inner lists must not paint structural edges beside the continuous sidebar rail"
    );

    let hover = Point::new(samples.min.x - 3.0, samples.center().y);
    assert_eq!(
        runtime.dispatch_event(Event::pointer_move(hover)),
        Some(crate::native_app::ui::ids::LIBRARY_SIDEBAR_RESIZE_HANDLE_ID),
        "the resize hit target should extend inward from the one-pixel rail"
    );
    let hovered_frame = runtime.frame(&radiant::prelude::ThemeTokens::default());
    let crossing_rail = hovered_frame
        .paint_plan
        .fill_rects()
        .find(|fill| {
            (fill.rect.min.x - tree.max.x).abs() < 0.01
                && (fill.rect.width() - 1.0).abs() < 0.01
                && fill.rect.min.y <= tree.min.y
                && fill.rect.max.y >= samples.max.y
        })
        .expect("resize rail during a fast pointer crossing");
    assert_eq!(crossing_rail.rect, rail.rect);
    assert_eq!(
        crossing_rail.color,
        radiant::prelude::ThemeTokens::default().border_emphasis,
        "a fast pointer crossing should not flash the resize highlight"
    );

    let initial_width = runtime.bridge().state().ui.chrome.folder_panel.size();
    assert_eq!(
        runtime.dispatch_event(Event::primary_press(hover)),
        Some(crate::native_app::ui::ids::LIBRARY_SIDEBAR_RESIZE_HANDLE_ID)
    );
    let pressed_frame = runtime.frame(&radiant::prelude::ThemeTokens::default());
    let active_rail = pressed_frame
        .paint_plan
        .fill_rects()
        .find(|fill| {
            fill.widget_id == crate::native_app::ui::ids::LIBRARY_SIDEBAR_RESIZE_HANDLE_ID
                && (fill.rect.width() - 1.0).abs() < 0.01
                && fill.color != radiant::prelude::ThemeTokens::default().border_emphasis
        })
        .expect("pointer-down should light the resize rail immediately");
    let active_rail_index = pressed_frame
        .paint_plan
        .primitives
        .iter()
        .rposition(|primitive| {
            primitive.widget_id()
                == Some(crate::native_app::ui::ids::LIBRARY_SIDEBAR_RESIZE_HANDLE_ID)
                && primitive.rects().any(|rect| rect == active_rail.rect)
        })
        .expect("active resize rail paint primitive");
    assert!(
        pressed_frame.paint_plan.primitives[active_rail_index + 1..]
            .iter()
            .filter(|primitive| primitive.is_paint())
            .flat_map(|primitive| primitive.rects())
            .all(|rect| !rect.overlaps(active_rail.rect)),
        "the resize rail must remain unobscured by later sidebar or workspace paint"
    );

    let drag = Point::new(hover.x + 20.0, hover.y);
    assert_eq!(
        runtime.dispatch_event(Event::pointer_move(drag)),
        Some(crate::native_app::ui::ids::LIBRARY_SIDEBAR_RESIZE_HANDLE_ID)
    );
    assert_eq!(
        runtime.bridge().state().ui.chrome.folder_panel.size(),
        initial_width + 20.0
    );
    assert_eq!(
        runtime.dispatch_event(Event::primary_release(drag)),
        Some(crate::native_app::ui::ids::LIBRARY_SIDEBAR_RESIZE_HANDLE_ID)
    );
    let _ = runtime.dispatch_event(Event::pointer_move(drag));
    let released_frame = runtime.frame(&radiant::prelude::ThemeTokens::default());
    let released_rail = released_frame
        .paint_plan
        .fill_rects()
        .find(|fill| {
            fill.widget_id == crate::native_app::ui::ids::LIBRARY_SIDEBAR_RESIZE_HANDLE_ID
                && (fill.rect.width() - 1.0).abs() < 0.01
        })
        .expect("released sidebar resize rail");
    assert_eq!(
        released_rail.color,
        radiant::prelude::ThemeTokens::default().border_emphasis,
        "releasing the mouse should immediately clear active resize highlighting"
    );
}

#[test]
fn keyboard_folder_navigation_keeps_selected_folder_in_tree_view() {
    let tempdir = tempfile::tempdir().expect("create temp root");
    let root = tempdir.path().join("wavecrate-folder-keyboard-follow");
    fs::create_dir_all(&root).expect("create source root");
    for index in 0..20 {
        let folder = root.join(format!("folder_{index:02}"));
        fs::create_dir_all(&folder).expect("create folder");
        fs::write(folder.join("sample.wav"), []).expect("write sample");
    }
    let mut state = NativeAppStateFixture::default()
        .with_folder_browser(FolderBrowserState::from_root(root.clone()))
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();
    assert!(
        state.library.folder_browser.visible_folders().len() > 12,
        "fixture should produce a long visible folder tree"
    );

    for _ in 0..12 {
        state.navigate_browser(1, false, false, &mut context);
    }

    assert_eq!(
        state.library.folder_browser.selected_folder_id(),
        Some(root.join("folder_11").to_string_lossy().as_ref())
    );
    assert_eq!(
        last_fixed_row_scroll(context.into_command()),
        Some((
            12,
            23.0,
            FOLDER_TREE_SELECTION_CONTEXT_ROWS,
            FOLDER_TREE_SELECTION_CONTEXT_ROWS,
            1,
        ))
    );
}

#[test]
fn full_gui_folder_tree_pointer_selection_preserves_manual_scroll_window() {
    let tempdir = tempfile::tempdir().expect("create temp root");
    let root = tempdir
        .path()
        .join("wavecrate-folder-tree-pointer-stability");
    fs::create_dir_all(&root).expect("create source root");
    for index in 0..100 {
        let folder = root.join(format!("folder_{index:02}"));
        fs::create_dir_all(&folder).expect("create folder");
        fs::write(folder.join("sample.wav"), []).expect("write sample");
    }
    let state = NativeAppStateFixture::default()
        .with_folder_browser(FolderBrowserState::from_root(root))
        .build();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 900.0));
    let tree_rect = runtime
        .layout()
        .rects
        .get(&crate::native_app::ui::ids::FOLDER_TREE_LIST_ID)
        .copied()
        .expect("folder tree list should be laid out");
    let scroll_point = text_center(&runtime.frame_with_default_theme(), "folder_00");
    for _ in 0..24 {
        if !runtime.wheel_or_scroll_at(scroll_point, Vector2::new(0.0, 110.0)) {
            break;
        }
    }

    let before = runtime
        .bridge()
        .state()
        .library
        .folder_browser
        .tree_view_start();
    assert!(
        before > 0,
        "fixture should establish a manually scrolled tree: rect={tree_rect:?}, point={scroll_point:?}"
    );
    let viewport_rows = (tree_rect.height()
        / crate::native_app::sample_library::folder_browser::view_contract::TREE_ROW_HEIGHT)
        .ceil()
        .max(1.0) as usize;
    let visible_folders = runtime
        .bridge()
        .state()
        .library
        .folder_browser
        .visible_folders();
    let target_index = (before + viewport_rows.saturating_sub(3)).min(visible_folders.len() - 1);
    let target_id = visible_folders[target_index].id.clone();
    let target_label = visible_folders[target_index].name.clone();
    let frame = runtime.frame_with_default_theme();

    runtime.dispatch_primary_click(text_center(&frame, &target_label));

    assert_eq!(
        runtime
            .bridge()
            .state()
            .library
            .folder_browser
            .selected_folder_id(),
        Some(target_id.as_str()),
        "folder tree click should select the intended folder"
    );

    let mut starts = vec![
        runtime
            .bridge()
            .state()
            .library
            .folder_browser
            .tree_view_start(),
    ];
    let mut selected_row_tops = vec![text_top(&runtime.frame_with_default_theme(), &target_label)];
    for _ in 0..4 {
        runtime.refresh();
        starts.push(
            runtime
                .bridge()
                .state()
                .library
                .folder_browser
                .tree_view_start(),
        );
        selected_row_tops.push(text_top(&runtime.frame_with_default_theme(), &target_label));
    }

    assert_eq!(
        starts,
        vec![before; starts.len()],
        "clicking a visible folder should not move the manually scrolled tree viewport"
    );
    assert!(
        selected_row_tops
            .windows(2)
            .all(|pair| (pair[0] - pair[1]).abs() < 0.5),
        "clicking a visible folder should not repaint it at a different y position: {selected_row_tops:?}"
    );
}

#[test]
fn x_toggle_marks_focused_folder_without_sample_focus() {
    let tempdir = tempfile::tempdir().expect("create temp root");
    let root = tempdir.path().join("wavecrate-folder-x-toggle");
    let drums = root.join("drums");
    let loops = root.join("loops");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    fs::write(drums.join("kick.wav"), []).expect("write drums sample");
    fs::write(loops.join("loop.wav"), []).expect("write loops sample");
    let mut state = NativeAppStateFixture::default()
        .with_folder_browser(FolderBrowserState::from_root(root.clone()))
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(GuiMessage::ToggleSelectedSampleAndAdvance, &mut context);

    let visible = state.library.folder_browser.visible_folders();
    let root_id = root.display().to_string();
    let drums_id = drums.display().to_string();
    let loops_id = loops.display().to_string();
    let root_row = visible
        .iter()
        .find(|folder| folder.id == root_id)
        .expect("root row should stay visible");
    let drums_row = visible
        .iter()
        .find(|folder| folder.id == drums_id)
        .expect("drums row should stay visible");
    assert!(root_row.selected);
    assert!(root_row.selection_flash);
    assert!(!root_row.focused);
    assert!(!drums_row.selected);
    assert!(!drums_row.selection_flash);
    assert!(drums_row.focused);
    assert!(state.ui.status.sample.contains("Marked"));

    state.apply_message(GuiMessage::ToggleSelectedSampleAndAdvance, &mut context);

    let visible = state.library.folder_browser.visible_folders();
    let drums_row = visible
        .iter()
        .find(|folder| folder.id == drums_id)
        .expect("drums row should stay visible");
    let loops_row = visible
        .iter()
        .find(|folder| folder.id == loops_id)
        .expect("loops row should stay visible");
    assert!(drums_row.selected);
    assert!(drums_row.selection_flash);
    assert!(!drums_row.focused);
    assert!(!loops_row.selected);
    assert!(!loops_row.selection_flash);
    assert!(loops_row.focused);
    assert!(state.ui.status.sample.contains("2 selected"));
}

fn text_center(frame: &SurfaceFrame, label: &str) -> Point {
    frame
        .paint_plan
        .text_runs()
        .find(|text| text.text.as_str() == label)
        .map(|text| text.rect.center())
        .unwrap_or_else(|| panic!("{label} should paint"))
}

fn text_top(frame: &SurfaceFrame, label: &str) -> f32 {
    frame
        .paint_plan
        .text_runs()
        .find(|text| text.text.as_str() == label)
        .map(|text| text.rect.min.y)
        .unwrap_or_else(|| panic!("{label} should paint"))
}

fn last_fixed_row_scroll(command: Command<GuiMessage>) -> Option<(usize, f32, usize, usize, i32)> {
    match command {
        Command::Batch(commands) => commands
            .into_iter()
            .filter_map(last_fixed_row_scroll)
            .last(),
        Command::ScrollFixedRowIntoView {
            row_index,
            row_stride,
            leading_context_rows,
            trailing_context_rows,
            direction,
            ..
        } => Some((
            row_index,
            row_stride,
            leading_context_rows,
            trailing_context_rows,
            direction,
        )),
        _ => None,
    }
}
