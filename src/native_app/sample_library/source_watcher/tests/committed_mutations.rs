use super::super::SOURCE_CHANGE_DEBOUNCE;
use super::{Event, EventKind, GuiSourceWatchState, Instant, PathBuf, SampleSource, SourceId};
use crate::native_app::sample_library::committed_file_mutations::{
    CommittedSourceRevision, CommittedWatcherEcho, CommittedWatcherPathState,
    ProcessLocalMutationCorrelationId, RevisionFirstCursor, observed_watcher_path_state,
};

fn cursor() -> RevisionFirstCursor {
    RevisionFirstCursor::new(
        CommittedSourceRevision::from_raw(0),
        ProcessLocalMutationCorrelationId::from_raw(42),
    )
}

fn missing_echo(path: &str) -> CommittedWatcherEcho {
    CommittedWatcherEcho {
        relative_path: PathBuf::from(path),
        expected_state: CommittedWatcherPathState::Missing,
    }
}

#[test]
fn committed_mutation_acknowledgement_removes_matching_pending_echo() {
    let root = tempfile::tempdir().expect("source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("source_id::committed"),
        root.path().to_path_buf(),
    );
    let mut state = GuiSourceWatchState {
        sources: vec![source],
        ..Default::default()
    };
    let started = Instant::now();
    state.collect_event(
        &Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Any),
            paths: vec![root.path().join("kick.wav")],
            attrs: Default::default(),
        },
        started,
    );

    state.acknowledge_committed_paths(
        SourceId::from_string("source_id::committed").as_str(),
        &[missing_echo("kick.wav")],
        cursor(),
        started,
    );

    assert!(state.pending.is_empty());
    assert!(
        state
            .drain_ready_sources(started + SOURCE_CHANGE_DEBOUNCE, SOURCE_CHANGE_DEBOUNCE)
            .is_empty()
    );
}

#[test]
fn watcher_acknowledgement_consumes_only_one_echo() {
    let root = tempfile::tempdir().expect("source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("source_id::fallback"),
        root.path().to_path_buf(),
    );
    let mut state = GuiSourceWatchState {
        sources: vec![source],
        ..Default::default()
    };
    let started = Instant::now();
    state.acknowledge_committed_paths(
        SourceId::from_string("source_id::fallback").as_str(),
        &[missing_echo("kick.wav")],
        cursor(),
        started,
    );
    let event = Event {
        kind: EventKind::Modify(notify::event::ModifyKind::Any),
        paths: vec![root.path().join("kick.wav")],
        attrs: Default::default(),
    };

    state.collect_event(&event, started + SOURCE_CHANGE_DEBOUNCE);
    assert!(
        state.pending.is_empty(),
        "matching watcher echo is suppressed"
    );

    let external_change_at = started + SOURCE_CHANGE_DEBOUNCE + std::time::Duration::from_millis(1);
    state.collect_event(&event, external_change_at);
    assert!(
        state
            .pending
            .get("source_id::fallback")
            .is_some_and(|pending| pending.paths.contains(&PathBuf::from("kick.wav"))),
        "a second same-path change must retain watcher fallback"
    );
}

#[test]
fn watcher_acknowledgement_does_not_hide_external_change_before_internal_echo() {
    let root = tempfile::tempdir().expect("source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("source_id::external"),
        root.path().to_path_buf(),
    );
    let path = root.path().join("kick.wav");
    std::fs::write(&path, b"committed").expect("write committed file");
    let committed_modified = std::fs::metadata(&path)
        .expect("committed metadata")
        .modified()
        .expect("committed modified time");
    let expected_state = observed_watcher_path_state(&path).expect("committed path identity");
    let mut state = GuiSourceWatchState {
        sources: vec![source],
        ..Default::default()
    };
    let started = Instant::now();
    state.acknowledge_committed_paths(
        SourceId::from_string("source_id::external").as_str(),
        &[CommittedWatcherEcho {
            relative_path: PathBuf::from("kick.wav"),
            expected_state,
        }],
        cursor(),
        started,
    );

    std::fs::write(&path, b"external!").expect("write equal-size external change");
    std::fs::File::open(&path)
        .expect("open external file")
        .set_times(std::fs::FileTimes::new().set_modified(committed_modified))
        .expect("restore committed modified time");
    assert_eq!(
        std::fs::metadata(&path).expect("external metadata").len(),
        b"committed".len() as u64
    );
    assert_eq!(
        std::fs::metadata(&path)
            .expect("external metadata")
            .modified()
            .expect("external modified time"),
        committed_modified
    );
    state.collect_event(
        &Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Any),
            paths: vec![path],
            attrs: Default::default(),
        },
        started + std::time::Duration::from_millis(1),
    );

    assert!(
        state
            .pending
            .get("source_id::external")
            .is_some_and(|pending| pending.paths.contains(&PathBuf::from("kick.wav"))),
        "mismatched filesystem identity must retain external watcher event"
    );
    assert!(
        state.acknowledged_paths.is_empty(),
        "mismatched pending event must not leave a future suppression token"
    );
}
