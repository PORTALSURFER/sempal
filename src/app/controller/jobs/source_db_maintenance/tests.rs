use super::*;
use crate::app::controller::library::source_write_priority::FileOpWritePriorityGuard;
use crate::sample_sources::SampleSource;
use std::io;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
/// Stores state for shared buffer.
struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl SharedBuffer {
    /// Handles captured.
    fn captured(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl<'a> MakeWriter<'a> for SharedBuffer {
    /// Names the writer type.
    type Writer = SharedBufferWriter;

    /// Handles make writer.
    fn make_writer(&'a self) -> Self::Writer {
        SharedBufferWriter(self.0.clone())
    }
}

/// Stores state for shared buffer writer.
struct SharedBufferWriter(Arc<Mutex<Vec<u8>>>);

impl io::Write for SharedBufferWriter {
    /// Handles write.
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    /// Handles flush.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Handles capture debug logs.
fn capture_debug_logs(run: impl FnOnce()) -> String {
    let buffer = SharedBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(buffer.clone())
        .finish();
    crate::logging::with_debug_logging_enabled_for_tests(true, || {
        tracing::subscriber::with_default(subscriber, run);
    });
    buffer.captured()
}

#[test]
fn source_db_maintenance_defers_quietly_during_same_source_file_op() {
    let temp = tempdir().expect("create temp dir");
    let source = SampleSource::new(temp.path().join("source"));
    std::fs::create_dir_all(&source.root).expect("create source root");
    let _db =
        crate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(&source.root)
            .expect("open db");
    let _guard = FileOpWritePriorityGuard::new(&source.id);

    let outcome = run_source_db_maintenance_job(SourceDbMaintenanceJob {
        source_id: source.id.clone(),
        source_root: source.root.clone(),
    });

    assert!(outcome.deferred_due_to_file_op);
    assert_eq!(outcome.source_id, source.id);
    assert!(outcome.error.is_none());
    assert_eq!(outcome.refresh, SourceDbMaintenanceRefresh::None);
}

#[test]
fn deferred_hash_failure_preserves_committed_empty_source_refresh() {
    let committed = crate::sample_sources::scanner::ScanStats {
        added: 1,
        ..Default::default()
    };

    assert!(scan_changed_after_deferred(&committed, None));
}

#[test]
/// Verifies deferred maintenance retry records source scoped telemetry.
fn deferred_maintenance_retry_records_source_scoped_telemetry() {
    let temp = tempdir().expect("create temp dir");
    let source = SampleSource::new(temp.path().join("source"));
    std::fs::create_dir_all(&source.root).expect("create source root");
    let job = SourceDbMaintenanceJob {
        source_id: source.id,
        source_root: source.root.clone(),
    };

    let captured = capture_debug_logs(|| {
        record_deferred_maintenance_retry(&job, 1, "database is locked");
    });

    assert!(
        captured.contains("Retrying source DB work after failure"),
        "retry should be visible in logs: {captured}"
    );
    assert!(
        captured.contains("operation=\"analysis_deferred_maintenance\""),
        "retry should preserve the maintenance operation name: {captured}"
    );
    assert!(
        captured.contains("source_root=") && captured.contains("source"),
        "retry should include source-root context: {captured}"
    );
    assert!(
        captured.contains("busy=true"),
        "retry should classify locked DB failures as busy: {captured}"
    );
}
