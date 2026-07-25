use super::*;

#[test]
fn metadata_mutation_paths_dedup_across_source_and_analysis_ops() {
    let _lock = metadata_async_test_lock();
    let paths = metadata_mutation_paths(
        &[
            SourceMetadataMutationOp::SetLooped {
                relative_path: PathBuf::from("one.wav"),
                looped: true,
            },
            SourceMetadataMutationOp::SetLastPlayedAt {
                relative_path: PathBuf::from("two.wav"),
                played_at: 5,
            },
        ],
        &[
            AnalysisMetadataMutationOp::SetBpm {
                relative_path: PathBuf::from("one.wav"),
                bpm: Some(120.0),
            },
            AnalysisMetadataMutationOp::SetLoadedDuration {
                relative_path: PathBuf::from("two.wav"),
                duration_seconds: 1.0,
                sample_rate: 44_100,
                long_sample_mark: Some(false),
            },
        ],
    );

    assert_eq!(
        paths.into_iter().collect::<Vec<_>>(),
        vec![PathBuf::from("one.wav"), PathBuf::from("two.wav")]
    );
}

#[test]
fn metadata_mutation_waits_behind_same_source_file_op_priority() {
    let _lock = metadata_async_test_lock();
    let temp = tempfile::tempdir().expect("create temp dir");
    let source = SampleSource::new(temp.path().join("source"));
    std::fs::create_dir_all(&source.root).expect("create source root");
    let relative_path = PathBuf::from("alpha.wav");
    let db =
        SourceDatabase::open_for_test_fixture_source_write(&source.root).expect("open source db");
    db.upsert_file(&relative_path, 1, 1)
        .expect("insert source row");
    let worker_source_id = source.id.clone();
    let (active_tx, active_rx) = std::sync::mpsc::channel();
    let priority_worker = std::thread::spawn(move || {
        let _guard = source_write_priority::FileOpWritePriorityGuard::new(&worker_source_id);
        active_tx.send(()).expect("signal active file-op priority");
        std::thread::sleep(Duration::from_millis(260));
    });
    active_rx.recv().expect("wait for active file-op priority");

    let result = run_metadata_mutation_job(MetadataMutationJob {
        request_id: 7,
        source_id: source.id.clone(),
        source_root: source.root.clone(),
        paths: [relative_path.clone()].into_iter().collect(),
        source_ops: vec![SourceMetadataMutationOp::SetUserTag {
            relative_path: relative_path.clone(),
            user_tag: Some(String::from("Vintage")),
        }],
        analysis_ops: Vec::new(),
    });

    assert!(result.elapsed >= Duration::from_millis(200));
    assert!(result.result.is_ok(), "{:?}", result.result);
    assert_eq!(
        db.user_tag_for_path(&relative_path).expect("read user tag"),
        Some(String::from("Vintage"))
    );
    priority_worker.join().expect("priority worker");
}
