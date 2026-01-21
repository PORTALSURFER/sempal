use super::super::DragDropController;
use crate::egui_app::state::DragSample;
use crate::egui_app::state::TriageFlagColumn;
use crate::sample_sources::{Rating, SourceId};
use std::path::PathBuf;

impl DragDropController<'_> {
    pub(crate) fn handle_sample_drop(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        triage_target: Option<TriageFlagColumn>,
    ) {
        if let Some(column) = triage_target {
            self.selection_state.suppress_autoplay_once = true;
            let target_tag = match column {
                TriageFlagColumn::Trash => Rating::TRASH_1,
                TriageFlagColumn::Neutral => Rating::NEUTRAL,
                TriageFlagColumn::Keep => Rating::KEEP_1,
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
        triage_target: Option<TriageFlagColumn>,
    ) {
        for sample in samples {
            self.handle_sample_drop(
                sample.source_id.clone(),
                sample.relative_path.clone(),
                triage_target,
            );
        }
    }
}
