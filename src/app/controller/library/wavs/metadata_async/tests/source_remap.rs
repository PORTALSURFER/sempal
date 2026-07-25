use super::*;

#[test]
fn source_metadata_job_follows_completed_browser_rename() {
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
    batch.commit().expect("commit rename batch");
    let _rename_scope = source_write_priority::CompletedBrowserRenameTestGuard::new(
        &source.id,
        &old_relative,
        &new_relative,
    );

    let result = run_metadata_mutation_job(MetadataMutationJob {
        request_id: 13,
        source_id: source.id.clone(),
        source_root: source.root.clone(),
        paths: [old_relative.clone()].into_iter().collect(),
        source_ops: vec![
            SourceMetadataMutationOp::SetLooped {
                relative_path: old_relative.clone(),
                looped: true,
            },
            SourceMetadataMutationOp::AssignNormalTag {
                relative_path: old_relative.clone(),
                label: String::from("Vintage Loop"),
            },
        ],
        analysis_ops: Vec::new(),
    });

    assert!(result.result.is_ok(), "{:?}", result.result);
    assert_eq!(db.looped_for_path(&old_relative).expect("old looped"), None);
    assert_eq!(
        db.looped_for_path(&new_relative).expect("new looped"),
        Some(true)
    );
    assert_eq!(
        db.tags_for_path(&new_relative)
            .expect("new tags")
            .into_iter()
            .map(|tag| tag.display_label)
            .collect::<Vec<_>>(),
        vec![String::from("Vintage Loop")]
    );
}
