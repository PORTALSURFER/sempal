//! Library state for sources, collections, and missing entries.

use super::super::{Collection, SampleSource, SourceId};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Clone)]
pub(crate) struct RowFlags {
    pub(crate) focused: bool,
    pub(crate) loaded: bool,
}

pub(crate) struct MissingState {
    pub(crate) sources: HashSet<SourceId>,
    pub(crate) wavs: HashMap<SourceId, HashSet<PathBuf>>,
}

impl MissingState {
    pub(crate) fn new() -> Self {
        Self {
            sources: HashSet::new(),
            wavs: HashMap::new(),
        }
    }
}

pub(crate) struct LibraryState {
    pub(crate) sources: Vec<SampleSource>,
    pub(crate) collections: Vec<Collection>,
    pub(crate) missing: MissingState,
}

impl LibraryState {
    pub(crate) fn new() -> Self {
        Self {
            sources: Vec::new(),
            collections: Vec::new(),
            missing: MissingState::new(),
        }
    }
}
