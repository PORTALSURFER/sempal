use super::selection_drag;
use super::selection_geometry::{
    fade_handle_rect, fade_lower_handle_rect, fade_mute_handle_rect, paint_fade_handle,
    paint_fade_mute_handle, selection_rect_for_view,
};
use super::style;
use super::*;
use crate::egui_app::state::WaveformView;
use eframe::egui::{self, CursorIcon, StrokeKind};

pub(super) fn render_edit_selection_overlay(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    view: WaveformView,
    view_width: f64,
) {
    let Some(selection) = app.controller.ui.waveform.edit_selection else {
        return;
    };

    let selection_rect = selection_rect_for_view(selection, rect, view, view_width);
    let highlight = egui::Color32::from_rgb(76, 122, 218);
    let fill = style::with_alpha(highlight, 50);
    let stroke = egui::Stroke::new(1.5, style::with_alpha(highlight, 180));
    let painter = ui.painter();
    
    // Render selection background
    painter.rect_filled(selection_rect, 0.0, fill);
    
    // Render fade overlays if fades are present
    if selection.fade_in().is_some() || selection.fade_out().is_some() {
        let fade_in_width = selection_rect.width() * selection.fade_in_length();
        let fade_out_width = selection_rect.width() * selection.fade_out_length();
        let fade_in_mute_width = selection_rect.width() * selection.fade_in_mute_length();
        let fade_out_mute_width = selection_rect.width() * selection.fade_out_mute_length();
        
        // Fade-in gradient (left side)
        if fade_in_width > 1.0 {
            let fade_in_rect = egui::Rect::from_min_size(
                selection_rect.min,
                egui::vec2(fade_in_width, selection_rect.height()),
            );
            let mut mesh = egui::epaint::Mesh::default();
            let base = mesh.vertices.len() as u32;
            let fade_color = style::with_alpha(highlight, 120);
            let transparent = style::with_alpha(highlight, 0);
            mesh.colored_vertex(fade_in_rect.left_top(), fade_color);
            mesh.colored_vertex(fade_in_rect.left_bottom(), fade_color);
            mesh.colored_vertex(fade_in_rect.right_top(), transparent);
            mesh.colored_vertex(fade_in_rect.right_bottom(), transparent);
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base + 2, base + 1, base + 3);
            painter.add(egui::Shape::mesh(mesh));

            if fade_in_mute_width > 1.0 {
                let mute_rect = egui::Rect::from_min_size(
                    egui::pos2(selection_rect.left() - fade_in_mute_width, selection_rect.top()),
                    egui::vec2(fade_in_mute_width, selection_rect.height()),
                );
                painter.rect_filled(mute_rect, 0.0, style::with_alpha(highlight, 150));
            }
            
            // Draw fade curve line
            if let Some(fade_params) = selection.fade_in() {
                let curve_width = fade_in_width.max(0.0);
                if curve_width > 1.0 {
                    let curve_rect = egui::Rect::from_min_size(
                        egui::pos2(selection_rect.left(), selection_rect.top()),
                        egui::vec2(curve_width, selection_rect.height()),
                    );
                    draw_fade_curve(
                        &painter,
                        curve_rect,
                        fade_params.curve,
                        true, // fade in
                        highlight,
                    );
                }
            }
        }
        
        // Fade-out gradient (right side)
        if fade_out_width > 1.0 {
            let fade_out_rect = egui::Rect::from_min_size(
                egui::pos2(selection_rect.right() - fade_out_width, selection_rect.top()),
                egui::vec2(fade_out_width, selection_rect.height()),
            );
            let mut mesh = egui::epaint::Mesh::default();
            let base = mesh.vertices.len() as u32;
            let fade_color = style::with_alpha(highlight, 120);
            let transparent = style::with_alpha(highlight, 0);
            mesh.colored_vertex(fade_out_rect.left_top(), transparent);
            mesh.colored_vertex(fade_out_rect.left_bottom(), transparent);
            mesh.colored_vertex(fade_out_rect.right_top(), fade_color);
            mesh.colored_vertex(fade_out_rect.right_bottom(), fade_color);
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base + 2, base + 1, base + 3);
            painter.add(egui::Shape::mesh(mesh));

            if fade_out_mute_width > 1.0 {
                let mute_rect = egui::Rect::from_min_size(
                    egui::pos2(selection_rect.right(), selection_rect.top()),
                    egui::vec2(fade_out_mute_width, selection_rect.height()),
                );
                painter.rect_filled(mute_rect, 0.0, style::with_alpha(highlight, 150));
            }
            
            // Draw fade curve line
            if let Some(fade_params) = selection.fade_out() {
                let curve_width = fade_out_width.max(0.0);
                if curve_width > 1.0 {
                    let curve_rect = egui::Rect::from_min_size(
                        egui::pos2(selection_rect.right() - fade_out_width, selection_rect.top()),
                        egui::vec2(curve_width, selection_rect.height()),
                    );
                    draw_fade_curve(
                        &painter,
                        curve_rect,
                        fade_params.curve,
                        false, // fade out
                        highlight,
                    );
                }
            }
        }
    }
    
    painter.rect_stroke(selection_rect, 0.0, stroke, StrokeKind::Inside);

    let gain_handle_rect = egui::Rect::from_center_size(
        egui::pos2(selection_rect.center().x, selection_rect.top() + 4.0),
        egui::vec2(18.0, 6.0),
    );
    
    // Create interactive regions for the entire fade areas (for Alt+drag curve adjustment)
    let fade_in_region_rect = if selection.fade_in().is_some() {
        let fade_width = selection_rect.width() * selection.fade_in_length();
        if fade_width > 1.0 {
            egui::Rect::from_min_size(
                selection_rect.min,
                egui::vec2(fade_width, selection_rect.height()),
            )
        } else {
            egui::Rect::NOTHING
        }
    } else {
        egui::Rect::NOTHING
    };
    
    let fade_out_region_rect = if selection.fade_out().is_some() {
        let fade_width = selection_rect.width() * selection.fade_out_length();
        if fade_width > 1.0 {
            egui::Rect::from_min_size(
                egui::pos2(selection_rect.right() - fade_width, selection_rect.top()),
                egui::vec2(fade_width, selection_rect.height()),
            )
        } else {
            egui::Rect::NOTHING
        }
    } else {
        egui::Rect::NOTHING
    };
    
    // Render and handle fade handles - position them at the curve endpoints
    let fade_in_handle_rect = if let Some(fade_params) = selection.fade_in() {
        let fade_width = selection_rect.width() * selection.fade_in_length();
        if fade_width > 1.0 {
            // Calculate Y position at the end of the fade curve
            let curve_value = apply_s_curve(1.0, fade_params.curve);
            let y_offset = selection_rect.height() * (1.0 - curve_value);
            let y = selection_rect.top() + y_offset;
            let x = selection_rect.left() + fade_width;
            egui::Rect::from_center_size(
                egui::pos2(x, y),
                egui::vec2(8.0, 8.0),
            )
        } else {
            fade_handle_rect(selection_rect, true)
        }
    } else {
        fade_handle_rect(selection_rect, true)
    };
    
    let fade_out_handle_rect = if let Some(fade_params) = selection.fade_out() {
        let fade_width = selection_rect.width() * selection.fade_out_length();
        if fade_width > 1.0 {
            // Calculate Y position at the start of the fade curve
            let curve_value = apply_s_curve(0.0, fade_params.curve);
            let y_offset = selection_rect.height() * curve_value;
            let y = selection_rect.top() + y_offset;
            let x = selection_rect.right() - fade_width;
            egui::Rect::from_center_size(
                egui::pos2(x, y),
                egui::vec2(8.0, 8.0),
            )
        } else {
            fade_handle_rect(selection_rect, false)
        }
    } else {
        fade_handle_rect(selection_rect, false)
    };

    let fade_in_lower_handle_rect = selection
        .fade_in()
        .map(|_| fade_lower_handle_rect(selection_rect, true))
        .unwrap_or(egui::Rect::NOTHING);
    let fade_out_lower_handle_rect = selection
        .fade_out()
        .map(|_| fade_lower_handle_rect(selection_rect, false))
        .unwrap_or(egui::Rect::NOTHING);
    let fade_in_mute_handle_rect = selection
        .fade_in()
        .map(|_| fade_mute_handle_rect(selection_rect, true))
        .unwrap_or(egui::Rect::NOTHING);
    let fade_out_mute_handle_rect = selection
        .fade_out()
        .map(|_| fade_mute_handle_rect(selection_rect, false))
        .unwrap_or(egui::Rect::NOTHING);
    
    // Also create responses for the entire fade regions (for Alt+drag)
    let slide_bar_left = fade_in_handle_rect.center().x.min(fade_out_handle_rect.center().x);
    let slide_bar_right = fade_in_handle_rect.center().x.max(fade_out_handle_rect.center().x);
    let slide_bar_width = (slide_bar_right - slide_bar_left).max(16.0);
    let slide_bar_rect = egui::Rect::from_center_size(
        egui::pos2(
            (slide_bar_left + slide_bar_right) * 0.5,
            selection_rect.bottom() - 4.0,
        ),
        egui::vec2(slide_bar_width, 6.0),
    );
    let gain_handle_response = ui.interact(
        gain_handle_rect,
        ui.id().with("edit_selection_gain_handle"),
        egui::Sense::click_and_drag(),
    );
    let slide_bar_response = ui.interact(
        slide_bar_rect,
        ui.id().with("edit_selection_slide_bar"),
        egui::Sense::click_and_drag(),
    );
    let slide_blocks_fades = slide_bar_response.hovered() || slide_bar_response.dragged();
    let fade_in_region_response = ui.interact(
        if slide_blocks_fades {
            egui::Rect::NOTHING
        } else {
            fade_in_region_rect
        },
        ui.id().with("edit_fade_in_region"),
        egui::Sense::click_and_drag(),
    );
    let fade_out_region_response = ui.interact(
        if slide_blocks_fades {
            egui::Rect::NOTHING
        } else {
            fade_out_region_rect
        },
        ui.id().with("edit_fade_out_region"),
        egui::Sense::click_and_drag(),
    );
    
    let fade_in_response = ui.interact(
        if slide_blocks_fades {
            egui::Rect::NOTHING
        } else {
            fade_in_handle_rect
        },
        ui.id().with("edit_fade_in_handle"),
        egui::Sense::click_and_drag(),
    );
    let fade_out_response = ui.interact(
        if slide_blocks_fades {
            egui::Rect::NOTHING
        } else {
            fade_out_handle_rect
        },
        ui.id().with("edit_fade_out_handle"),
        egui::Sense::click_and_drag(),
    );
    let fade_in_lower_response = ui.interact(
        if slide_blocks_fades {
            egui::Rect::NOTHING
        } else {
            fade_in_lower_handle_rect
        },
        ui.id().with("edit_fade_in_lower_handle"),
        egui::Sense::click_and_drag(),
    );
    let fade_out_lower_response = ui.interact(
        if slide_blocks_fades {
            egui::Rect::NOTHING
        } else {
            fade_out_lower_handle_rect
        },
        ui.id().with("edit_fade_out_lower_handle"),
        egui::Sense::click_and_drag(),
    );
    let fade_in_mute_response = ui.interact(
        if slide_blocks_fades {
            egui::Rect::NOTHING
        } else {
            fade_in_mute_handle_rect
        },
        ui.id().with("edit_fade_in_mute_handle"),
        egui::Sense::click_and_drag(),
    );
    let fade_out_mute_response = ui.interact(
        if slide_blocks_fades {
            egui::Rect::NOTHING
        } else {
            fade_out_mute_handle_rect
        },
        ui.id().with("edit_fade_out_mute_handle"),
        egui::Sense::click_and_drag(),
    );
    
    let fade_in_active = fade_in_response.hovered() || fade_in_response.dragged();
    let fade_out_active = fade_out_response.hovered() || fade_out_response.dragged();
    let fade_in_lower_active =
        fade_in_lower_response.hovered() || fade_in_lower_response.dragged();
    let fade_out_lower_active =
        fade_out_lower_response.hovered() || fade_out_lower_response.dragged();
    let fade_in_mute_active = fade_in_mute_response.hovered() || fade_in_mute_response.dragged();
    let fade_out_mute_active =
        fade_out_mute_response.hovered() || fade_out_mute_response.dragged();
    let paint_handle_effects =
        |painter: &egui::Painter, handle_rect: egui::Rect, color: egui::Color32, active: bool| {
            paint_fade_handle(painter, handle_rect, true, color);
            if active {
                let glow_rect = handle_rect.expand(2.0);
                let stroke = egui::Stroke::new(1.5, style::with_alpha(color, 180));
                painter.rect_stroke(glow_rect, 3.0, stroke, StrokeKind::Inside);
            }
        };
    let paint_mute_effects = |painter: &egui::Painter,
                             handle_rect: egui::Rect,
                             color: egui::Color32,
                             active: bool,
                             is_fade_in: bool| {
        paint_fade_mute_handle(painter, handle_rect, is_fade_in, color);
        if active {
            let glow_rect = handle_rect.expand(2.0);
            let stroke = egui::Stroke::new(1.5, style::with_alpha(color, 180));
            painter.rect_stroke(glow_rect, 3.0, stroke, StrokeKind::Inside);
        }
    };

    let gain_handle_active =
        gain_handle_response.hovered() || gain_handle_response.dragged();
    let slide_bar_active = slide_bar_response.hovered() || slide_bar_response.dragged();
    let gain_fill = if gain_handle_active {
        style::with_alpha(highlight, 240)
    } else {
        style::with_alpha(highlight, 180)
    };
    let slide_fill = if slide_bar_active {
        style::with_alpha(highlight, 240)
    } else {
        style::with_alpha(highlight, 180)
    };
    ui.painter()
        .rect_filled(gain_handle_rect, 2.0, gain_fill);
    ui.painter()
        .rect_stroke(gain_handle_rect, 2.0, stroke, StrokeKind::Inside);
    ui.painter()
        .rect_filled(slide_bar_rect, 2.0, slide_fill);
    ui.painter()
        .rect_stroke(slide_bar_rect, 2.0, stroke, StrokeKind::Inside);
    if gain_handle_active {
        let glow_rect = gain_handle_rect.expand(2.0);
        let glow_stroke = egui::Stroke::new(1.5, style::with_alpha(highlight, 180));
        ui.painter()
            .rect_stroke(glow_rect, 3.0, glow_stroke, StrokeKind::Inside);
    }
    if slide_bar_active {
        let glow_rect = slide_bar_rect.expand(2.0);
        let glow_stroke = egui::Stroke::new(1.5, style::with_alpha(highlight, 180));
        ui.painter()
            .rect_stroke(glow_rect, 3.0, glow_stroke, StrokeKind::Inside);
    }
    
    // Always show fade handles when edit selection exists
    let fade_in_color = if fade_in_active {
        style::with_alpha(highlight, 255)
    } else if selection.fade_in().is_some() {
        style::with_alpha(highlight, 230)
    } else {
        style::with_alpha(highlight, 200)
    };
    paint_handle_effects(ui.painter(), fade_in_handle_rect, fade_in_color, fade_in_active);
    if fade_in_active {
        ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
    }
    if selection.fade_in().is_some() && fade_in_lower_handle_rect != egui::Rect::NOTHING {
        let fade_in_lower_color = if fade_in_lower_active {
            style::with_alpha(highlight, 255)
        } else {
            style::with_alpha(highlight, 230)
        };
        paint_handle_effects(
            ui.painter(),
            fade_in_lower_handle_rect,
            fade_in_lower_color,
            fade_in_lower_active,
        );
        if fade_in_lower_active {
            ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
        }
    }
    if selection.fade_in().is_some() && fade_in_mute_handle_rect != egui::Rect::NOTHING {
        let fade_in_mute_color = if fade_in_mute_active {
            style::with_alpha(highlight, 255)
        } else {
            style::with_alpha(highlight, 230)
        };
        paint_mute_effects(
            ui.painter(),
            fade_in_mute_handle_rect,
            fade_in_mute_color,
            fade_in_mute_active,
            true,
        );
        if fade_in_mute_active {
            ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
        }
    }
    
    let fade_out_color = if fade_out_active {
        style::with_alpha(highlight, 255)
    } else if selection.fade_out().is_some() {
        style::with_alpha(highlight, 230)
    } else {
        style::with_alpha(highlight, 200)
    };
    paint_handle_effects(ui.painter(), fade_out_handle_rect, fade_out_color, fade_out_active);
    if fade_out_active {
        ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
    }
    if selection.fade_out().is_some() && fade_out_lower_handle_rect != egui::Rect::NOTHING {
        let fade_out_lower_color = if fade_out_lower_active {
            style::with_alpha(highlight, 255)
        } else {
            style::with_alpha(highlight, 230)
        };
        paint_handle_effects(
            ui.painter(),
            fade_out_lower_handle_rect,
            fade_out_lower_color,
            fade_out_lower_active,
        );
        if fade_out_lower_active {
            ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
        }
    }
    if selection.fade_out().is_some() && fade_out_mute_handle_rect != egui::Rect::NOTHING {
        let fade_out_mute_color = if fade_out_mute_active {
            style::with_alpha(highlight, 255)
        } else {
            style::with_alpha(highlight, 230)
        };
        paint_mute_effects(
            ui.painter(),
            fade_out_mute_handle_rect,
            fade_out_mute_color,
            fade_out_mute_active,
            false,
        );
        if fade_out_mute_active {
            ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
        }
    }
    
    selection_drag::handle_edit_selection_gain_drag(
        app,
        ui,
        selection,
        &gain_handle_response,
    );
    selection_drag::handle_edit_selection_slide_drag(
        app,
        ui,
        rect,
        view,
        view_width,
        selection,
        &slide_bar_response,
    );

    // Handle fade handle dragging
    selection_drag::handle_edit_fade_handle_drag(
        app,
        ui,
        rect,
        view,
        view_width,
        selection,
        selection_rect,
        &fade_in_response,
        &fade_out_response,
        &fade_in_lower_response,
        &fade_out_lower_response,
        &fade_in_mute_response,
        &fade_out_mute_response,
        &fade_in_region_response,
        &fade_out_region_response,
    );
}

/// Apply S-curve interpolation with adjustable tension.
/// t: 0.0-1.0 input
/// curve: 0.0 = linear, 0.5 = medium S-curve, 1.0 = maximum S-curve
fn apply_s_curve(t: f32, curve: f32) -> f32 {
    if curve <= 0.0 {
        // Linear
        return t;
    }
    
    // Blend between linear and smootherstep based on curve value
    let smootherstep = {
        let t2 = t * t;
        let t3 = t2 * t;
        t3 * (t * (t * 6.0 - 15.0) + 10.0)
    };
    
    // Interpolate between linear and smootherstep
    t * (1.0 - curve) + smootherstep * curve
}

/// Draw a fade curve line showing the S-curve shape.
fn draw_fade_curve(
    painter: &egui::Painter,
    fade_rect: egui::Rect,
    curve: f32,
    is_fade_in: bool,
    color: egui::Color32,
) {
    const NUM_POINTS: usize = 32;
    let mut points = Vec::with_capacity(NUM_POINTS);
    
    let height = fade_rect.height();
    let width = fade_rect.width();
    
    for i in 0..NUM_POINTS {
        let t = i as f32 / (NUM_POINTS - 1) as f32;
        let curve_value = apply_s_curve(t, curve);
        
        let x = if is_fade_in {
            fade_rect.left() + width * t
        } else {
            fade_rect.left() + width * t
        };
        
        let y_offset = if is_fade_in {
            height * (1.0 - curve_value)
        } else {
            height * curve_value
        };
        
        let y = fade_rect.top() + y_offset;
        points.push(egui::pos2(x, y));
    }
    
    // Draw the curve line
    let stroke = egui::Stroke::new(1.5, style::with_alpha(color, 200));
    painter.add(egui::Shape::line(points, stroke));
}
