//! egui renderer for the application UI.

mod chrome;
mod collections_panel;
mod drag_overlay;
mod flat_items_list;
mod helpers;
mod hotkey_overlay;
mod hotkey_runtime;
mod input;
mod layout;
mod platform;
mod progress_overlay;
mod sample_browser_panel;
mod sample_menus;
mod sources_panel;
pub mod style;
mod update;
mod waveform_view;

/// Default viewport sizes used when creating or restoring the window.
pub const DEFAULT_VIEWPORT_SIZE: [f32; 2] = [960.0, 560.0];
pub const MIN_VIEWPORT_SIZE: [f32; 2] = [640.0, 400.0];

use crate::{audio::AudioPlayer, egui_app::controller::EguiController, waveform::WaveformRenderer};
use eframe::egui::{self, TextureHandle};

/// Renders the egui UI using the shared controller state.
pub struct EguiApp {
    controller: EguiController,
    visuals_set: bool,
    waveform_tex: Option<TextureHandle>,
    last_viewport_log: Option<(u32, u32, u32, u32, &'static str)>,
    sources_panel_rect: Option<egui::Rect>,
    sources_panel_drop_hovered: bool,
    sources_panel_drop_armed: bool,
    selection_edge_offset: Option<f32>,
    pending_chord: Option<hotkey_runtime::PendingChord>,
    key_feedback: hotkey_runtime::KeyFeedback,
    requested_initial_focus: bool,
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
            pending_chord: None,
            key_feedback: hotkey_runtime::KeyFeedback::default(),
            requested_initial_focus: false,
        })
    }
}
