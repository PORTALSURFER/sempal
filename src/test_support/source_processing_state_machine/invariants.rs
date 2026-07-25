use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use wavecrate_scan::CommittedSourceDelta;

use crate::sample_sources::SourceDatabase;

use super::{
    Event, FailureBoundary, FailureSnapshot, ScanCause, StateMachineHarness, generated_path,
};

const MAX_PENDING_WATCHER_PATHS: usize = 16;

impl StateMachineHarness {
    pub(super) fn accept_commit(
        &mut self,
        cause: ScanCause,
        delta: &CommittedSourceDelta,
        publication_lost: bool,
    ) -> Result<(), String> {
        let revision = delta.revision;
        if revision < self.last_revision {
            return Err(format!(
                "accepted source revision regressed from {} to {revision}",
                self.last_revision
            ));
        }
        if revision > self.last_revision {
            self.accepted_revisions
                .push(format!("{}:{revision}", self.model.lifecycle_generation));
            self.last_revision = revision;
        }
        if !delta.is_empty() && !publication_lost {
            if !self.accepted_publications.insert((
                self.model.lifecycle_generation,
                revision,
                cause,
            )) {
                return Err(format!(
                    "lifecycle {} revision {revision} was published more than once for {cause:?}",
                    self.model.lifecycle_generation
                ));
            }
            self.observable_commits = self.observable_commits.saturating_add(1);
        }
        Ok(())
    }

    pub(super) fn assert_committed_manifest(
        &self,
        database: &SourceDatabase,
    ) -> Result<(), String> {
        let committed = database
            .list_files()
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|entry| !entry.missing)
            .map(|entry| {
                (
                    slash_path(&entry.relative_path),
                    entry
                        .content_hash
                        .unwrap_or_else(|| String::from("<pending>")),
                )
            })
            .collect::<BTreeMap<_, _>>();
        if committed != self.model.files {
            return Err(format!(
                "committed manifest differs from reference filesystem model: expected={:?}, actual={committed:?}",
                self.model.files
            ));
        }
        let browser_projection =
            crate::app::controller::library::wavs::browser_search_worker::project_source_paths_for_state_machine(
                &self.source,
            )?;
        let expected_projection = self.model.files.keys().cloned().collect::<BTreeSet<_>>();
        if browser_projection != expected_projection {
            return Err(format!(
                "production browser projection differs from reference model: expected={expected_projection:?}, actual={browser_projection:?}"
            ));
        }
        Ok(())
    }

    pub(super) fn assert_queue_bound(&self) -> Result<(), String> {
        if self.model.watcher_paths.len() > MAX_PENDING_WATCHER_PATHS {
            return Err(format!(
                "coalesced watcher path count {} exceeded generated fixture bound {MAX_PENDING_WATCHER_PATHS}",
                self.model.watcher_paths.len()
            ));
        }
        Ok(())
    }

    pub(super) fn database(&self) -> Result<SourceDatabase, String> {
        self.source.open_db().map_err(|error| error.to_string())
    }

    pub(super) fn record_model_path(&mut self, relative: &str) -> Result<(), String> {
        let bytes = fs::read(self.source.root.join(relative))
            .map_err(|error| format!("read modeled file {relative}: {error}"))?;
        self.model.files.insert(
            relative.to_string(),
            blake3::hash(&bytes).to_hex().to_string(),
        );
        Ok(())
    }

    pub(super) fn generated_slot_path(&self, slot: u8) -> Option<String> {
        [generated_path(slot, false), generated_path(slot, true)]
            .into_iter()
            .find(|path| self.model.files.contains_key(path))
    }

    pub(super) fn require_online(&self) -> Result<(), String> {
        if !self.model.root_online || !self.source.root.is_dir() {
            return Err(String::from(
                "generated operation escaped an online fixture root",
            ));
        }
        Ok(())
    }

    pub(super) fn take_failure(&mut self, boundary: FailureBoundary) -> bool {
        if self.next_failure == Some(boundary) {
            self.next_failure = None;
            true
        } else {
            false
        }
    }

    pub(super) fn failure(
        &self,
        event_index: usize,
        event: &Event,
        message: String,
    ) -> FailureSnapshot {
        FailureSnapshot {
            message,
            event_index,
            event: event.clone(),
            model: serde_json::to_value(&self.model).unwrap_or_default(),
            accepted_revisions: self.accepted_revisions.clone(),
            accepted_publications: self
                .accepted_publications
                .iter()
                .map(|(lifecycle, revision, cause)| format!("{lifecycle}:{revision}:{cause:?}"))
                .collect(),
            observable_commits: self.observable_commits,
            runtime: self.supervisor.as_ref().map(|supervisor| {
                let observation = super::super::liveness_tests::runtime_observation(
                    supervisor,
                    self.source.id.as_str(),
                );
                let readiness = super::super::liveness_tests::readiness_snapshot(&self.source).map(
                    |snapshot| {
                        serde_json::json!({
                            "availability": format!("{:?}", snapshot.availability),
                            "activity": format!("{:?}", snapshot.activity),
                            "source_generation": snapshot.source_generation,
                            "readiness_revision": snapshot.readiness_revision,
                            "deficit_count": snapshot.deficits.len(),
                        })
                    },
                );
                serde_json::json!({
                    "observation": format!("{observation:?}"),
                    "health": readiness,
                })
            }),
        }
    }
}

pub(super) fn filesystem_inventory(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut inventory = BTreeMap::new();
    collect_files(root, root, &mut inventory)?;
    Ok(inventory)
}

fn collect_files(
    root: &Path,
    directory: &Path,
    inventory: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| format!("read fixture directory {}: {error}", directory.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read fixture entry: {error}"))?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|error| format!("classify fixture entry: {error}"))?;
        let path = entry.path();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_files(root, &path, inventory)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
        {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| format!("confine fixture path: {error}"))?;
            let bytes = fs::read(&path)
                .map_err(|error| format!("read fixture WAV {}: {error}", path.display()))?;
            inventory.insert(
                slash_path(relative),
                blake3::hash(&bytes).to_hex().to_string(),
            );
        }
    }
    Ok(())
}

pub(super) fn perturb(bytes: &mut [u8], salt: u8) {
    if let Some(byte) = bytes.get_mut(44) {
        *byte ^= salt.wrapping_mul(31).wrapping_add(1);
    }
}

fn slash_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
