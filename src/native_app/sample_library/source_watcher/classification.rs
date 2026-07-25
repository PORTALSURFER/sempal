use notify::{Event, EventKind};
use std::path::Path;
use wavecrate_library::sample_sources::{
    SourceEntryFileType, SourceEntryKind, SourceEntryProbeError, classify_path_without_following,
    classify_source_entry,
};

pub(super) fn event_triggers_source_refresh(event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
    )
}

pub(super) fn retain_source_refresh_candidates(event: &mut Event) -> bool {
    if !event_triggers_source_refresh(event) {
        return false;
    }
    event
        .paths
        .retain(|path| path_is_source_refresh_candidate(path, event.kind));
    !event.paths.is_empty()
}

pub(super) fn path_is_source_refresh_candidate(path: &Path, kind: EventKind) -> bool {
    if is_wavecrate_metadata_file(path)
        || is_wavecrate_transient_analysis_path(path)
        || wavecrate_library::sample_sources::is_apple_double_sidecar(path)
    {
        return false;
    }
    matches!(kind, EventKind::Remove(_) | EventKind::Any) || watcher_entry_is_candidate(path)
}

fn watcher_entry_is_candidate(path: &Path) -> bool {
    match classify_path_without_following(path) {
        Ok(classification) => {
            classification.has_supported_audio()
                || classification.visible_kind() == Some(SourceEntryKind::Directory)
                // A live link is not visible to source traversal, but it can
                // replace a previously indexed WAV. Keep the bounded watcher
                // candidate so targeted sync can retire that stale row while
                // refusing to follow the link.
                || path_only_source_candidate(path)
        }
        // Watcher events commonly arrive after the entry vanished. Retain a
        // bounded path-only candidate so targeted reconciliation can remove a
        // previously indexed WAV or a directory subtree without following it.
        Err(SourceEntryProbeError::Missing | SourceEntryProbeError::Unavailable(_)) => {
            path_only_source_candidate(path)
        }
    }
}

fn path_only_source_candidate(path: &Path) -> bool {
    classify_source_entry(path, SourceEntryFileType::File).has_supported_audio()
        || path.extension().is_none()
}

fn is_wavecrate_transient_analysis_path(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        is_tempfile_name(&name, "ann_container") || is_tempfile_name(&name, "ann_dump")
    })
}

fn is_tempfile_name(name: &str, prefix: &str) -> bool {
    let Some(suffix) = name.strip_prefix(prefix) else {
        return false;
    };
    suffix.len() == 6 && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn is_wavecrate_metadata_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.starts_with(wavecrate::sample_sources::db::DB_FILE_NAME)
        || name.starts_with(wavecrate::sample_sources::db::LEGACY_DB_FILE_NAME)
}
