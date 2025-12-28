//! Library state for sources, collections, and missing entries.

use super::super::{Collection, SampleSource, SourceId};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Clone)]
pub(in crate::egui_app::controller) struct RowFlags {
    pub(in crate::egui_app::controller) focused: bool,
    pub(in crate::egui_app::controller) loaded: bool,
}

pub(in crate::egui_app::controller) struct MissingState {
    pub(in crate::egui_app::controller) sources: HashSet<SourceId>,
    pub(in crate::egui_app::controller) wavs: HashMap<SourceId, HashSet<PathBuf>>,
}

impl MissingState {
    pub(in crate::egui_app::controller) fn new() -> Self {
        Self {
            sources: HashSet::new(),
            wavs: HashMap::new(),
        }
    }
}

pub(in crate::egui_app::controller) struct LibraryState {
    pub(in crate::egui_app::controller) sources: Vec<SampleSource>,
    pub(in crate::egui_app::controller) collections: Vec<Collection>,
    pub(in crate::egui_app::controller) missing: MissingState,
}

impl LibraryState {
    pub(in crate::egui_app::controller) fn new() -> Self {
        Self {
            sources: Vec::new(),
            collections: Vec::new(),
            missing: MissingState::new(),
        }
    }
}
