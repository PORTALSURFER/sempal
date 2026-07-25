use radiant::{
    gui::types::{Point, Rect, Vector2},
    layout::LayoutOutput,
    prelude as ui,
    runtime::{
        PaintPrimitive, push_fill_polygon, push_fill_rect, push_fill_rect_batch,
        push_stroke_polyline,
    },
    theme::ThemeTokens,
    widgets::{
        CanvasGestureEvent, CanvasGestureState, PointerButton, PointerModifiers, TextInputMessage,
        Widget, WidgetCommon, WidgetInput, WidgetOutput, WidgetSizing,
    },
};
use std::{
    collections::{BTreeMap, HashMap, HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use crate::native_app::app::{
    GuiMessage, StarmapAuditionDragState, StarmapViewport, StarmapViewportChange,
};
use crate::native_app::app_chrome::palette::{ACCENT, TEXT_PRIMARY};
use crate::native_app::sample_library::folder_browser::commands::FolderBrowserMessage;
use crate::native_app::sample_library::folder_browser::starmap::{
    StarmapItem, StarmapStatus, starmap_cluster_palette_color,
};
use crate::native_app::starmap_audition_telemetry::{
    self as starmap_telemetry, StarmapAuditionCounter, StarmapAuditionDuration,
};
use crate::native_app::ui::ids as widget_ids;
use wavecrate::sample_sources::config::SimilarityAspectSettings;
use wavecrate_analysis::aspects::SimilarityAspect;

use super::similarity_aspect_color;

mod hit_targets;
mod overlays;

use hit_targets::*;
use overlays::*;

const MAP_MIN_HEIGHT: f32 = 240.0;
const MAP_NODE_SIZE: f32 = 6.8;
const MAP_NODE_SIZE_DENSE: f32 = 4.4;
const MAP_NODE_SIZE_VERY_DENSE: f32 = 3.2;
const MAP_SELECTED_SIZE: f32 = 9.0;
const MAP_SELECTED_GLOW_SIZE: f32 = 17.0;
const MAP_SELECTED_OUTER_GLOW_SIZE: f32 = 25.0;
const MAP_ANCHOR_SIZE: f32 = 12.0;
const MAP_ANCHOR_GLOW_SIZE: f32 = 22.0;
const MAP_ACTIVE_AUDITION_SIZE: f32 = 11.0;
const MAP_ACTIVE_AUDITION_GLOW_SIZE: f32 = 24.0;
const MAP_COLD_AUDITION_SIZE_PAD: f32 = 3.0;
const MAP_HOVER_SIZE: f32 = 8.0;
const MAP_HOVER_GLOW_SIZE: f32 = 16.0;
const MAP_HIT_RADIUS: f32 = 8.0;
const MAP_HIT_GRID_CELL_SIZE: f32 = MAP_HIT_RADIUS * 2.0;
const MAP_SEGMENT_HIT_HANDOFF_LIMIT: usize = 16;
const MAP_DENSE_ITEM_COUNT: usize = 1_000;
const MAP_VERY_DENSE_ITEM_COUNT: usize = 4_000;
const MAP_DENSE_OVERVIEW_ITEM_COUNT: usize = 3_000;
const MAP_DENSE_OVERVIEW_MAX_ZOOM: f32 = 1.35;
const MAP_DENSE_OVERVIEW_GRID_SIZE: i32 = 72;
const MAP_DENSE_OVERVIEW_NODE_SIZE: f32 = 3.6;
const MAP_DENSE_OVERVIEW_NODE_SIZE_MAX: f32 = 6.8;
const MAP_CONTROL_ICON_ENABLED_COLOR: ui::Rgba8 = TEXT_PRIMARY;
const MAP_CONTROL_ICON_ACTIVE_COLOR: ui::Rgba8 = ACCENT;
const MAP_CONTROL_ICON_TINTS: ui::SvgIconTintPalette = ui::SvgIconTintPalette::new(
    MAP_CONTROL_ICON_ENABLED_COLOR,
    MAP_CONTROL_ICON_ACTIVE_COLOR,
    MAP_CONTROL_ICON_ENABLED_COLOR,
);
const MAP_CONTROL_ZOOM_FACTOR: f32 = 1.35;
const MAP_CONTROL_ANCHOR: Vector2 = Vector2 { x: 0.5, y: 0.5 };
const MAP_LEGEND_SWATCH_SIZE: u8 = 7;

pub(super) fn starmap_view(
    items: impl Into<Arc<[StarmapItem]>>,
    viewport: StarmapViewport,
    name_filter: String,
    similarity_controls: &SimilarityAspectSettings,
    status: StarmapStatus,
    prep_running: bool,
    curation_mode_enabled: bool,
    active_drag: Option<StarmapAuditionDragState>,
    active_audition_file_id: Option<String>,
) -> ui::View<GuiMessage> {
    let items = items.into();
    let map = if items.is_empty() {
        ui::column([
            ui::text_line(starmap_empty_message(curation_mode_enabled, status), 23.0).muted_text(),
            ui::spacer().fill_height(),
        ])
        .spacing(0.0)
        .fill()
    } else {
        ui::custom_widget_direct(
            StarmapWidget::new(items, viewport, active_drag)
                .with_active_audition_file_id(active_audition_file_id),
        )
        .id(widget_ids::SAMPLE_BROWSER_MAP_ID)
        .height(MAP_MIN_HEIGHT)
        .fill()
    };
    ui::stack([
        map,
        starmap_search_overlay(name_filter),
        starmap_controls_overlay(),
        starmap_legend_overlay(similarity_controls, status),
        starmap_status_overlay(status, prep_running),
    ])
    .fill()
    .height(MAP_MIN_HEIGHT)
}

pub(in crate::native_app) fn paint_active_starmap_audition_overlay(
    primitives: &mut Vec<PaintPrimitive>,
    bounds: Rect,
    items: &[StarmapItem],
    viewport: StarmapViewport,
    active_file_id: &str,
) {
    let Some(item) = items.iter().find(|item| item.file_id == active_file_id) else {
        return;
    };
    let center = item_center(bounds, item, viewport);
    if paint_bounds(bounds).contains(center) {
        paint_active_audition_item(
            primitives,
            widget_ids::SAMPLE_BROWSER_MAP_ID,
            center,
            starmap_item_color(item),
        );
    }
}

#[derive(Clone, Debug)]
struct StarmapWidget {
    common: WidgetCommon,
    gesture: CanvasGestureState,
    items: Arc<[StarmapItem]>,
    viewport: StarmapViewport,
    last_hit_file_id: Option<String>,
    last_hit_index: Option<usize>,
    active_audition_file_id: Option<String>,
    last_primary_position: Option<Point>,
    last_pan_position: Option<Point>,
    active_drag: Option<StarmapAuditionDragState>,
    item_metadata: Arc<OnceLock<StarmapItemMetadata>>,
    hit_index: StarmapHitIndex,
    hit_scratch: Arc<Mutex<StarmapHitScratch>>,
    paint_cache: Arc<Mutex<StarmapPaintCache>>,
    hovered_file_id: Option<String>,
    hovered_item_index: Option<usize>,
}

impl StarmapWidget {
    fn new(
        items: impl Into<Arc<[StarmapItem]>>,
        viewport: StarmapViewport,
        active_drag: Option<StarmapAuditionDragState>,
    ) -> Self {
        let items = items.into();
        let common = WidgetCommon::new(
            widget_ids::SAMPLE_BROWSER_MAP_ID,
            WidgetSizing::new(
                Vector2::new(1.0, MAP_MIN_HEIGHT),
                Vector2::new(640.0, 360.0),
            ),
        )
        .with_pointer_focus()
        .without_default_chrome();
        Self {
            common,
            gesture: CanvasGestureState::new(),
            items,
            viewport,
            last_hit_file_id: None,
            last_hit_index: None,
            active_audition_file_id: None,
            last_primary_position: None,
            last_pan_position: None,
            active_drag,
            item_metadata: Arc::new(OnceLock::new()),
            hit_index: StarmapHitIndex::default(),
            hit_scratch: Arc::new(Mutex::new(StarmapHitScratch::default())),
            paint_cache: Arc::new(Mutex::new(StarmapPaintCache::default())),
            hovered_file_id: None,
            hovered_item_index: None,
        }
    }

    fn with_active_audition_file_id(mut self, file_id: Option<String>) -> Self {
        self.active_audition_file_id = file_id;
        self
    }

    fn begin_audition_drag_message(
        &mut self,
        bounds: Rect,
        point: Point,
        modifiers: PointerModifiers,
    ) -> Option<WidgetOutput> {
        let hit_started_at = starmap_telemetry::stage_timer();
        let hit_index = self.hit_item_index(bounds, point);
        let hit_elapsed = starmap_telemetry::elapsed_since(hit_started_at);
        if let Some(elapsed) = hit_elapsed {
            starmap_telemetry::record_duration(StarmapAuditionDuration::WidgetHitTest, elapsed);
        }
        let hit_file_id = hit_index.map(|index| self.items[index].file_id.clone());
        starmap_telemetry::record_event(
            Some(if hit_file_id.is_some() {
                StarmapAuditionCounter::WidgetPointHit
            } else {
                StarmapAuditionCounter::WidgetPointMiss
            }),
            "widget.point_hit_test",
            if hit_file_id.is_some() { "hit" } else { "miss" },
            hit_file_id.as_deref(),
            usize::from(hit_file_id.is_some()),
            0,
            false,
            hit_elapsed,
        );
        self.last_hit_file_id = hit_file_id.clone();
        self.last_hit_index = hit_index;
        self.last_primary_position = Some(point);
        Some(WidgetOutput::typed(GuiMessage::BeginStarmapAuditionDrag {
            path: hit_file_id,
            position: point,
            modifiers,
        }))
    }

    fn update_audition_drag_message(
        &mut self,
        bounds: Rect,
        point: Point,
        modifiers: PointerModifiers,
    ) -> Option<WidgetOutput> {
        let previous = self
            .active_drag
            .as_ref()
            .map(|drag| drag.last_position)
            .or(self.last_primary_position)
            .unwrap_or(point);
        let last_hit_file_id = self
            .active_drag
            .as_ref()
            .and_then(|drag| drag.last_hit_file_id.as_deref())
            .or(self.last_hit_file_id.as_deref())
            .map(str::to_owned);
        self.last_primary_position = Some(point);
        let hit_started_at = starmap_telemetry::stage_timer();
        let hits = self.hits_between(bounds, previous, point, last_hit_file_id.as_deref());
        let hit_elapsed = starmap_telemetry::elapsed_since(hit_started_at);
        if let Some(elapsed) = hit_elapsed {
            starmap_telemetry::record_duration(StarmapAuditionDuration::WidgetHitTest, elapsed);
        }
        if hits.is_empty() {
            starmap_telemetry::record_event(
                Some(StarmapAuditionCounter::WidgetSegmentMiss),
                "widget.segment_hit_test",
                "miss",
                None,
                0,
                0,
                last_hit_file_id.is_some(),
                hit_elapsed,
            );
            return None;
        }
        let Some(last_hit) = hits.last() else {
            return None;
        };
        let hit_file_id = self.items[last_hit.item_index].file_id.clone();
        let hit_item_index = last_hit.item_index;
        let hit_file_ids = hits
            .iter()
            .map(|hit| self.items[hit.item_index].file_id.clone())
            .collect::<Vec<_>>();
        starmap_telemetry::record_event(
            Some(StarmapAuditionCounter::WidgetSegmentHit),
            "widget.segment_hit_test",
            if hits.raw_count > MAP_SEGMENT_HIT_HANDOFF_LIMIT {
                "hit_capped"
            } else {
                "hit"
            },
            Some(hit_file_id.as_str()),
            hits.raw_count,
            hit_file_ids.len(),
            last_hit_file_id.is_some(),
            hit_elapsed,
        );
        if Some(&hit_file_id) == last_hit_file_id.as_ref() {
            return None;
        }
        self.last_hit_file_id = Some(hit_file_id.clone());
        self.last_hit_index = Some(hit_item_index);
        starmap_telemetry::record_event(
            None,
            "widget.drag_update",
            "hit_changed",
            Some(hit_file_id.as_str()),
            hit_file_ids.len(),
            0,
            true,
            None,
        );
        Some(WidgetOutput::typed(GuiMessage::UpdateStarmapAuditionDrag {
            paths: hit_file_ids,
            position: point,
            modifiers,
        }))
    }

    fn hit_item_index(&mut self, bounds: Rect, point: Point) -> Option<usize> {
        self.ensure_hit_index(bounds);
        let mut best: Option<(usize, f32)> = None;
        let mut hit_scratch = lock_starmap_mutex(&self.hit_scratch);
        let candidates = self.hit_index.collect_item_indices_near_point(
            point,
            self.items.len(),
            &mut hit_scratch,
        );
        for &index in candidates {
            let item = &self.items[index];
            let center = item_center(bounds, item, self.viewport);
            let distance_sq = distance_squared(center, point);
            if distance_sq > MAP_HIT_RADIUS * MAP_HIT_RADIUS {
                continue;
            }
            if best.is_none_or(|(_, best_distance)| distance_sq < best_distance) {
                best = Some((index, distance_sq));
            }
        }
        best.map(|(index, _)| index)
    }

    fn hits_between(
        &mut self,
        bounds: Rect,
        from: Point,
        to: Point,
        last_hit_file_id: Option<&str>,
    ) -> StarmapSegmentHits {
        self.ensure_hit_index(bounds);
        let mut hits = StarmapSegmentHits::default();
        let mut hit_scratch = lock_starmap_mutex(&self.hit_scratch);
        let candidates = self.hit_index.collect_item_indices_near_segment(
            from,
            to,
            self.items.len(),
            &mut hit_scratch,
        );
        for &index in candidates {
            let item = &self.items[index];
            if last_hit_file_id == Some(item.file_id.as_str()) {
                continue;
            }
            let center = item_center(bounds, item, self.viewport);
            let distance_sq = point_segment_distance_squared(center, from, to);
            if distance_sq > MAP_HIT_RADIUS * MAP_HIT_RADIUS {
                continue;
            }
            hits.retain(StarmapSegmentHit {
                item_index: index,
                segment_t: point_segment_t(center, from, to),
                distance_sq,
            });
        }
        hits.sort();
        hits
    }

    fn ensure_hit_index(&mut self, bounds: Rect) {
        let item_signature = self.item_signature();
        if self
            .hit_index
            .matches(bounds, self.viewport, item_signature)
        {
            return;
        }
        self.hit_index = StarmapHitIndex::build(bounds, self.viewport, item_signature, &self.items);
    }

    fn set_hovered_file_at(&mut self, bounds: Rect, point: Point) {
        let index = self.hit_item_index(bounds, point);
        self.hovered_item_index = index;
        self.hovered_file_id = index.map(|index| self.items[index].file_id.clone());
    }

    fn hovered_item(&self) -> Option<&StarmapItem> {
        let hovered_file_id = self.hovered_file_id.as_deref()?;
        self.item_for_cached_index(self.hovered_item_index, hovered_file_id)
            .or_else(|| self.item_for_file_id(hovered_file_id))
    }

    fn active_drag_item(&self) -> Option<&StarmapItem> {
        let active_file_id = self
            .last_hit_file_id
            .as_deref()
            .or_else(|| self.active_drag.as_ref()?.last_hit_file_id.as_deref())?;
        self.item_for_cached_index(self.last_hit_index, active_file_id)
            .or_else(|| self.item_for_file_id(active_file_id))
    }

    fn active_audition_item(&self) -> Option<&StarmapItem> {
        if self.active_drag.is_some() || self.last_primary_position.is_some() {
            return self.active_drag_item();
        }
        let active_file_id = self.active_audition_file_id.as_deref()?;
        self.item_for_file_id(active_file_id)
    }

    fn focused_item(&self) -> Option<&StarmapItem> {
        self.items.get(self.item_metadata().focused_item_index?)
    }

    fn item_for_cached_index(
        &self,
        index: Option<usize>,
        expected_file_id: &str,
    ) -> Option<&StarmapItem> {
        let item = self.items.get(index?)?;
        (item.file_id == expected_file_id).then_some(item)
    }

    fn item_for_file_id(&self, file_id: &str) -> Option<&StarmapItem> {
        let index = self.item_metadata().item_indices.get(file_id).copied();
        self.item_for_cached_index(index, file_id)
    }

    fn item_signature(&self) -> u64 {
        self.item_metadata().signatures.hit
    }

    fn paint_signature(&self) -> u64 {
        self.item_metadata().signatures.paint
    }

    fn item_metadata(&self) -> &StarmapItemMetadata {
        self.item_metadata
            .get_or_init(|| starmap_item_metadata(&self.items))
    }

    fn sync_drag_hit_from_previous(&mut self, previous: &Self) {
        let previous_model_hit = previous
            .active_drag
            .as_ref()
            .and_then(|drag| drag.last_hit_file_id.clone());
        let current_model_hit = self
            .active_drag
            .as_ref()
            .and_then(|drag| drag.last_hit_file_id.clone());
        let previous_local_hit = previous.last_hit_file_id.clone();
        let previous_local_is_newer = previous_local_hit.is_some()
            && previous_local_hit.as_deref() != previous_model_hit.as_deref();

        if previous_local_is_newer && current_model_hit.as_deref() == previous_model_hit.as_deref()
        {
            self.last_hit_file_id = previous.last_hit_file_id.clone();
            self.last_hit_index = previous.last_hit_index;
            return;
        }

        if let Some(current_hit) = current_model_hit {
            self.last_hit_index = self.item_metadata().item_indices.get(&current_hit).copied();
            self.last_hit_file_id = Some(current_hit);
            return;
        }

        self.last_hit_file_id = previous.last_hit_file_id.clone();
        self.last_hit_index = previous.last_hit_index;
    }
}

impl Widget for StarmapWidget {
    fn common(&self) -> &WidgetCommon {
        &self.common
    }

    fn common_mut(&mut self) -> &mut WidgetCommon {
        &mut self.common
    }

    fn handle_input(&mut self, bounds: Rect, input: WidgetInput) -> Option<WidgetOutput> {
        let event = self.gesture.handle_input(bounds, &input)?;
        match event {
            CanvasGestureEvent::Press {
                pointer,
                button: PointerButton::Primary,
                modifiers,
            } => self.begin_audition_drag_message(bounds, pointer.position, modifiers),
            CanvasGestureEvent::Drag {
                pointer,
                button: PointerButton::Primary,
                modifiers,
                ..
            } => self.update_audition_drag_message(bounds, pointer.position, modifiers),
            CanvasGestureEvent::Hover(pointer) if self.active_drag.is_some() => self
                .update_audition_drag_message(
                    bounds,
                    pointer.position,
                    self.active_drag
                        .as_ref()
                        .map(|drag| drag.modifiers)
                        .unwrap_or_default(),
                ),
            CanvasGestureEvent::Hover(pointer) => {
                self.set_hovered_file_at(bounds, pointer.position);
                None
            }
            CanvasGestureEvent::Press {
                pointer,
                button: PointerButton::Secondary,
                ..
            } => {
                self.last_pan_position = Some(pointer.position);
                None
            }
            CanvasGestureEvent::Drag {
                pointer,
                button: PointerButton::Secondary,
                ..
            } => {
                let previous = self.last_pan_position.replace(pointer.position)?;
                Some(WidgetOutput::typed(GuiMessage::ChangeStarmapViewport(
                    StarmapViewportChange::Pan {
                        delta: Vector2::new(
                            (pointer.position.x - previous.x) / bounds.width().max(1.0),
                            (pointer.position.y - previous.y) / bounds.height().max(1.0),
                        ),
                    },
                )))
            }
            CanvasGestureEvent::Wheel { pointer, delta } => {
                let factor = if delta.y < 0.0 { 1.15 } else { 1.0 / 1.15 };
                Some(WidgetOutput::typed(GuiMessage::ChangeStarmapViewport(
                    StarmapViewportChange::Zoom {
                        anchor: pointer.normalized,
                        factor,
                    },
                )))
            }
            CanvasGestureEvent::DoubleClick {
                pointer,
                button: PointerButton::Primary,
                modifiers,
            } => self.begin_audition_drag_message(bounds, pointer.position, modifiers),
            CanvasGestureEvent::DoubleClick { .. } => None,
            CanvasGestureEvent::Release {
                button: PointerButton::Primary,
                ..
            }
            | CanvasGestureEvent::Drop {
                button: PointerButton::Primary,
                ..
            } => {
                self.last_hit_file_id = None;
                self.last_primary_position = None;
                self.last_pan_position = None;
                Some(WidgetOutput::typed(GuiMessage::FinishStarmapAuditionDrag))
            }
            CanvasGestureEvent::Release {
                button: PointerButton::Secondary,
                ..
            }
            | CanvasGestureEvent::Drop {
                button: PointerButton::Secondary,
                ..
            } => {
                self.last_pan_position = None;
                None
            }
            CanvasGestureEvent::Release { .. } | CanvasGestureEvent::Drop { .. } => None,
            _ => None,
        }
    }

    fn synchronize_from_previous(&mut self, previous: &dyn Widget) {
        let Some(previous) = previous.as_any().downcast_ref::<Self>() else {
            return;
        };
        self.common.state = previous.common.state;
        self.gesture = previous.gesture.clone();
        if Arc::ptr_eq(&previous.items, &self.items) {
            self.item_metadata = previous.item_metadata.clone();
        }
        self.sync_drag_hit_from_previous(previous);
        self.last_primary_position = previous.last_primary_position;
        self.last_pan_position = previous.last_pan_position;
        self.hovered_file_id = previous.hovered_file_id.clone();
        self.hovered_item_index = previous.hovered_item_index;
        self.hit_scratch = previous.hit_scratch.clone();
        self.paint_cache = previous.paint_cache.clone();
        let item_signature = self.item_signature();
        if previous
            .hit_index
            .matches_current(self.viewport, item_signature)
        {
            self.hit_index = previous.hit_index.clone();
        }
    }

    fn accepts_pointer_move(&self) -> bool {
        true
    }

    fn accepts_wheel_input(&self) -> bool {
        true
    }

    fn prefers_pointer_move_paint_only(&self) -> bool {
        true
    }

    fn append_paint(
        &self,
        primitives: &mut Vec<PaintPrimitive>,
        bounds: Rect,
        _layout: &LayoutOutput,
        _theme: &ThemeTokens,
    ) {
        push_fill_rect(
            primitives,
            self.common.id,
            bounds,
            ui::Rgba8::new(21, 24, 25, 255),
        );
        append_cached_items_paint(
            primitives,
            self.common.id,
            bounds,
            &self.items,
            self.viewport,
            self.paint_signature(),
            &self.paint_cache,
        );
        self.append_active_audition_paint(primitives, bounds);
    }

    fn append_runtime_overlay_paint(
        &self,
        primitives: &mut Vec<PaintPrimitive>,
        bounds: Rect,
        _layout: &LayoutOutput,
        _theme: &ThemeTokens,
    ) {
        if self.active_audition_item().is_some() {
            self.append_active_audition_paint(primitives, bounds);
            return;
        }
        let Some(item) = self.hovered_item() else {
            let Some(item) = self.focused_item() else {
                return;
            };
            let center = item_center(bounds, item, self.viewport);
            if !paint_bounds(bounds).contains(center) {
                return;
            }
            paint_focused_item(primitives, self.common.id, center, starmap_item_color(item));
            return;
        };
        let center = item_center(bounds, item, self.viewport);
        if !paint_bounds(bounds).contains(center) {
            return;
        }
        paint_hover_item(primitives, self.common.id, center, starmap_item_color(item));
    }
}

impl StarmapWidget {
    fn append_active_audition_paint(&self, primitives: &mut Vec<PaintPrimitive>, bounds: Rect) {
        let Some(item) = self.active_audition_item() else {
            return;
        };
        let center = item_center(bounds, item, self.viewport);
        if paint_bounds(bounds).contains(center) {
            paint_active_audition_item(
                primitives,
                self.common.id,
                center,
                starmap_item_color(item),
            );
        }
    }
}

fn append_cached_items_paint(
    primitives: &mut Vec<PaintPrimitive>,
    widget_id: u64,
    bounds: Rect,
    items: &[StarmapItem],
    viewport: StarmapViewport,
    paint_signature: u64,
    cache: &Mutex<StarmapPaintCache>,
) {
    let started_at = starmap_telemetry::stage_timer();
    if should_use_dense_overview(items.len(), viewport) {
        let (cells, exact_items) = dense_overview_paint(cache, items, paint_signature);
        paint_dense_overview_items(
            primitives,
            widget_id,
            bounds,
            viewport,
            &cells,
            &exact_items,
        );
        let elapsed = starmap_telemetry::elapsed_since(started_at);
        if let Some(elapsed) = elapsed {
            starmap_telemetry::record_duration(StarmapAuditionDuration::WidgetPaintBuild, elapsed);
        }
        starmap_telemetry::record_event(
            None,
            "widget.paint_dense_overview",
            "painted",
            None,
            items.len(),
            cells.len() + exact_items.len(),
            false,
            elapsed,
        );
        return;
    }
    if let Some(cached) = cached_item_paint(cache, bounds, viewport, paint_signature) {
        starmap_telemetry::record_event(
            Some(StarmapAuditionCounter::WidgetPaintCacheHit),
            "widget.paint_cache",
            "hit",
            None,
            items.len(),
            cached.len(),
            false,
            starmap_telemetry::elapsed_since(started_at),
        );
        primitives.extend(cached.iter().cloned());
        return;
    }

    let mut item_primitives = Vec::new();
    paint_items(&mut item_primitives, widget_id, bounds, items, viewport);
    let elapsed = starmap_telemetry::elapsed_since(started_at);
    if let Some(elapsed) = elapsed {
        starmap_telemetry::record_duration(StarmapAuditionDuration::WidgetPaintBuild, elapsed);
    }
    let item_primitives = Arc::<[PaintPrimitive]>::from(item_primitives);
    store_cached_item_paint(
        cache,
        bounds,
        viewport,
        paint_signature,
        item_primitives.clone(),
    );
    starmap_telemetry::record_event(
        Some(StarmapAuditionCounter::WidgetPaintCacheMiss),
        "widget.paint_cache",
        "miss",
        None,
        items.len(),
        item_primitives.len(),
        false,
        elapsed,
    );
    primitives.extend(item_primitives.iter().cloned());
}

fn cached_item_paint(
    cache: &Mutex<StarmapPaintCache>,
    bounds: Rect,
    viewport: StarmapViewport,
    paint_signature: u64,
) -> Option<Arc<[PaintPrimitive]>> {
    lock_starmap_mutex(cache)
        .entry
        .as_ref()
        .filter(|entry| entry.matches(bounds, viewport, paint_signature))
        .map(|entry| entry.primitives.clone())
}

fn store_cached_item_paint(
    cache: &Mutex<StarmapPaintCache>,
    bounds: Rect,
    viewport: StarmapViewport,
    paint_signature: u64,
    primitives: Arc<[PaintPrimitive]>,
) {
    lock_starmap_mutex(cache).entry = Some(StarmapPaintCacheEntry {
        bounds,
        viewport,
        paint_signature,
        primitives,
    });
}

fn dense_overview_paint(
    cache: &Mutex<StarmapPaintCache>,
    items: &[StarmapItem],
    paint_signature: u64,
) -> (
    Arc<[StarmapDenseOverviewCell]>,
    Arc<[StarmapDenseOverviewExactItem]>,
) {
    if let Some((cells, exact_items)) = lock_starmap_mutex(cache)
        .dense_overview
        .as_ref()
        .filter(|entry| entry.paint_signature == paint_signature)
        .map(|entry| (entry.cells.clone(), entry.exact_items.clone()))
    {
        return (cells, exact_items);
    }

    let (cells, exact_items) = build_dense_overview_paint(items);
    let cells = Arc::<[StarmapDenseOverviewCell]>::from(cells);
    let exact_items = Arc::<[StarmapDenseOverviewExactItem]>::from(exact_items);
    lock_starmap_mutex(cache).dense_overview = Some(StarmapDenseOverviewCacheEntry {
        paint_signature,
        cells: cells.clone(),
        exact_items: exact_items.clone(),
    });
    (cells, exact_items)
}

fn lock_starmap_mutex<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Clone, Debug, Default)]
struct StarmapPaintCache {
    entry: Option<StarmapPaintCacheEntry>,
    dense_overview: Option<StarmapDenseOverviewCacheEntry>,
}

#[derive(Clone, Debug)]
struct StarmapPaintCacheEntry {
    bounds: Rect,
    viewport: StarmapViewport,
    paint_signature: u64,
    primitives: Arc<[PaintPrimitive]>,
}

impl StarmapPaintCacheEntry {
    fn matches(&self, bounds: Rect, viewport: StarmapViewport, paint_signature: u64) -> bool {
        self.bounds == bounds
            && self.viewport == viewport
            && self.paint_signature == paint_signature
    }
}

#[derive(Clone, Debug)]
struct StarmapDenseOverviewCacheEntry {
    paint_signature: u64,
    cells: Arc<[StarmapDenseOverviewCell]>,
    exact_items: Arc<[StarmapDenseOverviewExactItem]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct StarmapDenseOverviewCell {
    x: f32,
    y: f32,
    color: ui::Rgba8,
    side: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct StarmapDenseOverviewExactItem {
    x: f32,
    y: f32,
    color: ui::Rgba8,
    selected: bool,
    selection_flash: bool,
    copy_flash: bool,
    similarity_anchor: bool,
}

fn paint_items(
    primitives: &mut Vec<PaintPrimitive>,
    widget_id: u64,
    bounds: Rect,
    items: &[StarmapItem],
    viewport: StarmapViewport,
) {
    let node_size = map_node_size(items.len());
    let mut batches = BTreeMap::<ColorKey, Vec<Rect>>::new();
    for item in items {
        queue_or_paint_item(
            primitives,
            widget_id,
            bounds,
            item,
            viewport,
            node_size,
            &mut batches,
        );
    }
    for (color, rects) in batches {
        push_fill_rect_batch(primitives, widget_id, rects, color.rgba());
    }
}

fn should_use_dense_overview(item_count: usize, viewport: StarmapViewport) -> bool {
    item_count >= MAP_DENSE_OVERVIEW_ITEM_COUNT && viewport.zoom <= MAP_DENSE_OVERVIEW_MAX_ZOOM
}

fn build_dense_overview_paint(
    items: &[StarmapItem],
) -> (
    Vec<StarmapDenseOverviewCell>,
    Vec<StarmapDenseOverviewExactItem>,
) {
    let mut cells = BTreeMap::<StarmapDenseOverviewCellKey, StarmapDenseOverviewAccumulator>::new();
    let mut exact_items = Vec::new();
    for item in items {
        if item.selected || item.selection_flash || item.copy_flash || item.similarity_anchor {
            exact_items.push(StarmapDenseOverviewExactItem::from_item(item));
            continue;
        }
        let key = StarmapDenseOverviewCellKey::from_item(item);
        cells
            .entry(key)
            .or_default()
            .add(item, starmap_item_color(item));
    }
    let cells = cells
        .into_iter()
        .filter_map(|(key, accumulator)| accumulator.into_cell(key))
        .collect();
    (cells, exact_items)
}

fn paint_dense_overview_items(
    primitives: &mut Vec<PaintPrimitive>,
    widget_id: u64,
    bounds: Rect,
    viewport: StarmapViewport,
    cells: &[StarmapDenseOverviewCell],
    exact_items: &[StarmapDenseOverviewExactItem],
) {
    let mut batches = BTreeMap::<ColorKey, Vec<Rect>>::new();
    for cell in cells {
        let center = dense_overview_cell_center(bounds, *cell, viewport);
        if !paint_bounds(bounds).contains(center) {
            continue;
        }
        batches
            .entry(ColorKey::from(cell.color))
            .or_default()
            .push(centered_rect(center, cell.side));
    }
    for (color, rects) in batches {
        push_fill_rect_batch(primitives, widget_id, rects, color.rgba());
    }
    for item in exact_items {
        paint_dense_overview_exact_item(primitives, widget_id, bounds, item, viewport);
    }
}

fn paint_dense_overview_exact_item(
    primitives: &mut Vec<PaintPrimitive>,
    widget_id: u64,
    bounds: Rect,
    item: &StarmapDenseOverviewExactItem,
    viewport: StarmapViewport,
) {
    if !item.selected && !item.selection_flash && !item.copy_flash && !item.similarity_anchor {
        return;
    }
    let center = dense_overview_exact_item_center(bounds, *item, viewport);
    if !paint_bounds(bounds).contains(center) {
        return;
    }
    let color = item.color;
    if item.selection_flash {
        paint_selection_flash_item(primitives, widget_id, center);
    }
    if item.copy_flash {
        paint_copy_flash_item(primitives, widget_id, center, color);
    }
    if item.selected {
        paint_selected_item(primitives, widget_id, center, color);
        return;
    }
    if item.similarity_anchor {
        paint_similarity_anchor_item(primitives, widget_id, center, color);
    }
}

fn queue_or_paint_item(
    primitives: &mut Vec<PaintPrimitive>,
    widget_id: u64,
    bounds: Rect,
    item: &StarmapItem,
    viewport: StarmapViewport,
    node_size: f32,
    batches: &mut BTreeMap<ColorKey, Vec<Rect>>,
) {
    let center = item_center(bounds, item, viewport);
    if !paint_bounds(bounds).contains(center) {
        return;
    }
    let color = starmap_item_color(item);
    if item.selection_flash {
        paint_selection_flash_item(primitives, widget_id, center);
    }
    if item.copy_flash {
        paint_copy_flash_item(primitives, widget_id, center, color);
    }
    if item.selected {
        paint_selected_item(primitives, widget_id, center, color);
        return;
    }
    if item.similarity_anchor {
        paint_similarity_anchor_item(primitives, widget_id, center, color);
        return;
    }
    if !item.audition_candidate() {
        paint_cold_audition_item(primitives, widget_id, center, node_size, color);
        return;
    }
    batches
        .entry(ColorKey::from(color))
        .or_default()
        .push(centered_rect(center, node_size));
}

fn paint_cold_audition_item(
    primitives: &mut Vec<PaintPrimitive>,
    widget_id: u64,
    center: Point,
    node_size: f32,
    color: ui::Rgba8,
) {
    let side = node_size + MAP_COLD_AUDITION_SIZE_PAD;
    paint_diamond(primitives, widget_id, center, side, color.with_alpha(46));
    stroke_diamond(
        primitives,
        widget_id,
        center,
        side,
        ui::Rgba8::new(232, 236, 238, 165),
        1.0,
    );
}

fn paint_copy_flash_item(
    primitives: &mut Vec<PaintPrimitive>,
    widget_id: u64,
    center: Point,
    color: ui::Rgba8,
) {
    paint_diamond(
        primitives,
        widget_id,
        center,
        MAP_SELECTED_GLOW_SIZE + 6.0,
        color.with_alpha(78),
    );
    stroke_diamond(
        primitives,
        widget_id,
        center,
        MAP_SELECTED_SIZE + 7.0,
        ui::Rgba8::new(245, 245, 245, 235),
        1.25,
    );
}

fn paint_selection_flash_item(primitives: &mut Vec<PaintPrimitive>, widget_id: u64, center: Point) {
    let color = crate::native_app::app_chrome::palette::ACCENT;
    paint_diamond(
        primitives,
        widget_id,
        center,
        MAP_SELECTED_OUTER_GLOW_SIZE + 8.0,
        color.with_alpha(92),
    );
    stroke_diamond(
        primitives,
        widget_id,
        center,
        MAP_SELECTED_SIZE + 10.0,
        color.with_alpha(245),
        1.5,
    );
}

fn paint_selected_item(
    primitives: &mut Vec<PaintPrimitive>,
    widget_id: u64,
    center: Point,
    color: ui::Rgba8,
) {
    paint_diamond(
        primitives,
        widget_id,
        center,
        MAP_SELECTED_OUTER_GLOW_SIZE,
        color.with_alpha(64),
    );
    paint_diamond(
        primitives,
        widget_id,
        center,
        MAP_SELECTED_GLOW_SIZE,
        color.with_alpha(118),
    );
    paint_diamond(
        primitives,
        widget_id,
        center,
        MAP_SELECTED_SIZE + 2.0,
        color.with_alpha(255),
    );
    stroke_diamond(
        primitives,
        widget_id,
        center,
        MAP_SELECTED_SIZE + 6.0,
        ui::Rgba8::new(255, 252, 229, 245),
        1.35,
    );
    stroke_diamond(
        primitives,
        widget_id,
        center,
        MAP_SELECTED_SIZE + 1.5,
        ui::Rgba8::new(255, 255, 255, 210),
        0.85,
    );
}

fn paint_similarity_anchor_item(
    primitives: &mut Vec<PaintPrimitive>,
    widget_id: u64,
    center: Point,
    color: ui::Rgba8,
) {
    paint_diamond(
        primitives,
        widget_id,
        center,
        MAP_ANCHOR_GLOW_SIZE,
        color.with_alpha(42),
    );
    paint_diamond(primitives, widget_id, center, MAP_ANCHOR_SIZE, color);
    stroke_diamond(
        primitives,
        widget_id,
        center,
        MAP_ANCHOR_SIZE + 4.0,
        ui::Rgba8::new(245, 245, 245, 220),
        1.0,
    );
}

fn paint_hover_item(
    primitives: &mut Vec<PaintPrimitive>,
    widget_id: u64,
    center: Point,
    color: ui::Rgba8,
) {
    paint_diamond(
        primitives,
        widget_id,
        center,
        MAP_HOVER_GLOW_SIZE,
        color.with_alpha(50),
    );
    stroke_diamond(
        primitives,
        widget_id,
        center,
        MAP_HOVER_SIZE,
        ui::Rgba8::new(248, 248, 248, 230),
        1.0,
    );
}

fn paint_focused_item(
    primitives: &mut Vec<PaintPrimitive>,
    widget_id: u64,
    center: Point,
    color: ui::Rgba8,
) {
    paint_diamond(
        primitives,
        widget_id,
        center,
        MAP_HOVER_GLOW_SIZE,
        color.with_alpha(42),
    );
    stroke_diamond(
        primitives,
        widget_id,
        center,
        MAP_HOVER_SIZE,
        ui::Rgba8::new(248, 248, 248, 190),
        1.0,
    );
}

fn paint_active_audition_item(
    primitives: &mut Vec<PaintPrimitive>,
    widget_id: u64,
    center: Point,
    color: ui::Rgba8,
) {
    paint_diamond(
        primitives,
        widget_id,
        center,
        MAP_ACTIVE_AUDITION_GLOW_SIZE + 6.0,
        color.with_alpha(72),
    );
    paint_diamond(
        primitives,
        widget_id,
        center,
        MAP_ACTIVE_AUDITION_GLOW_SIZE,
        color.with_alpha(132),
    );
    paint_diamond(
        primitives,
        widget_id,
        center,
        MAP_ACTIVE_AUDITION_SIZE + 2.0,
        color.with_alpha(255),
    );
    stroke_diamond(
        primitives,
        widget_id,
        center,
        MAP_ACTIVE_AUDITION_SIZE + 7.0,
        ui::Rgba8::new(255, 250, 224, 245),
        1.45,
    );
    stroke_diamond(
        primitives,
        widget_id,
        center,
        MAP_ACTIVE_AUDITION_SIZE + 2.0,
        ui::Rgba8::new(255, 255, 255, 210),
        0.9,
    );
}

fn paint_diamond(
    primitives: &mut Vec<PaintPrimitive>,
    widget_id: u64,
    center: Point,
    side: f32,
    color: ui::Rgba8,
) {
    push_fill_polygon(primitives, widget_id, diamond_points(center, side), color);
}

fn stroke_diamond(
    primitives: &mut Vec<PaintPrimitive>,
    widget_id: u64,
    center: Point,
    side: f32,
    color: ui::Rgba8,
    width: f32,
) {
    push_stroke_polyline(
        primitives,
        widget_id,
        diamond_outline_points(center, side),
        color,
        width,
    );
}

fn starmap_item_color(item: &StarmapItem) -> ui::Rgba8 {
    if item.missing {
        ui::Rgba8::new(120, 120, 120, 180)
    } else if !item.audition_candidate() {
        item.color.with_alpha(item.color.a.min(150))
    } else if !item.fast_audition_ready() {
        item.color.with_alpha(item.color.a.min(205))
    } else {
        item.color
    }
}

fn map_node_size(item_count: usize) -> f32 {
    if item_count >= MAP_VERY_DENSE_ITEM_COUNT {
        MAP_NODE_SIZE_VERY_DENSE
    } else if item_count >= MAP_DENSE_ITEM_COUNT {
        MAP_NODE_SIZE_DENSE
    } else {
        MAP_NODE_SIZE
    }
}

fn item_center(bounds: Rect, item: &StarmapItem, viewport: StarmapViewport) -> Point {
    Point::new(
        bounds.x_for_ratio_unclamped((item.x - viewport.center_x) * viewport.zoom + 0.5),
        bounds.y_for_ratio_unclamped((item.y - viewport.center_y) * viewport.zoom + 0.5),
    )
}

fn paint_bounds(bounds: Rect) -> Rect {
    let margin = MAP_SELECTED_OUTER_GLOW_SIZE * 0.5;
    Rect::from_min_max(
        Point::new(bounds.min.x - margin, bounds.min.y - margin),
        Point::new(bounds.max.x + margin, bounds.max.y + margin),
    )
}

fn centered_rect(center: Point, side: f32) -> Rect {
    Rect::from_xy_size(center.x - side * 0.5, center.y - side * 0.5, side, side)
}

fn dense_overview_cell_center(
    bounds: Rect,
    cell: StarmapDenseOverviewCell,
    viewport: StarmapViewport,
) -> Point {
    Point::new(
        bounds.x_for_ratio_unclamped((cell.x - viewport.center_x) * viewport.zoom + 0.5),
        bounds.y_for_ratio_unclamped((cell.y - viewport.center_y) * viewport.zoom + 0.5),
    )
}

fn dense_overview_exact_item_center(
    bounds: Rect,
    item: StarmapDenseOverviewExactItem,
    viewport: StarmapViewport,
) -> Point {
    Point::new(
        bounds.x_for_ratio_unclamped((item.x - viewport.center_x) * viewport.zoom + 0.5),
        bounds.y_for_ratio_unclamped((item.y - viewport.center_y) * viewport.zoom + 0.5),
    )
}

impl StarmapDenseOverviewExactItem {
    fn from_item(item: &StarmapItem) -> Self {
        Self {
            x: item.x,
            y: item.y,
            color: starmap_item_color(item),
            selected: item.selected,
            selection_flash: item.selection_flash,
            copy_flash: item.copy_flash,
            similarity_anchor: item.similarity_anchor,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct StarmapDenseOverviewCellKey {
    x: i32,
    y: i32,
}

impl StarmapDenseOverviewCellKey {
    fn from_item(item: &StarmapItem) -> Self {
        Self {
            x: dense_overview_coordinate(item.x),
            y: dense_overview_coordinate(item.y),
        }
    }

    fn center(self) -> (f32, f32) {
        let grid = MAP_DENSE_OVERVIEW_GRID_SIZE as f32;
        ((self.x as f32 + 0.5) / grid, (self.y as f32 + 0.5) / grid)
    }
}

fn dense_overview_coordinate(value: f32) -> i32 {
    let max = MAP_DENSE_OVERVIEW_GRID_SIZE - 1;
    ((value.clamp(0.0, 0.999_999) * MAP_DENSE_OVERVIEW_GRID_SIZE as f32).floor() as i32)
        .clamp(0, max)
}

#[derive(Clone, Copy, Debug, Default)]
struct StarmapDenseOverviewAccumulator {
    count: u32,
    x_sum: f32,
    y_sum: f32,
    r_sum: u32,
    g_sum: u32,
    b_sum: u32,
    a_sum: u32,
}

impl StarmapDenseOverviewAccumulator {
    fn add(&mut self, item: &StarmapItem, color: ui::Rgba8) {
        self.count += 1;
        self.x_sum += item.x;
        self.y_sum += item.y;
        self.r_sum += u32::from(color.r);
        self.g_sum += u32::from(color.g);
        self.b_sum += u32::from(color.b);
        self.a_sum += u32::from(color.a);
    }

    fn into_cell(self, key: StarmapDenseOverviewCellKey) -> Option<StarmapDenseOverviewCell> {
        if self.count == 0 {
            return None;
        }
        let (fallback_x, fallback_y) = key.center();
        Some(StarmapDenseOverviewCell {
            x: self.average_coordinate(self.x_sum, fallback_x),
            y: self.average_coordinate(self.y_sum, fallback_y),
            color: self.quantized_color(),
            side: self.side(),
        })
    }

    fn average_coordinate(self, sum: f32, fallback: f32) -> f32 {
        if !sum.is_finite() {
            return fallback;
        }
        (sum / self.count as f32).clamp(0.0, 1.0)
    }

    fn quantized_color(self) -> ui::Rgba8 {
        ui::Rgba8::new(
            quantized_average_channel(self.r_sum, self.count),
            quantized_average_channel(self.g_sum, self.count),
            quantized_average_channel(self.b_sum, self.count),
            quantized_average_alpha(self.a_sum, self.count),
        )
    }

    fn side(self) -> f32 {
        let density = (self.count as f32).log2().max(0.0);
        (MAP_DENSE_OVERVIEW_NODE_SIZE + density * 0.55).min(MAP_DENSE_OVERVIEW_NODE_SIZE_MAX)
    }
}

fn quantized_average_channel(sum: u32, count: u32) -> u8 {
    let average = ((sum + count / 2) / count).min(u32::from(u8::MAX)) as u8;
    ((((u32::from(average) + 6) / 12) * 12).min(u32::from(u8::MAX))) as u8
}

fn quantized_average_alpha(sum: u32, count: u32) -> u8 {
    let average = ((sum + count / 2) / count).min(u32::from(u8::MAX)) as u8;
    ((((u32::from(average) + 6) / 12) * 12).min(u32::from(u8::MAX)) as u8).max(96)
}

fn diamond_points(center: Point, side: f32) -> [Point; 4] {
    let radius = side * 0.5;
    [
        Point::new(center.x, center.y - radius),
        Point::new(center.x + radius, center.y),
        Point::new(center.x, center.y + radius),
        Point::new(center.x - radius, center.y),
    ]
}

fn diamond_outline_points(center: Point, side: f32) -> [Point; 5] {
    let [top, right, bottom, left] = diamond_points(center, side);
    [top, right, bottom, left, top]
}

trait StarmapRectExt {
    fn expanded(self, padding: f32) -> Rect;
}

impl StarmapRectExt for Rect {
    fn expanded(self, padding: f32) -> Rect {
        Rect::from_min_max(
            Point::new(self.min.x - padding, self.min.y - padding),
            Point::new(self.max.x + padding, self.max.y + padding),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ColorKey {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl ColorKey {
    fn rgba(self) -> ui::Rgba8 {
        ui::Rgba8::new(self.r, self.g, self.b, self.a)
    }
}

impl From<ui::Rgba8> for ColorKey {
    fn from(color: ui::Rgba8) -> Self {
        Self {
            r: color.r,
            g: color.g,
            b: color.b,
            a: color.a,
        }
    }
}

fn distance_squared(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
}

#[cfg(test)]
mod tests;
