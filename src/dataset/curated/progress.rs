/// Progress update emitted during curated dataset ingestion.
#[derive(Clone, Copy, Debug)]
pub struct TrainingProgress {
    /// Human-readable stage name.
    pub stage: &'static str,
    /// Number of samples processed so far.
    pub processed: usize,
    /// Total samples expected.
    pub total: usize,
    /// Total samples skipped so far.
    pub skipped: usize,
}

pub(super) fn progress_tick(
    progress: &mut Option<&mut dyn FnMut(TrainingProgress)>,
    stage: &'static str,
    processed: usize,
    total: usize,
    skipped: usize,
) {
    if let Some(callback) = progress.as_deref_mut() {
        callback(TrainingProgress {
            stage,
            processed,
            total,
            skipped,
        });
    }
}
