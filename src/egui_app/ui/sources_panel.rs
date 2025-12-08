use super::helpers::{clamp_label_for_width, list_row_height, render_list_row};
use super::style;
use super::*;
use eframe::egui::{self, Align2, RichText, StrokeKind, TextStyle, Ui};

impl EguiApp {
    pub(super) fn render_sources_panel(&mut self, ui: &mut Ui) {
        let panel_rect = ui.max_rect();
        self.sources_panel_rect = Some(panel_rect);
        let drop_hovered = self.update_sources_panel_drop_state(ui.ctx(), panel_rect);
        if drop_hovered {
            let highlight = style::with_alpha(style::semantic_palette().drag_highlight, 32);
            ui.painter().rect_filled(panel_rect, 0.0, highlight);
        }
        let palette = style::palette();
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Sources").color(palette.text_primary));
                if ui
                    .button(RichText::new("+").color(palette.text_primary))
                    .clicked()
                {
                    self.controller.add_source_via_dialog();
                }
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .id_salt("sources_scroll")
                .show(ui, |ui| {
                    let rows = self.controller.ui.sources.rows.clone();
                    let selected = self.controller.ui.sources.selected;
                    let row_height = list_row_height(ui);
                    for (index, row) in rows.iter().enumerate() {
                        let is_selected = Some(index) == selected;
                        ui.push_id(&row.id, |ui| {
                            let row_width = ui.available_width();
                            let padding = ui.spacing().button_padding.x * 2.0;
                            let base_label = clamp_label_for_width(&row.name, row_width - padding);
                            let label = if row.missing {
                                format!("! {base_label}")
                            } else {
                                base_label
                            };
                            let text_color = if row.missing {
                                style::missing_text()
                            } else {
                                style::high_contrast_text()
                            };
                            let bg = is_selected.then_some(style::row_selected_fill());
                            let response = render_list_row(
                                ui,
                                &label,
                                row_width,
                                row_height,
                                bg,
                                text_color,
                                egui::Sense::click(),
                                None,
                                None,
                            )
                            .on_hover_text(&row.path);
                            if response.clicked() {
                                self.controller.select_source_by_index(index);
                            }
                        });
                    }
                });
        });
        if drop_hovered {
            let painter = ui.painter();
            painter.rect_stroke(
                panel_rect.shrink(0.5),
                0.0,
                style::drag_target_stroke(),
                StrokeKind::Inside,
            );
            let font = TextStyle::Button.resolve(ui.style());
            painter.text(
                panel_rect.center(),
                Align2::CENTER_CENTER,
                "Drop folders to add",
                font,
                style::high_contrast_text(),
            );
        }
    }

    fn update_sources_panel_drop_state(&mut self, ctx: &egui::Context, rect: egui::Rect) -> bool {
        self.sources_panel_drop_hovered = ctx.input(|i| {
            let pointer_pos = i.pointer.hover_pos().or_else(|| i.pointer.interact_pos());
            let pointer_over = pointer_pos.map_or(true, |pos| rect.contains(pos));
            let hovered_has_path = i.raw.hovered_files.iter().any(|file| file.path.is_some());
            hovered_has_path && pointer_over
        });
        if self.sources_panel_drop_hovered {
            self.sources_panel_drop_armed = true;
        } else if ctx.input(|i| {
            i.pointer
                .hover_pos()
                .or_else(|| i.pointer.interact_pos())
                .is_some_and(|pos| !rect.contains(pos))
        }) {
            self.sources_panel_drop_armed = false;
        }
        self.sources_panel_drop_hovered
    }
}
