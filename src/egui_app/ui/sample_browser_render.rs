use super::drag_targets;
use super::flat_items_list::{FlatItemsListConfig, FlatItemsListResponse, render_flat_items_list};
use super::helpers::{self, external_dropped_paths, external_hover_has_audio};
use super::sample_browser_row::{SampleBrowserRowContext, render_sample_browser_row};
use super::style;
use super::EguiApp;
use crate::egui_app::state::{FocusContext, SampleBrowserTab, TriageFlagColumn};
use eframe::egui::{self, StrokeKind, Ui};
use std::path::PathBuf;
use std::time::Duration;

struct SampleBrowserRenderState {
    palette: style::Palette,
    selected_row: Option<usize>,
    loaded_row: Option<usize>,
    drop_target: TriageFlagColumn,
    now_epoch: i64,
    flash_alpha: Option<u8>,
    flash_paths: Vec<PathBuf>,
}

struct SampleBrowserListState {
    list_height: f32,
    drag_active: bool,
    pointer_pos: Option<egui::Pos2>,
    external_pointer_pos: Option<egui::Pos2>,
    external_drop_ready: bool,
    autoscroll_to: Option<usize>,
    total_rows: usize,
    focused_section: bool,
}

impl EguiApp {
    pub(super) fn render_sample_browser(&mut self, ui: &mut Ui) {
        let state = prepare_sample_browser_state(self);
        if render_sample_browser_tabs(self, ui) {
            self.render_map_panel(ui);
            return;
        }
        self.render_sample_browser_filter(ui);
        ui.add_space(6.0);

        let list_state = prepare_sample_browser_list_state(self, ui, state.selected_row);
        let list_response = render_sample_browser_list(self, ui, &state, &list_state);
        if list_state.autoscroll_to.is_some() {
            self.controller.ui.browser.autoscroll = false;
        }

        render_sample_browser_hover_hint(self, ui, &list_response);
        render_sample_browser_drop_targets(self, ui, &list_state, &list_response, state.drop_target);
        render_sample_browser_external_drop(self, ui, &list_state, &list_response);
    }
}

fn prepare_sample_browser_state(app: &mut EguiApp) -> SampleBrowserRenderState {
    let palette = style::palette();
    app.controller.prepare_feature_cache_for_browser();
    let selected_row = app.controller.ui.browser.selected_visible;
    let loaded_row = app.controller.ui.browser.loaded_visible;
    let drop_target = app.controller.triage_flag_drop_target();
    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let flash_alpha = helpers::flash_alpha(
        &mut app.controller.ui.browser.copy_flash_at,
        Duration::from_millis(260),
        60,
    );
    if flash_alpha.is_none() {
        app.controller.ui.browser.copy_flash_paths.clear();
    }
    let flash_paths = app.controller.ui.browser.copy_flash_paths.clone();

    SampleBrowserRenderState {
        palette,
        selected_row,
        loaded_row,
        drop_target,
        now_epoch,
        flash_alpha,
        flash_paths,
    }
}

fn render_sample_browser_tabs(app: &mut EguiApp, ui: &mut Ui) -> bool {
    let mut tab = app.controller.ui.browser.active_tab;
    ui.horizontal(|ui| {
        if ui
            .selectable_label(tab == SampleBrowserTab::List, "Samples")
            .clicked()
        {
            tab = SampleBrowserTab::List;
        }
        if ui
            .selectable_label(tab == SampleBrowserTab::Map, "Similarity map")
            .clicked()
        {
            tab = SampleBrowserTab::Map;
        }
    });
    if tab != app.controller.ui.browser.active_tab {
        app.controller.ui.browser.active_tab = tab;
    }
    ui.add_space(4.0);
    app.controller.ui.browser.active_tab == SampleBrowserTab::Map
}

fn prepare_sample_browser_list_state(
    app: &mut EguiApp,
    ui: &mut Ui,
    selected_row: Option<usize>,
) -> SampleBrowserListState {
    let list_height = ui.available_height().max(0.0);
    let drag_active = app.controller.ui.drag.payload.is_some();
    let pointer_pos = drag_targets::pointer_pos_for_drag(ui, app.controller.ui.drag.position);
    let external_pointer_pos = pointer_pos.or(app.external_drop_hover_pos);
    let external_drop_ready = external_hover_has_audio(ui.ctx());
    let autoscroll_enabled = app.controller.ui.browser.autoscroll;
    let total_rows = app.controller.visible_browser_len();
    let focused_section = matches!(app.controller.ui.focus.context, FocusContext::SampleBrowser);
    let autoscroll_to = selected_row.filter(|_| autoscroll_enabled);

    SampleBrowserListState {
        list_height,
        drag_active,
        pointer_pos,
        external_pointer_pos,
        external_drop_ready,
        autoscroll_to,
        total_rows,
        focused_section,
    }
}

fn render_sample_browser_list(
    app: &mut EguiApp,
    ui: &mut Ui,
    state: &SampleBrowserRenderState,
    list_state: &SampleBrowserListState,
) -> FlatItemsListResponse {
    let row_context = SampleBrowserRowContext {
        palette: &state.palette,
        selected_row: state.selected_row,
        loaded_row: state.loaded_row,
        drag_active: list_state.drag_active,
        pointer_pos: list_state.pointer_pos,
        drop_target: state.drop_target,
        flash_alpha: state.flash_alpha,
        flash_paths: &state.flash_paths,
        now_epoch: state.now_epoch,
    };

    render_flat_items_list(
        ui,
        FlatItemsListConfig {
            scroll_id_salt: "sample_browser_scroll",
            min_height: list_state.list_height,
            total_rows: list_state.total_rows,
            focused_section: list_state.focused_section,
            autoscroll_to: list_state.autoscroll_to,
            autoscroll_padding_rows: 1.0,
        },
        |ui, row, metrics| {
            render_sample_browser_row(app, ui, row, metrics, &row_context);
        },
    )
}

fn render_sample_browser_hover_hint(
    app: &mut EguiApp,
    ui: &mut Ui,
    list_response: &FlatItemsListResponse,
) {
    let hover_pos = ui.input(|i| i.pointer.hover_pos()).unwrap_or(egui::Pos2::ZERO);
    if list_response.frame_rect.contains(hover_pos) {
        helpers::show_hover_hint(
            ui,
            app.controller.ui.controls.tooltip_mode,
            "Click: Select/Play | Shift+Click: Range Select | Ctrl+Click: Toggle Select | Drag: Export",
        );
    }
}

fn render_sample_browser_drop_targets(
    app: &mut EguiApp,
    ui: &mut Ui,
    list_state: &SampleBrowserListState,
    list_response: &FlatItemsListResponse,
    drop_target: TriageFlagColumn,
) {
    let drag_source = app
        .controller
        .ui
        .drag
        .origin_source
        .unwrap_or(crate::egui_app::state::DragSource::Browser);
    drag_targets::handle_drop_zone(
        ui,
        &mut app.controller,
        list_state.drag_active,
        list_state.pointer_pos,
        list_response.frame_rect,
        drag_source,
        crate::egui_app::state::DragTarget::BrowserTriage(drop_target),
        style::drag_target_stroke(),
        StrokeKind::Inside,
    );
}

fn render_sample_browser_external_drop(
    app: &mut EguiApp,
    ui: &mut Ui,
    list_state: &SampleBrowserListState,
    list_response: &FlatItemsListResponse,
) {
    if !app.external_drop_handled {
        let dropped_paths = external_dropped_paths(ui.ctx());
        if !dropped_paths.is_empty()
            && list_state
                .external_pointer_pos
                .is_some_and(|pos| list_response.frame_rect.contains(pos))
        {
            app.external_drop_handled = true;
            app.controller
                .import_external_files_to_source_folder(PathBuf::new(), dropped_paths);
        }
    }
    if list_state.external_drop_ready
        && list_state
            .external_pointer_pos
            .is_some_and(|pos| list_response.frame_rect.contains(pos))
    {
        let highlight = style::with_alpha(style::semantic_palette().drag_highlight, 32);
        ui.painter()
            .rect_filled(list_response.frame_rect, 6.0, highlight);
        ui.painter().rect_stroke(
            list_response.frame_rect,
            6.0,
            style::drag_target_stroke(),
            StrokeKind::Inside,
        );
    }
}
