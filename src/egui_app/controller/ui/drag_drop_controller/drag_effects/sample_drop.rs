use super::super::DragDropController;
use crate::egui_app::state::DragSample;
use crate::egui_app::state::TriageFlagColumn;
use crate::egui_app::ui::style::StatusTone;
use crate::sample_sources::{CollectionId, SampleTag, SourceId};
use std::path::PathBuf;

impl DragDropController<'_> {
    pub(crate) fn handle_sample_drop(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        collection_target: Option<CollectionId>,
        triage_target: Option<TriageFlagColumn>,
        move_to_collection: bool,
    ) {
        if let Some(collection_id) = collection_target {
            if move_to_collection {
                let Some(source) = self
                    .library
                    .sources
                    .iter()
                    .find(|s| s.id == source_id)
                    .cloned()
                else {
                    self.set_status("Source not available for move", StatusTone::Error);
                    return;
                };
                match self.move_sample_to_collection_for_source(
                    &collection_id,
                    &source,
                    &relative_path,
                ) {
                    Ok(name) => {
                        self.set_status(format!("Moved sample to '{name}'"), StatusTone::Info)
                    }
                    Err(err) => self.set_status(err, StatusTone::Error),
                }
            } else if let Some(source) = self
                .library
                .sources
                .iter()
                .find(|s| s.id == source_id)
                .cloned()
            {
                if let Err(err) = self.add_sample_to_collection_for_source(
                    &collection_id,
                    &source,
                    &relative_path,
                ) {
                    self.set_status(err, StatusTone::Error);
                }
            } else if let Err(err) = self.add_sample_to_collection(&collection_id, &relative_path) {
                self.set_status(err, StatusTone::Error);
            }
            return;
        }
        if let Some(column) = triage_target {
            self.selection_state.suppress_autoplay_once = true;
            let target_tag = match column {
                TriageFlagColumn::Trash => SampleTag::Trash,
                TriageFlagColumn::Neutral => SampleTag::Neutral,
                TriageFlagColumn::Keep => SampleTag::Keep,
            };
            if let Some(source) = self
                .library
                .sources
                .iter()
                .find(|s| s.id == source_id)
                .cloned()
            {
                let _ = self.set_sample_tag_for_source(&source, &relative_path, target_tag, false);
            } else {
                let _ = self.set_sample_tag(&relative_path, column);
            }
        }
    }

    pub(crate) fn handle_samples_drop(
        &mut self,
        samples: &[DragSample],
        collection_target: Option<CollectionId>,
        triage_target: Option<TriageFlagColumn>,
        move_to_collection: bool,
    ) {
        for sample in samples {
            self.handle_sample_drop(
                sample.source_id.clone(),
                sample.relative_path.clone(),
                collection_target.clone(),
                triage_target,
                move_to_collection,
            );
        }
    }
}
