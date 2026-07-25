use super::*;
use std::path::PathBuf;
use std::sync::{Arc, atomic::Ordering};
use std::time::{Duration, Instant};
use wavecrate::sample_sources::{SourceDatabase, SourceId};

#[test]
fn scanner_failures_preserve_retry_and_cancellation_policy() {
    let unavailable = SourceProcessingFailure::from(wavecrate_scan::ScanError::Io {
        path: PathBuf::from("unavailable.wav"),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "temporarily unavailable"),
    });
    assert!(matches!(
        unavailable.class,
        SourceProcessingFailureClass::Retryable
    ));
    assert_eq!(unavailable.code.as_str(), "scanner_io");
    assert_eq!(
        unavailable.readiness_failure_classification(),
        wavecrate::sample_sources::readiness::ReadinessFailureClassification::Retryable
    );

    let stale = SourceProcessingFailure::from(wavecrate_scan::ScanError::StaleRevision {
        expected: 4,
        actual: 5,
    });
    assert!(matches!(
        stale.class,
        SourceProcessingFailureClass::Retryable
    ));
    assert_eq!(stale.code.as_str(), "scanner_stale_revision");

    let cancelled = SourceProcessingFailure::from(wavecrate_scan::ScanError::Canceled);
    assert!(cancelled.is_cancelled());
    assert_eq!(cancelled.code.as_str(), "scanner_cancelled");

    let invalid_root = SourceProcessingFailure::from(wavecrate_scan::ScanError::InvalidRoot(
        PathBuf::from("missing-source"),
    ));
    assert!(matches!(
        invalid_root.class,
        SourceProcessingFailureClass::Permanent
    ));
    assert_eq!(invalid_root.code.as_str(), "scanner_invalid_root");
}

#[test]
fn cancelled_embedding_waiting_for_writer_does_not_publish() {
    let directory = tempfile::tempdir().expect("temporary readiness source");
    let source = SampleSource::new_with_id(
        SourceId::from_string("cancelled-embedding-publication"),
        directory.path().to_path_buf(),
    );
    let analysis_version = wavecrate_analysis::analysis_version();
    let relative_path = Path::new("sample.wav");
    let absolute_path = source.root.join(relative_path);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav = hound::WavWriter::create(&absolute_path, spec).expect("create test wav");
    for sample in 0_i16..800_i16 {
        wav.write_sample(sample).expect("write test wav");
    }
    wav.finalize().expect("finalize test wav");
    let content_hash = blake3::hash(&std::fs::read(&absolute_path).expect("read test wav"))
        .to_hex()
        .to_string();
    let metadata = std::fs::metadata(&absolute_path).expect("read test wav metadata");
    let mut connection =
        SourceDatabase::open_connection_for_background_job(&source.root).expect("open source db");
    connection
        .execute(
            "INSERT INTO wav_files (
                path, file_size, modified_ns, content_hash, extension, missing, file_identity
             ) VALUES ('sample.wav', ?1, 1, ?2, 'wav', 0,
                       'cancelled-embedding-publication-identity')",
            rusqlite::params![
                i64::try_from(metadata.len()).expect("test wav size fits i64"),
                content_hash,
            ],
        )
        .expect("seed authoritative manifest");
    assert!(
        wavecrate::readiness_execution::run_feature_stage(
            &mut connection,
            &source.root,
            source.id.as_str(),
            relative_path,
            &content_hash,
            analysis_version,
            &AtomicBool::new(false),
        )
        .expect("seed current feature stage")
    );
    drop(connection);

    let gate = DatabaseWriterGate::default();
    let writer = gate.lock(DatabasePhase::Publish);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_gate = gate.clone();
    let worker_cancel = Arc::clone(&cancel);
    let worker_source = source.clone();
    let worker_content_hash = content_hash.clone();
    let worker = std::thread::spawn(move || {
        let mut connection =
            SourceDatabase::open_connection_for_background_job(&worker_source.root)
                .expect("open worker source db");
        run_readiness_embedding_stage(
            &mut connection,
            &worker_gate,
            &worker_source,
            relative_path,
            &worker_content_hash,
            analysis_version,
            worker_cancel.as_ref(),
        )
        .expect("embedding stage returns")
    });
    let deadline = Instant::now() + Duration::from_secs(1);
    while gate.waiting_count() == 0 {
        assert!(
            Instant::now() < deadline,
            "embedding publication did not wait for the writer gate"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    cancel.store(true, Ordering::Release);
    drop(writer);

    assert!(!worker.join().expect("embedding worker joins"));
    let connection =
        SourceDatabase::open_connection_for_background_job(&source.root).expect("reopen source db");
    let cached_embeddings: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM analysis_cache_embeddings WHERE content_hash = ?1",
            [&content_hash],
            |row| row.get(0),
        )
        .expect("query embedding cache");
    assert_eq!(
        cached_embeddings, 0,
        "cancelled prepared output must not publish"
    );
}
