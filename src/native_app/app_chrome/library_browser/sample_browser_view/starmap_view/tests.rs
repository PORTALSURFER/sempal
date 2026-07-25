use radiant::widgets::WidgetInput;

use super::*;

#[test]
fn ordinary_starmap_nodes_are_batched_by_color() {
    let color = ui::Rgba8::new(255, 160, 80, 220);
    let widget = StarmapWidget::new(
        vec![
            starmap_item("/samples/kick.wav", 0.25, 0.25, color),
            starmap_item("/samples/snare.wav", 0.50, 0.50, color),
            starmap_item("/samples/hat.wav", 0.75, 0.75, color),
        ],
        StarmapViewport::default(),
        None,
    );
    let mut primitives = Vec::new();

    widget.append_paint(
        &mut primitives,
        Rect::from_size(200.0, 100.0),
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    let batches = primitives
        .iter()
        .filter_map(|primitive| match primitive {
            PaintPrimitive::FillRectBatch(batch) => Some(batch),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].color, color);
    assert_eq!(batches[0].rects.len(), 3);
    assert!((batches[0].rects[0].width() - MAP_NODE_SIZE).abs() < 0.001);
}

#[test]
fn similarity_color_groups_do_not_paint_backdrop_regions() {
    let color = ui::Rgba8::new(255, 160, 80, 220);
    let items = (0..12)
        .map(|index| {
            starmap_item(
                &format!("/samples/group-{index}.wav"),
                0.25 + index as f32 * 0.04,
                0.25 + index as f32 * 0.04,
                color.with_alpha(190 + index.min(4) as u8 * 10),
            )
        })
        .chain(std::iter::once(starmap_item(
            "/samples/lone.wav",
            0.90,
            0.12,
            ui::Rgba8::new(57, 187, 245, 220),
        )))
        .collect::<Vec<_>>();
    let widget = StarmapWidget::new(items, StarmapViewport::default(), None);
    let mut primitives = Vec::new();

    widget.append_paint(
        &mut primitives,
        Rect::from_size(200.0, 100.0),
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    assert!(!primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::FillRect(fill)
            if fill.color.a < 100
                && (fill.rect.width() > MAP_ACTIVE_AUDITION_GLOW_SIZE
                    || fill.rect.height() > MAP_ACTIVE_AUDITION_GLOW_SIZE)
    )));
    assert!(!primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::StrokeRect(stroke)
            if stroke.color.a < 100
                && (stroke.rect.width() > MAP_ACTIVE_AUDITION_GLOW_SIZE
                    || stroke.rect.height() > MAP_ACTIVE_AUDITION_GLOW_SIZE)
    )));
}

#[test]
fn same_color_runs_still_paint_individual_nodes() {
    let color = ui::Rgba8::new(255, 160, 80, 220);
    let widget = StarmapWidget::new(
        vec![
            starmap_item("/samples/kick.wav", 0.25, 0.25, color.with_alpha(190)),
            starmap_item("/samples/snare.wav", 0.50, 0.50, color.with_alpha(220)),
            starmap_item("/samples/hat.wav", 0.75, 0.75, color.with_alpha(240)),
        ],
        StarmapViewport::default(),
        None,
    );
    let mut primitives = Vec::new();

    widget.append_paint(
        &mut primitives,
        Rect::from_size(200.0, 100.0),
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    let node_count = primitives
        .iter()
        .filter_map(|primitive| match primitive {
            PaintPrimitive::FillRectBatch(batch)
                if (batch.color.r, batch.color.g, batch.color.b) == (color.r, color.g, color.b) =>
            {
                Some(batch.rects.len())
            }
            _ => None,
        })
        .sum::<usize>();
    assert_eq!(node_count, 3);
}

#[test]
fn non_previewable_cold_audition_nodes_paint_as_hollow_markers() {
    let color = ui::Rgba8::new(255, 160, 80, 220);
    let mut cold = starmap_item("/samples/unsupported.aiff", 0.50, 0.50, color);
    cold.instant_audition_ready = false;
    let widget = StarmapWidget::new(vec![cold], StarmapViewport::default(), None);
    let mut primitives = Vec::new();

    widget.append_paint(
        &mut primitives,
        Rect::from_size(200.0, 100.0),
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::FillPolygon(fill) if fill.color.a < 80
    )));
    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::StrokePolyline(stroke)
            if stroke.color == ui::Rgba8::new(232, 236, 238, 165)
    )));
    assert!(primitives.iter().all(|primitive| !matches!(
        primitive,
        PaintPrimitive::FillRectBatch(batch) if batch.color == color
    )));
}

#[test]
fn cold_long_wav_nodes_paint_as_preview_decode_candidates() {
    let color = ui::Rgba8::new(255, 160, 80, 220);
    let mut cold = starmap_item("/samples/long.wav", 0.50, 0.50, color);
    cold.instant_audition_ready = false;
    let widget = StarmapWidget::new(vec![cold], StarmapViewport::default(), None);
    let mut primitives = Vec::new();

    widget.append_paint(
        &mut primitives,
        Rect::from_size(200.0, 100.0),
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    assert!(!primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::StrokePolyline(stroke)
            if stroke.color == ui::Rgba8::new(232, 236, 238, 165)
    )));
    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::FillRectBatch(batch) if batch.color == color.with_alpha(205)
    )));
}

#[test]
fn preview_ready_nodes_do_not_paint_as_cold_audition_markers() {
    let color = ui::Rgba8::new(255, 160, 80, 220);
    let mut preview_ready = starmap_item("/samples/preview-ready.wav", 0.50, 0.50, color);
    preview_ready.instant_audition_ready = false;
    preview_ready.preview_audition_ready = true;
    let widget = StarmapWidget::new(vec![preview_ready], StarmapViewport::default(), None);
    let mut primitives = Vec::new();

    widget.append_paint(
        &mut primitives,
        Rect::from_size(200.0, 100.0),
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    assert!(!primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::StrokePolyline(stroke)
            if stroke.color == ui::Rgba8::new(232, 236, 238, 165)
    )));
    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::FillRectBatch(batch) if batch.color == color
    )));
}

#[test]
fn copied_starmap_nodes_paint_confirmation_glow() {
    let color = ui::Rgba8::new(255, 160, 80, 220);
    let mut copied = starmap_item("/samples/copied.wav", 0.50, 0.50, color);
    copied.copy_flash = true;
    let widget = StarmapWidget::new(vec![copied], StarmapViewport::default(), None);
    let mut primitives = Vec::new();

    widget.append_paint(
        &mut primitives,
        Rect::from_size(200.0, 100.0),
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::FillPolygon(fill)
            if fill.color == color.with_alpha(78)
    )));
    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::StrokePolyline(stroke)
            if stroke.color == ui::Rgba8::new(245, 245, 245, 235)
    )));
}

#[test]
fn x_marked_starmap_nodes_paint_selection_confirmation_glow() {
    let color = ui::Rgba8::new(255, 160, 80, 220);
    let mut marked = starmap_item("/samples/marked.wav", 0.50, 0.50, color);
    marked.selected = true;
    marked.selection_flash = true;
    let widget = StarmapWidget::new(vec![marked], StarmapViewport::default(), None);
    let mut primitives = Vec::new();

    widget.append_paint(
        &mut primitives,
        Rect::from_size(200.0, 100.0),
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::FillPolygon(fill)
            if fill.color == ACCENT.with_alpha(92)
    )));
    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::StrokePolyline(stroke)
            if stroke.color == ACCENT.with_alpha(245) && stroke.width == 1.5
    )));
}

#[test]
fn selected_starmap_nodes_paint_stronger_than_similarity_anchor() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let mut selected = starmap_item("/samples/kick.wav", 0.25, 0.5, color);
    selected.selected = true;
    let mut anchor = starmap_item("/samples/snare.wav", 0.75, 0.5, color);
    anchor.similarity_anchor = true;
    let widget = StarmapWidget::new(vec![selected, anchor], StarmapViewport::default(), None);
    let mut primitives = Vec::new();

    widget.append_paint(
        &mut primitives,
        Rect::from_size(200.0, 100.0),
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    let fills = primitives
        .iter()
        .filter_map(|primitive| match primitive {
            PaintPrimitive::FillPolygon(fill) => Some(fill),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(fills.iter().any(|fill| fill.color == color.with_alpha(64)
        && fill.points.len() == 4
        && fill.points[0] == Point::new(50.0, 50.0 - MAP_SELECTED_OUTER_GLOW_SIZE * 0.5)));
    assert!(fills.iter().any(|fill| fill.color == color.with_alpha(118)
        && fill.points.len() == 4
        && fill.points[0] == Point::new(50.0, 50.0 - MAP_SELECTED_GLOW_SIZE * 0.5)));
    assert!(fills.iter().any(|fill| fill.color == color.with_alpha(255)
        && fill.points.len() == 4
        && fill.points[0] == Point::new(50.0, 50.0 - (MAP_SELECTED_SIZE + 2.0) * 0.5)));
    assert!(fills.iter().any(|fill| fill.color == color.with_alpha(42)
        && fill.points.len() == 4
        && fill.points[0] == Point::new(150.0, 50.0 - MAP_ANCHOR_GLOW_SIZE * 0.5)));
    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::StrokePolyline(stroke)
            if stroke.color == ui::Rgba8::new(255, 252, 229, 245)
                && stroke.width == 1.35
                && stroke.points.len() == 5
                && stroke.points[0] == Point::new(50.0, 50.0 - (MAP_SELECTED_SIZE + 6.0) * 0.5)
    )));
    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::StrokePolyline(stroke)
            if stroke.color == ui::Rgba8::new(255, 255, 255, 210)
                && stroke.width == 0.85
                && stroke.points.len() == 5
                && stroke.points[0] == Point::new(50.0, 50.0 - (MAP_SELECTED_SIZE + 1.5) * 0.5)
    )));
    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::StrokePolyline(stroke)
            if stroke.color == ui::Rgba8::new(245, 245, 245, 220)
                && stroke.width == 1.0
                && stroke.points.len() == 5
                && stroke.points[0] == Point::new(150.0, 50.0 - (MAP_ANCHOR_SIZE + 4.0) * 0.5)
    )));
}

#[test]
fn hovering_starmap_node_paints_lightweight_runtime_highlight_without_label() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let bounds = Rect::from_size(200.0, 100.0);
    let mut item = starmap_item("/samples/kick.wav", 0.25, 0.5, color);
    item.label = String::from("Kick Tight 01");
    let mut widget = StarmapWidget::new(vec![item], StarmapViewport::default(), None);

    assert_eq!(
        widget
            .handle_input(bounds, WidgetInput::pointer_move(Point::new(50.0, 50.0)))
            .and_then(|output| output.typed_cloned::<GuiMessage>()),
        None,
        "ordinary starmap hover should update widget-local paint state without host output"
    );
    let mut primitives = Vec::new();
    widget.append_runtime_overlay_paint(
        &mut primitives,
        bounds,
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    assert_eq!(widget.hovered_file_id.as_deref(), Some("/samples/kick.wav"));
    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::FillPolygon(fill)
            if fill.color == color.with_alpha(50)
                && fill.points.len() == 4
                && fill.points[0] == Point::new(50.0, 50.0 - MAP_HOVER_GLOW_SIZE * 0.5)
    )));
    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::StrokePolyline(stroke)
            if stroke.color == ui::Rgba8::new(248, 248, 248, 230)
                && stroke.points.len() == 5
    )));
    assert!(
        !primitives.iter().any(|primitive| matches!(
            primitive,
            PaintPrimitive::Text(text) if text.text.as_str() == "Kick Tight 01"
        )),
        "hovering a map sample should not paint a sample-name tooltip"
    );
}

#[test]
fn focused_starmap_node_paints_highlight_without_label() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let bounds = Rect::from_size(200.0, 100.0);
    let mut item = starmap_item("/samples/kick.wav", 0.25, 0.5, color);
    item.label = String::from("Kick Tight 01");
    item.selected = true;
    item.focused = true;
    let widget = StarmapWidget::new(vec![item], StarmapViewport::default(), None);
    let mut primitives = Vec::new();

    widget.append_runtime_overlay_paint(
        &mut primitives,
        bounds,
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::StrokePolyline(stroke)
            if stroke.color == ui::Rgba8::new(248, 248, 248, 190)
                && stroke.points.len() == 5
    )));
    assert!(
        !primitives.iter().any(|primitive| matches!(
            primitive,
            PaintPrimitive::Text(text) if text.text.as_str() == "Kick Tight 01"
        )),
        "focused map samples should not paint persistent sample-name labels"
    );
    assert!(
        !primitives.iter().any(|primitive| matches!(
            primitive,
            PaintPrimitive::StrokeRect(stroke)
                if stroke.color == ui::Rgba8::new(248, 248, 248, 230)
        )),
        "focused selection should not paint a rectangular tooltip"
    );
}

#[test]
fn active_starmap_drag_paints_current_audition_node_without_hover_label() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let bounds = Rect::from_size(200.0, 100.0);
    let mut item = starmap_item("/samples/kick.wav", 0.25, 0.5, color);
    item.label = String::from("Kick Tight 01");
    let widget = StarmapWidget::new(
        vec![item],
        StarmapViewport::default(),
        Some(StarmapAuditionDragState {
            last_hit_file_id: Some(String::from("/samples/kick.wav")),
            last_position: Point::new(50.0, 50.0),
            modifiers: PointerModifiers::default(),
        }),
    );
    let mut primitives = Vec::new();

    widget.append_runtime_overlay_paint(
        &mut primitives,
        bounds,
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    assert!(primitives.iter().any(|primitive| matches!(
            primitive,
            PaintPrimitive::FillPolygon(fill)
                if fill.color == color.with_alpha(72)
                    && fill.points.len() == 4
                    && fill.points[0] == Point::new(50.0, 50.0 - (MAP_ACTIVE_AUDITION_GLOW_SIZE + 6.0) * 0.5)
        )));
    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::FillPolygon(fill)
            if fill.color == color.with_alpha(255)
                && fill.points.len() == 4
                && fill.points[0] == Point::new(50.0, 50.0 - (MAP_ACTIVE_AUDITION_SIZE + 2.0) * 0.5)
    )));
    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::StrokePolyline(stroke)
            if stroke.color == ui::Rgba8::new(255, 250, 224, 245)
                && stroke.width == 1.45
                && stroke.points.len() == 5
    )));
    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::StrokePolyline(stroke)
            if stroke.color == ui::Rgba8::new(255, 255, 255, 210)
                && stroke.width == 0.9
                && stroke.points.len() == 5
    )));
    assert!(
        !primitives.iter().any(|primitive| matches!(
            primitive,
            PaintPrimitive::Text(text) if text.text.as_str() == "Kick Tight 01"
        )),
        "dragging should highlight the active hit without painting hover labels"
    );
}

#[test]
fn active_starmap_drag_overlay_tracks_local_pointer_hit_before_controller_refresh() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let bounds = Rect::from_size(200.0, 100.0);
    let mut widget = StarmapWidget::new(
        vec![
            starmap_item("/samples/kick.wav", 0.25, 0.5, color),
            starmap_item("/samples/snare.wav", 0.75, 0.5, color),
        ],
        StarmapViewport::default(),
        Some(StarmapAuditionDragState {
            last_hit_file_id: Some(String::from("/samples/kick.wav")),
            last_position: Point::new(50.0, 50.0),
            modifiers: PointerModifiers::default(),
        }),
    );

    let output = widget
        .handle_input(bounds, WidgetInput::pointer_move(Point::new(150.0, 50.0)))
        .and_then(|output| output.typed_cloned::<GuiMessage>());
    let mut primitives = Vec::new();
    widget.append_runtime_overlay_paint(
        &mut primitives,
        bounds,
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    assert_eq!(
        output,
        Some(GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![String::from("/samples/snare.wav")],
            position: Point::new(150.0, 50.0),
            modifiers: PointerModifiers::default(),
        })
    );
    assert_eq!(
        widget.active_drag_item().map(|item| item.file_id.as_str()),
        Some("/samples/snare.wav"),
        "runtime overlay should follow the widget-local hit before app state refreshes"
    );
    assert!(primitives.iter().any(|primitive| matches!(
            primitive,
            PaintPrimitive::FillPolygon(fill)
                if fill.color == color.with_alpha(255)
                    && fill.points.len() == 4
                    && fill.points[0] == Point::new(150.0, 50.0 - (MAP_ACTIVE_AUDITION_SIZE + 2.0) * 0.5)
        )));
}

#[test]
fn active_starmap_drag_paints_local_hit_in_rebuilt_base_scene() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let bounds = Rect::from_size(200.0, 100.0);
    let left_id = String::from("/samples/kick.wav");
    let right_id = String::from("/samples/snare.wav");
    let mut previous = StarmapWidget::new(
        vec![
            starmap_item(left_id.as_str(), 0.25, 0.5, color),
            starmap_item(right_id.as_str(), 0.75, 0.5, color),
        ],
        StarmapViewport::default(),
        Some(StarmapAuditionDragState {
            last_hit_file_id: Some(left_id.clone()),
            last_position: Point::new(50.0, 50.0),
            modifiers: PointerModifiers::default(),
        }),
    );
    previous
        .handle_input(bounds, WidgetInput::pointer_move(Point::new(150.0, 50.0)))
        .expect("pointer move should emit drag update");

    let mut next = StarmapWidget::new(
        vec![
            starmap_item(left_id.as_str(), 0.25, 0.5, color),
            starmap_item(right_id.as_str(), 0.75, 0.5, color),
        ],
        StarmapViewport::default(),
        Some(StarmapAuditionDragState {
            last_hit_file_id: Some(left_id),
            last_position: Point::new(50.0, 50.0),
            modifiers: PointerModifiers::default(),
        }),
    );
    next.synchronize_from_previous(&previous);
    let mut primitives = Vec::new();

    next.append_paint(
        &mut primitives,
        bounds,
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    assert!(primitives.iter().any(|primitive| matches!(
            primitive,
            PaintPrimitive::FillPolygon(fill)
                if fill.color == color.with_alpha(255)
                    && fill.points.len() == 4
                    && fill.points[0] == Point::new(150.0, 50.0 - (MAP_ACTIVE_AUDITION_SIZE + 2.0) * 0.5)
        )));
}

#[test]
fn stale_local_drag_hit_does_not_paint_after_drag_clears() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let bounds = Rect::from_size(200.0, 100.0);
    let mut widget = StarmapWidget::new(
        vec![starmap_item("/samples/kick.wav", 0.25, 0.5, color)],
        StarmapViewport::default(),
        None,
    );
    widget.last_hit_file_id = Some(String::from("/samples/kick.wav"));
    widget.last_hit_index = Some(0);
    let mut primitives = Vec::new();

    widget.append_paint(
        &mut primitives,
        bounds,
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    assert!(!primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::FillPolygon(fill)
            if fill.color == color.with_alpha(255)
                && fill.points.len() == 4
                && fill.points[0] == Point::new(50.0, 50.0 - (MAP_ACTIVE_AUDITION_SIZE + 2.0) * 0.5)
    )));
}

#[test]
fn app_transient_drag_overlay_paints_current_controller_target() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let bounds = Rect::from_size(200.0, 100.0);
    let items = vec![
        starmap_item("/samples/kick.wav", 0.25, 0.5, color),
        starmap_item("/samples/snare.wav", 0.75, 0.5, color),
    ];
    let mut primitives = Vec::new();

    paint_active_starmap_audition_overlay(
        &mut primitives,
        bounds,
        &items,
        StarmapViewport::default(),
        "/samples/snare.wav",
    );

    assert!(primitives.iter().any(|primitive| matches!(
            primitive,
            PaintPrimitive::FillPolygon(fill)
                if fill.color == color.with_alpha(255)
                    && fill.points.len() == 4
                    && fill.points[0] == Point::new(150.0, 50.0 - (MAP_ACTIVE_AUDITION_SIZE + 2.0) * 0.5)
        )));
    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::StrokePolyline(stroke)
            if stroke.color == ui::Rgba8::new(255, 250, 224, 245)
                && stroke.points.len() == 5
    )));
}

#[test]
fn controller_active_audition_target_paints_over_cached_starmap_geometry() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let bounds = Rect::from_size(200.0, 100.0);
    let widget = StarmapWidget::new(
        vec![
            starmap_item("/samples/kick.wav", 0.25, 0.5, color),
            starmap_item("/samples/snare.wav", 0.75, 0.5, color),
        ],
        StarmapViewport::default(),
        None,
    )
    .with_active_audition_file_id(Some(String::from("/samples/snare.wav")));
    let mut primitives = Vec::new();

    widget.append_paint(
        &mut primitives,
        bounds,
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    assert!(primitives.iter().any(|primitive| matches!(
            primitive,
            PaintPrimitive::FillPolygon(fill)
                if fill.color == color.with_alpha(255)
                    && fill.points.len() == 4
                    && fill.points[0] == Point::new(150.0, 50.0 - (MAP_ACTIVE_AUDITION_SIZE + 2.0) * 0.5)
        )));
}

#[test]
fn starmap_widget_synchronizes_hover_and_hit_index_from_previous_instance() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let bounds = Rect::from_size(200.0, 100.0);
    let mut previous = StarmapWidget::new(
        vec![starmap_item("/samples/kick.wav", 0.25, 0.5, color)],
        StarmapViewport::default(),
        None,
    );
    assert_eq!(
        previous
            .handle_input(bounds, WidgetInput::pointer_move(Point::new(50.0, 50.0)))
            .and_then(|output| output.typed_cloned::<GuiMessage>()),
        None,
        "hover should update local runtime state without host output"
    );
    previous.ensure_hit_index(bounds);

    let mut next = StarmapWidget::new(
        vec![starmap_item("/samples/kick.wav", 0.25, 0.5, color)],
        StarmapViewport::default(),
        None,
    );
    next.synchronize_from_previous(&previous);

    assert_eq!(next.hovered_file_id.as_deref(), Some("/samples/kick.wav"));
    assert!(
        next.hit_index
            .matches(bounds, StarmapViewport::default(), next.item_signature())
    );
}

#[test]
fn starmap_widget_reuses_dense_base_paint_cache_from_previous_instance() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let bounds = Rect::from_size(640.0, 360.0);
    let items = (0..MAP_DENSE_ITEM_COUNT)
        .map(|index| {
            starmap_item(
                &format!("/samples/dense-{index}.wav"),
                (index % 100) as f32 / 100.0,
                (index / 100) as f32 / 10.0,
                color,
            )
        })
        .collect::<Vec<_>>();
    let previous = StarmapWidget::new(items.clone(), StarmapViewport::default(), None);
    let mut previous_primitives = Vec::new();
    previous.append_paint(
        &mut previous_primitives,
        bounds,
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );
    let previous_cached = lock_starmap_mutex(&previous.paint_cache)
        .entry
        .as_ref()
        .expect("initial paint should populate base paint cache")
        .primitives
        .clone();

    let mut next = StarmapWidget::new(
        items,
        StarmapViewport::default(),
        Some(StarmapAuditionDragState {
            last_hit_file_id: Some(String::from("/samples/dense-42.wav")),
            last_position: Point::new(100.0, 100.0),
            modifiers: PointerModifiers::default(),
        }),
    );
    next.synchronize_from_previous(&previous);
    let mut next_primitives = Vec::new();
    next.append_paint(
        &mut next_primitives,
        bounds,
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    let next_cached = lock_starmap_mutex(&next.paint_cache)
        .entry
        .as_ref()
        .expect("synchronized widget should retain base paint cache")
        .primitives
        .clone();
    assert!(
        Arc::ptr_eq(&previous_cached, &next_cached),
        "active drag refreshes should replay cached dense node paint instead of rebuilding it"
    );
    assert!(next_primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::FillRectBatch(batch) if batch.rects.len() == MAP_DENSE_ITEM_COUNT
    )));
}

#[test]
fn zoomed_out_dense_starmap_paints_bounded_overview_cells() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let bounds = Rect::from_size(1_000.0, 600.0);
    let items = dense_overview_test_items(MAP_DENSE_OVERVIEW_ITEM_COUNT + 900, color);
    let item_count = items.len();
    let widget = StarmapWidget::new(items, StarmapViewport::default(), None);
    let mut primitives = Vec::new();

    widget.append_paint(
        &mut primitives,
        bounds,
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    let overview_rect_count = primitives
        .iter()
        .filter_map(|primitive| match primitive {
            PaintPrimitive::FillRectBatch(batch) => Some(batch.rects.len()),
            _ => None,
        })
        .sum::<usize>();
    let cache = lock_starmap_mutex(&widget.paint_cache);
    let cached_cell_count = cache
        .dense_overview
        .as_ref()
        .map(|entry| entry.cells.len())
        .unwrap_or_default();

    assert!(
        overview_rect_count < item_count / 2,
        "fully zoomed-out dense maps should aggregate ordinary nodes instead of painting one rect per sample"
    );
    assert_eq!(overview_rect_count, cached_cell_count);
    assert!(
        cache.entry.is_none(),
        "dense overview paint should not churn exact-viewport primitive caches while panning"
    );
}

#[test]
fn dense_overview_cells_use_actual_item_centroids_not_grid_centers() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let items = vec![
        starmap_item("/samples/a.wav", 0.0973, 0.1947, color),
        starmap_item("/samples/b.wav", 0.0981, 0.1955, color),
    ];

    let (cells, exact_items) = build_dense_overview_paint(&items);

    assert!(exact_items.is_empty());
    assert_eq!(cells.len(), 1);
    assert!((cells[0].x - 0.0977).abs() < 0.0001);
    assert!((cells[0].y - 0.1951).abs() < 0.0001);
    assert!(
        (cells[0].x - (7.5 / MAP_DENSE_OVERVIEW_GRID_SIZE as f32)).abs() > 0.005,
        "dense overview should aggregate at the real local centroid instead of snapping to the grid center"
    );
}

#[test]
fn zoomed_out_dense_starmap_reuses_overview_cache_while_panning() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let bounds = Rect::from_size(1_000.0, 600.0);
    let items = Arc::<[StarmapItem]>::from(dense_overview_test_items(
        MAP_DENSE_OVERVIEW_ITEM_COUNT + 900,
        color,
    ));
    let previous = StarmapWidget::new(items.clone(), StarmapViewport::default(), None);
    let mut previous_primitives = Vec::new();
    previous.append_paint(
        &mut previous_primitives,
        bounds,
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );
    let previous_cells = lock_starmap_mutex(&previous.paint_cache)
        .dense_overview
        .as_ref()
        .expect("initial dense paint should cache overview cells")
        .cells
        .clone();
    let mut panned_viewport = StarmapViewport::default();
    panned_viewport.center_x = 0.58;
    panned_viewport.center_y = 0.42;
    let mut next = StarmapWidget::new(items, panned_viewport, None);
    next.synchronize_from_previous(&previous);
    let mut next_primitives = Vec::new();

    next.append_paint(
        &mut next_primitives,
        bounds,
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    let next_cells = lock_starmap_mutex(&next.paint_cache)
        .dense_overview
        .as_ref()
        .expect("panned dense paint should retain overview cells")
        .cells
        .clone();
    assert!(
        Arc::ptr_eq(&previous_cells, &next_cells),
        "panning a fully zoomed-out dense map should transform cached map-space overview cells instead of rebuilding from every item"
    );
    assert!(lock_starmap_mutex(&next.paint_cache).entry.is_none());
}

#[test]
fn zoomed_out_dense_starmap_keeps_selected_node_exact() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let bounds = Rect::from_size(1_000.0, 600.0);
    let mut items = dense_overview_test_items(MAP_DENSE_OVERVIEW_ITEM_COUNT + 900, color);
    items.push({
        let mut selected = starmap_item("/samples/selected.wav", 0.5, 0.5, color);
        selected.selected = true;
        selected
    });
    let widget = StarmapWidget::new(items, StarmapViewport::default(), None);
    let mut primitives = Vec::new();

    widget.append_paint(
        &mut primitives,
        bounds,
        &LayoutOutput::default(),
        &ThemeTokens::default(),
    );

    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        PaintPrimitive::FillPolygon(fill)
            if fill.color == color.with_alpha(255)
                && fill.points.len() == 4
                && fill.points[0] == Point::new(500.0, 300.0 - (MAP_SELECTED_SIZE + 2.0) * 0.5)
    )));
}

#[test]
fn starmap_widget_synchronizes_hit_scratch_from_previous_instance() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let bounds = Rect::from_size(200.0, 100.0);
    let mut previous = StarmapWidget::new(
        vec![starmap_item("/samples/kick.wav", 0.25, 0.5, color)],
        StarmapViewport::default(),
        None,
    );
    previous.handle_input(bounds, WidgetInput::pointer_move(Point::new(50.0, 50.0)));

    let mut next = StarmapWidget::new(
        vec![starmap_item("/samples/kick.wav", 0.25, 0.5, color)],
        StarmapViewport::default(),
        None,
    );
    next.synchronize_from_previous(&previous);

    assert!(
        Arc::ptr_eq(&previous.hit_scratch, &next.hit_scratch),
        "hit-test scratch should survive widget refreshes during dense drag playback"
    );
}

#[test]
fn starmap_widget_reuses_item_metadata_for_shared_item_arc() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let mut focused = starmap_item("/samples/focused.wav", 0.25, 0.5, color);
    focused.focused = true;
    let items = Arc::<[StarmapItem]>::from(vec![
        starmap_item("/samples/kick.wav", 0.10, 0.5, color),
        focused,
        starmap_item("/samples/snare.wav", 0.75, 0.5, color),
    ]);
    let previous = StarmapWidget::new(items.clone(), StarmapViewport::default(), None);
    assert_eq!(
        previous.focused_item().map(|item| item.file_id.as_str()),
        Some("/samples/focused.wav")
    );

    let mut next = StarmapWidget::new(
        items,
        StarmapViewport::default(),
        Some(StarmapAuditionDragState {
            last_hit_file_id: Some(String::from("/samples/snare.wav")),
            last_position: Point::new(150.0, 50.0),
            modifiers: PointerModifiers::default(),
        }),
    );
    next.synchronize_from_previous(&previous);

    assert!(
        Arc::ptr_eq(&previous.item_metadata, &next.item_metadata),
        "active drag widget refreshes should reuse starmap signature/focus metadata for the same prepared item Arc"
    );
    assert_eq!(next.item_signature(), previous.item_signature());
}

#[test]
fn starmap_widget_synchronizes_drag_hit_index_for_runtime_overlay() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let bounds = Rect::from_size(200.0, 100.0);
    let file_id = String::from("/samples/kick.wav");
    let mut previous = StarmapWidget::new(
        vec![starmap_item(file_id.as_str(), 0.25, 0.5, color)],
        StarmapViewport::default(),
        None,
    );
    previous.handle_input(bounds, WidgetInput::primary_press(Point::new(50.0, 50.0)));
    assert_eq!(previous.last_hit_index, Some(0));

    let mut next = StarmapWidget::new(
        vec![starmap_item(file_id.as_str(), 0.25, 0.5, color)],
        StarmapViewport::default(),
        Some(StarmapAuditionDragState {
            last_hit_file_id: Some(file_id.clone()),
            last_position: Point::new(50.0, 50.0),
            modifiers: PointerModifiers::default(),
        }),
    );
    next.synchronize_from_previous(&previous);

    assert_eq!(next.last_hit_index, Some(0));
    assert_eq!(
        next.active_drag_item().map(|item| item.file_id.as_str()),
        Some(file_id.as_str()),
        "runtime overlay paint should reuse the synchronized hit index for active drag nodes"
    );
}

#[test]
fn starmap_widget_sync_prefers_new_controller_drag_hit_over_stale_local_hit() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let left_id = String::from("/samples/kick.wav");
    let right_id = String::from("/samples/snare.wav");
    let mut previous = StarmapWidget::new(
        vec![
            starmap_item(left_id.as_str(), 0.25, 0.5, color),
            starmap_item(right_id.as_str(), 0.75, 0.5, color),
        ],
        StarmapViewport::default(),
        Some(StarmapAuditionDragState {
            last_hit_file_id: Some(left_id.clone()),
            last_position: Point::new(50.0, 50.0),
            modifiers: PointerModifiers::default(),
        }),
    );
    previous.last_hit_file_id = Some(left_id.clone());
    previous.last_hit_index = Some(0);

    let mut next = StarmapWidget::new(
        vec![
            starmap_item(left_id.as_str(), 0.25, 0.5, color),
            starmap_item(right_id.as_str(), 0.75, 0.5, color),
        ],
        StarmapViewport::default(),
        Some(StarmapAuditionDragState {
            last_hit_file_id: Some(right_id.clone()),
            last_position: Point::new(150.0, 50.0),
            modifiers: PointerModifiers::default(),
        }),
    );
    next.synchronize_from_previous(&previous);

    assert_eq!(
        next.active_drag_item().map(|item| item.file_id.as_str()),
        Some(right_id.as_str()),
        "a fresh controller drag target must not be overwritten by the previous widget-local hit"
    );
}

#[test]
fn starmap_widget_sync_preserves_local_hit_ahead_of_controller_refresh() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let left_id = String::from("/samples/kick.wav");
    let right_id = String::from("/samples/snare.wav");
    let bounds = Rect::from_size(200.0, 100.0);
    let mut previous = StarmapWidget::new(
        vec![
            starmap_item(left_id.as_str(), 0.25, 0.5, color),
            starmap_item(right_id.as_str(), 0.75, 0.5, color),
        ],
        StarmapViewport::default(),
        Some(StarmapAuditionDragState {
            last_hit_file_id: Some(left_id.clone()),
            last_position: Point::new(50.0, 50.0),
            modifiers: PointerModifiers::default(),
        }),
    );
    previous
        .handle_input(bounds, WidgetInput::pointer_move(Point::new(150.0, 50.0)))
        .expect("pointer move should emit drag update");

    let mut next = StarmapWidget::new(
        vec![
            starmap_item(left_id.as_str(), 0.25, 0.5, color),
            starmap_item(right_id.as_str(), 0.75, 0.5, color),
        ],
        StarmapViewport::default(),
        Some(StarmapAuditionDragState {
            last_hit_file_id: Some(left_id),
            last_position: Point::new(50.0, 50.0),
            modifiers: PointerModifiers::default(),
        }),
    );
    next.synchronize_from_previous(&previous);

    assert_eq!(
        next.active_drag_item().map(|item| item.file_id.as_str()),
        Some(right_id.as_str()),
        "widget-local hit testing should still drive live overlay paint before controller refresh catches up"
    );
}

#[test]
fn active_starmap_drag_lookup_recovers_from_stale_cached_index() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let target = String::from("/samples/target.wav");
    let mut items = vec![starmap_item("/samples/wrong.wav", 0.10, 0.10, color)];
    items.extend((0..MAP_DENSE_ITEM_COUNT).map(|index| {
        starmap_item(
            &format!("/samples/filler-{index}.wav"),
            0.15 + (index % 100) as f32 * 0.001,
            0.20 + (index / 100) as f32 * 0.001,
            color,
        )
    }));
    items.push(starmap_item(target.as_str(), 0.80, 0.80, color));
    let mut widget = StarmapWidget::new(
        items,
        StarmapViewport::default(),
        Some(StarmapAuditionDragState {
            last_hit_file_id: Some(target.clone()),
            last_position: Point::new(160.0, 80.0),
            modifiers: PointerModifiers::default(),
        }),
    );
    widget.last_hit_index = Some(0);

    assert_eq!(
        widget.active_drag_item().map(|item| item.file_id.as_str()),
        Some(target.as_str()),
        "active drag overlay lookup should use metadata when synchronized hit index is stale"
    );
}

#[test]
fn hovered_starmap_lookup_recovers_from_stale_cached_index() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let target = String::from("/samples/hovered.wav");
    let mut widget = StarmapWidget::new(
        vec![
            starmap_item("/samples/wrong.wav", 0.10, 0.10, color),
            starmap_item(target.as_str(), 0.80, 0.80, color),
        ],
        StarmapViewport::default(),
        None,
    );
    widget.hovered_file_id = Some(target.clone());
    widget.hovered_item_index = Some(0);

    assert_eq!(
        widget.hovered_item().map(|item| item.file_id.as_str()),
        Some(target.as_str()),
        "hover overlay lookup should use metadata when synchronized hover index is stale"
    );
}

#[test]
fn starmap_widget_rebuilds_hit_index_when_filtered_items_change_with_same_count() {
    let color = ui::Rgba8::new(57, 187, 245, 220);
    let bounds = Rect::from_size(200.0, 100.0);
    let mut previous = StarmapWidget::new(
        vec![starmap_item("/samples/kick.wav", 0.25, 0.5, color)],
        StarmapViewport::default(),
        None,
    );
    previous.ensure_hit_index(bounds);

    let mut next = StarmapWidget::new(
        vec![starmap_item("/samples/snare.wav", 0.75, 0.5, color)],
        StarmapViewport::default(),
        None,
    );
    next.synchronize_from_previous(&previous);
    assert!(
        !next
            .hit_index
            .matches(bounds, StarmapViewport::default(), next.item_signature()),
        "same-count filtered listings must not reuse stale node cells"
    );

    next.handle_input(bounds, WidgetInput::pointer_move(Point::new(150.0, 50.0)));

    assert_eq!(next.hovered_file_id.as_deref(), Some("/samples/snare.wav"));
    assert!(
        next.hit_index
            .matches(bounds, StarmapViewport::default(), next.item_signature())
    );
}

#[test]
fn dense_starmaps_use_smaller_node_sizes() {
    assert_eq!(map_node_size(10), MAP_NODE_SIZE);
    assert_eq!(map_node_size(MAP_DENSE_ITEM_COUNT), MAP_NODE_SIZE_DENSE);
    assert_eq!(
        map_node_size(MAP_VERY_DENSE_ITEM_COUNT),
        MAP_NODE_SIZE_VERY_DENSE
    );
}

#[test]
fn primary_drag_auditions_node_crossed_between_pointer_samples() {
    let mut widget = StarmapWidget::new(
        vec![starmap_item(
            "/samples/clap.wav",
            0.5,
            0.5,
            ui::Rgba8::new(255, 160, 80, 220),
        )],
        StarmapViewport::default(),
        None,
    );
    let bounds = Rect::from_size(200.0, 100.0);

    assert_eq!(
        widget
            .handle_input(bounds, WidgetInput::primary_press(Point::new(10.0, 50.0)))
            .and_then(|output| output.typed_cloned::<GuiMessage>()),
        Some(GuiMessage::BeginStarmapAuditionDrag {
            path: None,
            position: Point::new(10.0, 50.0),
            modifiers: PointerModifiers::default(),
        }),
        "press starts the drag even when it begins away from a node"
    );
    let output = widget
        .handle_input(bounds, WidgetInput::pointer_move(Point::new(190.0, 50.0)))
        .expect("swept drag should catch the crossed node");

    assert_eq!(
        output.typed_cloned::<GuiMessage>(),
        Some(GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![String::from("/samples/clap.wav")],
            position: Point::new(190.0, 50.0),
            modifiers: PointerModifiers::default(),
        })
    );
}

#[test]
fn primary_drag_auditions_nodes_crossed_between_pointer_samples_in_order() {
    let mut widget = StarmapWidget::new(
        vec![
            starmap_item(
                "/samples/kick.wav",
                0.25,
                0.5,
                ui::Rgba8::new(255, 160, 80, 220),
            ),
            starmap_item(
                "/samples/snare.wav",
                0.5,
                0.5,
                ui::Rgba8::new(57, 187, 245, 220),
            ),
            starmap_item(
                "/samples/hat.wav",
                0.75,
                0.5,
                ui::Rgba8::new(125, 220, 140, 220),
            ),
        ],
        StarmapViewport::default(),
        None,
    );
    let bounds = Rect::from_size(200.0, 100.0);

    widget
        .handle_input(bounds, WidgetInput::primary_press(Point::new(5.0, 50.0)))
        .expect("press starts audition drag");
    let output = widget
        .handle_input(bounds, WidgetInput::pointer_move(Point::new(195.0, 50.0)))
        .expect("swept drag should catch the latest crossed node");

    assert_eq!(
        output.typed_cloned::<GuiMessage>(),
        Some(GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![
                String::from("/samples/kick.wav"),
                String::from("/samples/snare.wav"),
                String::from("/samples/hat.wav")
            ],
            position: Point::new(195.0, 50.0),
            modifiers: PointerModifiers::default(),
        })
    );
}

#[test]
fn primary_drag_caps_dense_segment_handoff_to_latest_nodes() {
    let total = MAP_SEGMENT_HIT_HANDOFF_LIMIT + 8;
    let items = (0..total)
        .map(|index| {
            starmap_item(
                &format!("/samples/dense-{index}.wav"),
                (index + 1) as f32 / (total + 1) as f32,
                0.5,
                ui::Rgba8::new(57, 187, 245, 220),
            )
        })
        .collect::<Vec<_>>();
    let mut widget = StarmapWidget::new(items, StarmapViewport::default(), None);
    let bounds = Rect::from_size(1_000.0, 100.0);
    let hits = widget.hits_between(
        bounds,
        Point::new(0.0, 50.0),
        Point::new(1_000.0, 50.0),
        None,
    );
    assert_eq!(
        hits.raw_count, total,
        "test setup should cross every dense node"
    );
    assert_eq!(
        hits.retained.len(),
        MAP_SEGMENT_HIT_HANDOFF_LIMIT,
        "hit testing itself should retain only a bounded latest-node payload"
    );

    widget
        .handle_input(bounds, WidgetInput::primary_press(Point::new(0.0, 50.0)))
        .expect("press starts audition drag");
    let output = widget
        .handle_input(bounds, WidgetInput::pointer_move(Point::new(1_000.0, 50.0)))
        .expect("swept drag should catch the crossed nodes");
    let Some(GuiMessage::UpdateStarmapAuditionDrag { paths, .. }) =
        output.typed_cloned::<GuiMessage>()
    else {
        panic!("expected starmap audition update");
    };

    assert_eq!(
        paths.len(),
        MAP_SEGMENT_HIT_HANDOFF_LIMIT,
        "dense segment handoff should stay bounded on the UI path"
    );
    let expected_first = format!(
        "/samples/dense-{}.wav",
        total - MAP_SEGMENT_HIT_HANDOFF_LIMIT
    );
    let expected_last = format!("/samples/dense-{}.wav", total - 1);
    assert_eq!(
        paths.first().map(String::as_str),
        Some(expected_first.as_str())
    );
    assert_eq!(
        paths.last().map(String::as_str),
        Some(expected_last.as_str()),
        "the latest crossed node must always survive handoff capping"
    );
}

#[test]
fn primary_release_finishes_starmap_audition_drag() {
    let mut widget = StarmapWidget::new(
        vec![starmap_item(
            "/samples/kick.wav",
            0.25,
            0.5,
            ui::Rgba8::new(57, 187, 245, 220),
        )],
        StarmapViewport::default(),
        None,
    );
    let bounds = Rect::from_size(200.0, 100.0);

    widget
        .handle_input(bounds, WidgetInput::primary_press(Point::new(50.0, 50.0)))
        .expect("primary press starts audition drag");
    let output = widget
        .handle_input(bounds, WidgetInput::primary_release(Point::new(50.0, 50.0)))
        .expect("primary release finishes audition drag");

    assert_eq!(
        output.typed_cloned::<GuiMessage>(),
        Some(GuiMessage::FinishStarmapAuditionDrag)
    );
}

#[test]
/// Primary double-click on a node retriggers audition without resetting the viewport.
fn primary_click_then_double_click_retriggers_starmap_node_without_zooming() {
    let mut widget = StarmapWidget::new(
        vec![starmap_item(
            "/samples/kick.wav",
            0.25,
            0.5,
            ui::Rgba8::new(57, 187, 245, 220),
        )],
        StarmapViewport::default(),
        None,
    );
    let bounds = Rect::from_size(200.0, 100.0);
    let node_position = Point::new(50.0, 50.0);

    let first_click = widget
        .handle_input(bounds, WidgetInput::primary_press(node_position))
        .expect("primary press should audition the node")
        .typed_cloned::<GuiMessage>();
    let first_release = widget
        .handle_input(bounds, WidgetInput::primary_release(node_position))
        .expect("primary release should finish the audition click")
        .typed_cloned::<GuiMessage>();
    let double_click = widget
        .handle_input(bounds, WidgetInput::primary_double_click(node_position))
        .expect("primary double-click should retrigger the node")
        .typed_cloned::<GuiMessage>();

    assert_eq!(
        [first_click, first_release, double_click],
        [
            Some(GuiMessage::BeginStarmapAuditionDrag {
                path: Some(String::from("/samples/kick.wav")),
                position: node_position,
                modifiers: PointerModifiers::default(),
            }),
            Some(GuiMessage::FinishStarmapAuditionDrag),
            Some(GuiMessage::BeginStarmapAuditionDrag {
                path: Some(String::from("/samples/kick.wav")),
                position: node_position,
                modifiers: PointerModifiers::default(),
            }),
        ]
    );
}

#[test]
/// Primary double-click on empty map space behaves like an empty click, not zoom reset.
fn primary_double_click_empty_starmap_space_does_not_zoom_out() {
    let mut widget = StarmapWidget::new(
        vec![starmap_item(
            "/samples/kick.wav",
            0.25,
            0.5,
            ui::Rgba8::new(57, 187, 245, 220),
        )],
        StarmapViewport::default(),
        None,
    );
    let bounds = Rect::from_size(200.0, 100.0);
    let empty_position = Point::new(180.0, 20.0);
    let output = widget
        .handle_input(bounds, WidgetInput::primary_double_click(empty_position))
        .expect("primary double-click should be handled by the starmap");

    assert_eq!(
        output.typed_cloned::<GuiMessage>(),
        Some(GuiMessage::BeginStarmapAuditionDrag {
            path: None,
            position: empty_position,
            modifiers: PointerModifiers::default(),
        })
    );
}

#[test]
fn starmap_accepts_wheel_input_for_cursor_zoom() {
    let widget = StarmapWidget::new(
        vec![starmap_item(
            "/samples/kick.wav",
            0.25,
            0.5,
            ui::Rgba8::new(57, 187, 245, 220),
        )],
        StarmapViewport::default(),
        None,
    );

    assert!(
        widget.accepts_wheel_input(),
        "map widgets must opt into wheel routing before scroll fallback"
    );
}

#[test]
fn starmap_wheel_zooms_at_pointer_position() {
    let mut widget = StarmapWidget::new(
        vec![starmap_item(
            "/samples/kick.wav",
            0.25,
            0.5,
            ui::Rgba8::new(57, 187, 245, 220),
        )],
        StarmapViewport::default(),
        None,
    );
    let bounds = Rect::from_size(200.0, 100.0);

    let output = widget
        .handle_input(
            bounds,
            WidgetInput::plain_wheel(Point::new(50.0, 75.0), Vector2::new(0.0, -120.0)),
        )
        .expect("wheel over the map should emit a viewport zoom");

    assert_eq!(
        output.typed_cloned::<GuiMessage>(),
        Some(GuiMessage::ChangeStarmapViewport(
            StarmapViewportChange::Zoom {
                anchor: Vector2::new(0.25, 0.75),
                factor: 1.15,
            }
        ))
    );
}

#[test]
fn secondary_drag_pans_starmap() {
    let mut widget = StarmapWidget::new(
        vec![starmap_item(
            "/samples/kick.wav",
            0.25,
            0.5,
            ui::Rgba8::new(57, 187, 245, 220),
        )],
        StarmapViewport::default(),
        None,
    );
    let bounds = Rect::from_size(200.0, 100.0);

    assert!(
        widget
            .handle_input(
                bounds,
                WidgetInput::pointer_press(
                    Point::new(50.0, 40.0),
                    PointerButton::Secondary,
                    PointerModifiers::default(),
                ),
            )
            .is_none(),
        "secondary press should only arm panning"
    );
    let output = widget
        .handle_input(bounds, WidgetInput::pointer_move(Point::new(70.0, 25.0)))
        .expect("secondary drag should pan the map viewport");

    assert_eq!(
        output.typed_cloned::<GuiMessage>(),
        Some(GuiMessage::ChangeStarmapViewport(
            StarmapViewportChange::Pan {
                delta: Vector2::new(0.1, -0.15),
            }
        ))
    );
}

#[test]
fn secondary_release_does_not_finish_starmap_audition_drag() {
    let mut widget = StarmapWidget::new(
        vec![starmap_item(
            "/samples/kick.wav",
            0.25,
            0.5,
            ui::Rgba8::new(57, 187, 245, 220),
        )],
        StarmapViewport::default(),
        Some(StarmapAuditionDragState {
            last_hit_file_id: Some(String::from("/samples/kick.wav")),
            last_position: Point::new(50.0, 50.0),
            modifiers: PointerModifiers::default(),
        }),
    );
    let bounds = Rect::from_size(200.0, 100.0);

    assert!(
        widget
            .handle_input(
                bounds,
                WidgetInput::pointer_press(
                    Point::new(90.0, 50.0),
                    PointerButton::Secondary,
                    PointerModifiers::default(),
                ),
            )
            .is_none(),
        "secondary press only arms map panning"
    );
    assert!(
        widget
            .handle_input(
                bounds,
                WidgetInput::pointer_release(
                    Point::new(90.0, 50.0),
                    PointerButton::Secondary,
                    PointerModifiers::default(),
                ),
            )
            .is_none(),
        "secondary release must not finish the primary audition drag"
    );
}

#[test]
fn secondary_drop_does_not_finish_starmap_audition_drag() {
    let mut widget = StarmapWidget::new(
        vec![starmap_item(
            "/samples/kick.wav",
            0.25,
            0.5,
            ui::Rgba8::new(57, 187, 245, 220),
        )],
        StarmapViewport::default(),
        Some(StarmapAuditionDragState {
            last_hit_file_id: Some(String::from("/samples/kick.wav")),
            last_position: Point::new(50.0, 50.0),
            modifiers: PointerModifiers::default(),
        }),
    );
    let bounds = Rect::from_size(200.0, 100.0);

    assert!(
        widget
            .handle_input(
                bounds,
                WidgetInput::pointer_press(
                    Point::new(90.0, 50.0),
                    PointerButton::Secondary,
                    PointerModifiers::default(),
                ),
            )
            .is_none(),
        "secondary press should not emit a message"
    );
    assert!(
        widget
            .handle_input(
                bounds,
                WidgetInput::pointer_drop(
                    Point::new(90.0, 50.0),
                    PointerButton::Secondary,
                    PointerModifiers::default(),
                ),
            )
            .is_none(),
        "secondary drop must not finish the primary audition drag"
    );
}

#[test]
fn point_segment_distance_detects_crossed_node() {
    assert_eq!(
        point_segment_distance_squared(
            Point::new(100.0, 50.0),
            Point::new(10.0, 50.0),
            Point::new(190.0, 50.0),
        ),
        0.0
    );
}

#[test]
fn starmap_hit_index_limits_segment_candidates_to_nearby_cells() {
    let bounds = Rect::from_size(1_000.0, 1_000.0);
    let viewport = StarmapViewport::default();
    let mut items = Vec::new();
    for index in 0..2_000 {
        items.push(starmap_item(
            &format!("/samples/far-{index}.wav"),
            0.05 + (index % 20) as f32 * 0.001,
            0.05 + (index / 20) as f32 * 0.001,
            ui::Rgba8::new(255, 160, 80, 220),
        ));
    }
    items.push(starmap_item(
        "/samples/crossed.wav",
        0.75,
        0.75,
        ui::Rgba8::new(57, 187, 245, 220),
    ));

    let index = StarmapHitIndex::build(bounds, viewport, starmap_items_signature(&items), &items);
    let candidates = index.item_indices_near_segment(
        Point::new(720.0, 750.0),
        Point::new(780.0, 750.0),
        items.len(),
    );

    assert_eq!(candidates, vec![2_000]);
}

#[test]
fn starmap_hit_index_walks_diagonal_segments_without_scanning_bounding_box() {
    let bounds = Rect::from_size(1_000.0, 1_000.0);
    let viewport = StarmapViewport::default();
    let mut items = Vec::new();
    for index in 0..2_000 {
        items.push(starmap_item(
            &format!("/samples/off-diagonal-{index}.wav"),
            0.20 + (index % 40) as f32 * 0.002,
            0.80 + (index / 40) as f32 * 0.002,
            ui::Rgba8::new(255, 160, 80, 220),
        ));
    }
    items.push(starmap_item(
        "/samples/crossed.wav",
        0.50,
        0.50,
        ui::Rgba8::new(57, 187, 245, 220),
    ));

    let index = StarmapHitIndex::build(bounds, viewport, starmap_items_signature(&items), &items);
    let candidates = index.item_indices_near_segment(
        Point::new(100.0, 100.0),
        Point::new(900.0, 900.0),
        items.len(),
    );

    assert_eq!(
        candidates,
        vec![2_000],
        "diagonal drag sweeps should visit cells along the pointer path instead of every populated cell inside the path bounding box"
    );
}

fn dense_overview_test_items(count: usize, color: ui::Rgba8) -> Vec<StarmapItem> {
    (0..count)
        .map(|index| {
            starmap_item(
                &format!("/samples/dense-overview-{index}.wav"),
                ((index % 36) as f32 + 0.5) / 36.0,
                (((index / 36) % 36) as f32 + 0.5) / 36.0,
                color,
            )
        })
        .collect()
}

fn starmap_item(file_id: &str, x: f32, y: f32, color: ui::Rgba8) -> StarmapItem {
    StarmapItem {
        file_id: String::from(file_id),
        label: String::from(file_id),
        x,
        y,
        color,
        selected: false,
        focused: false,
        selection_flash: false,
        copy_flash: false,
        similarity_anchor: false,
        instant_audition_ready: true,
        preview_audition_ready: false,
        preview_audition_candidate: file_id.rsplit_once('.').is_some_and(|(_, extension)| {
            extension.eq_ignore_ascii_case("wav") || extension.eq_ignore_ascii_case("wave")
        }),
        missing: false,
    }
}
