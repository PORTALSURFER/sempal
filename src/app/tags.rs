use super::*;

impl DropHandler {
    /// Apply a keep/trash/neutral tag to the selected wav entry.
    pub(super) fn apply_tag_to_selection(&self, tag: SampleTag) -> bool {
        let Some(source) = self.current_source() else {
            return false;
        };
        let Some((target_path, new_tag)) = self.update_tag_in_memory(tag) else {
            return false;
        };
        self.enqueue_tag_for_flush(&source.id, target_path.clone(), new_tag);
        if let Some(app) = self.app() {
            self.update_wav_view(&app);
            let label = match new_tag {
                SampleTag::Keep => "Marked keep",
                SampleTag::Trash => "Marked trash",
                SampleTag::Neutral => "Cleared tag",
            };
            self.set_status(
                &app,
                format!("{label} for {}", target_path.display()),
                StatusState::Info,
            );
        }
        true
    }

    fn update_tag_in_memory(&self, desired_tag: SampleTag) -> Option<(PathBuf, SampleTag)> {
        let mut entries = self.wav_entries.borrow_mut();
        if entries.is_empty() {
            return None;
        }
        let selected_index = Self::entry_index(&entries, &self.selected_wav.borrow()).unwrap_or(0);
        let entry = entries.get_mut(selected_index)?;
        let new_tag = toggle_tag(entry.tag, desired_tag);
        let path = entry.relative_path.clone();
        entry.tag = new_tag;
        if self.selected_wav.borrow().is_none() {
            self.selected_wav.borrow_mut().replace(path.clone());
        }
        Some((path, new_tag))
    }

    fn enqueue_tag_for_flush(&self, source_id: &SourceId, path: PathBuf, tag: SampleTag) {
        {
            let mut pending = self.pending_tags.borrow_mut();
            pending
                .entry(source_id.clone())
                .or_default()
                .push((path, tag));
        }
        self.start_tag_flush_timer();
    }

    fn start_tag_flush_timer(&self) {
        if *self.shutting_down.borrow() {
            return;
        }
        let flusher = self.clone();
        self.tag_flush_timer.start(
            slint::TimerMode::SingleShot,
            Duration::from_millis(40),
            move || flusher.flush_pending_tags(),
        );
    }

    /// Persist any queued tag updates to the appropriate source databases.
    pub(super) fn flush_pending_tags(&self) {
        self.tag_flush_timer.stop();
        let pending = std::mem::take(&mut *self.pending_tags.borrow_mut());
        if pending.is_empty() {
            return;
        }
        let mut errors = Vec::new();
        for (source_id, updates) in pending {
            if updates.is_empty() {
                continue;
            }
            let Some(source) = self
                .sources
                .borrow()
                .iter()
                .find(|s| s.id == source_id)
                .cloned()
            else {
                continue;
            };
            let deduped = coalesce_tag_updates(updates);
            if let Err(error) = self
                .database_for(&source)
                .and_then(|db| db.set_tags_batch(&deduped))
            {
                errors.push(error);
            }
        }
        if !errors.is_empty() {
            if let Some(app) = self.app() {
                let text = errors
                    .into_iter()
                    .map(|err| err.to_string())
                    .collect::<Vec<_>>()
                    .join("; ");
                self.set_status(
                    &app,
                    format!("Failed to save tags: {text}"),
                    StatusState::Error,
                );
            }
        }
    }
}

/// Keep only the latest tag per path to minimize DB writes.
fn coalesce_tag_updates(updates: Vec<(PathBuf, SampleTag)>) -> Vec<(PathBuf, SampleTag)> {
    let mut latest: HashMap<PathBuf, SampleTag> = HashMap::new();
    for (path, tag) in updates {
        latest.insert(path, tag);
    }
    latest.into_iter().collect()
}

/// Toggle tags so repeating the same choice clears back to neutral.
fn toggle_tag(current: SampleTag, desired: SampleTag) -> SampleTag {
    if current == desired {
        SampleTag::Neutral
    } else {
        desired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_tag_toggles_to_neutral_on_repeat() {
        assert_eq!(
            toggle_tag(SampleTag::Neutral, SampleTag::Keep),
            SampleTag::Keep
        );
        assert_eq!(
            toggle_tag(SampleTag::Keep, SampleTag::Keep),
            SampleTag::Neutral
        );
        assert_eq!(
            toggle_tag(SampleTag::Neutral, SampleTag::Trash),
            SampleTag::Trash
        );
        assert_eq!(
            toggle_tag(SampleTag::Trash, SampleTag::Trash),
            SampleTag::Neutral
        );
        assert_eq!(
            toggle_tag(SampleTag::Keep, SampleTag::Trash),
            SampleTag::Trash
        );
    }
}
