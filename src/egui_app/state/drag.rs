use super::browser::TriageFlagColumn;
use crate::sample_sources::{CollectionId, SourceId};
use crate::selection::SelectionRange;
use egui::Pos2;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

/// Single sample reference used for multi-sample drags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DragSample {
    pub source_id: SourceId,
    pub relative_path: PathBuf,
}

/// Active drag payload carried across UI panels.
#[derive(Clone, Debug, PartialEq)]
pub enum DragPayload {
    Sample {
        source_id: SourceId,
        relative_path: PathBuf,
    },
    Samples {
        samples: Vec<DragSample>,
    },
    Folder {
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
    Sources,
    Folders,
    DropTargets,
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
    SourcesRow(SourceId),
    FolderPanel { folder: Option<PathBuf> },
    DropTarget { path: PathBuf },
    DropTargetsPanel,
    External,
}

impl DragTarget {
    fn priority(&self) -> u8 {
        match self {
            DragTarget::External => 6,
            DragTarget::CollectionsDropZone { .. } => 5,
            DragTarget::CollectionsRow(_) => 4,
            DragTarget::SourcesRow(_) => 3,
            DragTarget::FolderPanel { .. } => 2,
            DragTarget::DropTarget { .. } => 2,
            DragTarget::DropTargetsPanel => 2,
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
    pub origin_source: Option<DragSource>,
    targets: HashMap<DragSource, DragTarget>,
    pub active_target: DragTarget,
    pub target_history: Vec<DragTargetSnapshot>,
    pub last_folder_target: Option<PathBuf>,
    /// True when the user is requesting a copy on drop (alt key held).
    pub copy_on_drop: bool,
    pub external_started: bool,
    pub external_arm_at: Option<Instant>,
    /// Best-effort signal that the cursor has left the app window mid-drag (Windows-only use).
    ///
    /// Some platforms/input backends stop sending pointer positions once the cursor leaves the
    /// window. We latch this on `egui::Event::PointerGone` and clear it when pointer movement
    /// resumes.
    pub pointer_left_window: bool,
    /// When Windows doesn't deliver a reliable press event (e.g. after an external drag/drop),
    /// we use OS-level mouse state to synthesize drag starts on hovered widgets.
    pub pending_os_drag: Option<PendingOsDragStart>,
    /// True while the OS reports the left mouse button as held down (Windows-only use).
    pub os_left_mouse_down: bool,
    /// True only on the frame the OS transitions the left mouse button from up -> down.
    pub os_left_mouse_pressed: bool,
    /// True only on the frame the OS transitions the left mouse button from down -> up.
    pub os_left_mouse_released: bool,
    /// OS cursor position in client points (Windows-only; best-effort).
    pub os_cursor_pos: Option<Pos2>,
    os_left_mouse_down_last: bool,
}

impl Default for DragState {
    fn default() -> Self {
        Self {
            payload: None,
            label: String::new(),
            position: None,
            origin_source: None,
            targets: HashMap::new(),
            active_target: DragTarget::None,
            target_history: Vec::new(),
            last_folder_target: None,
            copy_on_drop: false,
            external_started: false,
            external_arm_at: None,
            pointer_left_window: false,
            pending_os_drag: None,
            os_left_mouse_down: false,
            os_left_mouse_pressed: false,
            os_left_mouse_released: false,
            os_cursor_pos: None,
            os_left_mouse_down_last: false,
        }
    }
}

/// Deferred drag start candidate used when the OS eats the initial mouse press event.
#[derive(Clone, Debug)]
pub struct PendingOsDragStart {
    pub payload: DragPayload,
    pub label: String,
    pub origin: Pos2,
}

impl DragState {
    pub fn update_os_mouse_state(&mut self, left_mouse_down: bool) {
        self.os_left_mouse_down = left_mouse_down;
        self.os_left_mouse_pressed = left_mouse_down && !self.os_left_mouse_down_last;
        self.os_left_mouse_released = !left_mouse_down && self.os_left_mouse_down_last;
        self.os_left_mouse_down_last = left_mouse_down;
        if self.os_left_mouse_released {
            self.pending_os_drag = None;
        }
    }

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
        let max_priority = self
            .targets
            .values()
            .map(|target| target.priority())
            .max()
            .unwrap_or(0);
        let new_active = if incoming.priority() == max_priority {
            incoming.clone()
        } else {
            self.targets
                .values()
                .max_by_key(|target| target.priority())
                .cloned()
                .unwrap_or(DragTarget::None)
        };
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
