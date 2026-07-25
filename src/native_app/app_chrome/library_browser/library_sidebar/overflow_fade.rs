use radiant::prelude as ui;
use radiant::runtime::{
    PaintBrush, PaintFillPath, PaintLinearGradient, PaintPath, PaintPathCommand, PaintPrimitive,
    TransientOverlayContext,
};

use crate::native_app::app::OverflowFadeAnimations;
use crate::native_app::waveform::{WAVEFORM_SIGNAL_WIDGET_ID, WAVEFORM_WIDGET_ID, WaveformState};

const BOTTOM_OVERFLOW_FADE_HEIGHT: f32 = 48.0;
const TOP_OVERFLOW_FADE_HEIGHT: f32 = BOTTOM_OVERFLOW_FADE_HEIGHT;
const SCROLL_AFFORDANCE_TRACK_INSET: f32 = 6.0;
const SCROLL_EDGE_EPSILON: f32 = 0.5;
const WAVEFORM_EDGE_FADE_WIDTH: f32 = 26.0;
const WAVEFORM_EDGE_FADE_ALPHA: u8 = 110;
const WAVEFORM_SCROLL_EPSILON: f32 = 0.001;

/// Paint directional fades for content clipped above or below a vertical
/// scroll viewport. The scroll affordance is emitted by Radiant only for a
/// clipped viewport, so it is a reliable runtime signal rather than a proxy
/// based on row or panel counts. `entry_opacity` allows a surface with an
/// opacity ramp to make its first clipped frame visible immediately, but only
/// when neither directional edge was previously active.
pub(in crate::native_app) fn paint_vertical_scroll_overflow_fades(
    context: TransientOverlayContext<'_>,
    scroll_node_id: u64,
    top_fade_id: u64,
    bottom_fade_id: u64,
    maximum_opacity: u8,
    entry_opacity: u8,
    animations: &mut OverflowFadeAnimations,
    primitives: &mut Vec<PaintPrimitive>,
) {
    let Some((bounds, scroll_thumb)) =
        scroll_viewport(context.plan.primitives.as_slice(), scroll_node_id)
    else {
        animations.clear(top_fade_id);
        animations.clear(bottom_fade_id);
        return;
    };
    let edges = vertical_scroll_clipping_edges(bounds, scroll_thumb);
    let entering_clipped_surface =
        !animations.contains(top_fade_id) && !animations.contains(bottom_fade_id);
    let entry_opacity = entering_clipped_surface
        .then_some(entry_opacity)
        .unwrap_or(0);
    paint_vertical_scroll_edge_fade(
        primitives,
        bounds,
        context.plan.clear_color,
        VerticalScrollEdge::Top,
        top_fade_id,
        animations.opacity_from(
            top_fade_id,
            edges.above.then_some(maximum_opacity).unwrap_or(0),
            edges.above.then_some(entry_opacity).unwrap_or(0),
            context.animation_time,
        ),
    );
    paint_vertical_scroll_edge_fade(
        primitives,
        bounds,
        context.plan.clear_color,
        VerticalScrollEdge::Bottom,
        bottom_fade_id,
        animations.opacity_from(
            bottom_fade_id,
            edges.below.then_some(maximum_opacity).unwrap_or(0),
            edges.below.then_some(entry_opacity).unwrap_or(0),
            context.animation_time,
        ),
    );
}

/// Paint subtle directional edge fades when the waveform is horizontally
/// zoomed or panned, leaving the fully zoomed-out waveform unadorned.
pub(in crate::native_app) fn paint_waveform_scroll_fades(
    context: TransientOverlayContext<'_>,
    waveform: &WaveformState,
    animations: &mut OverflowFadeAnimations,
    primitives: &mut Vec<PaintPrimitive>,
) {
    let Some(bounds) = context
        .plan
        .first_widget_rect_by_priority([WAVEFORM_SIGNAL_WIDGET_ID, WAVEFORM_WIDGET_ID])
    else {
        animations.clear(WAVEFORM_LEFT_EDGE_FADE_ID);
        animations.clear(WAVEFORM_RIGHT_EDGE_FADE_ID);
        return;
    };
    if !bounds.has_finite_positive_area() {
        animations.clear(WAVEFORM_LEFT_EDGE_FADE_ID);
        animations.clear(WAVEFORM_RIGHT_EDGE_FADE_ID);
        return;
    }
    let (has_left, has_right) = (!waveform.fully_zoomed_out())
        .then(|| {
            let offset = waveform.offset_fraction();
            let visible = waveform.visible_fraction();
            (offset, visible)
        })
        .filter(|(offset, visible)| offset.is_finite() && visible.is_finite())
        .map(|(offset, visible)| {
            (
                offset > WAVEFORM_SCROLL_EPSILON,
                offset + visible < 1.0 - WAVEFORM_SCROLL_EPSILON,
            )
        })
        .unwrap_or_default();
    let background = context.plan.clear_color;
    paint_waveform_edge_fade(
        primitives,
        bounds,
        fade_tone(
            background,
            animations.opacity(
                WAVEFORM_LEFT_EDGE_FADE_ID,
                has_left.then_some(WAVEFORM_EDGE_FADE_ALPHA).unwrap_or(0),
                context.animation_time,
            ),
        ),
        WaveformEdge::Left,
        WAVEFORM_LEFT_EDGE_FADE_ID,
    );
    paint_waveform_edge_fade(
        primitives,
        bounds,
        fade_tone(
            background,
            animations.opacity(
                WAVEFORM_RIGHT_EDGE_FADE_ID,
                has_right.then_some(WAVEFORM_EDGE_FADE_ALPHA).unwrap_or(0),
                context.animation_time,
            ),
        ),
        WaveformEdge::Right,
        WAVEFORM_RIGHT_EDGE_FADE_ID,
    );
}

const WAVEFORM_LEFT_EDGE_FADE_ID: u64 = 0x7761_7665_5f6c_6566;
const WAVEFORM_RIGHT_EDGE_FADE_ID: u64 = 0x7761_7665_5f72_6967;

#[derive(Clone, Copy)]
enum WaveformEdge {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct VerticalScrollClippingEdges {
    above: bool,
    below: bool,
}

#[derive(Clone, Copy)]
enum VerticalScrollEdge {
    Top,
    Bottom,
}

fn scroll_viewport(
    primitives: &[PaintPrimitive],
    scroll_node_id: u64,
) -> Option<(ui::Rect, Option<ui::Rect>)> {
    let bounds = primitives
        .iter()
        .filter_map(PaintPrimitive::clip_start)
        .find(|clip| clip.node_id == scroll_node_id)
        .map(|clip| clip.rect)?;
    let thumb = primitives
        .iter()
        .filter_map(PaintPrimitive::fill_rect)
        .find(|fill| fill.widget_id == scroll_node_id)
        .map(|fill| fill.rect);
    Some((bounds, thumb))
}

fn vertical_scroll_clipping_edges(
    bounds: ui::Rect,
    scroll_thumb: Option<ui::Rect>,
) -> VerticalScrollClippingEdges {
    let Some(scroll_thumb) = scroll_thumb else {
        return VerticalScrollClippingEdges::default();
    };
    let track_top = bounds.min.y + SCROLL_AFFORDANCE_TRACK_INSET;
    let track_bottom = bounds.max.y - SCROLL_AFFORDANCE_TRACK_INSET;
    VerticalScrollClippingEdges {
        above: scroll_thumb.min.y > track_top + SCROLL_EDGE_EPSILON,
        below: scroll_thumb.max.y < track_bottom - SCROLL_EDGE_EPSILON,
    }
}

fn paint_vertical_scroll_edge_fade(
    primitives: &mut Vec<PaintPrimitive>,
    bounds: ui::Rect,
    background: ui::Rgba8,
    edge: VerticalScrollEdge,
    fade_id: u64,
    opacity: u8,
) {
    if opacity == 0 || !bounds.has_finite_positive_area() {
        return;
    }
    let height = match edge {
        VerticalScrollEdge::Top => TOP_OVERFLOW_FADE_HEIGHT,
        VerticalScrollEdge::Bottom => BOTTOM_OVERFLOW_FADE_HEIGHT,
    }
    .min(bounds.height());
    if height <= 0.0 {
        return;
    }
    let fade_bounds = match edge {
        VerticalScrollEdge::Top => ui::Rect::from_min_max(
            bounds.min,
            ui::Point::new(bounds.max.x, bounds.min.y + height),
        ),
        VerticalScrollEdge::Bottom => ui::Rect::from_min_max(
            ui::Point::new(bounds.min.x, bounds.max.y - height),
            bounds.max,
        ),
    };
    let tone = fade_tone(background, opacity);
    let (top, bottom) = match edge {
        VerticalScrollEdge::Top => (tone, tone.with_alpha(0)),
        VerticalScrollEdge::Bottom => (tone.with_alpha(0), tone),
    };
    primitives.push(PaintPrimitive::FillPath(PaintFillPath::new(
        fade_id,
        rectangle_path(fade_bounds),
        PaintBrush::linear_gradient(PaintLinearGradient::vertical(fade_bounds, top, bottom)),
    )));
}

fn paint_waveform_edge_fade(
    primitives: &mut Vec<PaintPrimitive>,
    bounds: ui::Rect,
    tone: ui::Rgba8,
    edge: WaveformEdge,
    fade_id: u64,
) {
    if tone.a == 0 || !bounds.has_finite_positive_area() {
        return;
    }
    let width = WAVEFORM_EDGE_FADE_WIDTH.min(bounds.width());
    if width <= 0.0 {
        return;
    }
    let fade_bounds = match edge {
        WaveformEdge::Left => ui::Rect::from_min_max(
            bounds.min,
            ui::Point::new(bounds.min.x + width, bounds.max.y),
        ),
        WaveformEdge::Right => ui::Rect::from_min_max(
            ui::Point::new(bounds.max.x - width, bounds.min.y),
            bounds.max,
        ),
    };
    let (start, end, start_color, end_color) = match edge {
        WaveformEdge::Left => (
            ui::Point::new(fade_bounds.min.x, fade_bounds.min.y),
            ui::Point::new(fade_bounds.max.x, fade_bounds.min.y),
            tone,
            tone.with_alpha(0),
        ),
        WaveformEdge::Right => (
            ui::Point::new(fade_bounds.min.x, fade_bounds.min.y),
            ui::Point::new(fade_bounds.max.x, fade_bounds.min.y),
            tone.with_alpha(0),
            tone,
        ),
    };
    primitives.push(PaintPrimitive::FillPath(PaintFillPath::new(
        fade_id,
        rectangle_path(fade_bounds),
        PaintBrush::linear_gradient(PaintLinearGradient::new(start, end, start_color, end_color)),
    )));
}

fn fade_tone(background: ui::Rgba8, opacity: u8) -> ui::Rgba8 {
    // A slightly deeper ending tone keeps the cue visible over sparse content
    // without introducing a competing surface color.
    ui::Rgba8::new(
        background.r.saturating_sub(8),
        background.g.saturating_sub(8),
        background.b.saturating_sub(8),
        opacity,
    )
}

fn rectangle_path(rect: ui::Rect) -> PaintPath {
    PaintPath::from([
        PaintPathCommand::MoveTo(rect.min),
        PaintPathCommand::LineTo(ui::Point::new(rect.max.x, rect.min.y)),
        PaintPathCommand::LineTo(rect.max),
        PaintPathCommand::LineTo(ui::Point::new(rect.min.x, rect.max.y)),
        PaintPathCommand::Close,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_app::waveform::WaveformInteraction;
    use radiant::runtime::{PaintClipStart, PaintFillRect, SurfacePaintPlan};
    use std::time::Duration;

    const SCROLL_ID: u64 = 0xfeed;
    const TOP_FADE_ID: u64 = 0xbeef;
    const BOTTOM_FADE_ID: u64 = 0xbeee;

    #[derive(Clone, Copy)]
    enum ScrollThumbPosition {
        Absent,
        Top,
        Middle,
        Bottom,
    }

    fn plan_with_scroll_viewport(position: ScrollThumbPosition) -> SurfacePaintPlan {
        let bounds =
            ui::Rect::from_min_size(ui::Point::new(20.0, 40.0), ui::Vector2::new(180.0, 60.0));
        let mut plan = SurfacePaintPlan::empty(&ui::ThemeTokens::default());
        plan.primitives
            .push(PaintPrimitive::ClipStart(PaintClipStart {
                node_id: SCROLL_ID,
                rect: bounds,
            }));
        let thumb = match position {
            ScrollThumbPosition::Absent => None,
            ScrollThumbPosition::Top => Some(ui::Rect::from_min_max(
                ui::Point::new(bounds.max.x - 3.0, bounds.min.y + 6.0),
                ui::Point::new(bounds.max.x, bounds.min.y + 30.0),
            )),
            ScrollThumbPosition::Middle => Some(ui::Rect::from_min_max(
                ui::Point::new(bounds.max.x - 3.0, bounds.min.y + 18.0),
                ui::Point::new(bounds.max.x, bounds.min.y + 42.0),
            )),
            ScrollThumbPosition::Bottom => Some(ui::Rect::from_min_max(
                ui::Point::new(bounds.max.x - 3.0, bounds.max.y - 30.0),
                ui::Point::new(bounds.max.x, bounds.max.y - 6.0),
            )),
        };
        if let Some(rect) = thumb {
            plan.primitives
                .push(PaintPrimitive::FillRect(PaintFillRect {
                    widget_id: SCROLL_ID,
                    rect,
                    color: ui::Rgba8::new(1, 2, 3, 4),
                }));
        }
        plan
    }

    #[test]
    fn vertical_overflow_fades_are_absent_when_the_viewport_fits() {
        let plan = plan_with_scroll_viewport(ScrollThumbPosition::Absent);
        let mut primitives = Vec::new();
        let mut animations = OverflowFadeAnimations::default();

        paint_vertical_scroll_overflow_fades(
            TransientOverlayContext::new(&plan, ui::Vector2::new(240.0, 180.0), Duration::ZERO),
            SCROLL_ID,
            TOP_FADE_ID,
            BOTTOM_FADE_ID,
            u8::MAX,
            0,
            &mut animations,
            &mut primitives,
        );

        assert!(primitives.is_empty());
    }

    #[test]
    fn vertical_overflow_fade_fades_in_at_the_visible_clipping_edge() {
        let plan = plan_with_scroll_viewport(ScrollThumbPosition::Top);
        let mut primitives = Vec::new();
        let mut animations = OverflowFadeAnimations::default();

        paint_vertical_scroll_overflow_fades(
            TransientOverlayContext::new(&plan, ui::Vector2::new(240.0, 180.0), Duration::ZERO),
            SCROLL_ID,
            TOP_FADE_ID,
            BOTTOM_FADE_ID,
            u8::MAX,
            0,
            &mut animations,
            &mut primitives,
        );
        assert!(primitives.is_empty());
        paint_vertical_scroll_overflow_fades(
            TransientOverlayContext::new(
                &plan,
                ui::Vector2::new(240.0, 180.0),
                Duration::from_millis(300),
            ),
            SCROLL_ID,
            TOP_FADE_ID,
            BOTTOM_FADE_ID,
            u8::MAX,
            0,
            &mut animations,
            &mut primitives,
        );

        let [PaintPrimitive::FillPath(fill)] = primitives.as_slice() else {
            panic!("expected exactly one overflow fade");
        };
        assert_eq!(fill.widget_id, BOTTOM_FADE_ID);
        let PaintBrush::LinearGradient(gradient) = fill.brush else {
            panic!("expected an overflow gradient");
        };
        assert_eq!(gradient.start_color.a, 0);
        assert_eq!(gradient.end_color, fade_tone(plan.clear_color, u8::MAX));
    }

    #[test]
    fn vertical_overflow_fade_can_paint_a_visible_entry_on_its_first_clipped_frame() {
        let plan = plan_with_scroll_viewport(ScrollThumbPosition::Top);
        let mut primitives = Vec::new();
        let mut animations = OverflowFadeAnimations::default();
        const ENTRY_ALPHA: u8 = 59;

        paint_vertical_scroll_overflow_fades(
            TransientOverlayContext::new(&plan, ui::Vector2::new(240.0, 180.0), Duration::ZERO),
            SCROLL_ID,
            TOP_FADE_ID,
            BOTTOM_FADE_ID,
            ENTRY_ALPHA,
            ENTRY_ALPHA,
            &mut animations,
            &mut primitives,
        );

        let [PaintPrimitive::FillPath(fill)] = primitives.as_slice() else {
            panic!("expected the first clipped frame to paint an overflow fade");
        };
        assert_eq!(fill.widget_id, BOTTOM_FADE_ID);
        let PaintBrush::LinearGradient(gradient) = fill.brush else {
            panic!("expected an overflow gradient");
        };
        assert_eq!(gradient.end_color, fade_tone(plan.clear_color, ENTRY_ALPHA));

        primitives.clear();
        let opposite_edge_plan = plan_with_scroll_viewport(ScrollThumbPosition::Bottom);
        paint_vertical_scroll_overflow_fades(
            TransientOverlayContext::new(
                &opposite_edge_plan,
                ui::Vector2::new(240.0, 180.0),
                Duration::from_millis(100),
            ),
            SCROLL_ID,
            TOP_FADE_ID,
            BOTTOM_FADE_ID,
            ENTRY_ALPHA,
            ENTRY_ALPHA,
            &mut animations,
            &mut primitives,
        );
        assert!(
            primitives.iter().all(|primitive| {
                primitive
                    .fill_path()
                    .is_none_or(|fill| fill.widget_id != TOP_FADE_ID)
            }),
            "an already-clipped surface must crossfade its newly exposed edge"
        );
    }

    #[test]
    fn vertical_overflow_fade_restarts_from_transparent_after_its_viewport_unmounts() {
        let overflowing_plan = plan_with_scroll_viewport(ScrollThumbPosition::Top);
        let absent_plan = SurfacePaintPlan::empty(&ui::ThemeTokens::default());
        let mut primitives = Vec::new();
        let mut animations = OverflowFadeAnimations::default();

        paint_vertical_scroll_overflow_fades(
            TransientOverlayContext::new(
                &overflowing_plan,
                ui::Vector2::new(240.0, 180.0),
                Duration::ZERO,
            ),
            SCROLL_ID,
            TOP_FADE_ID,
            BOTTOM_FADE_ID,
            u8::MAX,
            0,
            &mut animations,
            &mut primitives,
        );
        primitives.clear();
        paint_vertical_scroll_overflow_fades(
            TransientOverlayContext::new(
                &overflowing_plan,
                ui::Vector2::new(240.0, 180.0),
                Duration::from_millis(300),
            ),
            SCROLL_ID,
            TOP_FADE_ID,
            BOTTOM_FADE_ID,
            u8::MAX,
            0,
            &mut animations,
            &mut primitives,
        );
        assert_eq!(primitives.len(), 1, "the first overflow should settle in");

        primitives.clear();
        paint_vertical_scroll_overflow_fades(
            TransientOverlayContext::new(
                &absent_plan,
                ui::Vector2::new(240.0, 180.0),
                Duration::from_millis(400),
            ),
            SCROLL_ID,
            TOP_FADE_ID,
            BOTTOM_FADE_ID,
            u8::MAX,
            0,
            &mut animations,
            &mut primitives,
        );
        assert!(
            primitives.is_empty(),
            "an unmounted viewport has no fade bounds"
        );

        paint_vertical_scroll_overflow_fades(
            TransientOverlayContext::new(
                &overflowing_plan,
                ui::Vector2::new(240.0, 180.0),
                Duration::from_millis(500),
            ),
            SCROLL_ID,
            TOP_FADE_ID,
            BOTTOM_FADE_ID,
            u8::MAX,
            0,
            &mut animations,
            &mut primitives,
        );
        assert!(
            primitives.is_empty(),
            "a remounted viewport must fade in instead of popping at full opacity"
        );
    }

    #[test]
    fn vertical_overflow_fades_follow_content_hidden_above_and_below() {
        let bounds =
            ui::Rect::from_min_size(ui::Point::new(20.0, 40.0), ui::Vector2::new(180.0, 60.0));
        assert_eq!(
            vertical_scroll_clipping_edges(bounds, None),
            VerticalScrollClippingEdges::default()
        );
        let top = vertical_scroll_clipping_edges(
            bounds,
            scroll_viewport(
                plan_with_scroll_viewport(ScrollThumbPosition::Top)
                    .primitives
                    .as_slice(),
                SCROLL_ID,
            )
            .and_then(|(_, thumb)| thumb),
        );
        let middle = vertical_scroll_clipping_edges(
            bounds,
            scroll_viewport(
                plan_with_scroll_viewport(ScrollThumbPosition::Middle)
                    .primitives
                    .as_slice(),
                SCROLL_ID,
            )
            .and_then(|(_, thumb)| thumb),
        );
        let bottom = vertical_scroll_clipping_edges(
            bounds,
            scroll_viewport(
                plan_with_scroll_viewport(ScrollThumbPosition::Bottom)
                    .primitives
                    .as_slice(),
                SCROLL_ID,
            )
            .and_then(|(_, thumb)| thumb),
        );
        assert_eq!(
            top,
            VerticalScrollClippingEdges {
                above: false,
                below: true,
            }
        );
        assert_eq!(
            middle,
            VerticalScrollClippingEdges {
                above: true,
                below: true,
            }
        );
        assert_eq!(
            bottom,
            VerticalScrollClippingEdges {
                above: true,
                below: false,
            }
        );
    }

    #[test]
    fn top_overflow_fade_is_opaque_at_the_clipped_top_edge() {
        let bounds =
            ui::Rect::from_min_size(ui::Point::new(20.0, 40.0), ui::Vector2::new(180.0, 60.0));
        let tone = ui::Rgba8::new(10, 14, 13, u8::MAX);
        let mut primitives = Vec::new();

        paint_vertical_scroll_edge_fade(
            &mut primitives,
            bounds,
            ui::Rgba8::new(18, 22, 21, 255),
            VerticalScrollEdge::Top,
            TOP_FADE_ID,
            u8::MAX,
        );

        let [PaintPrimitive::FillPath(fill)] = primitives.as_slice() else {
            panic!("expected a top overflow fade");
        };
        let PaintBrush::LinearGradient(gradient) = fill.brush else {
            panic!("expected a top overflow gradient");
        };
        assert_eq!(gradient.start_color, tone);
        assert_eq!(gradient.end_color.a, 0);
    }

    #[test]
    fn waveform_edge_fades_point_toward_offscreen_audio() {
        let bounds =
            ui::Rect::from_min_size(ui::Point::new(20.0, 40.0), ui::Vector2::new(180.0, 60.0));
        let tone = ui::Rgba8::new(10, 14, 13, WAVEFORM_EDGE_FADE_ALPHA);
        let mut primitives = Vec::new();

        paint_waveform_edge_fade(
            &mut primitives,
            bounds,
            tone,
            WaveformEdge::Left,
            WAVEFORM_LEFT_EDGE_FADE_ID,
        );
        paint_waveform_edge_fade(
            &mut primitives,
            bounds,
            tone,
            WaveformEdge::Right,
            WAVEFORM_RIGHT_EDGE_FADE_ID,
        );

        let [
            PaintPrimitive::FillPath(left),
            PaintPrimitive::FillPath(right),
        ] = primitives.as_slice()
        else {
            panic!("expected both waveform edge fades");
        };
        let PaintBrush::LinearGradient(left_gradient) = left.brush else {
            panic!("expected left gradient");
        };
        let PaintBrush::LinearGradient(right_gradient) = right.brush else {
            panic!("expected right gradient");
        };
        assert_eq!(left_gradient.start_color, tone);
        assert_eq!(left_gradient.end_color.a, 0);
        assert_eq!(right_gradient.start_color.a, 0);
        assert_eq!(right_gradient.end_color, tone);
    }

    #[test]
    fn waveform_fade_only_marks_the_edges_with_offscreen_audio() {
        let bounds =
            ui::Rect::from_min_size(ui::Point::new(20.0, 40.0), ui::Vector2::new(180.0, 60.0));
        let mut plan = SurfacePaintPlan::empty(&ui::ThemeTokens::default());
        plan.primitives
            .push(PaintPrimitive::FillRect(PaintFillRect {
                widget_id: WAVEFORM_SIGNAL_WIDGET_ID,
                rect: bounds,
                color: ui::Rgba8::new(1, 2, 3, 4),
            }));
        let mut waveform = WaveformState::synthetic_for_tests();
        waveform.set_play_selection_range(0.25, 0.75);
        waveform.apply_interaction(WaveformInteraction::ZoomToPlaySelection);
        waveform.apply_interaction(WaveformInteraction::ScrollTo {
            offset_fraction: 0.0,
        });

        let mut primitives = Vec::new();
        let mut animations = OverflowFadeAnimations::default();
        paint_waveform_scroll_fades(
            TransientOverlayContext::new(&plan, ui::Vector2::new(240.0, 180.0), Duration::ZERO),
            &waveform,
            &mut animations,
            &mut primitives,
        );
        assert!(primitives.is_empty());
        paint_waveform_scroll_fades(
            TransientOverlayContext::new(
                &plan,
                ui::Vector2::new(240.0, 180.0),
                Duration::from_millis(300),
            ),
            &waveform,
            &mut animations,
            &mut primitives,
        );
        assert!(matches!(
            primitives.as_slice(),
            [PaintPrimitive::FillPath(fill)] if fill.widget_id == WAVEFORM_RIGHT_EDGE_FADE_ID
        ));

        waveform.apply_interaction(WaveformInteraction::ScrollTo {
            offset_fraction: 0.25,
        });
        primitives.clear();
        paint_waveform_scroll_fades(
            TransientOverlayContext::new(
                &plan,
                ui::Vector2::new(240.0, 180.0),
                Duration::from_millis(500),
            ),
            &waveform,
            &mut animations,
            &mut primitives,
        );
        primitives.clear();
        paint_waveform_scroll_fades(
            TransientOverlayContext::new(
                &plan,
                ui::Vector2::new(240.0, 180.0),
                Duration::from_millis(800),
            ),
            &waveform,
            &mut animations,
            &mut primitives,
        );
        assert!(matches!(
            primitives.as_slice(),
            [PaintPrimitive::FillPath(left), PaintPrimitive::FillPath(right)]
                if left.widget_id == WAVEFORM_LEFT_EDGE_FADE_ID
                    && right.widget_id == WAVEFORM_RIGHT_EDGE_FADE_ID
        ));

        waveform.apply_interaction(WaveformInteraction::ScrollTo {
            offset_fraction: 0.5,
        });
        primitives.clear();
        paint_waveform_scroll_fades(
            TransientOverlayContext::new(
                &plan,
                ui::Vector2::new(240.0, 180.0),
                Duration::from_millis(1000),
            ),
            &waveform,
            &mut animations,
            &mut primitives,
        );
        primitives.clear();
        paint_waveform_scroll_fades(
            TransientOverlayContext::new(
                &plan,
                ui::Vector2::new(240.0, 180.0),
                Duration::from_millis(1300),
            ),
            &waveform,
            &mut animations,
            &mut primitives,
        );
        assert!(matches!(
            primitives.as_slice(),
            [PaintPrimitive::FillPath(fill)] if fill.widget_id == WAVEFORM_LEFT_EDGE_FADE_ID
        ));

        waveform.apply_interaction(WaveformInteraction::ZoomFull);
        primitives.clear();
        paint_waveform_scroll_fades(
            TransientOverlayContext::new(
                &plan,
                ui::Vector2::new(240.0, 180.0),
                Duration::from_millis(1500),
            ),
            &waveform,
            &mut animations,
            &mut primitives,
        );
        primitives.clear();
        paint_waveform_scroll_fades(
            TransientOverlayContext::new(
                &plan,
                ui::Vector2::new(240.0, 180.0),
                Duration::from_millis(1800),
            ),
            &waveform,
            &mut animations,
            &mut primitives,
        );
        assert!(primitives.is_empty());
    }

    #[test]
    fn waveform_fade_stops_animating_and_restarts_after_its_anchor_unmounts() {
        let bounds =
            ui::Rect::from_min_size(ui::Point::new(20.0, 40.0), ui::Vector2::new(180.0, 60.0));
        let mut anchored_plan = SurfacePaintPlan::empty(&ui::ThemeTokens::default());
        anchored_plan
            .primitives
            .push(PaintPrimitive::FillRect(PaintFillRect {
                widget_id: WAVEFORM_SIGNAL_WIDGET_ID,
                rect: bounds,
                color: ui::Rgba8::new(1, 2, 3, 4),
            }));
        let absent_plan = SurfacePaintPlan::empty(&ui::ThemeTokens::default());
        let mut waveform = WaveformState::synthetic_for_tests();
        waveform.set_play_selection_range(0.25, 0.75);
        waveform.apply_interaction(WaveformInteraction::ZoomToPlaySelection);
        let mut primitives = Vec::new();
        let mut animations = OverflowFadeAnimations::default();

        paint_waveform_scroll_fades(
            TransientOverlayContext::new(
                &anchored_plan,
                ui::Vector2::new(240.0, 180.0),
                Duration::ZERO,
            ),
            &waveform,
            &mut animations,
            &mut primitives,
        );
        assert!(animations.is_animating());

        paint_waveform_scroll_fades(
            TransientOverlayContext::new(
                &absent_plan,
                ui::Vector2::new(240.0, 180.0),
                Duration::from_millis(100),
            ),
            &waveform,
            &mut animations,
            &mut primitives,
        );
        assert!(primitives.is_empty());
        assert!(!animations.is_animating());

        paint_waveform_scroll_fades(
            TransientOverlayContext::new(
                &anchored_plan,
                ui::Vector2::new(240.0, 180.0),
                Duration::from_millis(200),
            ),
            &waveform,
            &mut animations,
            &mut primitives,
        );
        assert!(
            primitives.is_empty(),
            "a remounted waveform must fade in instead of popping at full opacity"
        );
    }

    #[test]
    fn overflow_fade_frame_request_arms_once_then_settles_after_transition() {
        let mut animations = OverflowFadeAnimations::default();
        assert!(animations.frame_needed());

        animations.consume_pending_visibility_sample();
        assert!(!animations.frame_needed());

        animations.opacity(0xfeed, 110, Duration::ZERO);
        assert!(animations.frame_needed());
        animations.opacity(0xfeed, 110, Duration::from_millis(259));
        assert!(animations.frame_needed());
        animations.opacity(0xfeed, 110, Duration::from_millis(260));
        assert!(!animations.frame_needed());

        animations.arm();
        assert!(animations.frame_needed());
        animations.consume_pending_visibility_sample();
        assert!(!animations.frame_needed());
    }
}
