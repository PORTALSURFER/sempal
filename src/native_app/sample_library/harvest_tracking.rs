use std::path::{Path, PathBuf};

use wavecrate::{
    sample_sources::{
        HarvestDerivationOperation, HarvestFileIdentity, HarvestFileKey, HarvestMetadataSnapshot,
        HarvestSeenPersistRequest, HarvestSeenPersistResult, HarvestSourceRange, HarvestState,
        NewHarvestDerivation, SampleSource, WavEntry, persist_harvest_seen,
    },
    selection::SelectionRange,
};

use crate::native_app::{
    app::{GuiMessage, NativeAppState, emit_gui_action, sample_path_label},
    audio::playback::tagged_playback_mode_for_tag,
    sample_library::{
        context_menu_target::{self as context_menu, BrowserContextTargetKind},
        folder_browser::commands::FolderMoveRequest,
        sample_list::{
            SAMPLE_BROWSER_LIST_ID, SAMPLE_BROWSER_ROW_HEIGHT,
            SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
        },
    },
};

const HARVEST_ROOT_FOLDER: &str = "_Harvests";

mod destination;
mod mutation;
mod persistence;
mod query;
mod relationships;
mod targets;

pub(in crate::native_app) use persistence::execute_harvest_selection_derivation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct HarvestFamilySummary {
    pub(in crate::native_app) state_label: String,
    pub(in crate::native_app) origin_count: usize,
    pub(in crate::native_app) derivative_count: usize,
    pub(in crate::native_app) missing_origin_count: usize,
    pub(in crate::native_app) missing_derivative_count: usize,
    pub(in crate::native_app) first_origin_label: Option<String>,
    pub(in crate::native_app) first_derivative_label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HarvestStateTarget {
    path: PathBuf,
    identity: HarvestFileIdentity,
    state: HarvestState,
}
