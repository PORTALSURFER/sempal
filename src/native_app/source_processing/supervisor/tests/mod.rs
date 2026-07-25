use std::path::{Path, PathBuf};
use std::{io, sync::Mutex};

use tracing_subscriber::fmt::MakeWriter;
use wavecrate::sample_sources::{
    SourceId,
    readiness::{
        ArtifactPublishOutcome, ClaimedReadinessWork, ReadinessArtifact, ReadinessDeficit,
        ReadinessEligibility, ReadinessError, ReadinessSnapshot, ReadinessStore,
        ReadinessTargetPublication, ReadinessView, ReadinessWorkStats, SourceAvailability,
    },
};

use super::*;

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl LogBuffer {
    fn captured(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl<'a> MakeWriter<'a> for LogBuffer {
    type Writer = LogBufferWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogBufferWriter(Arc::clone(&self.0))
    }
}

struct LogBufferWriter(Arc<Mutex<Vec<u8>>>);

impl io::Write for LogBufferWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn capture_logs(level: tracing::Level, run: impl FnOnce()) -> String {
    let buffer = LogBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_max_level(level)
        .with_writer(buffer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, run);
    buffer.captured()
}

fn reconcile_readiness(
    connection: &rusqlite::Connection,
    source_id: &str,
    now: i64,
) -> Result<ReadinessSnapshot, ReadinessError> {
    ReadinessView::new(connection).reconcile(source_id, now)
}

fn reconcile_readiness_with_cancel_and_progress(
    connection: &rusqlite::Connection,
    source_id: &str,
    now: i64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(ReadinessProgress),
) -> Result<ReadinessSnapshot, ReadinessError> {
    ReadinessView::new(connection)
        .reconcile_with_cancel_and_progress(source_id, now, cancel, progress)
}

fn persist_readiness_deficits(
    connection: &mut rusqlite::Connection,
    deficits: &[ReadinessDeficit],
    created_at: i64,
) -> Result<usize, ReadinessError> {
    ReadinessStore::new(connection).persist_deficits(deficits, created_at)
}

fn publish_readiness_artifact(
    connection: &mut rusqlite::Connection,
    artifact: &ReadinessArtifact,
) -> Result<ArtifactPublishOutcome, ReadinessError> {
    ReadinessStore::new(connection).publish_artifact(artifact)
}

fn readiness_work_stats(
    connection: &rusqlite::Connection,
    now: i64,
) -> Result<ReadinessWorkStats, ReadinessError> {
    ReadinessView::new(connection).work_stats(now)
}

#[allow(clippy::too_many_arguments)]
fn replace_readiness_targets(
    connection: &mut rusqlite::Connection,
    source_id: &str,
    source_generation: i64,
    readiness_revision: i64,
    availability: SourceAvailability,
    targets: &[ReadinessTarget],
    updated_at: i64,
) -> Result<(), ReadinessError> {
    ReadinessStore::new(connection).publish_targets(&ReadinessTargetPublication::new(
        source_id,
        source_generation,
        readiness_revision,
        availability,
        "wavecrate-source-readiness-v1",
        targets,
        updated_at,
    ))
}

fn claim_readiness_target(
    connection: &mut rusqlite::Connection,
    target: &ReadinessTarget,
    now: i64,
    lease_duration_seconds: i64,
) -> Result<Option<ClaimedReadinessWork>, ReadinessError> {
    ReadinessStore::new(connection).claim(target, now, lease_duration_seconds)
}

fn complete_readiness_work(
    connection: &mut rusqlite::Connection,
    claim: &ClaimedReadinessWork,
    completed_at: i64,
) -> Result<ArtifactPublishOutcome, ReadinessError> {
    ReadinessStore::new(connection).complete(claim, completed_at)
}

fn reclassify_known_unsupported_audio_failures(
    connection: &mut rusqlite::Connection,
) -> Result<usize, String> {
    ReadinessStore::new(connection)
        .reclassify_known_unsupported_failures(legacy_unsupported_decode_failure_text)
        .map_err(|error| error.to_string())
}

fn readiness_stage_is_unsupported(
    connection: &rusqlite::Connection,
    target: &ReadinessTarget,
    stage: &str,
) -> Result<bool, String> {
    let stage = match stage {
        "analysis_features" => ReadinessStage::AnalysisFeatures,
        "embedding_aspects" => ReadinessStage::EmbeddingAspects,
        _ => return Ok(false),
    };
    ReadinessView::new(connection)
        .stage_is_unsupported(target, stage)
        .map_err(|error| error.to_string())
}

include!("lifecycle_retirement.rs");
include!("admission_lifecycle.rs");
include!("priority_and_epochs.rs");
include!("retirement_recovery.rs");
include!("discovery_progress.rs");
include!("admission_and_audits.rs");
include!("scan_writer_coordination.rs");
include!("progress_events.rs");
include!("progress_lifecycle.rs");
include!("readiness_execution.rs");
include!("execution_retries.rs");
include!("terminal_inputs.rs");
include!("coordinator_fairness.rs");
include!("recording_and_failures.rs");
include!("unsupported_recovery.rs");

fn unhashed_source(id: &str) -> (tempfile::TempDir, SampleSource) {
    let directory = tempfile::tempdir().expect("temporary source");
    let path = directory.path().join("pending.wav");
    std::fs::write(&path, [1_u8; 64]).expect("write sample bytes");
    let source =
        SampleSource::new_with_id(SourceId::from_string(id), directory.path().to_path_buf());
    let db = source.open_db().expect("open source database");
    db.upsert_file(Path::new("pending.wav"), 64, 1)
        .expect("insert pending hash row");
    let mut batch = db.write_batch().expect("open identity batch");
    batch
        .set_file_identity(Path::new("pending.wav"), Some(&format!("identity-{id}")))
        .expect("assign pending identity");
    batch.commit().expect("commit pending identity");
    (directory, source)
}

fn ready_analysis_source(id: &str) -> (tempfile::TempDir, SampleSource) {
    let directory = tempfile::tempdir().expect("temporary readiness source");
    let path = directory.path().join("ready.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: wavecrate_analysis::ANALYSIS_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).expect("create readiness wav");
    for index in 0..4_096 {
        let phase = index as f32 / 32.0;
        writer
            .write_sample((phase.sin() * i16::MAX as f32 * 0.25) as i16)
            .expect("write readiness sample");
    }
    writer.finalize().expect("finalize readiness wav");
    let size = path.metadata().expect("read readiness metadata").len();
    let content_hash = blake3::hash(&std::fs::read(&path).expect("read readiness wav"))
        .to_hex()
        .to_string();
    let source =
        SampleSource::new_with_id(SourceId::from_string(id), directory.path().to_path_buf());
    let db = source.open_db().expect("open readiness source database");
    db.upsert_file(Path::new("ready.wav"), size, 1)
        .expect("insert readiness wav row");
    let database_root = source.database_root().expect("database root");
    let connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open readiness database");
    connection
        .execute(
            "UPDATE wav_files
                 SET file_identity = 'identity-1', content_hash = ?1
                 WHERE path = 'ready.wav'",
            [&content_hash],
        )
        .expect("assign readiness identity");
    (directory, source)
}

fn seed_legacy_playback_artifact(source: &SampleSource) -> (std::path::PathBuf, i64) {
    let now = now_epoch_seconds();
    let database_root = source.database_root().expect("database root");
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("open legacy playback database");
    publish_current_readiness_targets(&mut connection, source.id.as_str(), now)
        .expect("publish current target matrix");
    let cache_ref = seed_managed_legacy_cache_ref(source, "first", now);
    connection
        .execute(
            "INSERT INTO source_readiness_targets (
                    source_id, scope_kind, scope_id, relative_path, stage, required_version,
                    source_generation, content_generation, eligibility, updated_at
                 )
                 SELECT source_id, scope_kind, scope_id, relative_path, 'playback_summary',
                        'legacy-playback-v1', source_generation, content_generation,
                        eligibility, ?2
                 FROM source_readiness_targets
                 WHERE source_id = ?1 AND stage = 'indexed_identity'",
            params![source.id.as_str(), now],
        )
        .expect("seed legacy playback target");
    connection
        .execute(
            "INSERT INTO source_readiness_artifacts (
                    source_id, scope_kind, scope_id, relative_path, stage, artifact_version,
                    source_generation, content_generation, artifact_ref, completed_at
                 )
                 SELECT source_id, scope_kind, scope_id, relative_path, stage, required_version,
                        source_generation, content_generation, ?2, ?3
                 FROM source_readiness_targets
                 WHERE source_id = ?1 AND stage = 'playback_summary'",
            params![source.id.as_str(), cache_ref.to_string_lossy(), now],
        )
        .expect("seed legacy playback artifact");
    (cache_ref, now)
}

fn seed_managed_legacy_cache_ref(
    source: &SampleSource,
    label: &str,
    now: i64,
) -> std::path::PathBuf {
    let cache_directory =
        wavecrate::app_dirs::waveform_cache_dir().expect("resolve waveform cache directory");
    std::fs::create_dir_all(&cache_directory).expect("create waveform cache directory");
    let cache_ref = cache_directory.join(format!(
        "legacy-playback-{}-{label}-{now}.wfc",
        source.id.as_str()
    ));
    std::fs::write(&cache_ref, b"legacy playback cache").expect("seed legacy playback cache");
    cache_ref
}

fn source_is_hashed(source: &SampleSource) -> bool {
    source
        .open_db()
        .expect("open source database")
        .entry_for_path(Path::new("pending.wav"))
        .expect("read pending file")
        .and_then(|entry| entry.content_hash)
        .is_some()
}

fn discovery_durable_counts(connection: &rusqlite::Connection) -> (i64, i64, i64) {
    connection
        .query_row(
            "SELECT
                    (SELECT COUNT(*) FROM source_readiness_targets),
                    (SELECT COUNT(*) FROM analysis_jobs),
                    (SELECT COUNT(*) FROM source_readiness_sources
                     WHERE contract_version != '')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read durable discovery counts")
}

fn readiness_stages_for_identity(
    connection: &rusqlite::Connection,
    source_id: &str,
    identity: &str,
) -> Vec<String> {
    let mut statement = connection
        .prepare(
            "SELECT stage FROM source_readiness_targets
                 WHERE source_id = ?1 AND scope_id = ?2
                 ORDER BY stage",
        )
        .expect("prepare identity readiness stages");
    statement
        .query_map(params![source_id, identity], |row| row.get(0))
        .expect("query identity readiness stages")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect identity readiness stages")
}

fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(condition(), "condition did not become true before timeout");
}
