use super::EguiApp;
use super::helpers::{
    RowBackground, RowMarker, clamp_label_for_width, external_dropped_paths,
    external_hover_has_audio, list_row_height, render_list_row,
};
use crate::egui_app::ui::helpers;
use super::style;
use crate::egui_app::state::{DragPayload, DragSource, DragTarget};
use crate::egui_app::ui::drag_targets::{handle_drop_zone, handle_sample_row_drag};
use crate::sample_sources::config::DropTargetColor;
use eframe::egui::{self, Align2, RichText, StrokeKind, TextStyle, Ui};

impl EguiApp {
    pub(super) fn render_drop_targets(
        &mut self,
        ui: &mut Ui,
        height: f32,
        pointer_pos: Option<egui::Pos2>,
    ) {
        let palette = style::palette();
        let tooltip_mode = self.controller.ui.controls.tooltip_mode;
        let header_response = ui.horizontal(|ui| {
            ui.label(RichText::new("Drop targets").color(palette.text_primary));
            if helpers::tooltip(
                ui.button(RichText::new("+").color(palette.text_primary)),
                "Add Drop Target",
                "Create a shortcut to a subfolder within your sources. Dragging samples onto these targets will move or copy the files to those folders.",
                tooltip_mode,
            ).clicked() {
                self.controller.add_drop_target_via_dialog();
            }
        });
        self.controller.ui.sources.drop_targets.header_height =
            header_response.response.rect.height();
        let header_gap = ui.spacing().item_spacing.y;
        ui.add_space(header_gap);
        let content_height =
            (height - header_response.response.rect.height() - header_gap).max(0.0);

        let drag_payload = self.controller.ui.drag.payload.clone();
        let drag_active = drag_payload.is_some();
        let sample_drag_active = matches!(
            drag_payload,
            Some(DragPayload::Sample { .. } | DragPayload::Samples { .. })
        );
        let folder_drag_active = matches!(
            drag_payload,
            Some(DragPayload::Folder { .. })
        );
        let drop_target_drag_active = matches!(
            drag_payload,
            Some(DragPayload::DropTargetReorder { .. })
        );
        let external_pointer_pos = pointer_pos.or(self.external_drop_hover_pos);
        let external_drop_ready = external_hover_has_audio(ui.ctx());
        let external_drop_paths = external_dropped_paths(ui.ctx());
        let mut external_drop_paths = if external_drop_paths.is_empty() {
            None
        } else {
            Some(external_drop_paths)
        };
        let rows = std::mem::take(&mut self.controller.ui.sources.drop_targets.rows);
        let selected = self.controller.ui.sources.drop_targets.selected;
        let frame = style::section_frame();
        let frame_response = frame.show(ui, |ui| {
            ui.set_min_height(content_height);
            ui.set_max_height(content_height);
            let row_height = list_row_height(ui);
            egui::ScrollArea::vertical()
                .id_salt("drop_targets_scroll")
                .max_height(content_height)
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
                            is_selected.then_some(style::row_primary_selection_fill()),
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
                                sense: egui::Sense::click_and_drag(),
                                number: None,
                                marker,
                                rating: None,
                                looped: false,
                                long_sample: false,
                                bpm_label: None,
                            },
                        );
                        let response = helpers::tooltip(
                            response,
                            &row.tooltip_path,
                            "Dragging samples onto this target will move or copy them to this specific folder location. Right-click to change the marker color or remove the target.",
                            tooltip_mode,
                        );
                        if is_selected {
                            ui.painter().rect_stroke(
                                response.rect,
                                0.0,
                                style::focused_row_stroke(),
                                StrokeKind::Inside,
                            );
                        }
                        handle_sample_row_drag(
                            ui,
                            &response,
                            drag_active,
                            &mut self.controller,
                            DragSource::DropTargets,
                            DragTarget::DropTarget {
                                path: row.path.clone(),
                            },
                            move |pos, controller| {
                                controller.start_drop_target_drag(
                                    row.path.clone(),
                                    row.drag_label.clone(),
                                    pos,
                                );
                            },
                            move |pos, _controller| {
                                Some(crate::egui_app::state::PendingOsDragStart {
                                    payload: DragPayload::DropTargetReorder {
                                        path: row.path.clone(),
                                    },
                                    label: row.drag_label.clone(),
                                    origin: pos,
                                })
                            },
                            move |pending| match &pending.payload {
                                DragPayload::DropTargetReorder { path } => *path == row.path,
                                DragPayload::Sample { .. } => false,
                                DragPayload::Samples { .. } => false,
                                DragPayload::Folder { .. } => false,
                                DragPayload::Selection { .. } => false,
                            },
                        );
                        if response.clicked() {
                            self.controller.select_drop_target_by_index(index);
                        }
                        let drop_target = DragTarget::DropTarget {
                            path: row.path.clone(),
                        };
                        if sample_drag_active {
                            handle_drop_zone(
                                ui,
                                &mut self.controller,
                                sample_drag_active,
                                pointer_pos,
                                response.rect,
                                DragSource::DropTargets,
                                drop_target.clone(),
                                style::drag_target_stroke(),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if drop_target_drag_active {
                            handle_drop_zone(
                                ui,
                                &mut self.controller,
                                drop_target_drag_active,
                                pointer_pos,
                                response.rect,
                                DragSource::DropTargets,
                                drop_target,
                                style::drag_target_stroke(),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if external_drop_ready
                            && external_pointer_pos
                                .is_some_and(|pos| response.rect.contains(pos))
                        {
                            ui.painter().rect_stroke(
                                response.rect.expand(2.0),
                                0.0,
                                style::drag_target_stroke(),
                                StrokeKind::Inside,
                            );
                        }
                        if !self.external_drop_handled
                            && let Some(pointer) = external_pointer_pos
                            && response.rect.contains(pointer)
                            && let Some(paths) = external_drop_paths.take()
                        {
                            let Some(location) = self
                                .controller
                                .resolve_drop_target_location(&row.path)
                            else {
                                self.controller.set_status(
                                    "Drop target is no longer inside a configured source",
                                    style::StatusTone::Warning,
                                );
                                self.external_drop_handled = true;
                                continue;
                            };
                            let target_dir = location.source.root.join(&location.relative_folder);
                            if !target_dir.is_dir() {
                                self.controller.set_status(
                                    format!("Drop target missing: {}", target_dir.display()),
                                    style::StatusTone::Warning,
                                );
                                self.external_drop_handled = true;
                                continue;
                            }
                            self.controller.select_drop_target_by_index(index);
                            self.controller.import_external_files_to_source_folder(
                                location.relative_folder,
                                paths,
                            );
                            self.external_drop_handled = true;
                        }
                        self.drop_target_row_menu(&response, index, row);
                    }
                });
        });
        handle_drop_zone(
            ui,
            &mut self.controller,
            folder_drag_active,
            pointer_pos,
            frame_response.response.rect,
            DragSource::DropTargets,
            DragTarget::DropTargetsPanel,
            style::drag_target_stroke(),
            egui::StrokeKind::Inside,
        );
        handle_drop_zone(
            ui,
            &mut self.controller,
            drop_target_drag_active,
            pointer_pos,
            frame_response.response.rect,
            DragSource::DropTargets,
            DragTarget::DropTargetsPanel,
            style::drag_target_stroke(),
            egui::StrokeKind::Inside,
        );
        if external_drop_ready
            && external_pointer_pos
                .is_some_and(|pos| frame_response.response.rect.contains(pos))
        {
            ui.painter().rect_stroke(
                frame_response.response.rect,
                6.0,
                style::drag_target_stroke(),
                StrokeKind::Inside,
            );
        }
        style::paint_section_border(ui, frame_response.response.rect, false);
        self.controller.ui.sources.drop_targets.rows = rows;
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
