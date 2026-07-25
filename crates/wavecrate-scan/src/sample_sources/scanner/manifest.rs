use std::collections::{BTreeMap, BTreeSet};

use wavecrate_library::sample_sources::{SourceDatabase, SourceManifestEntry};

use super::scan::{
    CommittedSourceDelta, ManifestIdentityDelta, MovedManifestIdentity, ScanError, ScanStats,
};

pub(super) fn capture_manifest_with_revision(
    database: &SourceDatabase,
) -> Result<(u64, Vec<SourceManifestEntry>), ScanError> {
    database
        .manifest_snapshot_with_revision()
        .map_err(ScanError::from)
}

pub(super) fn capture_manifest_under_paths_with_revision(
    database: &SourceDatabase,
    paths: &[std::path::PathBuf],
) -> Result<(u64, Vec<SourceManifestEntry>), ScanError> {
    let unicode_paths = paths
        .iter()
        .filter(|path| path.to_str().is_some())
        .cloned()
        .collect::<Vec<_>>();
    database
        .manifest_snapshot_with_revision_under_paths(&unicode_paths)
        .map_err(ScanError::from)
}

pub(super) fn publish_committed_delta(
    stats: &mut ScanStats,
    before: Vec<SourceManifestEntry>,
    committed_snapshot: (u64, Vec<SourceManifestEntry>),
) {
    let (revision, after) = committed_snapshot;
    stats.committed_delta = build_committed_delta(&before, &after, revision);
    stats.manifest_before = before;
    stats.manifest_after = after;
}

pub(super) fn publish_targeted_committed_delta(
    stats: &mut ScanStats,
    before: Vec<SourceManifestEntry>,
    committed_snapshot: (u64, Vec<SourceManifestEntry>),
) {
    let (revision, after) = committed_snapshot;
    stats.committed_delta =
        build_targeted_committed_delta(&before, &after, revision, &stats.renamed_samples);
    stats.manifest_before = before;
    stats.manifest_after = after;
}

pub(super) fn build_targeted_committed_delta(
    before: &[SourceManifestEntry],
    after: &[SourceManifestEntry],
    revision: u64,
    renames: &[super::scan::RenamedSample],
) -> CommittedSourceDelta {
    let mut delta = build_committed_delta(before, after, revision);
    for rename in renames {
        if delta.moved.iter().any(|moved| {
            moved.old_relative_path == rename.old_relative_path
                && moved.new_relative_path == rename.new_relative_path
        }) {
            continue;
        }
        let Some(current) = after
            .iter()
            .find(|entry| entry.relative_path == rename.new_relative_path)
        else {
            continue;
        };
        delta
            .created
            .retain(|created| created.relative_path != rename.new_relative_path);
        delta
            .deleted
            .retain(|deleted| deleted.relative_path != rename.old_relative_path);
        delta.moved.push(MovedManifestIdentity {
            identity: stable_identity(current, None),
            old_relative_path: rename.old_relative_path.clone(),
            new_relative_path: rename.new_relative_path.clone(),
            content_generation: content_generation(current),
        });
    }
    delta.moved.sort_by(|left, right| {
        left.old_relative_path
            .cmp(&right.old_relative_path)
            .then_with(|| left.new_relative_path.cmp(&right.new_relative_path))
    });
    delta
}

pub(super) fn build_committed_delta(
    before: &[SourceManifestEntry],
    after: &[SourceManifestEntry],
    revision: u64,
) -> CommittedSourceDelta {
    let mut matched_before = BTreeSet::new();
    let mut matched_after = BTreeSet::new();
    let mut matches = Vec::new();

    match_unique(
        before,
        after,
        |entry| normalized(entry.file_identity.as_deref()),
        &mut matched_before,
        &mut matched_after,
        &mut matches,
    );
    match_same_path_and_identity(
        before,
        after,
        &mut matched_before,
        &mut matched_after,
        &mut matches,
    );
    match_same_path_with_missing_identity(
        before,
        after,
        &mut matched_before,
        &mut matched_after,
        &mut matches,
    );
    match_unique(
        before,
        after,
        |entry| {
            normalized(entry.file_identity.as_deref())
                .is_none()
                .then(|| entry.relative_path.to_string_lossy().into_owned())
        },
        &mut matched_before,
        &mut matched_after,
        &mut matches,
    );
    match_unique(
        before,
        after,
        |entry| {
            normalized(entry.file_identity.as_deref())
                .is_none()
                .then(|| normalized(entry.content_hash.as_deref()))
                .flatten()
        },
        &mut matched_before,
        &mut matched_after,
        &mut matches,
    );

    let mut delta = CommittedSourceDelta {
        revision,
        ..CommittedSourceDelta::default()
    };
    for (before_index, after_index) in matches {
        let previous = &before[before_index];
        let current = &after[after_index];
        let identity = stable_identity(current, Some(previous));
        let generation = content_generation(current);
        if previous.relative_path != current.relative_path {
            delta.moved.push(MovedManifestIdentity {
                identity: identity.clone(),
                old_relative_path: previous.relative_path.clone(),
                new_relative_path: current.relative_path.clone(),
                content_generation: generation.clone(),
            });
        }
        if content_generation(previous) != generation {
            delta.changed.push(ManifestIdentityDelta {
                identity,
                relative_path: current.relative_path.clone(),
                content_generation: generation,
                source_metadata_changed: previous.file_size != current.file_size
                    || previous.modified_ns != current.modified_ns,
            });
        }
    }
    for (index, entry) in after.iter().enumerate() {
        if !matched_after.contains(&index) {
            delta.created.push(identity_delta(entry, None));
        }
    }
    for (index, entry) in before.iter().enumerate() {
        if !matched_before.contains(&index) {
            delta.deleted.push(identity_delta(entry, None));
        }
    }
    delta
        .created
        .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    delta
        .changed
        .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    delta.moved.sort_by(|left, right| {
        left.old_relative_path
            .cmp(&right.old_relative_path)
            .then_with(|| left.new_relative_path.cmp(&right.new_relative_path))
    });
    delta
        .deleted
        .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    delta
}

fn match_same_path_with_missing_identity(
    before: &[SourceManifestEntry],
    after: &[SourceManifestEntry],
    matched_before: &mut BTreeSet<usize>,
    matched_after: &mut BTreeSet<usize>,
    matches: &mut Vec<(usize, usize)>,
) {
    let after_by_path = after
        .iter()
        .enumerate()
        .filter(|(index, _)| !matched_after.contains(index))
        .map(|(index, entry)| (entry.relative_path.clone(), index))
        .collect::<BTreeMap<_, _>>();
    for (before_index, previous) in before.iter().enumerate() {
        if matched_before.contains(&before_index) {
            continue;
        }
        let Some(after_index) = after_by_path.get(&previous.relative_path).copied() else {
            continue;
        };
        let current = &after[after_index];
        if normalized(previous.file_identity.as_deref()).is_some()
            && normalized(current.file_identity.as_deref()).is_some()
        {
            continue;
        }
        matched_before.insert(before_index);
        matched_after.insert(after_index);
        matches.push((before_index, after_index));
    }
}

fn match_same_path_and_identity(
    before: &[SourceManifestEntry],
    after: &[SourceManifestEntry],
    matched_before: &mut BTreeSet<usize>,
    matched_after: &mut BTreeSet<usize>,
    matches: &mut Vec<(usize, usize)>,
) {
    let after_by_path = after
        .iter()
        .enumerate()
        .filter(|(index, entry)| {
            !matched_after.contains(index) && normalized(entry.file_identity.as_deref()).is_some()
        })
        .map(|(index, entry)| (entry.relative_path.clone(), index))
        .collect::<BTreeMap<_, _>>();
    for (before_index, previous) in before.iter().enumerate() {
        if matched_before.contains(&before_index) {
            continue;
        }
        let Some(previous_identity) = normalized(previous.file_identity.as_deref()) else {
            continue;
        };
        let Some(after_index) = after_by_path.get(&previous.relative_path).copied() else {
            continue;
        };
        let Some(current_identity) = normalized(after[after_index].file_identity.as_deref()) else {
            continue;
        };
        if previous_identity != current_identity {
            continue;
        }
        matched_before.insert(before_index);
        matched_after.insert(after_index);
        matches.push((before_index, after_index));
    }
}

fn match_unique(
    before: &[SourceManifestEntry],
    after: &[SourceManifestEntry],
    key: impl Fn(&SourceManifestEntry) -> Option<String>,
    matched_before: &mut BTreeSet<usize>,
    matched_after: &mut BTreeSet<usize>,
    matches: &mut Vec<(usize, usize)>,
) {
    let before_by_key = unique_indexes(before, &key, matched_before);
    let after_by_key = unique_indexes(after, &key, matched_after);
    for (value, before_index) in before_by_key {
        let Some(after_index) = after_by_key.get(&value).copied() else {
            continue;
        };
        matched_before.insert(before_index);
        matched_after.insert(after_index);
        matches.push((before_index, after_index));
    }
}

fn unique_indexes(
    entries: &[SourceManifestEntry],
    key: &impl Fn(&SourceManifestEntry) -> Option<String>,
    already_matched: &BTreeSet<usize>,
) -> BTreeMap<String, usize> {
    let mut indexes = BTreeMap::new();
    let mut duplicates = BTreeSet::new();
    for (index, entry) in entries.iter().enumerate() {
        if already_matched.contains(&index) {
            continue;
        }
        let Some(value) = key(entry) else {
            continue;
        };
        if indexes.insert(value.clone(), index).is_some() {
            duplicates.insert(value);
        }
    }
    indexes.retain(|value, _| !duplicates.contains(value));
    indexes
}

fn normalized(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn identity_delta(
    entry: &SourceManifestEntry,
    previous: Option<&SourceManifestEntry>,
) -> ManifestIdentityDelta {
    ManifestIdentityDelta {
        identity: stable_identity(entry, previous),
        relative_path: entry.relative_path.clone(),
        content_generation: content_generation(entry),
        source_metadata_changed: true,
    }
}

fn stable_identity(entry: &SourceManifestEntry, previous: Option<&SourceManifestEntry>) -> String {
    normalized(entry.file_identity.as_deref())
        .or_else(|| previous.and_then(|entry| normalized(entry.file_identity.as_deref())))
        .map(|identity| format!("file:{identity}"))
        .or_else(|| {
            normalized(entry.content_hash.as_deref())
                .map(|content_hash| format!("hash:{content_hash}"))
        })
        .unwrap_or_else(|| format!("path:{}", entry.relative_path.display()))
}

fn content_generation(entry: &SourceManifestEntry) -> String {
    normalized(entry.content_hash.as_deref()).unwrap_or_else(|| {
        format!(
            "pending:{}:{}:{}",
            normalized(entry.file_identity.as_deref())
                .unwrap_or_else(|| entry.relative_path.display().to_string()),
            entry.file_size,
            entry.modified_ns,
        )
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::build_committed_delta;
    use wavecrate_library::sample_sources::SourceManifestEntry;

    fn entry(
        path: &str,
        identity: &str,
        hash: Option<&str>,
        size: u64,
        modified: i64,
    ) -> SourceManifestEntry {
        SourceManifestEntry {
            relative_path: PathBuf::from(path),
            file_identity: Some(identity.to_string()),
            content_hash: hash.map(str::to_string),
            file_size: size,
            modified_ns: modified,
        }
    }

    #[test]
    fn committed_delta_classifies_create_change_move_and_delete() {
        let before = vec![
            entry("changed.wav", "changed", Some("old"), 4, 1),
            entry("old.wav", "moved", Some("same"), 4, 1),
            entry("deleted.wav", "deleted", Some("gone"), 4, 1),
        ];
        let after = vec![
            entry("changed.wav", "changed", Some("new"), 4, 1),
            entry("new.wav", "moved", Some("same"), 4, 1),
            entry("created.wav", "created", None, 4, 1),
        ];

        let delta = build_committed_delta(&before, &after, 17);

        assert_eq!(delta.revision, 17);
        assert_eq!(delta.created.len(), 1);
        assert_eq!(delta.changed.len(), 1);
        assert_eq!(delta.moved.len(), 1);
        assert_eq!(delta.deleted.len(), 1);
        assert_eq!(delta.moved[0].old_relative_path, PathBuf::from("old.wav"));
        assert_eq!(delta.moved[0].new_relative_path, PathBuf::from("new.wav"));
    }

    #[test]
    fn same_path_identity_replacement_retires_the_old_identity() {
        let before = vec![entry("same.wav", "old-file", Some("same"), 4, 1)];
        let after = vec![entry("same.wav", "new-file", Some("same"), 4, 1)];

        let delta = build_committed_delta(&before, &after, 18);

        assert_eq!(delta.created.len(), 1);
        assert_eq!(delta.deleted.len(), 1);
        assert!(delta.changed.is_empty());
        assert!(delta.moved.is_empty());
    }

    #[test]
    fn unchanged_duplicate_filesystem_identities_are_matched_by_path() {
        let before = vec![
            entry("first.wav", "shared-inode", Some("same"), 4, 1),
            entry("second.wav", "shared-inode", Some("same"), 4, 1),
        ];
        let after = before.clone();

        let delta = build_committed_delta(&before, &after, 19);

        assert!(delta.is_empty());
        assert_eq!(delta.revision, 19);
    }

    #[test]
    fn materializing_identity_and_hash_at_same_path_is_a_change() {
        let before = vec![SourceManifestEntry {
            relative_path: PathBuf::from("same.wav"),
            file_identity: None,
            content_hash: None,
            file_size: 4,
            modified_ns: 1,
        }];
        let after = vec![entry("same.wav", "materialized", Some("hash"), 4, 1)];

        let delta = build_committed_delta(&before, &after, 20);

        assert!(delta.created.is_empty());
        assert_eq!(delta.changed.len(), 1);
        assert!(delta.moved.is_empty());
        assert!(delta.deleted.is_empty());
    }
}
