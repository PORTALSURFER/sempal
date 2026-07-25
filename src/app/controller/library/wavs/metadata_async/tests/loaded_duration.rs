use super::*;

fn persisted_duration_seconds(source: &SampleSource, relative_path: &Path) -> Option<f64> {
    let sample_id = analysis_jobs::build_sample_id(source.id.as_str(), relative_path);
    let conn = analysis_jobs::open_source_db(&source.root).expect("open analysis db");
    conn.query_row(
        "SELECT duration_seconds FROM samples WHERE sample_id = ?1",
        rusqlite::params![sample_id],
        |row| row.get::<_, Option<f64>>(0),
    )
    .ok()
    .flatten()
}

#[test]
fn loaded_duration_metadata_job_follows_completed_browser_rename() {
    let _lock = metadata_async_test_lock();
    let temp = tempfile::tempdir().expect("create temp dir");
    let source = SampleSource::new(temp.path().join("source"));
    std::fs::create_dir_all(&source.root).expect("create source root");
    let old_relative = PathBuf::from("old-name.wav");
    let new_relative = PathBuf::from("new-name.wav");
    let old_absolute = source.root.join(&old_relative);
    let new_absolute = source.root.join(&new_relative);
    std::fs::write(&old_absolute, b"metadata-fixture").expect("write fixture");

    let db =
        SourceDatabase::open_for_test_fixture_source_write(&source.root).expect("open source db");
    let (old_size, old_modified_ns) =
        crate::app::controller::library::wav_io::file_metadata(&old_absolute)
            .expect("old metadata");
    db.upsert_file(&old_relative, old_size, old_modified_ns)
        .expect("insert old row");
    std::fs::rename(&old_absolute, &new_absolute).expect("rename fixture");
    let (new_size, new_modified_ns) =
        crate::app::controller::library::wav_io::file_metadata(&new_absolute)
            .expect("new metadata");
    let mut batch = db.write_batch().expect("start rename batch");
    batch.remove_file(&old_relative).expect("remove old row");
    batch
        .upsert_file(&new_relative, new_size, new_modified_ns)
        .expect("insert new row");
    batch
        .remap_analysis_sample_identity(&old_relative, &new_relative)
        .expect("remap analysis identity");
    batch.commit().expect("commit rename batch");
    let _rename_scope = source_write_priority::CompletedBrowserRenameTestGuard::new(
        &source.id,
        &old_relative,
        &new_relative,
    );

    let result = run_metadata_mutation_job(MetadataMutationJob {
        request_id: 11,
        source_id: source.id.clone(),
        source_root: source.root.clone(),
        paths: [old_relative.clone()].into_iter().collect(),
        source_ops: Vec::new(),
        analysis_ops: vec![AnalysisMetadataMutationOp::SetLoadedDuration {
            relative_path: old_relative.clone(),
            duration_seconds: 2.5,
            sample_rate: 44_100,
            long_sample_mark: Some(false),
        }],
    });

    assert!(result.result.is_ok(), "{:?}", result.result);
    assert!(persisted_duration_seconds(&source, &old_relative).is_none());
    assert_eq!(
        persisted_duration_seconds(&source, &new_relative),
        Some(2.5)
    );
}
