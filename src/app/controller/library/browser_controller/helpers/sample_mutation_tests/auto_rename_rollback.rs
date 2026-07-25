use super::*;
#[test]
fn sample_auto_rename_logs_looped_metadata_provenance() {
    let (_temp, source) = setup_fixture(&["old.wav"]);
    let old_relative = Path::new("old.wav");
    let new_relative = Path::new("renamed.wav");
    let db =
        SourceDatabase::open_for_test_fixture_source_write(&source.root).expect("open source db");
    db.set_looped(old_relative, false)
        .expect("override old looped");
    let request = SampleAutoRenameRequest {
        old_relative: old_relative.to_path_buf(),
        new_relative: new_relative.to_path_buf(),
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
    };

    let captured = capture_info_logs(|| {
        let result = run_sample_auto_rename_job(
            source.clone(),
            vec![request],
            Arc::new(AtomicBool::new(false)),
            None,
        );
        assert!(result.errors.is_empty(), "{:?}", result.errors);
    });

    assert!(
        captured.contains("auto rename: persisted loop metadata provenance"),
        "rename persistence should log loop provenance: {captured}"
    );
    assert!(
        captured.contains("old_path=old.wav")
            && captured.contains("new_path=renamed.wav")
            && captured.contains("request_looped=true")
            && captured.contains("db_looped=Some(false)")
            && captured.contains("final_looped=true"),
        "log should identify request, DB, and final loop values: {captured}"
    );
}

#[test]
/// Auto-rename leaves every file at its original path when each DB rewrite attempt hits a busy lock.
fn sample_auto_rename_rolls_back_each_failed_file_when_db_is_busy() {
    let (_temp, source) = setup_fixture(&["alpha.wav", "beta.wav"]);
    let requests = vec![
        rename_request("alpha.wav", "alpha_renamed.wav"),
        rename_request("beta.wav", "beta_renamed.wav"),
    ];
    let (lock_release_tx, lock_done_rx) = lock_db_until_released(&source.root);

    let result = run_sample_auto_rename_job(
        source.clone(),
        requests,
        Arc::new(AtomicBool::new(false)),
        None,
    );

    release_db_lock(lock_release_tx, lock_done_rx);

    assert!(result.renamed.is_empty());
    assert!(result.skipped.is_empty());
    assert_eq!(result.errors.len(), 2);
    for (_, err) in &result.errors {
        assert_db_contention_error(err);
    }
    assert!(source.root.join("alpha.wav").is_file());
    assert!(source.root.join("beta.wav").is_file());
    assert!(!source.root.join("alpha_renamed.wav").exists());
    assert!(!source.root.join("beta_renamed.wav").exists());

    let db =
        SourceDatabase::open_for_test_fixture_source_write(&source.root).expect("open source db");
    for relative in [Path::new("alpha.wav"), Path::new("beta.wav")] {
        assert_eq!(
            db.tag_for_path(relative).expect("tag"),
            Some(Rating::KEEP_3)
        );
        assert_eq!(db.locked_for_path(relative).expect("locked"), Some(true));
        assert_eq!(
            db.user_tag_for_path(relative).expect("user tag"),
            Some(String::from("Vintage"))
        );
        assert_eq!(
            db.tag_labels_for_path(relative).expect("normal tags"),
            vec![String::from("Analog Kick"), String::from("Layer")]
        );
    }
}

#[test]
fn production_sample_rename_retry_budget_covers_multi_second_busy_windows() {
    assert!(
        SAMPLE_RENAME_DB_RETRY_DELAY_PRODUCTION
            .saturating_mul(SAMPLE_RENAME_DB_RETRIES_PRODUCTION as u32)
            >= Duration::from_millis(5_500)
    );
}

#[test]
/// Auto-rename waits past the old 200 ms retry budget instead of rolling back the file rename.
fn sample_auto_rename_retries_until_multi_attempt_db_lock_clears() {
    let (_temp, source) = setup_fixture(&["alpha.wav"]);
    let requests = vec![rename_request("alpha.wav", "alpha_renamed.wav")];
    let (lock_release_tx, lock_done_rx) = lock_db_until_released(&source.root);

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(260));
        release_db_lock(lock_release_tx, lock_done_rx);
    });

    let result = run_sample_auto_rename_job(
        source.clone(),
        requests,
        Arc::new(AtomicBool::new(false)),
        None,
    );

    assert!(
        result.errors.is_empty(),
        "rename should retry through short lock"
    );
    assert!(result.skipped.is_empty());
    assert_eq!(result.renamed.len(), 1);
    assert!(!source.root.join("alpha.wav").exists());
    assert!(source.root.join("alpha_renamed.wav").is_file());

    let db =
        SourceDatabase::open_for_test_fixture_source_write(&source.root).expect("open source db");
    assert!(
        db.tag_for_path(Path::new("alpha.wav"))
            .expect("old tag")
            .is_none()
    );
    assert_eq!(
        db.tag_for_path(Path::new("alpha_renamed.wav"))
            .expect("renamed tag"),
        Some(Rating::KEEP_3)
    );
    assert_eq!(
        db.locked_for_path(Path::new("alpha_renamed.wav"))
            .expect("renamed locked"),
        Some(true)
    );
    assert_eq!(
        db.tag_labels_for_path(Path::new("alpha_renamed.wav"))
            .expect("renamed normal tags"),
        vec![String::from("Analog Kick"), String::from("Layer")]
    );
}
