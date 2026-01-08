//! Undo and navigation history state for the controller.

use crate::egui_app::controller::undo;
use crate::sample_sources::SourceId;
use std::collections::VecDeque;
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
    pub(crate) entries: VecDeque<RandomHistoryEntry>,
    pub(crate) cursor: Option<usize>,
}

impl RandomHistoryState {
    pub(crate) fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            cursor: None,
        }
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
