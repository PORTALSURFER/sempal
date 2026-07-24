use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use radiant::prelude as ui;

use crate::native_app::sample_library::folder_browser::scan::{
    self, FolderScanDiscovery, FolderScanDiscoveryBatch, FolderScanProgress, FolderScanRequest,
    PreparedFolderScanResult, prepare_folder_scan_cache_update,
};
use wavecrate_scan::sample_sources::scanner::ScanWriter;
#[cfg(test)]
use wavecrate_scan::sample_sources::scanner::UncoordinatedScanWriter;

const DISCOVERY_BATCH_SIZE: usize = 64;

pub(in crate::native_app) enum FolderScanWorkerEvent {
    Progress(FolderScanProgress),
    DiscoveryBatch(FolderScanDiscoveryBatch),
}

pub(in crate::native_app) fn run_folder_scan_worker(
    request: FolderScanRequest,
    events: ui::BusinessEventSink<FolderScanWorkerEvent>,
    cancel: Arc<AtomicBool>,
    lifecycle_generation: Option<u64>,
    writer: &impl ScanWriter,
) -> PreparedFolderScanResult {
    run_folder_scan_worker_with_emit_and_cancel(
        request,
        move |event| events.emit(event),
        cancel.as_ref(),
        lifecycle_generation,
        writer,
    )
}

#[cfg(test)]
fn run_folder_scan_worker_with_emit(
    request: FolderScanRequest,
    emit: impl Fn(FolderScanWorkerEvent) -> bool + Clone,
) -> PreparedFolderScanResult {
    run_folder_scan_worker_with_emit_and_cancel(
        request,
        emit,
        &AtomicBool::new(false),
        None,
        &UncoordinatedScanWriter,
    )
}

fn run_folder_scan_worker_with_emit_and_cancel(
    request: FolderScanRequest,
    emit: impl Fn(FolderScanWorkerEvent) -> bool + Clone,
    cancel: &AtomicBool,
    lifecycle_generation: Option<u64>,
    writer: &impl ScanWriter,
) -> PreparedFolderScanResult {
    let rating_decay_maintenance =
        crate::native_app::sample_library::folder_browser::scan::RatingDecayMaintenanceRequest {
            source_id: request.source_id.clone(),
            root: request.root.clone(),
            database_root: request.database_root.clone(),
            rating_decay_weeks: request.rating_decay_weeks,
        };
    let mut discovery_transport = FolderScanDiscoveryTransport::new(
        emit.clone(),
        request.task_id,
        request.source_id.clone(),
        lifecycle_generation,
    );
    let scan = scan::scan_source_with_progress_cancellable(
        request,
        |progress| {
            let _ = emit(FolderScanWorkerEvent::Progress(progress));
        },
        |event| {
            discovery_transport.push(event);
        },
        cancel,
        writer,
    );
    if !cancel.load(Ordering::Acquire) {
        discovery_transport.flush();
    }
    let audio_file_paths = scan.audio_file_paths();
    let scan_cache_update = prepare_folder_scan_cache_update(&scan);
    PreparedFolderScanResult {
        scan,
        audio_file_paths,
        scan_cache_update,
        lifecycle_generation: None,
        terminal_failure: None,
        rating_decay_maintenance: Some(rating_decay_maintenance),
    }
}

struct FolderScanDiscoveryTransport<Emit> {
    emit: Emit,
    task_id: u64,
    source_id: String,
    lifecycle_generation: Option<u64>,
    pending: Vec<FolderScanDiscovery>,
    pending_revision: Option<u64>,
    next_sequence: u64,
}

impl<Emit> FolderScanDiscoveryTransport<Emit>
where
    Emit: Fn(FolderScanWorkerEvent) -> bool,
{
    fn new(emit: Emit, task_id: u64, source_id: String, lifecycle_generation: Option<u64>) -> Self {
        Self {
            emit,
            task_id,
            source_id,
            lifecycle_generation,
            pending: Vec::with_capacity(DISCOVERY_BATCH_SIZE),
            pending_revision: None,
            next_sequence: 0,
        }
    }

    fn push(&mut self, discovery: FolderScanDiscovery) {
        if !self.pending.is_empty() && self.pending_revision != discovery.committed_revision {
            self.flush();
        }
        self.pending_revision = discovery.committed_revision;
        self.pending.push(discovery);
        if self.pending.len() >= DISCOVERY_BATCH_SIZE {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let _ = (self.emit)(FolderScanWorkerEvent::DiscoveryBatch(
            FolderScanDiscoveryBatch {
                task_id: self.task_id,
                source_id: self.source_id.clone(),
                committed_revision: self.pending_revision,
                lifecycle_generation: self.lifecycle_generation,
                sequence: self.next_sequence,
                events: std::mem::take(&mut self.pending),
            },
        ));
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.pending_revision = None;
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
    };

    use crate::native_app::sample_library::folder_browser::scan::{
        FolderScanItem, FolderScanRequest, INDEX_PROGRESS_REPORT_INTERVAL,
    };
    use wavecrate_scan::sample_sources::scanner::UncoordinatedScanWriter;

    use super::{
        DISCOVERY_BATCH_SIZE, FolderScanWorkerEvent, run_folder_scan_worker_with_emit,
        run_folder_scan_worker_with_emit_and_cancel,
    };

    fn temp_source_with_wavs(count: usize) -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("source root");
        for index in 0..count {
            fs::write(
                root.path().join(format!("sample-{index:03}.wav")),
                [0_u8; 8],
            )
            .expect("write wav");
        }
        root
    }

    fn scan_request(root: &tempfile::TempDir) -> FolderScanRequest {
        FolderScanRequest {
            task_id: 42,
            source_id: String::from("source"),
            label: String::from("Source"),
            root: root.path().to_path_buf(),
            database_root: root.path().to_path_buf(),
            rating_decay_weeks: FolderScanRequest::default_rating_decay_weeks(),
        }
    }

    #[test]
    fn scan_worker_flushes_discoveries_in_batches() {
        let root = temp_source_with_wavs(DISCOVERY_BATCH_SIZE + 2);
        let (sender, receiver) = mpsc::channel();

        let result = run_folder_scan_worker_with_emit(scan_request(&root), move |event| {
            sender.send(event).is_ok()
        });

        assert_eq!(result.audio_file_paths.len(), DISCOVERY_BATCH_SIZE + 2);
        let batches = receiver
            .try_iter()
            .filter_map(|message| match message {
                FolderScanWorkerEvent::DiscoveryBatch(batch) => Some(batch.events),
                _ => None,
            })
            .collect::<Vec<_>>();
        let batch_lengths = batches.iter().map(Vec::len).collect::<Vec<_>>();
        let published_file_count = batches
            .iter()
            .flatten()
            .filter(|event| matches!(event.item, FolderScanItem::File(_)))
            .count();
        assert_eq!(batch_lengths.first().copied(), Some(DISCOVERY_BATCH_SIZE));
        assert_eq!(
            published_file_count, result.scan.file_count,
            "discovery transport should publish each scanned item once without completed-subtree clones"
        );
    }

    #[test]
    fn large_scan_worker_keeps_every_ui_discovery_message_bounded() {
        let file_count = DISCOVERY_BATCH_SIZE * 16;
        let root = temp_source_with_wavs(file_count);
        let (sender, receiver) = mpsc::channel();

        let result = run_folder_scan_worker_with_emit(scan_request(&root), move |event| {
            sender.send(event).is_ok()
        });

        let events = receiver.try_iter().collect::<Vec<_>>();
        let batches = events
            .iter()
            .filter_map(|message| match message {
                FolderScanWorkerEvent::DiscoveryBatch(batch) => Some(batch.events.as_slice()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let indexing_progress_count = events
            .iter()
            .filter(|message| {
                matches!(
                    message,
                    FolderScanWorkerEvent::Progress(progress)
                        if progress.detail.starts_with("Indexing | ")
                )
            })
            .count();
        let first_discovery = events
            .iter()
            .position(|message| matches!(message, FolderScanWorkerEvent::DiscoveryBatch(_)))
            .expect("large scan should publish a committed discovery batch");
        assert!(
            events
                .iter()
                .skip(first_discovery + 1)
                .any(|message| matches!(
                    message,
                    FolderScanWorkerEvent::Progress(progress)
                        if progress.detail.starts_with("Indexing | ")
                )),
            "committed discoveries must arrive before indexing completes"
        );
        let batch_lengths = batches.iter().map(|batch| batch.len()).collect::<Vec<_>>();
        let published_file_count = batches
            .iter()
            .flat_map(|batch| batch.iter())
            .filter(|event| matches!(event.item, FolderScanItem::File(_)))
            .count();
        assert!(batch_lengths.len() > 1);
        assert!(
            batch_lengths
                .iter()
                .all(|count| *count <= DISCOVERY_BATCH_SIZE)
        );
        assert_eq!(published_file_count, result.scan.file_count);
        assert_eq!(result.audio_file_paths.len(), file_count);
        assert_eq!(
            indexing_progress_count,
            1 + file_count / INDEX_PROGRESS_REPORT_INTERVAL,
            "indexing progress must stay bounded so a large background scan cannot saturate the UI queue"
        );
    }

    #[test]
    fn active_scan_stops_when_supervisor_cancels_its_permit() {
        let file_count = DISCOVERY_BATCH_SIZE * 4;
        let root = temp_source_with_wavs(file_count);
        let cancel = Arc::new(AtomicBool::new(false));
        let callback_cancel = Arc::clone(&cancel);

        let result = run_folder_scan_worker_with_emit_and_cancel(
            scan_request(&root),
            move |event| {
                if matches!(event, FolderScanWorkerEvent::DiscoveryBatch(_)) {
                    callback_cancel.store(true, Ordering::Release);
                }
                true
            },
            cancel.as_ref(),
            None,
            &UncoordinatedScanWriter,
        );

        assert!(result.scan.cancelled);
        assert!(result.scan.file_count < file_count);
    }
}
