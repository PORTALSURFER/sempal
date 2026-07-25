use super::*;

#[test]
fn readding_known_source_prepares_database_off_controller_thread() {
    let config_root = tempdir().expect("config root");
    let _guard = crate::app_dirs::ConfigBaseGuard::set(config_root.path().to_path_buf());
    let source_root = tempdir().expect("source root");
    let source_path = source_root.path().to_path_buf();
    let mut controller = AppController::new(WaveformRenderer::new(16, 16), None);

    controller
        .add_source_from_path(source_path.clone())
        .expect("initial source add");
    let original_source = controller.library.sources[0].clone();
    controller.remove_source(0);
    assert!(controller.library.sources.is_empty());

    let _open_count_scope =
        crate::sample_sources::db::test_scope_source_db_open_total_count(&source_path);
    with_source_add_async_enabled_for_tests(true, || {
        controller
            .add_source_from_path(source_path.clone())
            .expect("queue source re-add");
    });

    assert!(controller.library.sources.is_empty());
    assert_eq!(
        crate::sample_sources::db::test_source_db_open_total_count(&source_path),
        0,
        "source re-add must not open the source DB on the controller thread"
    );
    assert!(
        controller
            .ui
            .progress
            .has_task(crate::app::state::ProgressTaskKind::SourceAdd)
    );
    let pending = controller
        .runtime
        .source_lane
        .pending_adds
        .get(&source_path)
        .expect("pending source add");
    assert_eq!(pending.source.id, original_source.id);

    controller.apply_background_job_message_for_tests(JobMessage::SourceAddPrepared(
        SourceAddPreparedResult {
            request_id: pending.request_id,
            source: pending.source.clone(),
            elapsed: std::time::Duration::from_millis(3),
            result: Ok(()),
        },
    ));

    assert!(controller.runtime.source_lane.pending_adds.is_empty());
    assert_eq!(controller.library.sources.len(), 1);
    assert_eq!(controller.library.sources[0].id, original_source.id);
    assert_eq!(controller.selected_source_id(), Some(original_source.id));
    assert!(
        !controller
            .ui
            .progress
            .has_task(crate::app::state::ProgressTaskKind::SourceAdd)
    );
}
