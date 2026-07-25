use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use wavecrate_library::timestamps::system_time_to_unix_nanos;

const MAX_WATCHER_ECHO_HASH_BYTES: u64 = 8 * 1024 * 1024;

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
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_WATCHER_ECHO_HASH_BYTES => {
            match std::fs::read(path) {
                Ok(bytes) => {
                    ExpectedMutationPathState::ContentHash(*blake3::hash(&bytes).as_bytes())
                }
                Err(_) => ExpectedMutationPathState::Unverifiable,
            }
        }
        Ok(metadata) => ExpectedMutationPathState::Metadata {
            len: metadata.len(),
            modified_ns: metadata.modified().ok().map(system_time_to_unix_nanos),
            is_dir: metadata.is_dir(),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ExpectedMutationPathState::Missing
        }
        Err(_) => ExpectedMutationPathState::Unverifiable,
    }
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
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_WATCHER_ECHO_HASH_BYTES => {
            let bytes = std::fs::read(path).ok()?;
            Some(CommittedWatcherPathState::ContentHash(
                *blake3::hash(&bytes).as_bytes(),
            ))
        }
        Ok(_) => None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Some(CommittedWatcherPathState::Missing)
        }
        Err(_) => None,
    }
}
