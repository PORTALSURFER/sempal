use super::*;

/// Direction to move a sample tag when stepping between columns.
#[derive(Clone, Copy)]
pub(super) enum TagStep {
    Left,
    Right,
}

impl DropHandler {
    /// Step the selected wav entry's tag left/right across Trash ⇄ Neutral ⇄ Keep.
    pub(super) fn apply_tag_step(&self, step: TagStep) -> bool {
        let Some(source) = self.current_source() else {
            return false;
        };
        let Some((target_path, new_tag)) = self.update_tag_in_memory_step(step) else {
            return false;
        };
        self.enqueue_tag_for_flush(&source.id, target_path.clone(), new_tag);
        if let Some(app) = self.app() {
            self.update_wav_view(&app, true);
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

    fn update_tag_in_memory_step(&self, step: TagStep) -> Option<(PathBuf, SampleTag)> {
        let mut entries = self.wav_entries.borrow_mut();
        if entries.is_empty() {
            return None;
        }
        let selected_index = Self::entry_index(&entries, &self.selected_wav.borrow()).unwrap_or(0);
        let entry = entries.get_mut(selected_index)?;
        let new_tag = step_tag(entry.tag, step);
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

/// Move the tag one step left/right across Trash ⇄ Neutral ⇄ Keep, clamping at the ends.
fn step_tag(current: SampleTag, step: TagStep) -> SampleTag {
    let current_index = match current {
        SampleTag::Trash => 0,
        SampleTag::Neutral => 1,
        SampleTag::Keep => 2,
    };
    let delta = match step {
        TagStep::Left => -1,
        TagStep::Right => 1,
    };
    match (current_index as i8 + delta).clamp(0, 2) {
        0 => SampleTag::Trash,
        1 => SampleTag::Neutral,
        _ => SampleTag::Keep,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_tag_moves_left_and_right() {
        assert_eq!(
            step_tag(SampleTag::Neutral, TagStep::Left),
            SampleTag::Trash
        );
        assert_eq!(
            step_tag(SampleTag::Neutral, TagStep::Right),
            SampleTag::Keep
        );
        assert_eq!(step_tag(SampleTag::Keep, TagStep::Left), SampleTag::Neutral);
        assert_eq!(
            step_tag(SampleTag::Trash, TagStep::Right),
            SampleTag::Neutral
        );
    }

    #[test]
    fn step_tag_clamps_at_edges() {
        assert_eq!(step_tag(SampleTag::Trash, TagStep::Left), SampleTag::Trash);
        assert_eq!(step_tag(SampleTag::Keep, TagStep::Right), SampleTag::Keep);
    }
}
