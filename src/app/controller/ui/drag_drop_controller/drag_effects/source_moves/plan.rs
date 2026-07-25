use super::worker::run_source_move_task;
use crate::app::controller::StatusTone;
use crate::app::controller::jobs::{FileOpMessage, FileOpResult, SourceMoveRequest};
use crate::app::controller::ui::drag_drop_controller::DragDropController;
use crate::app::state::{DragSample, ProgressTaskKind};
use crate::sample_sources::SourceId;
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

impl DragDropController<'_> {
    /// Handle a single dragged sample dropped onto another source.
    pub(crate) fn handle_sample_drop_to_source(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        target_source_id: SourceId,
    ) -> bool {
        let sample = DragSample {
            source_id,
            relative_path,
        };
        self.handle_samples_drop_to_source(std::slice::from_ref(&sample), target_source_id);
        true
    }

    /// Handle multiple dragged samples dropped onto another source.
    pub(crate) fn handle_samples_drop_to_source(
        &mut self,
        samples: &[DragSample],
        target_source_id: SourceId,
    ) {
        if samples.is_empty() {
            return;
        }
        if self.runtime.jobs.file_ops_in_progress() {
            self.set_status(
                "Another file operation is already running",
                StatusTone::Warning,
            );
            return;
        }
        if samples
            .iter()
            .all(|sample| sample.source_id == target_source_id)
        {
            self.set_status("Samples are already in that source", StatusTone::Info);
            return;
        }
        let Some(target_source) = self
            .library
            .sources
            .iter()
            .find(|source| source.id == target_source_id)
            .cloned()
        else {
            self.set_status("Target source not available for move", StatusTone::Error);
            return;
        };
        let (requests, errors) = self.collect_source_move_requests(samples);
        if requests.is_empty() {
            if let Some(err) = errors.first() {
                self.set_status(err.clone(), StatusTone::Error);
            }
            return;
        }
        self.set_status("Moving samples...", StatusTone::Busy);
        self.show_status_progress(
            ProgressTaskKind::FileOps,
            "Moving samples",
            requests.len(),
            true,
        );
        self.spawn_source_move_job(
            target_source_id,
            target_source.root.clone(),
            requests,
            errors,
        );
    }

    /// Resolve drag samples into worker requests and collect missing-source errors.
    fn collect_source_move_requests(
        &self,
        samples: &[DragSample],
    ) -> (Vec<SourceMoveRequest>, Vec<String>) {
        let mut requests = Vec::new();
        let mut errors = Vec::new();
        for sample in samples {
            let Some(source) = self
                .library
                .sources
                .iter()
                .find(|source| source.id == sample.source_id)
                .cloned()
            else {
                errors.push(format!(
                    "Source not available for move: {}",
                    sample.relative_path.display()
                ));
                continue;
            };
            requests.push(SourceMoveRequest {
                source_id: source.id,
                source_root: source.root,
                relative_path: sample.relative_path.clone(),
            });
        }
        (requests, errors)
    }

    /// Start the background source-move worker or run it inline during tests.
    fn spawn_source_move_job(
        &mut self,
        target_source_id: SourceId,
        target_root: PathBuf,
        requests: Vec<SourceMoveRequest>,
        errors: Vec<String>,
    ) {
        let cancel = Arc::new(AtomicBool::new(false));
        #[cfg(test)]
        {
            let result = run_source_move_task(
                target_source_id,
                target_root,
                requests,
                errors,
                cancel,
                None,
            );
            self.finish_source_move_job(result);
        }
        #[cfg(not(test))]
        {
            let (tx, rx) = std::sync::mpsc::channel();
            self.start_file_ops_with_remap_cancellation(rx, cancel.clone());
            std::thread::spawn(move || {
                let result = run_source_move_task(
                    target_source_id,
                    target_root,
                    requests,
                    errors,
                    cancel,
                    Some(&tx),
                );
                let _ = tx.send(FileOpMessage::Finished(FileOpResult::SourceMove(result)));
            });
        }
    }

    #[cfg(test)]
    /// Apply a test-inline worker result and clear the progress overlay.
    fn finish_source_move_job(&mut self, result: crate::app::controller::jobs::SourceMoveResult) {
        let message = FileOpMessage::Finished(FileOpResult::SourceMove(result));
        if let FileOpMessage::Finished(FileOpResult::SourceMove(result)) = message {
            self.apply_source_move_result(result);
        }
        self.clear_progress_task(ProgressTaskKind::FileOps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::controller::AppController;
    use crate::app::controller::test_support::{sample_entry, write_test_wav};
    use crate::sample_sources::{Rating, SampleSource};
    use crate::waveform::WaveformRenderer;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn dropping_to_same_source_is_a_no_op() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source_a");
        std::fs::create_dir_all(&source_root).unwrap();
        let source = SampleSource::new(source_root);
        let renderer = WaveformRenderer::new(10, 10);
        let mut controller = AppController::new(renderer, None);
        controller.library.sources.push(source.clone());
        write_test_wav(&source.root.join("one.wav"), &[0.0, 0.1, -0.1]);
        controller.cache_db(&source).unwrap();
        controller
            .ensure_sample_db_entry(&source, Path::new("one.wav"))
            .unwrap();

        controller.drag_drop().handle_sample_drop_to_source(
            source.id.clone(),
            PathBuf::from("one.wav"),
            source.id.clone(),
        );

        assert!(source.root.join("one.wav").is_file());
        assert!(controller.ui.progress.task.is_none());
    }

    #[test]
    fn moving_multiple_samples_to_source_transfers_files() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source_a");
        let target_root = temp.path().join("source_b");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::create_dir_all(&target_root).unwrap();
        let source = SampleSource::new(source_root);
        let target = SampleSource::new(target_root);
        let renderer = WaveformRenderer::new(10, 10);
        let mut controller = AppController::new(renderer, None);
        controller.library.sources.push(source.clone());
        controller.library.sources.push(target.clone());
        controller.selection_state.ctx.selected_source = Some(source.id.clone());
        controller.cache_db(&source).unwrap();
        controller.cache_db(&target).unwrap();
        write_test_wav(&source.root.join("one.wav"), &[0.0, 0.1, -0.1]);
        write_test_wav(&source.root.join("two.wav"), &[0.0, 0.1, -0.1]);
        controller
            .ensure_sample_db_entry(&source, Path::new("one.wav"))
            .unwrap();
        controller
            .ensure_sample_db_entry(&source, Path::new("two.wav"))
            .unwrap();
        controller.set_wav_entries_for_tests(vec![
            sample_entry("one.wav", Rating::NEUTRAL),
            sample_entry("two.wav", Rating::NEUTRAL),
        ]);
        controller.rebuild_wav_lookup();
        controller.rebuild_browser_lists();

        let samples = vec![
            DragSample {
                source_id: source.id.clone(),
                relative_path: PathBuf::from("one.wav"),
            },
            DragSample {
                source_id: source.id.clone(),
                relative_path: PathBuf::from("two.wav"),
            },
        ];
        controller
            .drag_drop()
            .handle_samples_drop_to_source(&samples, target.id.clone());

        assert!(!source.root.join("one.wav").exists());
        assert!(!source.root.join("two.wav").exists());
        assert!(target.root.join("one.wav").is_file());
        assert!(target.root.join("two.wav").is_file());
    }
}
