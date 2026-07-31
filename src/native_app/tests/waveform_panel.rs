use crate::native_app::{
    app_chrome::{
        view_models::waveform_panel::WaveformPanelViewModel,
        waveform_panel::{WAVEFORM_PANEL_HEIGHT, WAVEFORM_VIEW_HEIGHT, waveform_panel},
    },
    test_support::state::NativeAppStateFixture,
    ui::ids::{
        WAVEFORM_LOADED_SAMPLE_DRAG_HANDLE_ID, WAVEFORM_PLAYMARK_LABEL_ID,
        WAVEFORM_RESIZE_HANDLE_ID,
    },
};
use radiant::prelude::{self as ui, IntoView};

#[test]
fn waveform_panel_uses_the_taller_editorial_viewport() {
    assert_eq!(WAVEFORM_VIEW_HEIGHT, 196.0);
    assert_eq!(WAVEFORM_PANEL_HEIGHT, 226.0);
}

#[test]
fn waveform_panel_resize_handle_routes_drag_messages() {
    let state = NativeAppStateFixture::default().build();
    let drag = ui::DragHandleMessage::started(ui::Point::new(320.0, 222.0));
    let surface = waveform_panel(WaveformPanelViewModel::from_app_state(&state)).into_surface();

    assert!(surface.find_widget(WAVEFORM_RESIZE_HANDLE_ID).is_some());
    assert_eq!(
        waveform_panel(WaveformPanelViewModel::from_app_state(&state)).view_dispatch_widget_output(
            WAVEFORM_RESIZE_HANDLE_ID,
            ui::WidgetOutput::typed(drag.clone()),
        ),
        Some(crate::native_app::app::GuiMessage::ResizeWaveformPanel(
            drag
        )),
    );
}

#[test]
fn waveform_panel_resize_updates_panel_and_viewport_height() {
    let mut state = NativeAppStateFixture::default().build();
    let initial_height = state.ui.chrome.waveform_panel_height();
    let delta = 64.0;

    state.apply_message(
        crate::native_app::app::GuiMessage::ResizeWaveformPanel(ui::DragHandleMessage::started(
            ui::Point::new(320.0, initial_height),
        )),
        &mut ui::UiUpdateContext::default(),
    );
    state.apply_message(
        crate::native_app::app::GuiMessage::ResizeWaveformPanel(ui::DragHandleMessage::moved(
            ui::Point::new(320.0, initial_height + delta),
        )),
        &mut ui::UiUpdateContext::default(),
    );

    assert_eq!(
        state.ui.chrome.waveform_panel_height(),
        initial_height + delta
    );
    let frame = waveform_panel(WaveformPanelViewModel::from_app_state(&state))
        .view_frame_at_size_with_default_theme(ui::Vector2::new(800.0, initial_height + delta));
    let waveform = frame
        .layout
        .rects
        .get(&crate::native_app::ui::ids::WAVEFORM_WIDGET_ID)
        .expect("resized waveform should be laid out");

    assert_eq!(waveform.height(), WAVEFORM_VIEW_HEIGHT + delta);
}

fn loaded_sample_drag_handle_tooltip(
    state: &crate::native_app::app::NativeAppState,
) -> Option<String> {
    waveform_panel(WaveformPanelViewModel::from_app_state(state))
        .into_surface()
        .find_widget(WAVEFORM_LOADED_SAMPLE_DRAG_HANDLE_ID)
        .and_then(|widget| {
            widget
                .widget_object()
                .common()
                .tooltip
                .as_deref()
                .map(str::to_owned)
        })
}

#[test]
fn waveform_panel_omits_section_header_label() {
    let state = NativeAppStateFixture::default().build();
    let surface = waveform_panel(WaveformPanelViewModel::from_app_state(&state)).into_surface();
    let frame = waveform_panel(WaveformPanelViewModel::from_app_state(&state))
        .view_frame_at_size_with_default_theme(ui::Vector2::new(800.0, WAVEFORM_PANEL_HEIGHT));

    assert!(frame.paint_plan.contains_text("No sample loaded"));
    assert!(!frame.paint_plan.contains_text("Waveform"));
    assert!(
        surface
            .find_widget(WAVEFORM_LOADED_SAMPLE_DRAG_HANDLE_ID)
            .is_none()
    );
    assert!(
        frame
            .paint_plan
            .stroke_polylines()
            .all(|stroke| stroke.widget_id != WAVEFORM_LOADED_SAMPLE_DRAG_HANDLE_ID)
    );
}

#[test]
fn loaded_waveform_title_includes_sample_drag_handle_before_name() {
    let state = NativeAppStateFixture::default()
        .with_synthetic_waveform()
        .build();
    let surface = waveform_panel(WaveformPanelViewModel::from_app_state(&state)).into_surface();
    let frame = waveform_panel(WaveformPanelViewModel::from_app_state(&state))
        .view_frame_at_size_with_default_theme(ui::Vector2::new(800.0, WAVEFORM_PANEL_HEIGHT));

    assert!(
        surface
            .find_widget(WAVEFORM_LOADED_SAMPLE_DRAG_HANDLE_ID)
            .is_some(),
        "loaded waveform title should include interactive sample drag handle"
    );
    let handle_right_edge = frame
        .paint_plan
        .stroke_polylines()
        .filter(|stroke| stroke.widget_id == WAVEFORM_LOADED_SAMPLE_DRAG_HANDLE_ID)
        .flat_map(|stroke| stroke.points.iter().map(|point| point.x))
        .fold(None, |max: Option<f32>, x| {
            Some(max.map_or(x, |max| max.max(x)))
        })
        .expect("loaded waveform title should include sample drag handle");
    let title_rect = frame
        .paint_plan
        .text_runs()
        .find(|run| run.text.starts_with("synthetic-waveform |"))
        .map(|run| run.rect)
        .expect("loaded waveform title should include sample name");

    assert!(handle_right_edge < title_rect.min.x);
}

#[test]
fn loaded_sample_drag_handle_omits_tooltip_when_help_is_inactive() {
    let state = NativeAppStateFixture::default()
        .with_synthetic_waveform()
        .build();

    assert_eq!(loaded_sample_drag_handle_tooltip(&state), None);
}

#[test]
fn loaded_sample_drag_handle_uses_help_tooltip_when_help_is_active() {
    let mut state = NativeAppStateFixture::default()
        .with_synthetic_waveform()
        .build();
    state.ui.chrome.help_tooltips_enabled = true;

    assert_eq!(
        loaded_sample_drag_handle_tooltip(&state).as_deref(),
        Some("Drag loaded sample")
    );
}

#[test]
fn waveform_help_tooltip_attaches_to_interaction_widget() {
    let mut state = NativeAppStateFixture::default()
        .with_synthetic_waveform()
        .build();
    state.ui.chrome.help_tooltips_enabled = true;
    let surface = waveform_panel(WaveformPanelViewModel::from_app_state(&state)).into_surface();
    let tooltip = surface
        .find_widget(crate::native_app::ui::ids::WAVEFORM_WIDGET_ID)
        .and_then(|widget| widget.widget_object().common().tooltip.as_deref());

    assert_eq!(
        tooltip,
        Some(
            "Waveform: click to set playback start, drag to select, Z zooms to selection, X zooms out."
        )
    );
}

#[test]
fn playmark_bottom_controls_keep_length_editing_with_toolbar_grid_controls() {
    let mut state = NativeAppStateFixture::default()
        .with_synthetic_waveform()
        .build();
    state.waveform.current.set_play_selection_range(0.25, 0.75);
    state.ui.chrome.beat_guides_enabled = true;
    state.ui.chrome.beat_guide_count = 16;
    assert!(state.waveform.current.begin_playmark_label_edit(true, 16));

    let surface = waveform_panel(WaveformPanelViewModel::from_app_state(&state)).into_surface();
    let frame = waveform_panel(WaveformPanelViewModel::from_app_state(&state))
        .view_frame_at_size_with_default_theme(ui::Vector2::new(800.0, WAVEFORM_PANEL_HEIGHT));

    assert!(surface.find_widget(WAVEFORM_PLAYMARK_LABEL_ID).is_some());
    assert_eq!(
        surface
            .find_widget(WAVEFORM_PLAYMARK_LABEL_ID)
            .expect("playmark length control")
            .widget_object()
            .automation_semantics()
            .value_text
            .as_deref(),
        Some("1920 BPM")
    );
    assert_eq!(frame.paint_plan.text_inputs().count(), 1);
}

#[test]
fn editing_playmark_label_paints_one_text_input() {
    let mut state = NativeAppStateFixture::default()
        .with_synthetic_waveform()
        .build();
    state.waveform.current.set_play_selection_range(0.25, 0.75);
    assert!(state.waveform.current.begin_playmark_label_edit(false, 4));

    let frame = waveform_panel(WaveformPanelViewModel::from_app_state(&state))
        .view_frame_at_size_with_default_theme(ui::Vector2::new(800.0, WAVEFORM_PANEL_HEIGHT));
    let inputs = frame
        .paint_plan
        .text_inputs()
        .filter(|input| input.widget_id == crate::native_app::ui::ids::WAVEFORM_PLAYMARK_LABEL_ID)
        .count();

    assert_eq!(inputs, 1);
}
