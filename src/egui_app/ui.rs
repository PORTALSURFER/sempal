//! egui renderer for the application UI.

mod chrome;
mod drag_overlay;
mod drag_targets;
mod feedback_issue;
mod flat_items_list;
mod helpers;
mod hotkey_overlay;
mod hotkey_runtime;
mod input;
mod layout;
mod loop_crossfade_prompt;
mod map_clusters;
mod map_empty;
mod map_interactions;
mod map_math;
mod map_view;
mod platform;
mod progress_overlay;
mod sample_browser_filter;
mod sample_browser_interaction;
mod sample_browser_rename;
mod sample_browser_render;
mod sample_menus;
mod sources_panel;
mod status_badges;
/// Shared color/typography style helpers.
pub mod style;
mod update;
mod waveform_view;

/// Default viewport sizes used when creating or restoring the window.
pub const DEFAULT_VIEWPORT_SIZE: [f32; 2] = [960.0, 560.0];
/// Minimum viewport size for the app window.
pub const MIN_VIEWPORT_SIZE: [f32; 2] = [640.0, 400.0];

use crate::{audio::AudioPlayer, egui_app::controller::EguiController, waveform::WaveformRenderer};
use eframe::egui::{self, TextureHandle};

/// Renders the egui UI using the shared controller state.
pub struct EguiApp {
    pub(crate) controller: EguiController,
    visuals_set: bool,
    waveform_tex: Option<TextureHandle>,
    #[allow(dead_code)]
    last_viewport_log: Option<(u32, u32, u32, u32, &'static str)>,
    sources_panel_rect: Option<egui::Rect>,
    sources_panel_drop_hovered: bool,
    sources_panel_drop_armed: bool,
    selection_edge_offset: Option<f32>,
    selection_edge_alt_scale: bool,
    selection_slide: Option<SelectionSlide>,
    edit_selection_slide: Option<SelectionSlide>,
    edit_selection_gain_drag: Option<EditSelectionGainDrag>,
    slice_drag: Option<SliceDragState>,
    slice_paint: Option<SlicePaintState>,
    pending_chord: Option<hotkey_runtime::PendingChord>,
    key_feedback: hotkey_runtime::KeyFeedback,
    requested_initial_focus: bool,
    external_drop_handled: bool,
    external_drop_hover_pos: Option<egui::Pos2>,
}

#[derive(Clone, Copy, Debug)]
struct SelectionSlide {
    anchor: f32,
    range: crate::selection::SelectionRange,
}

#[derive(Clone, Copy, Debug)]
struct EditSelectionGainDrag {
    anchor_y: f32,
    gain: f32,
}

#[derive(Clone, Copy, Debug)]
struct SliceDragState {
    index: usize,
    kind: SliceDragKind,
}

#[derive(Clone, Copy, Debug)]
struct SlicePaintState {
    anchor: f32,
    range: crate::selection::SelectionRange,
}

#[derive(Clone, Copy, Debug)]
enum SliceDragKind {
    Move {
        anchor: f32,
        range: crate::selection::SelectionRange,
    },
    Edge {
        edge: crate::selection::SelectionEdge,
        offset: f32,
    },
}

impl EguiApp {
    /// Create a new egui app, loading persisted configuration.
    pub fn new(
        renderer: WaveformRenderer,
        player: Option<std::rc::Rc<std::cell::RefCell<AudioPlayer>>>,
    ) -> Result<Self, String> {
        let mut controller = EguiController::new(renderer, player);
        controller
            .load_configuration()
            .map_err(|err| format!("Failed to load config: {err}"))?;
        controller.select_first_source();
        Ok(Self {
            controller,
            visuals_set: false,
            waveform_tex: None,
            last_viewport_log: None,
            sources_panel_rect: None,
            sources_panel_drop_hovered: false,
            sources_panel_drop_armed: false,
            selection_edge_offset: None,
            selection_edge_alt_scale: false,
            selection_slide: None,
            edit_selection_slide: None,
            edit_selection_gain_drag: None,
            slice_drag: None,
            slice_paint: None,
            pending_chord: None,
            key_feedback: hotkey_runtime::KeyFeedback::default(),
            requested_initial_focus: false,
            external_drop_handled: false,
            external_drop_hover_pos: None,
        })
    }
}
