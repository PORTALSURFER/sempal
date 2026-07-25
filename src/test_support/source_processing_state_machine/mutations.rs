use std::{fs, sync::atomic::AtomicBool};

use wavecrate_scan::{ScanError, ScanMode, scan_with_progress};

use super::{
    FailureBoundary, ScanCause, StateMachineHarness, generated_path,
    harness::start_observed_supervisor,
    invariants::{filesystem_inventory, perturb},
};

impl StateMachineHarness {
    pub(super) fn create(&mut self, slot: u8, nested: bool) -> Result<(), String> {
        self.require_online()?;
        let relative = generated_path(slot, nested);
        let path = self.source.root.join(&relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create generated source directory: {error}"))?;
        }
        let mut bytes = self.template.clone();
        perturb(&mut bytes, slot);
        fs::write(&path, bytes).map_err(|error| format!("create {relative}: {error}"))?;
        self.record_model_path(&relative)?;
        self.model.queue_path(relative);
        self.assert_queue_bound()
    }

    pub(super) fn modify(&mut self, slot: u8) -> Result<(), String> {
        self.require_online()?;
        let relative = self
            .generated_slot_path(slot)
            .unwrap_or_else(|| generated_path(slot, false));
        let path = self.source.root.join(&relative);
        if !path.is_file() {
            self.create(slot, false)?;
        }
        let mut bytes = fs::read(&path).map_err(|error| format!("read {relative}: {error}"))?;
        let original_len = bytes.len();
        perturb(
            &mut bytes,
            slot.wrapping_add(self.model.retry_count as u8)
                .wrapping_add(17),
        );
        fs::write(&path, bytes).map_err(|error| format!("modify {relative}: {error}"))?;
        if fs::metadata(&path)
            .map_err(|error| error.to_string())?
            .len()
            != original_len as u64
        {
            return Err(format!("same-size mutation changed length for {relative}"));
        }
        self.record_model_path(&relative)?;
        self.model.queue_path(relative);
        self.assert_queue_bound()
    }

    pub(super) fn move_file(&mut self, slot: u8, nested: bool) -> Result<(), String> {
        self.require_online()?;
        let source_relative = self
            .generated_slot_path(slot)
            .unwrap_or_else(|| generated_path(slot, !nested));
        if !self.source.root.join(&source_relative).is_file() {
            self.create(slot, !nested)?;
        }
        let destination_relative = generated_path(slot, nested);
        if source_relative == destination_relative {
            return Ok(());
        }
        let destination = self.source.root.join(&destination_relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create move destination: {error}"))?;
        }
        if destination.exists() {
            fs::remove_file(&destination)
                .map_err(|error| format!("replace move destination: {error}"))?;
        }
        fs::rename(self.source.root.join(&source_relative), &destination).map_err(|error| {
            format!("move {source_relative} to {destination_relative}: {error}")
        })?;
        let hash = self
            .model
            .files
            .remove(&source_relative)
            .ok_or_else(|| format!("reference model lost move source {source_relative}"))?;
        self.model.files.insert(destination_relative.clone(), hash);
        self.model.queue_path(source_relative);
        self.model.queue_path(destination_relative);
        self.assert_queue_bound()
    }

    pub(super) fn delete(&mut self, slot: u8) -> Result<(), String> {
        self.require_online()?;
        let Some(relative) = self.generated_slot_path(slot) else {
            return Ok(());
        };
        fs::remove_file(self.source.root.join(&relative))
            .map_err(|error| format!("delete {relative}: {error}"))?;
        self.model.files.remove(&relative);
        self.model.queue_path(relative);
        self.assert_queue_bound()
    }

    pub(super) fn cancel_scan(&mut self) -> Result<(), String> {
        let database = self.database()?;
        let cancel = AtomicBool::new(true);
        match scan_with_progress(&database, ScanMode::Quick, Some(&cancel), &mut |_, _| {}) {
            Err(ScanError::Canceled) | Err(ScanError::Incomplete { .. }) => {}
            Ok(_) => return Err(String::from("pre-cancelled scan unexpectedly completed")),
            Err(error) => return Err(format!("pre-cancelled scan failed unexpectedly: {error}")),
        }
        if let Some(supervisor) = &self.supervisor {
            supervisor
                .cancel_foreground_source_scan(self.source.id.as_str(), "state_machine_cancel");
        }
        self.model.queue(ScanCause::Retry);
        self.model.retry_count = self.model.retry_count.saturating_add(1);
        Ok(())
    }

    pub(super) fn shutdown_restart(&mut self) -> Result<(), String> {
        if let Some(mut supervisor) = self.supervisor.take() {
            let lifecycle_generation = supervisor
                .lifecycle_generations()
                .get(self.source.id.as_str())
                .copied();
            let report = supervisor.shutdown();
            if report["joined"] != true {
                return Err(String::from(
                    "supervisor failed to join during replayable restart",
                ));
            }
            self.collect_publications_from(&supervisor)?;
            if let Some(lifecycle_generation) = lifecycle_generation {
                self.mark_outstanding_publications_stale(lifecycle_generation);
            }
            let (supervisor, events) = start_observed_supervisor(&self.source);
            self.supervisor = Some(supervisor);
            self.supervisor_events = Some(events);
        }
        self.model.restart_count = self.model.restart_count.saturating_add(1);
        self.model.queue(ScanCause::Restart);
        Ok(())
    }

    pub(super) fn remove_readd(&mut self) -> Result<(), String> {
        let reject_lifecycle = self.take_failure(FailureBoundary::Lifecycle);
        if reject_lifecycle && let Some(supervisor) = &self.supervisor {
            let generation_before = supervisor
                .lifecycle_generations()
                .get(self.source.id.as_str())
                .copied();
            supervisor.reject_next_source_replacement_for_state_machine();
            if supervisor.replace_sources(Vec::new()).is_ok() {
                return Err(String::from(
                    "lifecycle boundary accepted an injected replacement failure",
                ));
            }
            let generation_after = supervisor
                .lifecycle_generations()
                .get(self.source.id.as_str())
                .copied();
            if generation_after != generation_before {
                return Err(String::from(
                    "rejected lifecycle replacement advanced the active generation",
                ));
            }
            self.model.retry_count = self.model.retry_count.saturating_add(1);
        }
        self.model.source_configured = false;
        let lifecycle_generation = self.supervisor.as_ref().and_then(|supervisor| {
            supervisor
                .lifecycle_generations()
                .get(self.source.id.as_str())
                .copied()
        });
        if let Some(supervisor) = &self.supervisor {
            supervisor.replace_sources(Vec::new())?;
        }
        self.collect_actual_publications()?;
        if let Some(lifecycle_generation) = lifecycle_generation {
            self.mark_outstanding_publications_stale(lifecycle_generation);
        }
        if let Some(supervisor) = &self.supervisor {
            supervisor.replace_sources(vec![self.source.clone()])?;
        }
        self.model.source_configured = true;
        self.model.lifecycle_generation = self.model.lifecycle_generation.saturating_add(1);
        self.model.queue(ScanCause::Lifecycle);
        Ok(())
    }

    pub(super) fn root_offline_online(&mut self) -> Result<(), String> {
        self.require_online()?;
        let offline = self.source.root.with_extension(format!(
            "state-machine-offline-{}",
            self.model.restart_count
        ));
        if offline.exists() {
            fs::remove_dir_all(&offline)
                .map_err(|error| format!("clear prior offline root: {error}"))?;
        }
        fs::rename(&self.source.root, &offline)
            .map_err(|error| format!("take source root offline: {error}"))?;
        self.model.root_online = false;
        let injected_failure = self.take_failure(FailureBoundary::Lifecycle);
        if let Some(supervisor) = &self.supervisor {
            supervisor.wake_source_for_full_reconciliation(
                self.source.id.as_str(),
                "state_machine_root_offline",
            );
            self.wait_for_supervisor_offline()?;
        }
        fs::rename(&offline, &self.source.root)
            .map_err(|error| format!("restore source root: {error}"))?;
        self.model.root_online = true;
        if injected_failure {
            self.model.retry_count = self.model.retry_count.saturating_add(1);
        }
        if let Some(supervisor) = &self.supervisor {
            supervisor.wake_source_for_full_reconciliation(
                self.source.id.as_str(),
                if injected_failure {
                    "state_machine_root_online_retry"
                } else {
                    "state_machine_root_online"
                },
            );
            self.wait_for_supervisor_online_terminal()?;
        }
        self.model.queue(ScanCause::Lifecycle);
        Ok(())
    }

    pub(super) fn root_replacement(&mut self) -> Result<(), String> {
        self.require_online()?;
        let retired = self.source.root.with_extension(format!(
            "state-machine-retired-{}",
            self.retired_roots.len()
        ));
        fs::rename(&self.source.root, &retired)
            .map_err(|error| format!("retire fixture source root: {error}"))?;
        fs::create_dir_all(&self.source.root)
            .map_err(|error| format!("create replacement source root: {error}"))?;
        let replacement = self.source.root.join("replacement.wav");
        fs::write(&replacement, &self.template)
            .map_err(|error| format!("write replacement source file: {error}"))?;
        self.retired_roots.push(retired);
        self.model.files = filesystem_inventory(&self.source.root)?;
        self.model.lifecycle_generation = self.model.lifecycle_generation.saturating_add(1);
        self.source.open_db().map_err(|error| error.to_string())?;
        let previous_lifecycle = self.supervisor.as_ref().and_then(|supervisor| {
            supervisor
                .lifecycle_generations()
                .get(self.source.id.as_str())
                .copied()
        });
        if let Some(supervisor) = &self.supervisor {
            supervisor.replace_sources(vec![self.source.clone()])?;
            supervisor.wake_source_for_full_reconciliation(
                self.source.id.as_str(),
                "state_machine_root_replacement",
            );
        }
        self.collect_actual_publications()?;
        if let Some(previous_lifecycle) = previous_lifecycle {
            self.mark_outstanding_publications_stale(previous_lifecycle);
        }
        self.pending_publication_retries.clear();
        self.last_revision = 0;
        self.model.queue(ScanCause::Lifecycle);
        Ok(())
    }

    pub(super) fn partial_enumeration(&mut self) -> Result<(), String> {
        self.create(7, true)?;
        self.next_failure = Some(FailureBoundary::Hashing);
        self.flush(ScanCause::Foreground)
    }

    pub(super) fn symlink_escape(&mut self) -> Result<(), String> {
        self.require_online()?;
        let link = self.source.root.join("generated/escape-link");
        if let Some(parent) = link.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create symlink regression parent: {error}"))?;
        }
        if link.symlink_metadata().is_ok() {
            fs::remove_file(&link)
                .map_err(|error| format!("replace symlink regression fixture: {error}"))?;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&self.escape_target, &link)
            .map_err(|error| format!("create confined symlink escape fixture: {error}"))?;
        self.model.queue(ScanCause::WatcherOverflow);
        Ok(())
    }
}
