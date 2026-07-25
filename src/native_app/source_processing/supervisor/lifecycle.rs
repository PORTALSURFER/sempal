use super::{
    Arc, AtomicBool, BTreeMap, BTreeSet, DatabasePhase, Ordering, PendingSourceRetirement,
    SampleSource, SourceProcessingSupervisor, source_descriptors_match, source_maps_match,
    source_storage_identity_matches, sources_by_id,
};

impl SourceProcessingSupervisor {
    pub(in crate::native_app) fn replace_sources(
        &self,
        sources: Vec<SampleSource>,
    ) -> Result<(), String> {
        let _replacement = self
            .shared
            .source_replacement
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let sources = sources_by_id(sources);
        let mut control = self.shared.control();
        #[cfg(test)]
        if std::mem::take(&mut control.reject_next_source_replacement) {
            return Err(String::from(
                "state-machine lifecycle replacement rejected at serialized boundary",
            ));
        }
        if source_maps_match(&control.sources, &sources) {
            if !control.quarantined_sources.is_empty() {
                control.quarantined_sources.clear();
                control.reset_source_work_tokens();
                control.mark_all_sources_dirty("configured_sources_reactivated");
                drop(control);
                self.shared.budget_wake.notify_all();
                self.shared.wake.notify_all();
            }
            return Ok(());
        }
        let changed_source_ids = control
            .sources
            .iter()
            .filter_map(|(source_id, current)| {
                let changed = sources
                    .get(source_id)
                    .is_none_or(|replacement| !source_descriptors_match(current, replacement));
                changed.then(|| source_id.clone())
            })
            .chain(sources.iter().filter_map(|(source_id, replacement)| {
                let changed = control
                    .sources
                    .get(source_id)
                    .is_none_or(|current| !source_descriptors_match(current, replacement));
                changed.then(|| source_id.clone())
            }))
            .collect::<std::collections::BTreeSet<_>>();
        let retired_sources = changed_source_ids
            .iter()
            .filter_map(|source_id| {
                Some((
                    control.sources.get(source_id)?.clone(),
                    *control.source_lifecycle_generations.get(source_id)?,
                ))
            })
            .collect::<Vec<_>>();
        for source_id in &changed_source_ids {
            if let Some(cancel) = control.source_work_cancels.get(source_id) {
                cancel.store(true, Ordering::Release);
            }
        }
        // Signal foreground scans before waiting for their publication permit, then serialize
        // the lifecycle generation transition itself. A publisher already holding the fence
        // commits in the old lifecycle; every later publisher observes the cancelled old token.
        drop(control);
        self.shared.cancel_external_scans(|registration| {
            changed_source_ids.contains(&registration.source_id)
        });
        let _publication_fence = self
            .shared
            .database_writer
            .lock(DatabasePhase::SerialCompatibility);
        let mut control = self.shared.control();
        for (source, lifecycle_generation) in retired_sources {
            let retirement_id = control.next_retirement_id;
            control.next_retirement_id = control.next_retirement_id.wrapping_add(1).max(1);
            control.pending_retirements.insert(
                retirement_id,
                PendingSourceRetirement {
                    source,
                    lifecycle_generation,
                    cancel: Arc::new(AtomicBool::new(false)),
                    retry_at: 0,
                    attempts: 0,
                    terminal_offline: false,
                },
            );
        }
        let mut source_work_cancels = BTreeMap::new();
        let mut source_lifecycle_generations = BTreeMap::new();
        for (source_id, source) in &sources {
            let unchanged = control
                .sources
                .get(source_id)
                .is_some_and(|current| source_descriptors_match(current, source));
            let cancel = if unchanged {
                control.source_work_cancels.get(source_id).cloned()
            } else {
                None
            }
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
            let lifecycle_generation = if unchanged {
                control
                    .source_lifecycle_generations
                    .get(source_id)
                    .copied()
                    .unwrap_or_else(|| control.allocate_lifecycle_generation())
            } else {
                control.allocate_lifecycle_generation()
            };
            source_work_cancels.insert(source_id.clone(), cancel);
            source_lifecycle_generations.insert(source_id.clone(), lifecycle_generation);
        }
        for retirement in control.pending_retirements.values() {
            if sources
                .values()
                .any(|active| source_storage_identity_matches(active, &retirement.source))
            {
                retirement.cancel.store(true, Ordering::Release);
            }
        }
        control.sources = sources;
        control.source_work_cancels = source_work_cancels;
        control.source_lifecycle_generations = source_lifecycle_generations;
        control.quarantined_sources.clear();
        let retained_source_ids = control.sources.keys().cloned().collect::<BTreeSet<_>>();
        control
            .force_manifest_audit_sources
            .retain(|source_id| retained_source_ids.contains(source_id));
        control
            .force_reanalysis_sources
            .retain(|source_id| retained_source_ids.contains(source_id));
        control.force_manifest_audit_sources.extend(
            changed_source_ids
                .iter()
                .filter(|source_id| retained_source_ids.contains(*source_id))
                .cloned(),
        );
        control
            .dirty_sources
            .retain(|source_id| retained_source_ids.contains(source_id));
        control.safety_probe_sources.retain(|source_id| {
            retained_source_ids.contains(source_id) && !changed_source_ids.contains(source_id)
        });
        control
            .deferred_lifecycle_audit_sources
            .retain(|source_id| retained_source_ids.contains(source_id));
        control.pending_readiness_deltas.retain(|source_id, _| {
            retained_source_ids.contains(source_id) && !changed_source_ids.contains(source_id)
        });
        control
            .awaiting_foreground_refresh_sources
            .retain(|source_id| {
                retained_source_ids.contains(source_id) && !changed_source_ids.contains(source_id)
            });
        control.dirty_sources.extend(
            changed_source_ids
                .iter()
                .filter(|source_id| retained_source_ids.contains(*source_id))
                .cloned(),
        );
        control.priority.immediate.retain(|priority| {
            retained_source_ids.contains(&priority.source_id)
                && !changed_source_ids.contains(&priority.source_id)
        });
        control.priority.visible.retain(|priority| {
            retained_source_ids.contains(&priority.source_id)
                && !changed_source_ids.contains(&priority.source_id)
        });
        control.priority.immediate_paths.retain(|(source_id, _)| {
            retained_source_ids.contains(source_id) && !changed_source_ids.contains(source_id)
        });
        control.priority.visible_paths.retain(|(source_id, _)| {
            retained_source_ids.contains(source_id) && !changed_source_ids.contains(source_id)
        });
        if control
            .priority
            .selected_source
            .as_ref()
            .is_some_and(|source_id| {
                !retained_source_ids.contains(source_id) || changed_source_ids.contains(source_id)
            })
        {
            control.priority.selected_source = None;
        }
        if control
            .priority
            .current_folder
            .as_ref()
            .is_some_and(|(source_id, _)| {
                !retained_source_ids.contains(source_id) || changed_source_ids.contains(source_id)
            })
        {
            control.priority.current_folder = None;
        }
        self.shared
            .published_source_health
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .retain(|source_id, health| {
                control.source_lifecycle_generations.get(source_id)
                    == Some(&health.lifecycle.generation)
            });
        control.notify("configured_sources_changed");
        drop(control);
        self.shared.budget_wake.notify_all();
        self.shared.wake.notify_all();
        self.shared.retirement_wake.notify_all();
        Ok(())
    }
}
