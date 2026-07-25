use super::sample_mutation::{RenameLoopedMetadata, perform_sample_rename};
use super::sample_mutation::{
    SAMPLE_RENAME_DB_RETRIES_PRODUCTION, SAMPLE_RENAME_DB_RETRY_DELAY_PRODUCTION,
};
use super::{SampleAutoRenameRequest, run_sample_auto_rename_job};
use crate::app::controller::jobs::{
    FileOpMessage, FileOpProgressSender, JobMessage, JobMessageSender, SampleAutoRenameProgress,
};
use crate::app::controller::test_support::write_test_wav;
use crate::sample_sources::db::DB_FILE_NAME;
use crate::sample_sources::{Rating, SampleSoundType, SampleSource, SourceDatabase};
use radiant::gui::repaint::SharedRepaintSignal;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, Sender},
};
use std::time::Duration;
use tempfile::{TempDir, tempdir};
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl SharedBuffer {
    fn captured(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl<'a> MakeWriter<'a> for SharedBuffer {
    type Writer = SharedBufferWriter;

    fn make_writer(&'a self) -> Self::Writer {
        SharedBufferWriter(self.0.clone())
    }
}

struct SharedBufferWriter(Arc<Mutex<Vec<u8>>>);

impl io::Write for SharedBufferWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn capture_info_logs<F>(run: F) -> String
where
    F: FnOnce(),
{
    let buffer = SharedBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::INFO)
        .with_writer(buffer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, run);
    buffer.captured()
}

fn assert_db_contention_error(err: &str) {
    let lowered = err.to_ascii_lowercase();
    assert!(
        err.contains("Failed to start database update")
            || lowered.contains("busy")
            || lowered.contains("locked"),
        "expected database contention error, got: {err}"
    );
}

mod auto_rename_rollback;
mod manual_rename;
mod metadata_artifacts;
mod progress_cancellation;

fn setup_fixture(names: &[&str]) -> (TempDir, SampleSource) {
    let temp = tempdir().expect("create temp dir");
    let source = SampleSource::new(temp.path().join("source"));
    std::fs::create_dir_all(&source.root).expect("create source root");
    let db =
        SourceDatabase::open_for_test_fixture_source_write(&source.root).expect("open source db");
    for name in names {
        let relative = Path::new(name);
        let absolute = source.root.join(relative);
        write_test_wav(&absolute, &[0.0, 0.1, -0.1]);
        let metadata = std::fs::metadata(&absolute).expect("read file metadata");
        db.upsert_file(relative, metadata.len(), 0)
            .expect("insert db row");
        db.set_tag(relative, Rating::KEEP_3).expect("set tag");
        db.set_looped(relative, true).expect("set looped");
        db.set_locked(relative, true).expect("set locked");
        db.set_sound_type(relative, Some(SampleSoundType::Kick))
            .expect("set sound type");
        db.set_user_tag(relative, Some("Vintage"))
            .expect("set user tag");
        db.set_last_played_at(relative, 42)
            .expect("set playback age");
        let mut batch = db.write_batch().expect("open tag batch");
        batch
            .replace_tags_for_path(
                relative,
                &[String::from("Analog Kick"), String::from("Layer")],
            )
            .expect("set normal tags");
        batch.commit().expect("commit normal tags");
        insert_analysis_artifacts(&source, relative);
    }
    (temp, source)
}

fn file_op_progress_capture() -> (FileOpProgressSender, std::sync::mpsc::Receiver<JobMessage>) {
    let (tx, rx) = std::sync::mpsc::sync_channel(16);
    (
        FileOpProgressSender::new(
            JobMessageSender::new(tx),
            Arc::new(SharedRepaintSignal::default()),
        ),
        rx,
    )
}

fn drain_file_op_progress(
    rx: std::sync::mpsc::Receiver<JobMessage>,
) -> Vec<(usize, Option<String>, Option<SampleAutoRenameProgress>)> {
    rx.try_iter()
        .filter_map(|message| match message {
            JobMessage::FileOps(FileOpMessage::Progress {
                completed,
                detail,
                item,
            }) => Some((completed, detail, item)),
            _ => None,
        })
        .collect()
}

fn insert_analysis_artifacts(source: &SampleSource, relative: &Path) {
    let conn = rusqlite::Connection::open(source.root.join(DB_FILE_NAME)).expect("open sqlite");
    let sample_id = format!(
        "{}::{}",
        source.id,
        relative.to_string_lossy().replace('\\', "/")
    );
    conn.execute(
        "INSERT INTO samples (
             sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version
         ) VALUES (?1, 'hash-a', 1, 1, 1.0, 48000, 'analysis_v1_test')",
        [&sample_id],
    )
    .expect("insert sample analysis row");
    conn.execute(
        "INSERT INTO features (sample_id, feat_version, vec_blob, light_dsp_blob, rms, computed_at)
         VALUES (?1, 1, x'00', x'00', 0.0, 1)",
        [&sample_id],
    )
    .expect("insert features");
    conn.execute(
        "INSERT INTO embeddings (sample_id, model_id, dim, dtype, l2_normed, vec, created_at)
         VALUES (?1, 'model', 1, 'f32', 1, x'00', 1)",
        [&sample_id],
    )
    .expect("insert embeddings");
    conn.execute(
        "INSERT INTO analysis_jobs (
             sample_id, source_id, relative_path, job_type, content_hash, status, attempts, created_at
         ) VALUES (?1, ?2, ?3, 'analyze_sample', 'hash-a', 'done', 0, 1)",
        rusqlite::params![sample_id, source.id.as_str(), relative.to_string_lossy()],
    )
    .expect("insert analysis job");
}

fn sample_id_count(conn: &rusqlite::Connection, table: &str, sample_id: &str) -> i64 {
    conn.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE sample_id = ?1"),
        [sample_id],
        |row| row.get(0),
    )
    .unwrap()
}

fn rename_request(old_relative: &str, new_relative: &str) -> SampleAutoRenameRequest {
    SampleAutoRenameRequest {
        old_relative: PathBuf::from(old_relative),
        new_relative: PathBuf::from(new_relative),
        tag: Rating::KEEP_3,
        looped: true,
        locked: true,
        sound_type: Some(SampleSoundType::Kick),
        user_tag: Some(String::from("Vintage")),
        tag_named: true,
        last_played_at: Some(42),
        resume_playback: false,
        resume_looped: false,
        resume_start_override: None,
    }
}

fn lock_db_until_released(source_root: &Path) -> (Sender<()>, Receiver<()>) {
    let (lock_release_tx, lock_release_rx) = std::sync::mpsc::channel();
    let (lock_done_tx, lock_done_rx) = std::sync::mpsc::channel();
    let (locked_tx, locked_rx) = std::sync::mpsc::channel();
    let db_file = source_root.join(DB_FILE_NAME);
    std::thread::spawn(move || {
        let conn = rusqlite::Connection::open(db_file).expect("open sqlite lock connection");
        conn.execute_batch("BEGIN IMMEDIATE")
            .expect("start immediate transaction");
        let _ = locked_tx.send(());
        let _ = lock_release_rx.recv();
        let _ = conn.execute_batch("COMMIT");
        drop(conn);
        let _ = lock_done_tx.send(());
    });
    locked_rx.recv().expect("wait for sqlite lock");
    (lock_release_tx, lock_done_rx)
}

fn release_db_lock(lock_release_tx: Sender<()>, lock_done_rx: Receiver<()>) {
    let _ = lock_release_tx.send(());
    lock_done_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("wait for sqlite lock release");
}
