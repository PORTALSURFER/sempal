use super::EguiApp;
use super::helpers::{
    RowBackground, RowMarker, clamp_label_for_width, list_row_height, render_list_row,
};
use super::style;
use crate::egui_app::state::{DragPayload, DragSource, DragTarget};
use crate::egui_app::ui::drag_targets::{handle_drop_zone, pointer_pos_for_drag};
use crate::sample_sources::config::DropTargetColor;
use eframe::egui::{self, Align2, RichText, StrokeKind, TextStyle, Ui};

impl EguiApp {
    pub(super) fn render_drop_targets(&mut self, ui: &mut Ui, height: f32) {
        let palette = style::palette();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Drop targets").color(palette.text_primary));
            if ui
                .button(RichText::new("+").color(palette.text_primary))
                .clicked()
            {
                self.controller.add_drop_target_via_dialog();
            }
        });
        ui.add_space(6.0);

        let drag_payload = self.controller.ui.drag.payload.clone();
        let drag_active = matches!(
            drag_payload,
            Some(DragPayload::Sample { .. } | DragPayload::Samples { .. })
        );
        let pointer_pos = pointer_pos_for_drag(ui, self.controller.ui.drag.position);
        let rows = self.controller.ui.sources.drop_targets.rows.clone();
        let selected = self.controller.ui.sources.drop_targets.selected;
        let frame = style::section_frame();
        let frame_response = frame.show(ui, |ui| {
            ui.set_min_height(height);
            ui.set_max_height(height);
            let row_height = list_row_height(ui);
            egui::ScrollArea::vertical()
                .id_salt("drop_targets_scroll")
                .max_height(height)
                .show(ui, |ui| {
                    if rows.is_empty() {
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), row_height),
                            egui::Sense::hover(),
                        );
                        ui.painter().text(
                            rect.left_center(),
                            Align2::LEFT_CENTER,
                            "Add a folder to create a drop target",
                            TextStyle::Body.resolve(ui.style()),
                            palette.text_muted,
                        );
                        return;
                    }
                    for (index, row) in rows.iter().enumerate() {
                        let is_selected = Some(index) == selected;
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
                        let bg = RowBackground::from_option(
                            is_selected.then_some(style::row_selected_fill()),
                        );
                        let marker = row
                            .color
                            .map(|color| RowMarker {
                                width: 6.0,
                                color: drop_target_color_fill(color),
                            });
                        let response = render_list_row(
                            ui,
                            super::helpers::ListRow {
                                label: &label,
                                row_width,
                                row_height,
                                background: bg,
                                skip_hover: false,
                                text_color,
                                sense: egui::Sense::click(),
                                number: None,
                                marker,
                                rating: None,
                            },
                        )
                        .on_hover_text(row.path.display().to_string());
                        if response.clicked() {
                            self.controller.select_drop_target_by_index(index);
                        }
                        handle_drop_zone(
                            ui,
                            &mut self.controller,
                            drag_active,
                            pointer_pos,
                            response.rect,
                            DragSource::DropTargets,
                            DragTarget::DropTarget {
                                path: row.path.clone(),
                            },
                            style::drag_target_stroke(),
                            egui::StrokeKind::Inside,
                        );
                        self.drop_target_row_menu(&response, index, row);
                    }
                });
        });
        style::paint_section_border(ui, frame_response.response.rect, false);
    }

    fn drop_target_row_menu(
        &mut self,
        response: &egui::Response,
        index: usize,
        row: &crate::egui_app::state::DropTargetRowView,
    ) {
        response.context_menu(|ui| {
            let palette = style::palette();
            ui.label(RichText::new(row.name.clone()).color(palette.text_primary));
            let mut close_menu = false;
            ui.separator();
            ui.label(RichText::new("Color").color(palette.text_primary));
            for swatch in drop_target_swatches() {
                let is_selected = row.color == Some(swatch.color);
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(12.0, 12.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(rect, 2.0, swatch.fill);
                    if is_selected {
                        ui.painter().rect_stroke(
                            rect,
                            2.0,
                            style::focused_row_stroke(),
                            StrokeKind::Inside,
                        );
                    }
                    let response = ui.selectable_label(is_selected, swatch.label);
                    if response.clicked() {
                        self.controller
                            .set_drop_target_color(index, Some(swatch.color));
                        close_menu = true;
                    }
                });
            }
            if ui.button("Clear color").clicked() {
                self.controller.set_drop_target_color(index, None);
                close_menu = true;
            }
            if ui.button("Remove drop target").clicked() {
                self.controller.remove_drop_target(index);
                close_menu = true;
            }
            if close_menu {
                ui.close();
            }
        });
    }
}

struct DropTargetSwatch {
    color: DropTargetColor,
    label: &'static str,
    fill: egui::Color32,
}

fn drop_target_swatches() -> [DropTargetSwatch; 8] {
    let palette = style::palette();
    let semantic = style::semantic_palette();
    [
        DropTargetSwatch {
            color: DropTargetColor::Mint,
            label: "Mint",
            fill: palette.accent_mint,
        },
        DropTargetSwatch {
            color: DropTargetColor::Ice,
            label: "Ice",
            fill: palette.accent_ice,
        },
        DropTargetSwatch {
            color: DropTargetColor::Copper,
            label: "Copper",
            fill: palette.accent_copper,
        },
        DropTargetSwatch {
            color: DropTargetColor::Fog,
            label: "Fog",
            fill: semantic.badge_info,
        },
        DropTargetSwatch {
            color: DropTargetColor::Amber,
            label: "Amber",
            fill: semantic.badge_warning,
        },
        DropTargetSwatch {
            color: DropTargetColor::Rose,
            label: "Rose",
            fill: semantic.badge_error,
        },
        DropTargetSwatch {
            color: DropTargetColor::Spruce,
            label: "Spruce",
            fill: semantic.triage_keep,
        },
        DropTargetSwatch {
            color: DropTargetColor::Clay,
            label: "Clay",
            fill: semantic.triage_trash_subtle,
        },
    ]
}

fn drop_target_color_fill(color: DropTargetColor) -> egui::Color32 {
    drop_target_swatches()
        .into_iter()
        .find(|swatch| swatch.color == color)
        .map(|swatch| swatch.fill)
        .unwrap_or_else(|| style::palette().accent_ice)
}
