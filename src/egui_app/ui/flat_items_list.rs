use super::helpers::{
    RowMetrics, list_row_height, number_column_width, scroll_offset_to_reveal_row,
};
use super::style;
use eframe::egui::{self, Ui};

pub(super) struct FlatItemsListConfig<'a> {
    pub scroll_id_salt: &'a str,
    pub min_height: f32,
    pub total_rows: usize,
    pub focused_section: bool,
    pub autoscroll_to: Option<usize>,
    pub autoscroll_padding_rows: f32,
}

pub(super) struct FlatItemsListMetrics {
    pub row_height: f32,
    pub number_width: f32,
    pub number_gap: f32,
    pub padding: f32,
}

pub(super) struct FlatItemsListResponse {
    pub frame_rect: egui::Rect,
}

pub(super) fn render_flat_items_list(
    ui: &mut Ui,
    config: FlatItemsListConfig<'_>,
    mut row_renderer: impl FnMut(&mut Ui, usize, &FlatItemsListMetrics),
) -> FlatItemsListResponse {
    let row_height = list_row_height(ui);
    let row_metrics = RowMetrics {
        height: row_height,
        spacing: ui.spacing().item_spacing.y,
    };
    let number_width = number_column_width(config.total_rows, ui);
    let number_gap = ui.spacing().button_padding.x * 0.5;
    let padding = ui.spacing().button_padding.x * 2.0;
    let metrics = FlatItemsListMetrics {
        row_height,
        number_width,
        number_gap,
        padding,
    };

    let frame = style::section_frame();
    let scroll_response = frame.show(ui, |ui| {
        ui.set_min_height(config.min_height);
        let scroll = egui::ScrollArea::vertical()
            .id_salt(config.scroll_id_salt)
            .max_height(config.min_height);
        if config.total_rows == 0 {
            scroll.show(ui, |ui| {
                let height = ui.available_height().max(config.min_height);
                ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), height),
                    egui::Sense::hover(),
                );
            })
        } else {
            scroll.show_rows(ui, row_height, config.total_rows, |ui, row_range| {
                for row in row_range {
                    row_renderer(ui, row, &metrics);
                }
            })
        }
    });

    let viewport_height = scroll_response.inner.inner_rect.height();
    let content_height = scroll_response.inner.content_size.y;
    let max_offset = (content_height - viewport_height).max(0.0);
    let mut desired_offset = scroll_response.inner.state.offset.y;
    if let Some(row) = config.autoscroll_to {
        desired_offset = scroll_offset_to_reveal_row(
            desired_offset,
            row,
            row_metrics,
            viewport_height,
            config.autoscroll_padding_rows,
        );
    }
    let mut state = scroll_response.inner.state;
    state.offset.y = desired_offset.clamp(0.0, max_offset);
    state.store(ui.ctx(), scroll_response.inner.id);

    style::paint_section_border(ui, scroll_response.response.rect, config.focused_section);

    FlatItemsListResponse {
        frame_rect: scroll_response.response.rect,
    }
}
