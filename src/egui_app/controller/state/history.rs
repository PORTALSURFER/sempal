//! Undo and navigation history state for the controller.

use super::super::{SourceId, undo};
use std::collections::VecDeque;
use std::path::PathBuf;

pub(in crate::egui_app::controller) struct ControllerHistoryState {
    pub(in crate::egui_app::controller) undo_stack: undo::UndoStack<super::super::EguiController>,
    pub(in crate::egui_app::controller) random_history: RandomHistoryState,
    pub(in crate::egui_app::controller) focus_history: FocusHistoryState,
}

impl ControllerHistoryState {
    pub(in crate::egui_app::controller) fn new(undo_limit: usize) -> Self {
        Self {
            undo_stack: undo::UndoStack::new(undo_limit),
            random_history: RandomHistoryState::new(),
            focus_history: FocusHistoryState::new(),
        }
    }
}

#[derive(Clone)]
pub(in crate::egui_app::controller) struct RandomHistoryEntry {
    pub(in crate::egui_app::controller) source_id: SourceId,
    pub(in crate::egui_app::controller) relative_path: PathBuf,
}

pub(in crate::egui_app::controller) struct RandomHistoryState {
    pub(in crate::egui_app::controller) entries: VecDeque<RandomHistoryEntry>,
    pub(in crate::egui_app::controller) cursor: Option<usize>,
}

impl RandomHistoryState {
    pub(in crate::egui_app::controller) fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            cursor: None,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(in crate::egui_app::controller) struct FocusHistoryEntry {
    pub(in crate::egui_app::controller) source_id: SourceId,
    pub(in crate::egui_app::controller) relative_path: PathBuf,
}

pub(in crate::egui_app::controller) struct FocusHistoryState {
    pub(in crate::egui_app::controller) entries: VecDeque<FocusHistoryEntry>,
    pub(in crate::egui_app::controller) cursor: Option<usize>,
    pub(in crate::egui_app::controller) suspend_push: bool,
}

impl FocusHistoryState {
    pub(in crate::egui_app::controller) fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            cursor: None,
            suspend_push: false,
        }
    }
}
