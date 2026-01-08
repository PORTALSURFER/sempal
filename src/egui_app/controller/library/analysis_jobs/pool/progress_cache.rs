use crate::egui_app::controller::library::analysis_jobs::types::AnalysisProgress;
use crate::sample_sources::SourceId;
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct ProgressCache {
    per_source: HashMap<SourceId, AnalysisProgress>,
}

impl ProgressCache {
    pub(crate) fn update(&mut self, source_id: SourceId, progress: AnalysisProgress) {
        self.per_source.insert(source_id, progress);
    }

    pub(crate) fn update_many(&mut self, updates: Vec<(SourceId, AnalysisProgress)>) {
        for (source_id, progress) in updates {
            self.per_source.insert(source_id, progress);
        }
    }

    pub(crate) fn total_for_sources<'a>(
        &self,
        sources: impl Iterator<Item = &'a SourceId>,
    ) -> AnalysisProgress {
        let mut total = AnalysisProgress::default();
        for source_id in sources {
            if let Some(progress) = self.per_source.get(source_id) {
                total.pending += progress.pending;
                total.running += progress.running;
                total.done += progress.done;
                total.failed += progress.failed;
                total.samples_total += progress.samples_total;
                total.samples_pending_or_running += progress.samples_pending_or_running;
            }
        }
        total
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.per_source.is_empty()
    }
}
