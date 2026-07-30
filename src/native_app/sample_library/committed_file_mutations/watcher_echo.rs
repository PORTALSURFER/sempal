use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use wavecrate::sample_sources::{SourceFileEvidence, capture_source_file_evidence};

use super::{ExpectedMutationPathState, FileMutationChange};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum CommittedWatcherPathState {
    Missing,
    ContentHash([u8; 32]),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct CommittedWatcherEcho {
    pub(in crate::native_app) relative_path: PathBuf,
    pub(in crate::native_app) expected_state: CommittedWatcherPathState,
}

pub(super) fn capture_expected_path_state(path: &Path) -> ExpectedMutationPathState {
    capture_source_file_evidence(path)
}

pub(super) fn watcher_echoes_for_changes(
    root: &Path,
    changes: &[FileMutationChange],
) -> Vec<CommittedWatcherEcho> {
    let mut echoes = BTreeMap::new();
    for change in changes {
        for (path, state) in [
            (
                change.before_path.as_deref(),
                change.expected_before_state.as_ref(),
            ),
            (
                change.after_path.as_deref(),
                change.expected_after_state.as_ref(),
            ),
        ] {
            let (Some(path), Some(state)) = (path, state) else {
                continue;
            };
            let Ok(relative_path) = path.strip_prefix(root) else {
                continue;
            };
            let expected_state = match state {
                ExpectedMutationPathState::Missing => CommittedWatcherPathState::Missing,
                ExpectedMutationPathState::ContentHash(hash) => {
                    CommittedWatcherPathState::ContentHash(*hash)
                }
                ExpectedMutationPathState::Metadata { .. }
                | ExpectedMutationPathState::Unverifiable => continue,
            };
            echoes.insert(relative_path.to_path_buf(), expected_state);
        }
    }
    echoes
        .into_iter()
        .map(|(relative_path, expected_state)| CommittedWatcherEcho {
            relative_path,
            expected_state,
        })
        .collect()
}

pub(in crate::native_app) fn observed_watcher_path_state(
    path: &Path,
) -> Option<CommittedWatcherPathState> {
    match capture_source_file_evidence(path) {
        SourceFileEvidence::ContentHash(hash) => Some(CommittedWatcherPathState::ContentHash(hash)),
        SourceFileEvidence::Missing => Some(CommittedWatcherPathState::Missing),
        SourceFileEvidence::Metadata { .. } | SourceFileEvidence::Unverifiable => None,
    }
}
