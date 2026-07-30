use std::path::PathBuf;

use wavecrate::sample_sources::SourceFileEvidence;

use super::FolderEntry;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct RenamePathRemap {
    pub(in crate::native_app) old_path: PathBuf,
    pub(in crate::native_app) new_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct RenameCommitResult {
    pub(in crate::native_app) status: String,
    pub(in crate::native_app) path_remap: Option<RenamePathRemap>,
    pub(in crate::native_app) metadata_error: Option<String>,
}

impl RenameCommitResult {
    pub(in crate::native_app) fn status(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            path_remap: None,
            metadata_error: None,
        }
    }

    pub(in crate::native_app) fn remapped(
        status: impl Into<String>,
        old_path: PathBuf,
        new_path: PathBuf,
    ) -> Self {
        Self {
            status: status.into(),
            path_remap: Some(RenamePathRemap { old_path, new_path }),
            metadata_error: None,
        }
    }

    pub(in crate::native_app) fn remapped_with_metadata_error(
        status: impl Into<String>,
        old_path: PathBuf,
        new_path: PathBuf,
        metadata_error: String,
    ) -> Self {
        Self {
            status: status.into(),
            path_remap: Some(RenamePathRemap { old_path, new_path }),
            metadata_error: Some(metadata_error),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum RenameInputResult {
    Status(RenameCommitResult),
    Commit(RenameCommitRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum RenameCommitRequest {
    FolderRename {
        old_path: PathBuf,
        new_path: PathBuf,
        new_name: String,
    },
    FolderCreate {
        parent_id: String,
        pending_id: String,
        new_path: PathBuf,
        new_name: String,
    },
    FileRename {
        old_path: PathBuf,
        new_path: PathBuf,
        new_name: String,
        metadata_remap: Option<FileMetadataRemap>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FileMetadataRemap {
    pub(in crate::native_app) source_root: PathBuf,
    pub(in crate::native_app) old_relative: PathBuf,
    pub(in crate::native_app) new_relative: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct RenameCommitCompletion {
    pub(in crate::native_app) request: RenameCommitRequest,
    pub(in crate::native_app) result: Result<RenameCommitSuccess, String>,
    /// Evidence captured by the rename worker immediately after the filesystem rename.
    pub(in crate::native_app) post_filesystem_evidence: Option<SourceFileEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum RenameCommitSuccess {
    FolderRenamed,
    FolderCreated {
        folder: FolderEntry,
    },
    FileRenamed {
        metadata_remap_result: Result<(), String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FileRenameView {
    pub(in crate::native_app) draft: String,
    pub(in crate::native_app) input_id: u64,
    pub(in crate::native_app) selection_start: usize,
    pub(in crate::native_app) selection_end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct RenameTargetView {
    pub(in crate::native_app) kind: &'static str,
    pub(in crate::native_app) label: String,
    pub(in crate::native_app) is_source_root: bool,
}
