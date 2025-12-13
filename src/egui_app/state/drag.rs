use super::browser::TriageFlagColumn;
use crate::sample_sources::{CollectionId, SourceId};
use crate::selection::SelectionRange;
use egui::Pos2;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

/// Active drag payload carried across UI panels.
#[derive(Clone, Debug, PartialEq)]
pub enum DragPayload {
    Sample {
        source_id: SourceId,
        relative_path: PathBuf,
    },
    Selection {
        source_id: SourceId,
        relative_path: PathBuf,
        bounds: SelectionRange,
        /// When true, keep focus on the source sample after exporting a clip.
        keep_source_focused: bool,
    },
}

/// Panel-originating drag target.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum DragSource {
    Collections,
    Browser,
    Folders,
    Waveform,
    External,
}

/// Unified drag target variants.
#[derive(Clone, Debug, PartialEq)]
pub enum DragTarget {
    None,
    CollectionsRow(CollectionId),
    CollectionsDropZone { collection_id: Option<CollectionId> },
    BrowserTriage(TriageFlagColumn),
    FolderPanel { folder: Option<PathBuf> },
    External,
}

impl DragTarget {
    fn priority(&self) -> u8 {
        match self {
            DragTarget::External => 6,
            DragTarget::CollectionsDropZone { .. } => 5,
            DragTarget::CollectionsRow(_) => 4,
            DragTarget::FolderPanel { .. } => 3,
            DragTarget::BrowserTriage(_) => 2,
            DragTarget::None => 0,
        }
    }
}

#[derive(Clone, Debug)]
/// Recorded drag target selection used for debugging/UX decisions.
pub struct DragTargetSnapshot {
    pub target: DragTarget,
    pub source: DragSource,
    pub recorded_at: Instant,
}

impl DragTargetSnapshot {
    fn new(target: DragTarget, source: DragSource) -> Self {
        Self {
            target,
            source,
            recorded_at: Instant::now(),
        }
    }
}

/// Drag/hover state shared between the sample browser and collections.
#[derive(Clone, Debug)]
pub struct DragState {
    pub payload: Option<DragPayload>,
    pub label: String,
    pub position: Option<Pos2>,
    targets: HashMap<DragSource, DragTarget>,
    pub active_target: DragTarget,
    pub target_history: Vec<DragTargetSnapshot>,
    pub last_folder_target: Option<PathBuf>,
    pub external_started: bool,
    pub external_arm_at: Option<Instant>,
}

impl Default for DragState {
    fn default() -> Self {
        Self {
            payload: None,
            label: String::new(),
            position: None,
            targets: HashMap::new(),
            active_target: DragTarget::None,
            target_history: Vec::new(),
            last_folder_target: None,
            external_started: false,
            external_arm_at: None,
        }
    }
}

impl DragState {
    /// Clear any target associated with a given drag source.
    pub fn clear_targets_from(&mut self, source: DragSource) {
        self.targets.remove(&source);
        self.recompute_active_target(source, DragTarget::None);
    }

    /// Set (or update) the drag target for a given source and recompute the active target.
    pub fn set_target(&mut self, source: DragSource, target: DragTarget) {
        if let DragTarget::FolderPanel { folder: Some(path) } = &target {
            self.last_folder_target = Some(path.clone());
        }
        self.targets.insert(source, target.clone());
        self.recompute_active_target(source, target);
    }

    /// Clear all known targets and reset the active target to `None`.
    pub fn clear_all_targets(&mut self) {
        self.targets.clear();
        self.active_target = DragTarget::None;
        self.record_transition(DragSource::External, DragTarget::None);
    }

    fn recompute_active_target(&mut self, source: DragSource, incoming: DragTarget) {
        let new_active = self
            .targets
            .values()
            .max_by_key(|target| target.priority())
            .cloned()
            .unwrap_or(DragTarget::None);
        if self.active_target != new_active {
            self.active_target = new_active.clone();
            self.record_transition(source, new_active);
        } else {
            self.record_transition(source, incoming);
        }
    }

    fn record_transition(&mut self, source: DragSource, target: DragTarget) {
        self.target_history
            .push(DragTargetSnapshot::new(target, source));
        const MAX_HISTORY: usize = 64;
        if self.target_history.len() > MAX_HISTORY {
            let excess = self.target_history.len() - MAX_HISTORY;
            self.target_history.drain(..excess);
        }
    }
}
