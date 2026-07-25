use super::*;

#[test]
fn large_auto_rename_background_dispatch_registers_file_ops_before_planning_finishes() {
    /// Large enough to exercise batch dispatch without making the regression test slow.
    const SAMPLE_COUNT: usize = 24;
    clear_batch_latency();
    let (mut controller, source, paths) = large_auto_rename_fixture(SAMPLE_COUNT);

    let started_at = Instant::now();
    BrowserController::new(&mut controller)
        .auto_rename_browser_sample_paths_background_for_tests(&paths)
        .expect("background auto rename dispatch should start");
    let elapsed = started_at.elapsed();

    assert!(
        elapsed <= LARGE_BROWSER_BATCH_CONTROLLER_BUDGET,
        "background auto-rename dispatch exceeded {:?}: {elapsed:?}",
        LARGE_BROWSER_BATCH_CONTROLLER_BUDGET
    );
    assert_eq!(
        controller.ui.progress.task,
        Some(crate::app::state::ProgressTaskKind::FileOps)
    );
    assert_eq!(controller.ui.progress.title, "Preparing auto rename");
    assert!(controller.ui.progress.cancelable);
    assert_eq!(controller.ui.progress.total, SAMPLE_COUNT);

    wait_for_background_jobs(&mut controller, LARGE_BACKGROUND_FILE_OP_TIMEOUT);
    assert!(source.root.join("artistname_SS.wav").exists());
}

#[test]
fn large_background_auto_rename_reuses_source_db_for_batch_execution() {
    /// Large enough to catch per-item database opens in the auto-rename worker.
    const SAMPLE_COUNT: usize = 24;
    let (mut controller, source, paths) = large_auto_rename_fixture(SAMPLE_COUNT);

    let _open_count_scope =
        crate::sample_sources::db::test_scope_source_db_open_total_count(&source.root);
    BrowserController::new(&mut controller)
        .auto_rename_browser_sample_paths_background_for_tests(&paths)
        .expect("background auto rename should start");
    wait_for_background_jobs(&mut controller, LARGE_BACKGROUND_FILE_OP_TIMEOUT);

    let open_count = crate::sample_sources::db::test_source_db_open_total_count(&source.root);
    assert!(
        open_count <= 3,
        "24-item background auto-rename should reuse source DB access instead of opening per item; observed {open_count}"
    );
    assert!(source.root.join("artistname_SS.wav").exists());
    assert!(source.root.join("artistname_SS_023.wav").exists());
}
