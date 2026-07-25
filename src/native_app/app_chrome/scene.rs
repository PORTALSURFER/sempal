use crate::native_app::app::{GuiMessage, NativeAppState, default_gui_shortcuts};
use crate::native_app::app_chrome::layout;
use crate::native_app::app_chrome::library_browser::library_sidebar;
use crate::native_app::app_chrome::library_browser::sample_browser_view;
use crate::native_app::ui::ids::{
    COLLECTIONS_LIST_SCROLL_NODE_ID, FILTER_SECTION_SCROLL_NODE_ID, FOLDER_TREE_LIST_ID,
    METADATA_TAG_SCROLL_NODE_ID, SAMPLE_BROWSER_MAP_ID,
};
use radiant::prelude as ui;

const APP_TRANSIENT_OVERLAY_KEY: u64 = 0x6170_705f_6f76_726c;
const OVERFLOW_FADE_TRANSIENT_OVERLAY_KEY: u64 = 0x6f76_6572_6661_6465;
const APP_FRAME_CLOCK_FPS: u32 = 60;

pub(in crate::native_app) fn view(state: &NativeAppState) -> ui::View<GuiMessage> {
    scene(state).into_view().fill()
}

fn scene(state: &NativeAppState) -> ui::Scene<GuiMessage> {
    ui::scene(layout::shell(state))
        .shortcuts(default_gui_shortcuts(state))
        .frame_clock(frame_clock())
        .overlay(app_transient_overlay())
        .overlay(overflow_fade_overlay())
}

fn frame_clock() -> ui::FrameClock<NativeAppState, GuiMessage> {
    ui::FrameClock::message(GuiMessage::Frame)
        .fps_with(|state: &mut NativeAppState| {
            (!state.playback_visual_activity_active()).then_some(APP_FRAME_CLOCK_FPS)
        })
        .surface_revisions(|state: &mut NativeAppState| state.frame_surface_revisions())
}

fn app_transient_overlay() -> ui::TransientOverlay<NativeAppState> {
    ui::TransientOverlay::new(APP_TRANSIENT_OVERLAY_KEY)
        .paint_only()
        .when(|state: &mut NativeAppState| state.should_paint_app_transient_overlay())
        .paint(paint_app_transient_overlay)
}

fn overflow_fade_overlay() -> ui::TransientOverlay<NativeAppState> {
    ui::TransientOverlay::new(OVERFLOW_FADE_TRANSIENT_OVERLAY_KEY)
        .paint_only()
        .fps(APP_FRAME_CLOCK_FPS)
        .when(|state: &mut NativeAppState| state.ui.chrome.overflow_fades.frame_needed())
        .paint(paint_overflow_fade_overlay)
}

fn paint_app_transient_overlay(
    state: &mut NativeAppState,
    context: radiant::runtime::TransientOverlayContext<'_>,
    primitives: &mut Vec<radiant::runtime::PaintPrimitive>,
) {
    state.paint_waveform_transient_overlay(context, primitives);
    state.paint_worker_progress_indicator(context, primitives);
    state.paint_source_processing_source_pulse(context, primitives);
    paint_starmap_active_audition_overlay(state, context, primitives);
}

fn paint_overflow_fade_overlay(
    state: &mut NativeAppState,
    context: radiant::runtime::TransientOverlayContext<'_>,
    primitives: &mut Vec<radiant::runtime::PaintPrimitive>,
) {
    state
        .ui
        .chrome
        .overflow_fades
        .consume_pending_visibility_sample();
    paint_library_overflow_fades(state, context, primitives);
    library_sidebar::paint_waveform_scroll_fades(
        context,
        &state.waveform.current,
        &mut state.ui.chrome.overflow_fades,
        primitives,
    );
}

fn collection_overflow_fade_alpha(state: &NativeAppState) -> u8 {
    let folder_browser = &state.library.folder_browser;
    library_sidebar::collection_overflow_fade_alpha(
        folder_browser.collections_panel_height(),
        folder_browser.max_collections_panel_height(),
    )
}

fn paint_library_overflow_fades(
    state: &mut NativeAppState,
    context: radiant::runtime::TransientOverlayContext<'_>,
    primitives: &mut Vec<radiant::runtime::PaintPrimitive>,
) {
    const COLLECTIONS_OVERFLOW_FADE_ID: u64 = 0x636f_6c6c_5f66_6164;
    const COLLECTIONS_TOP_OVERFLOW_FADE_ID: u64 = 0x636f_6c6c_5f74_6f70;
    const FOLDER_TREE_OVERFLOW_FADE_ID: u64 = 0x666f_6c64_5f66_6164;
    const FOLDER_TREE_TOP_OVERFLOW_FADE_ID: u64 = 0x666f_6c64_5f74_6f70;
    const FILTERS_OVERFLOW_FADE_ID: u64 = 0x6669_6c74_5f66_6164;
    const FILTERS_TOP_OVERFLOW_FADE_ID: u64 = 0x6669_6c74_5f74_6f70;
    const TAGS_OVERFLOW_FADE_ID: u64 = 0x7461_6773_5f66_6164;
    const TAGS_TOP_OVERFLOW_FADE_ID: u64 = 0x7461_6773_5f74_6f70;

    let collections_alpha = collection_overflow_fade_alpha(state);
    let fades = [
        (
            COLLECTIONS_LIST_SCROLL_NODE_ID,
            COLLECTIONS_TOP_OVERFLOW_FADE_ID,
            COLLECTIONS_OVERFLOW_FADE_ID,
            collections_alpha,
            collections_alpha,
        ),
        (
            FOLDER_TREE_LIST_ID,
            FOLDER_TREE_TOP_OVERFLOW_FADE_ID,
            FOLDER_TREE_OVERFLOW_FADE_ID,
            u8::MAX,
            0,
        ),
        (
            FILTER_SECTION_SCROLL_NODE_ID,
            FILTERS_TOP_OVERFLOW_FADE_ID,
            FILTERS_OVERFLOW_FADE_ID,
            u8::MAX,
            0,
        ),
        (
            METADATA_TAG_SCROLL_NODE_ID,
            TAGS_TOP_OVERFLOW_FADE_ID,
            TAGS_OVERFLOW_FADE_ID,
            u8::MAX,
            0,
        ),
    ];
    for (scroll_node_id, top_fade_id, bottom_fade_id, maximum_opacity, entry_opacity) in fades {
        library_sidebar::paint_vertical_scroll_overflow_fades(
            context,
            scroll_node_id,
            top_fade_id,
            bottom_fade_id,
            maximum_opacity,
            entry_opacity,
            &mut state.ui.chrome.overflow_fades,
            primitives,
        );
    }
}

fn paint_starmap_active_audition_overlay(
    state: &mut NativeAppState,
    context: radiant::runtime::TransientOverlayContext<'_>,
    primitives: &mut Vec<radiant::runtime::PaintPrimitive>,
) {
    let Some(active_file_id) = state.active_starmap_audition_file_id() else {
        return;
    };
    let Some(bounds) = context
        .plan
        .first_widget_rect_by_priority([SAMPLE_BROWSER_MAP_ID])
    else {
        return;
    };
    let Some(items) = state.library.folder_browser.cached_starmap_projection() else {
        return;
    };
    sample_browser_view::paint_active_starmap_audition_overlay(
        primitives,
        bounds,
        &items,
        state.ui.chrome.starmap_viewport,
        active_file_id,
    );
}
