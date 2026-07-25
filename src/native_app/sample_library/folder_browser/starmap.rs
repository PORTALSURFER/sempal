use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use radiant::prelude as ui;
use radiant::widgets::PointerModifiers;
use wavecrate::sample_sources::{StarmapLayoutLoadRequest, StarmapLayoutLoadResult};

use crate::native_app::waveform::should_use_file_backed_wav_decode_for_entry;

use super::{FileEntry, FolderBrowserState};

mod layout_query;
mod navigation;
mod projection_index;
mod projection_shaping;

use layout_query::starmap_layout_signature;
use navigation::{find_starmap_item_by_file_id, starmap_navigation_target};
#[cfg(test)]
use projection_index::STARMAP_PROJECTION_INDEX_GRID;
use projection_index::StarmapProjectionIndex;
pub(in crate::native_app) use projection_shaping::starmap_cluster_palette_color;
use projection_shaping::{
    STARMAP_CLUSTER_PALETTE, starmap_color, starmap_position, strongest_enabled_aspect,
};

#[derive(Clone, Copy)]
pub(in crate::native_app) struct StarmapProjection<'a> {
    pub(in crate::native_app) tags_by_file: &'a HashMap<String, Vec<String>>,
    pub(in crate::native_app) instant_audition_sample_paths: &'a HashSet<String>,
    pub(in crate::native_app) preview_audition_sample_paths: &'a HashSet<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::native_app) struct StarmapStatus {
    pub(in crate::native_app) listed_count: usize,
    pub(in crate::native_app) layout_count: usize,
    pub(in crate::native_app) clustered_count: usize,
    pub(in crate::native_app) cluster_color_count: usize,
}

impl StarmapStatus {
    pub(in crate::native_app) fn incomplete(self) -> bool {
        self.listed_count > 0 && self.layout_count < self.listed_count
    }

    pub(in crate::native_app) fn label(self, prep_running: bool) -> Option<String> {
        if !self.incomplete() {
            return None;
        }
        if prep_running {
            return Some(format!(
                "Preparing Starmap {} / {}",
                self.layout_count, self.listed_count
            ));
        }
        Some(format!(
            "Starmap {} / {}",
            self.layout_count, self.listed_count
        ))
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct StarmapLayoutCache {
    signature: Option<u64>,
    pub(super) points_by_file: HashMap<String, StarmapLayoutPoint>,
    pub(super) projection_items: Option<Arc<[StarmapItem]>>,
    projection_index: StarmapProjectionIndex,
    listed_count: usize,
    pending_load_signature: Option<u64>,
    loaded_signature: Option<u64>,
}

#[derive(Debug)]
pub(in crate::native_app) struct StarmapWarmCandidateSet {
    pub(in crate::native_app) items: Arc<[StarmapItem]>,
    pub(in crate::native_app) indices: Vec<usize>,
    pub(in crate::native_app) inspected_count: usize,
    pub(in crate::native_app) cell_count: usize,
    pub(in crate::native_app) visited_cell_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct StarmapLayoutPoint {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) cluster_id: Option<i32>,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct StarmapItem {
    pub(in crate::native_app) file_id: String,
    pub(in crate::native_app) label: String,
    pub(in crate::native_app) x: f32,
    pub(in crate::native_app) y: f32,
    pub(in crate::native_app) color: ui::Rgba8,
    pub(in crate::native_app) selected: bool,
    pub(in crate::native_app) focused: bool,
    pub(in crate::native_app) selection_flash: bool,
    pub(in crate::native_app) copy_flash: bool,
    pub(in crate::native_app) similarity_anchor: bool,
    pub(in crate::native_app) instant_audition_ready: bool,
    pub(in crate::native_app) preview_audition_ready: bool,
    pub(in crate::native_app) preview_audition_candidate: bool,
    pub(in crate::native_app) missing: bool,
}

impl StarmapItem {
    pub(in crate::native_app) fn fast_audition_ready(&self) -> bool {
        self.instant_audition_ready || self.preview_audition_ready
    }

    pub(in crate::native_app) fn audition_candidate(&self) -> bool {
        self.fast_audition_ready() || self.preview_audition_candidate
    }
}

impl FolderBrowserState {
    pub(in crate::native_app) fn prepare_starmap_layout(
        &mut self,
        tags_by_file: &HashMap<String, Vec<String>>,
    ) {
        let snapshot = self.browser_listing_snapshot(tags_by_file);
        let signature = starmap_layout_signature(self.selected_source_id(), snapshot.rows());
        if self.sample_list.starmap_layout.signature == Some(signature) {
            return;
        }
        self.sample_list.starmap_layout = StarmapLayoutCache {
            signature: Some(signature),
            listed_count: snapshot.rows().len(),
            ..StarmapLayoutCache::default()
        };
    }

    #[cfg(test)]
    pub(in crate::native_app) fn invalidate_starmap_layout(&mut self) {
        self.sample_list.starmap_layout = StarmapLayoutCache::default();
    }

    pub(in crate::native_app) fn take_starmap_layout_load_request(
        &mut self,
        tags_by_file: &HashMap<String, Vec<String>>,
    ) -> Option<StarmapLayoutLoadRequest> {
        if !self.starmap_layout_load_may_need_request() {
            return None;
        }
        let (request, listed_count) = self.starmap_layout_load_request(tags_by_file);
        let signature = request.signature;
        if self.sample_list.starmap_layout.signature != Some(signature) {
            self.sample_list.starmap_layout = StarmapLayoutCache {
                signature: Some(signature),
                listed_count,
                ..StarmapLayoutCache::default()
            };
        }
        let cache = &mut self.sample_list.starmap_layout;
        if cache.pending_load_signature == Some(signature)
            || cache.loaded_signature == Some(signature)
            || request.is_empty()
        {
            if request.is_empty() {
                cache.loaded_signature = Some(signature);
            }
            return None;
        }
        cache.pending_load_signature = Some(signature);
        Some(request)
    }

    pub(in crate::native_app) fn starmap_layout_load_may_need_request(&self) -> bool {
        let cache = &self.sample_list.starmap_layout;
        let Some(signature) = cache.signature else {
            return true;
        };
        cache.pending_load_signature != Some(signature) && cache.loaded_signature != Some(signature)
    }

    pub(in crate::native_app) fn apply_starmap_layout_load_result(
        &mut self,
        result: StarmapLayoutLoadResult,
    ) {
        let cache = &mut self.sample_list.starmap_layout;
        if cache.signature != Some(result.signature) {
            return;
        }
        cache.pending_load_signature = None;
        cache.loaded_signature = Some(result.signature);
        match result.result {
            Ok(points) => {
                cache.points_by_file = points
                    .into_iter()
                    .map(|(file_id, point)| {
                        (
                            file_id,
                            StarmapLayoutPoint {
                                x: point.x,
                                y: point.y,
                                cluster_id: point.cluster_id,
                            },
                        )
                    })
                    .collect();
            }
            Err(error) => {
                tracing::debug!(%error, "starmap layout unavailable");
                cache.points_by_file.clear();
            }
        }
    }

    pub(in crate::native_app) fn starmap_projection(
        &self,
        projection: StarmapProjection<'_>,
    ) -> Vec<StarmapItem> {
        self.build_starmap_projection(projection)
    }

    pub(in crate::native_app) fn prepare_starmap_projection(
        &mut self,
        projection: StarmapProjection<'_>,
    ) {
        let items = Arc::<[StarmapItem]>::from(self.build_starmap_projection(projection));
        self.sample_list.starmap_layout.projection_index = StarmapProjectionIndex::build(&items);
        self.sample_list.starmap_layout.projection_items = Some(items);
    }

    pub(in crate::native_app) fn cached_starmap_projection(&self) -> Option<Arc<[StarmapItem]>> {
        self.sample_list.starmap_layout.projection_items.clone()
    }

    pub(in crate::native_app) fn starmap_projection_prepared(&self) -> bool {
        self.sample_list.starmap_layout.projection_items.is_some()
    }

    pub(in crate::native_app) fn cached_starmap_projection_len(&self) -> usize {
        self.sample_list
            .starmap_layout
            .projection_items
            .as_ref()
            .map_or(0, |items| items.len())
    }

    pub(in crate::native_app) fn cached_starmap_preview_warm_candidates(
        &self,
        center_x: f32,
        center_y: f32,
        zoom: f32,
        viewport_pad: f32,
        selected_file_id: Option<&str>,
        limit: usize,
    ) -> Option<StarmapWarmCandidateSet> {
        let cache = &self.sample_list.starmap_layout;
        let items = cache.projection_items.clone()?;
        let scan = cache.projection_index.preview_warm_indices(
            &items,
            center_x,
            center_y,
            zoom,
            viewport_pad,
            selected_file_id,
            limit,
        );
        Some(StarmapWarmCandidateSet {
            items,
            indices: scan.indices,
            inspected_count: scan.inspected_count,
            cell_count: scan.cell_count,
            visited_cell_count: scan.visited_cell_count,
        })
    }

    fn build_starmap_projection(&self, projection: StarmapProjection<'_>) -> Vec<StarmapItem> {
        let snapshot = self.browser_listing_snapshot(projection.tags_by_file);
        let focused_file_id = self.selected_file_id();
        snapshot
            .rows()
            .iter()
            .filter_map(|file| {
                let layout_point = self
                    .sample_list
                    .starmap_layout
                    .points_by_file
                    .get(&file.id)
                    .copied()?;
                let instant_audition_ready = instant_audition_ready_for_starmap(
                    file,
                    projection.instant_audition_sample_paths,
                );
                let preview_audition_ready =
                    projection.preview_audition_sample_paths.contains(&file.id);
                let aspects = self.similarity_aspect_display_strengths_for_file(&file.id);
                let strength = self.similarity_display_strength_for_file(&file.id);
                let group = strongest_enabled_aspect(&aspects, self.similarity_controls());
                let (x, y) = starmap_position(layout_point);
                Some(StarmapItem {
                    file_id: file.id.clone(),
                    label: file.stem.clone(),
                    x,
                    y,
                    color: starmap_color(group, strength, Some(layout_point)),
                    selected: self.is_file_selected(&file.id),
                    focused: focused_file_id == Some(file.id.as_str()),
                    selection_flash: self.marked_item_flash_active(&file.id),
                    copy_flash: self.copied_file_flash_active(&file.id),
                    similarity_anchor: self.file_is_similarity_anchor(&file.id),
                    instant_audition_ready,
                    preview_audition_ready,
                    preview_audition_candidate: preview_audition_candidate_for_starmap(file),
                    missing: file.is_missing(),
                })
            })
            .collect()
    }

    pub(in crate::native_app) fn selected_starmap_position(
        &self,
        projection: StarmapProjection<'_>,
    ) -> Option<(f32, f32)> {
        let selected_file = self.selected_file_id()?;
        if let Some(items) = self.cached_starmap_projection()
            && let Some(item) = find_starmap_item_by_file_id(&items, selected_file)
        {
            return Some((item.x, item.y));
        }
        self.starmap_projection(projection)
            .into_iter()
            .find(|item| item.file_id == selected_file)
            .map(|item| (item.x, item.y))
    }

    pub(in crate::native_app) fn navigate_starmap_matching_tags(
        &mut self,
        delta: i32,
        extend: bool,
        tags_by_file: &HashMap<String, Vec<String>>,
        instant_audition_sample_paths: &HashSet<String>,
    ) -> Option<String> {
        if delta == 0 || self.rename_active() || !self.selection.selected_file_active() {
            return None;
        }
        self.prepare_starmap_layout(tags_by_file);
        let cached_projection = self.cached_starmap_projection();
        let built_projection;
        let projection_items = if let Some(items) = cached_projection.as_deref() {
            items
        } else {
            built_projection = self.starmap_projection(StarmapProjection {
                tags_by_file,
                instant_audition_sample_paths,
                preview_audition_sample_paths: &HashSet::new(),
            });
            &built_projection
        };
        let target =
            starmap_navigation_target(projection_items, self.selection.selected_file_id()?, delta)?;
        let visible_ids = self.browser_listing_snapshot(tags_by_file).ids().to_vec();
        if extend {
            self.selection.select_file_with_modifiers(
                target.clone(),
                &visible_ids,
                PointerModifiers {
                    shift: true,
                    ..PointerModifiers::default()
                },
            );
        } else {
            self.selection
                .navigate_file_to_adjacent_visible_id(target.clone())?;
        }
        Some(target)
    }

    pub(in crate::native_app) fn starmap_status(&self) -> StarmapStatus {
        let clustered_count = self
            .sample_list
            .starmap_layout
            .points_by_file
            .values()
            .filter(|point| point.cluster_id.is_some())
            .count();
        let cluster_color_count = if clustered_count == 0 {
            0
        } else {
            clustered_count.min(STARMAP_CLUSTER_PALETTE.len())
        };
        StarmapStatus {
            listed_count: self.sample_list.starmap_layout.listed_count,
            layout_count: self.sample_list.starmap_layout.points_by_file.len(),
            clustered_count,
            cluster_color_count,
        }
    }
}

fn instant_audition_ready_for_starmap(
    file: &FileEntry,
    instant_audition_sample_paths: &HashSet<String>,
) -> bool {
    !should_use_file_backed_wav_decode_for_entry(&file.extension, file.size_bytes)
        || instant_audition_sample_paths.contains(&file.id)
}

fn preview_audition_candidate_for_starmap(file: &FileEntry) -> bool {
    file.extension.eq_ignore_ascii_case("wav") || file.extension.eq_ignore_ascii_case("wave")
}

#[cfg(test)]
mod tests;
