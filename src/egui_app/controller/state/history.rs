//! Undo and navigation history state for the controller.

use crate::egui_app::controller::undo;
use crate::sample_sources::SourceId;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

pub(crate) struct ControllerHistoryState {
    pub(crate) undo_stack: undo::UndoStack<super::super::EguiController>,
    pub(crate) random_history: RandomHistoryState,
    pub(crate) focus_history: FocusHistoryState,
}

impl ControllerHistoryState {
    pub(crate) fn new(undo_limit: usize) -> Self {
        Self {
            undo_stack: undo::UndoStack::new(undo_limit),
            random_history: RandomHistoryState::new(),
            focus_history: FocusHistoryState::new(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct RandomHistoryEntry {
    pub(crate) source_id: SourceId,
    pub(crate) relative_path: PathBuf,
}

pub(crate) struct RandomHistoryState {
    /// Random samples already visited this session, tracked per source.
    pub(crate) played_by_source: HashMap<SourceId, HashSet<PathBuf>>,
    pub(crate) entries: VecDeque<RandomHistoryEntry>,
    pub(crate) cursor: Option<usize>,
}

impl RandomHistoryState {
    pub(crate) fn new() -> Self {
        Self {
            played_by_source: HashMap::new(),
            entries: VecDeque::new(),
            cursor: None,
        }
    }

    /// Returns true when a sample was already visited for random navigation.
    pub(crate) fn has_played(&self, source_id: &SourceId, relative_path: &Path) -> bool {
        self.played_by_source
            .get(source_id)
            .is_some_and(|set| set.contains(relative_path))
    }

    /// Marks a sample as visited for random navigation in the current session.
    pub(crate) fn mark_played(&mut self, source_id: &SourceId, relative_path: &Path) {
        self.played_by_source
            .entry(source_id.clone())
            .or_default()
            .insert(relative_path.to_path_buf());
    }

    /// Clears the visited set for a source, starting a new random cycle.
    pub(crate) fn reset_played_for_source(&mut self, source_id: &SourceId) {
        self.played_by_source.remove(source_id);
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct FocusHistoryEntry {
    pub(crate) source_id: SourceId,
    pub(crate) relative_path: PathBuf,
}

pub(crate) struct FocusHistoryState {
    pub(crate) entries: VecDeque<FocusHistoryEntry>,
    pub(crate) cursor: Option<usize>,
    pub(crate) suspend_push: bool,
}

impl FocusHistoryState {
    pub(crate) fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            cursor: None,
            suspend_push: false,
        }
    }
}
