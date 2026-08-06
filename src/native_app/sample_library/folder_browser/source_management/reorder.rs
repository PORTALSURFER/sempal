use radiant::widgets::DragHandleMessage;

use super::super::{FolderBrowserState, SOURCE_ROW_STEP};

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app::sample_library::folder_browser) struct SourceReorderDrag {
    source_id: String,
    start_y: f32,
    source_slot: usize,
    target_slot: usize,
}

impl SourceReorderDrag {
    fn new(source_id: String, start_y: f32, target_slot: usize) -> Self {
        Self {
            source_id,
            start_y,
            source_slot: target_slot,
            target_slot,
        }
    }
}

impl FolderBrowserState {
    pub(in crate::native_app) fn apply_source_reorder_drag(
        &mut self,
        source_id: String,
        message: DragHandleMessage,
    ) -> bool {
        match message {
            DragHandleMessage::Started {
                origin, position, ..
            } => {
                let Some(source_slot) = self.configured_source_slot(&source_id) else {
                    return false;
                };
                if self.configured_source_count() < 2 {
                    return false;
                }
                self.source.reorder_drag = Some(SourceReorderDrag::new(
                    source_id.clone(),
                    origin.y,
                    source_slot,
                ));
                self.update_source_reorder_target(&source_id, position.y);
                false
            }
            DragHandleMessage::Moved { position, .. } => {
                self.update_source_reorder_target(&source_id, position.y);
                false
            }
            DragHandleMessage::Ended { position, .. } => {
                self.update_source_reorder_target(&source_id, position.y);
                self.commit_source_reorder(&source_id)
            }
            DragHandleMessage::Cancelled { .. } => {
                self.source.reorder_drag = None;
                false
            }
            DragHandleMessage::DoubleActivate { .. } => false,
        }
    }

    pub(in crate::native_app) fn source_reorder_drag_active(&self) -> bool {
        self.source.reorder_drag.is_some()
    }

    pub(in crate::native_app) fn source_reorder_drag_source_id(&self) -> Option<&str> {
        self.source
            .reorder_drag
            .as_ref()
            .map(|drag| drag.source_id.as_str())
    }

    pub(in crate::native_app) fn source_reorder_enabled(&self, source_id: &str) -> bool {
        self.configured_source_count() > 1 && self.configured_source_slot(source_id).is_some()
    }

    #[cfg(test)]
    pub(in crate::native_app) fn source_reorder_target_source_id(&self) -> Option<&str> {
        let drag = self.source.reorder_drag.as_ref()?;
        self.source
            .sources
            .iter()
            .filter(|source| !source.is_default_assets_source())
            .nth(drag.target_slot)
            .map(|source| source.id.as_str())
    }

    pub(in crate::native_app) fn source_reorder_drop_marker_after(
        &self,
        source_id: &str,
    ) -> Option<bool> {
        let drag = self.source.reorder_drag.as_ref()?;
        if drag.target_slot == drag.source_slot
            || self
                .source
                .sources
                .iter()
                .filter(|source| !source.is_default_assets_source())
                .nth(drag.target_slot)
                .is_none_or(|source| source.id != source_id)
        {
            return None;
        }
        Some(drag.target_slot > drag.source_slot)
    }

    pub(in crate::native_app) fn clear_source_reorder_drag(&mut self) {
        self.source.reorder_drag = None;
    }

    fn configured_source_slot(&self, source_id: &str) -> Option<usize> {
        self.source
            .sources
            .iter()
            .filter(|source| !source.is_default_assets_source())
            .position(|source| source.id == source_id)
    }

    fn configured_source_count(&self) -> usize {
        self.source
            .sources
            .iter()
            .filter(|source| !source.is_default_assets_source())
            .count()
    }

    fn update_source_reorder_target(&mut self, source_id: &str, pointer_y: f32) {
        let Some(drag) = self.source.reorder_drag.as_ref() else {
            return;
        };
        if drag.source_id != source_id || !pointer_y.is_finite() {
            return;
        }
        let start_y = drag.start_y;
        let source_slot = drag.source_slot;
        if self.configured_source_slot(source_id).is_none() {
            self.source.reorder_drag = None;
            return;
        }
        let row_delta = ((pointer_y - start_y) / SOURCE_ROW_STEP).round() as isize;
        let last_slot = self.configured_source_count().saturating_sub(1) as isize;
        let target_slot = (source_slot as isize + row_delta).clamp(0, last_slot) as usize;
        if let Some(drag) = self.source.reorder_drag.as_mut() {
            drag.target_slot = target_slot;
        }
    }

    fn commit_source_reorder(&mut self, source_id: &str) -> bool {
        let Some(drag) = self.source.reorder_drag.take() else {
            return false;
        };
        if drag.source_id != source_id {
            return false;
        }
        let configured_indices = self
            .source
            .sources
            .iter()
            .enumerate()
            .filter_map(|(index, source)| (!source.is_default_assets_source()).then_some(index))
            .collect::<Vec<_>>();
        let Some(source_slot) = configured_indices
            .iter()
            .position(|index| self.source.sources[*index].id == source_id)
        else {
            return false;
        };
        let target_slot = drag
            .target_slot
            .min(configured_indices.len().saturating_sub(1));
        if source_slot == target_slot {
            return false;
        }
        if source_slot < target_slot {
            for slot in source_slot..target_slot {
                self.source
                    .sources
                    .swap(configured_indices[slot], configured_indices[slot + 1]);
            }
        } else {
            for slot in (target_slot..source_slot).rev() {
                self.source
                    .sources
                    .swap(configured_indices[slot], configured_indices[slot + 1]);
            }
        }
        true
    }
}
