use super::*;
#[derive(Clone, Copy, Debug)]
pub(super) struct StarmapSegmentHit {
    pub(super) item_index: usize,
    pub(super) segment_t: f32,
    pub(super) distance_sq: f32,
}

#[derive(Debug, Default)]
pub(super) struct StarmapSegmentHits {
    pub(super) raw_count: usize,
    pub(super) retained: Vec<StarmapSegmentHit>,
}

impl StarmapSegmentHits {
    pub(super) fn is_empty(&self) -> bool {
        self.retained.is_empty()
    }

    pub(super) fn last(&self) -> Option<&StarmapSegmentHit> {
        self.retained.last()
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &StarmapSegmentHit> {
        self.retained.iter()
    }

    pub(super) fn retain(&mut self, hit: StarmapSegmentHit) {
        self.raw_count += 1;
        if self.retained.len() < MAP_SEGMENT_HIT_HANDOFF_LIMIT {
            self.retained.push(hit);
            return;
        }
        let Some((worst_index, worst)) = self
            .retained
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| segment_hit_handoff_priority_order(left, right))
        else {
            return;
        };
        if segment_hit_handoff_priority_order(&hit, worst).is_gt() {
            self.retained[worst_index] = hit;
        }
    }

    pub(super) fn sort(&mut self) {
        self.retained.sort_by(segment_hit_display_order);
    }
}

fn segment_hit_display_order(
    left: &StarmapSegmentHit,
    right: &StarmapSegmentHit,
) -> std::cmp::Ordering {
    left.segment_t
        .total_cmp(&right.segment_t)
        .then_with(|| left.distance_sq.total_cmp(&right.distance_sq))
}

fn segment_hit_handoff_priority_order(
    left: &StarmapSegmentHit,
    right: &StarmapSegmentHit,
) -> std::cmp::Ordering {
    left.segment_t
        .total_cmp(&right.segment_t)
        .then_with(|| right.distance_sq.total_cmp(&left.distance_sq))
}

#[derive(Clone, Debug, Default)]
pub(super) struct StarmapHitIndex {
    pub(super) bounds: Option<Rect>,
    pub(super) viewport: Option<StarmapViewport>,
    pub(super) item_signature: u64,
    pub(super) cells: Arc<HashMap<StarmapGridCell, Vec<usize>>>,
}

impl StarmapHitIndex {
    pub(super) fn build(
        bounds: Rect,
        viewport: StarmapViewport,
        item_signature: u64,
        items: &[StarmapItem],
    ) -> Self {
        let mut cells = HashMap::<StarmapGridCell, Vec<usize>>::new();
        let indexed_bounds = paint_bounds(bounds).expanded(MAP_HIT_RADIUS);
        for (index, item) in items.iter().enumerate() {
            let center = item_center(bounds, item, viewport);
            if !indexed_bounds.contains(center) {
                continue;
            }
            cells
                .entry(StarmapGridCell::from_point(center))
                .or_default()
                .push(index);
        }
        Self {
            bounds: Some(bounds),
            viewport: Some(viewport),
            item_signature,
            cells: Arc::new(cells),
        }
    }

    pub(super) fn matches(
        &self,
        bounds: Rect,
        viewport: StarmapViewport,
        item_signature: u64,
    ) -> bool {
        self.bounds == Some(bounds)
            && self.viewport == Some(viewport)
            && self.item_signature == item_signature
    }

    pub(super) fn matches_current(&self, viewport: StarmapViewport, item_signature: u64) -> bool {
        self.bounds.is_some()
            && self.viewport == Some(viewport)
            && self.item_signature == item_signature
    }

    pub(super) fn collect_item_indices_near_point<'a>(
        &'a self,
        point: Point,
        item_count: usize,
        scratch: &'a mut StarmapHitScratch,
    ) -> &'a [usize] {
        self.collect_item_indices_for_rect(
            centered_rect(point, MAP_HIT_RADIUS * 2.0),
            item_count,
            scratch,
        )
    }

    pub(super) fn collect_item_indices_near_segment<'a>(
        &'a self,
        from: Point,
        to: Point,
        item_count: usize,
        scratch: &'a mut StarmapHitScratch,
    ) -> &'a [usize] {
        scratch.begin(item_count);
        let from = StarmapGridCell::from_point(from);
        let to = StarmapGridCell::from_point(to);
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let steps = dx.abs().max(dy.abs());
        if steps == 0 {
            self.collect_item_indices_near_cell(from, scratch);
            return scratch.indices.as_slice();
        }
        for step in 0..=steps {
            let t = step as f32 / steps as f32;
            let cell = StarmapGridCell {
                x: (from.x as f32 + dx as f32 * t).round() as i32,
                y: (from.y as f32 + dy as f32 * t).round() as i32,
            };
            self.collect_item_indices_near_cell(cell, scratch);
        }
        scratch.indices.as_slice()
    }

    pub(super) fn collect_item_indices_for_rect<'a>(
        &'a self,
        rect: Rect,
        item_count: usize,
        scratch: &'a mut StarmapHitScratch,
    ) -> &'a [usize] {
        scratch.begin(item_count);
        let min = StarmapGridCell::from_point(rect.min);
        let max = StarmapGridCell::from_point(rect.max);
        for y in min.y..=max.y {
            for x in min.x..=max.x {
                let Some(cell_indices) = self.cells.get(&StarmapGridCell { x, y }) else {
                    continue;
                };
                for &index in cell_indices {
                    if scratch.mark_seen(index) {
                        scratch.indices.push(index);
                    }
                }
            }
        }
        scratch.indices.as_slice()
    }

    pub(super) fn collect_item_indices_near_cell(
        &self,
        cell: StarmapGridCell,
        scratch: &mut StarmapHitScratch,
    ) {
        for y in cell.y - 1..=cell.y + 1 {
            for x in cell.x - 1..=cell.x + 1 {
                let cell = StarmapGridCell { x, y };
                if !scratch.mark_cell_seen(cell) {
                    continue;
                }
                let Some(cell_indices) = self.cells.get(&cell) else {
                    continue;
                };
                for &index in cell_indices {
                    if scratch.mark_seen(index) {
                        scratch.indices.push(index);
                    }
                }
            }
        }
    }

    #[cfg(test)]
    pub(super) fn item_indices_near_segment(
        &self,
        from: Point,
        to: Point,
        item_count: usize,
    ) -> Vec<usize> {
        let mut scratch = StarmapHitScratch::default();
        self.collect_item_indices_near_segment(from, to, item_count, &mut scratch)
            .to_vec()
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct StarmapHitScratch {
    pub(super) generation: u32,
    pub(super) seen_generation: Vec<u32>,
    pub(super) seen_cells: HashSet<StarmapGridCell>,
    pub(super) indices: Vec<usize>,
}

impl StarmapHitScratch {
    pub(super) fn begin(&mut self, item_count: usize) {
        self.indices.clear();
        self.seen_cells.clear();
        if self.seen_generation.len() < item_count {
            self.seen_generation.resize(item_count, 0);
        }
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.seen_generation.fill(0);
            self.generation = 1;
        }
    }

    pub(super) fn mark_seen(&mut self, index: usize) -> bool {
        let Some(seen_generation) = self.seen_generation.get_mut(index) else {
            return false;
        };
        if *seen_generation == self.generation {
            return false;
        }
        *seen_generation = self.generation;
        true
    }

    pub(super) fn mark_cell_seen(&mut self, cell: StarmapGridCell) -> bool {
        self.seen_cells.insert(cell)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct StarmapGridCell {
    pub(super) x: i32,
    pub(super) y: i32,
}

impl StarmapGridCell {
    pub(super) fn from_point(point: Point) -> Self {
        Self {
            x: grid_coordinate(point.x),
            y: grid_coordinate(point.y),
        }
    }
}

fn grid_coordinate(value: f32) -> i32 {
    (value / MAP_HIT_GRID_CELL_SIZE).floor() as i32
}

#[cfg(test)]
pub(super) fn starmap_items_signature(items: &[StarmapItem]) -> u64 {
    starmap_item_metadata(items).signatures.hit
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StarmapItemSignatures {
    pub(super) hit: u64,
    pub(super) paint: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct StarmapItemMetadata {
    pub(super) signatures: StarmapItemSignatures,
    pub(super) focused_item_index: Option<usize>,
    pub(super) item_indices: HashMap<String, usize>,
}

pub(super) fn starmap_item_metadata(items: &[StarmapItem]) -> StarmapItemMetadata {
    let mut hit_hasher = DefaultHasher::new();
    let mut paint_hasher = DefaultHasher::new();
    let mut item_indices = HashMap::with_capacity(items.len());
    items.len().hash(&mut hit_hasher);
    items.len().hash(&mut paint_hasher);
    let mut focused_item_index = None;
    for (index, item) in items.iter().enumerate() {
        item_indices.entry(item.file_id.clone()).or_insert(index);
        item.file_id.hash(&mut hit_hasher);
        item.x.to_bits().hash(&mut hit_hasher);
        item.y.to_bits().hash(&mut hit_hasher);

        item.file_id.hash(&mut paint_hasher);
        item.x.to_bits().hash(&mut paint_hasher);
        item.y.to_bits().hash(&mut paint_hasher);
        item.color.r.hash(&mut paint_hasher);
        item.color.g.hash(&mut paint_hasher);
        item.color.b.hash(&mut paint_hasher);
        item.color.a.hash(&mut paint_hasher);
        item.selected.hash(&mut paint_hasher);
        item.selection_flash.hash(&mut paint_hasher);
        item.copy_flash.hash(&mut paint_hasher);
        item.similarity_anchor.hash(&mut paint_hasher);
        item.instant_audition_ready.hash(&mut paint_hasher);
        item.preview_audition_ready.hash(&mut paint_hasher);
        item.preview_audition_candidate.hash(&mut paint_hasher);
        item.missing.hash(&mut paint_hasher);
        if item.focused && focused_item_index.is_none() {
            focused_item_index = Some(index);
        }
    }
    StarmapItemMetadata {
        signatures: StarmapItemSignatures {
            hit: hit_hasher.finish(),
            paint: paint_hasher.finish(),
        },
        focused_item_index,
        item_indices,
    }
}

pub(super) fn point_segment_t(point: Point, start: Point, end: Point) -> f32 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length_sq = dx * dx + dy * dy;
    if length_sq <= f32::EPSILON {
        return 1.0;
    }
    (((point.x - start.x) * dx + (point.y - start.y) * dy) / length_sq).clamp(0.0, 1.0)
}

pub(super) fn point_segment_distance_squared(point: Point, start: Point, end: Point) -> f32 {
    let t = point_segment_t(point, start, end);
    if (t - 1.0).abs() <= f32::EPSILON && distance_squared(start, end) <= f32::EPSILON {
        return distance_squared(point, start);
    }
    let closest = Point::new(
        start.x + (end.x - start.x) * t,
        start.y + (end.y - start.y) * t,
    );
    distance_squared(point, closest)
}
