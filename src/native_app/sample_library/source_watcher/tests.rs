use super::classification::{path_is_source_refresh_candidate, retain_source_refresh_candidates};
use super::handle::{GuiSourceWatcherHandle, doubled_backoff};
use super::roots::{RootIdentityRecovery, RootWatchUpdate, root_watch_status};
use super::state::GuiSourceWatchState;
use notify::{Event, EventKind, event::RemoveKind};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use wavecrate::sample_sources::{SampleSource, SourceDatabase, SourceId};
use wavecrate_scan::sample_sources::scanner::{scan_once, sync_paths};

use crate::native_app::app::GuiMessage;

#[path = "tests/committed_mutations.rs"]
mod committed_mutations;

fn stable_root_identity(root: &Path) -> String {
    let metadata = fs::metadata(root).expect("read root metadata");
    wavecrate_library::filesystem_identity::stable_filesystem_identity(root, &metadata)
        .expect("temporary root should expose stable identity")
}

#[test]
fn removed_extension_named_folder_triggers_source_refresh() {
    let root = PathBuf::from(r"C:\samples");
    let source =
        SampleSource::new_with_id(SourceId::from_string("source_id::samples"), root.clone());
    let mut state = GuiSourceWatchState {
        sources: vec![source],
        ..Default::default()
    };
    let event = Event {
        kind: EventKind::Remove(RemoveKind::Folder),
        paths: vec![root.join("Drum.Loops")],
        attrs: Default::default(),
    };

    state.collect_event(&event, Instant::now());

    let pending = state.pending.get("source_id::samples").unwrap();
    assert!(pending.paths.contains(&PathBuf::from("Drum.Loops")));
    assert!(!pending.overflowed);
}

#[test]
fn wavecrate_metadata_files_do_not_trigger_source_refresh() {
    let root = PathBuf::from(r"C:\samples");
    assert!(!path_is_source_refresh_candidate(
        &root.join(wavecrate::sample_sources::db::DB_FILE_NAME),
        EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Any
        )),
    ));
}

#[test]
fn metadata_event_storm_is_filtered_before_the_bounded_watcher_queue() {
    let root = PathBuf::from(r"C:\samples");
    let mut event = Event {
        kind: EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Any,
        )),
        paths: vec![
            root.join(wavecrate::sample_sources::db::DB_FILE_NAME),
            root.join(format!(
                "{}-wal",
                wavecrate::sample_sources::db::DB_FILE_NAME
            )),
            root.join("kick.wav"),
        ],
        attrs: Default::default(),
    };

    assert!(retain_source_refresh_candidates(&mut event));
    assert_eq!(event.paths, vec![root.join("kick.wav")]);

    event.paths = vec![root.join(wavecrate::sample_sources::db::DB_FILE_NAME)];
    assert!(!retain_source_refresh_candidates(&mut event));
    assert!(event.paths.is_empty());
}

#[test]
fn apple_double_sidecars_do_not_trigger_source_refresh() {
    let root = PathBuf::from(r"C:\samples");
    assert!(!path_is_source_refresh_candidate(
        &root.join("._kick.wav"),
        EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Any
        )),
    ));
    assert!(!path_is_source_refresh_candidate(
        &root.join("drums").join("._snare.wav"),
        EventKind::Create(notify::event::CreateKind::File),
    ));
}

#[cfg(unix)]
#[test]
fn symlink_replacement_stays_a_watcher_candidate_until_targeted_sync_retires_it() {
    use std::os::unix::fs as unix_fs;

    let root = tempfile::tempdir().expect("create source watcher fixture");
    let outside = tempfile::tempdir().expect("create outside fixture");
    let tracked = root.path().join("kick.wav");
    let target = outside.path().join("outside.wav");
    fs::write(&tracked, b"indexed").expect("write indexed source file");
    fs::write(&target, b"outside").expect("write outside file");
    let database = SourceDatabase::open_for_source_write(root.path()).expect("open source db");
    scan_once(&database).expect("index tracked file");

    fs::remove_file(&tracked).expect("remove indexed source file");
    unix_fs::symlink(&target, &tracked).expect("replace source file with link");
    assert!(path_is_source_refresh_candidate(
        &tracked,
        EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Any,
        )),
    ));
    let stats = sync_paths(&database, &[PathBuf::from("kick.wav")])
        .expect("reconcile link replacement without following it");
    assert_eq!(stats.missing, 1);
    assert!(
        database
            .entry_for_path(Path::new("kick.wav"))
            .expect("read indexed entry")
            .is_none()
    );
}

#[test]
fn transient_ann_persistence_artifacts_do_not_trigger_source_refresh() {
    let root = PathBuf::from(r"C:\samples");
    for path in [
        root.join("ann_containerMqsFy3"),
        root.join("ann_dumpJgi2JJ"),
        root.join("ann_dumpJgi2JJ").join("ann_dump.hnsw.data"),
        root.join("ann_dumpJgi2JJ").join("ann_dump.hnsw.graph"),
    ] {
        assert!(!path_is_source_refresh_candidate(
            &path,
            EventKind::Create(notify::event::CreateKind::Any),
        ));
    }
    assert!(path_is_source_refresh_candidate(
        &root.join("ann_dump.wav"),
        EventKind::Create(notify::event::CreateKind::File),
    ));
    assert!(path_is_source_refresh_candidate(
        &root.join("ann_dump_samples"),
        EventKind::Create(notify::event::CreateKind::Folder),
    ));
}

#[test]
fn source_root_event_overflows_to_full_refresh() {
    let root = PathBuf::from(r"C:\samples");
    let source =
        SampleSource::new_with_id(SourceId::from_string("source_id::samples"), root.clone());
    let mut state = GuiSourceWatchState {
        sources: vec![source],
        ..Default::default()
    };
    let event = Event {
        kind: EventKind::Any,
        paths: vec![root],
        attrs: Default::default(),
    };

    state.collect_event(&event, Instant::now());

    let pending = state.pending.get("source_id::samples").unwrap();
    assert!(pending.paths.is_empty());
    assert!(pending.overflowed);
}

#[test]
fn debounce_drain_waits_until_source_is_ready() {
    let root = PathBuf::from(r"C:\samples");
    let source =
        SampleSource::new_with_id(SourceId::from_string("source_id::samples"), root.clone());
    let mut state = GuiSourceWatchState {
        sources: vec![source],
        ..Default::default()
    };
    let started = Instant::now();
    let event = Event {
        kind: EventKind::Any,
        paths: vec![root.join("kick.wav")],
        attrs: Default::default(),
    };
    state.collect_event(&event, started);

    assert!(
        state
            .drain_ready_sources(started, super::SOURCE_CHANGE_DEBOUNCE)
            .is_empty()
    );

    let ready = state.drain_ready_sources(
        started + super::SOURCE_CHANGE_DEBOUNCE,
        super::SOURCE_CHANGE_DEBOUNCE,
    );

    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].source_id, "source_id::samples");
    assert_eq!(ready[0].paths, vec![PathBuf::from("kick.wav")]);
    assert!(!ready[0].overflowed);
}

#[test]
fn debounce_drain_reports_root_availability_from_the_watcher_thread() {
    let root = tempfile::tempdir().expect("source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("source_id::availability"),
        root.path().to_path_buf(),
    );
    let mut state = GuiSourceWatchState {
        sources: vec![source],
        ..Default::default()
    };
    let started = Instant::now();

    state.mark_all_overflowed(started);
    let ready = state.drain_ready_sources(
        started + super::SOURCE_CHANGE_DEBOUNCE,
        super::SOURCE_CHANGE_DEBOUNCE,
    );
    assert!(ready[0].source_root_available);

    std::fs::remove_dir_all(root.path()).expect("remove source root");
    state.mark_all_overflowed(started + super::SOURCE_CHANGE_DEBOUNCE);
    let ready = state.drain_ready_sources(
        started + super::SOURCE_CHANGE_DEBOUNCE * 2,
        super::SOURCE_CHANGE_DEBOUNCE,
    );
    assert!(!ready[0].source_root_available);
}

#[test]
fn live_root_events_do_not_feed_database_writes_back_into_scans() {
    let root = tempfile::tempdir().expect("source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("source_id::root-event"),
        root.path().to_path_buf(),
    );
    let mut state = GuiSourceWatchState {
        watched_roots: HashMap::from([(
            root.path().to_path_buf(),
            Some(stable_root_identity(root.path())),
        )]),
        sources: vec![source],
        ..Default::default()
    };

    state.collect_event(
        &Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Any),
            paths: vec![root.path().to_path_buf()],
            attrs: Default::default(),
        },
        Instant::now(),
    );

    assert!(state.pending.is_empty());
}

#[test]
fn destructive_root_event_invalidates_even_when_the_current_identity_matches() {
    let root = tempfile::tempdir().expect("source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("source_id::destructive-root-event"),
        root.path().to_path_buf(),
    );
    let mut state = GuiSourceWatchState {
        watched_roots: HashMap::from([(
            root.path().to_path_buf(),
            Some(stable_root_identity(root.path())),
        )]),
        sources: vec![source],
        ..Default::default()
    };

    let invalidated = state.collect_event(
        &Event {
            kind: EventKind::Remove(RemoveKind::Folder),
            paths: vec![root.path().to_path_buf()],
            attrs: Default::default(),
        },
        Instant::now(),
    );

    assert!(invalidated);
    assert!(
        state
            .pending
            .get("source_id::destructive-root-event")
            .is_some_and(|pending| pending.overflowed)
    );
}

#[test]
fn unreadable_live_root_identity_suppresses_metadata_echoes_but_not_replacement_events() {
    let root = tempfile::tempdir().expect("source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("source_id::uncertain-root-event"),
        root.path().to_path_buf(),
    );
    let mut state = GuiSourceWatchState {
        watched_roots: HashMap::from([(root.path().to_path_buf(), None)]),
        sources: vec![source],
        ..Default::default()
    };
    let started = Instant::now();

    let metadata_invalidated = state.collect_event(
        &Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Metadata(
                notify::event::MetadataKind::Any,
            )),
            paths: vec![root.path().to_path_buf()],
            attrs: Default::default(),
        },
        started,
    );
    assert!(!metadata_invalidated);
    assert!(state.pending.is_empty());

    let replacement_invalidated = state.collect_event(
        &Event {
            kind: EventKind::Create(notify::event::CreateKind::Folder),
            paths: vec![root.path().to_path_buf()],
            attrs: Default::default(),
        },
        started,
    );
    assert!(replacement_invalidated);
    assert!(
        state
            .pending
            .get("source_id::uncertain-root-event")
            .is_some_and(|pending| pending.overflowed)
    );
}

#[test]
fn same_path_root_identity_replacement_invalidates_the_watcher() {
    let parent = tempfile::tempdir().expect("source parent");
    let root = parent.path().join("source");
    let retired = parent.path().join("retired");
    fs::create_dir(&root).expect("create source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("source_id::identity-replacement"),
        root.clone(),
    );
    let mut state = GuiSourceWatchState {
        watched_roots: HashMap::from([(root.clone(), Some(stable_root_identity(&root)))]),
        sources: vec![source],
        ..Default::default()
    };

    fs::rename(&root, &retired).expect("retire original source root");
    fs::create_dir(&root).expect("create replacement source root");
    let invalidated = state.collect_event(
        &Event {
            kind: EventKind::Any,
            paths: vec![root],
            attrs: Default::default(),
        },
        Instant::now(),
    );

    assert!(invalidated);
    assert!(
        state
            .pending
            .get("source_id::identity-replacement")
            .is_some_and(|pending| pending.overflowed)
    );
}

#[test]
fn periodic_root_status_detects_atomic_same_path_replacement() {
    let parent = tempfile::tempdir().expect("source parent");
    let root = parent.path().join("source");
    let replacement = parent.path().join("replacement");
    let retired = parent.path().join("retired");
    fs::create_dir(&root).expect("create source root");
    fs::create_dir(&replacement).expect("create replacement root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("source_id::atomic-replacement"),
        root.clone(),
    );
    let watched = HashMap::from([(root.clone(), Some(stable_root_identity(&root)))]);

    fs::rename(&root, &retired).expect("retire source root");
    fs::rename(&replacement, &root).expect("install replacement root");
    let status = root_watch_status(&watched, &[source]);

    assert_eq!(status.changed_roots, vec![root]);
    assert!(status.uncertain_roots.is_empty());
    assert!(!status.has_unavailable_roots);
}

#[test]
fn periodic_root_status_detects_disappearance_and_reappearance() {
    let parent = tempfile::tempdir().expect("source parent");
    let root = parent.path().join("source");
    let offline = parent.path().join("offline");
    fs::create_dir(&root).expect("create source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("source_id::root-recovery"),
        root.clone(),
    );
    let watched = HashMap::from([(root.clone(), Some(stable_root_identity(&root)))]);

    fs::rename(&root, &offline).expect("make root unavailable");
    let unavailable = root_watch_status(&watched, std::slice::from_ref(&source));
    assert_eq!(unavailable.changed_roots, vec![root.clone()]);
    assert!(unavailable.has_unavailable_roots);

    fs::rename(&offline, &root).expect("restore root");
    let reappeared = root_watch_status(&HashMap::new(), &[source]);
    assert_eq!(reappeared.changed_roots, vec![root]);
    assert!(!reappeared.has_unavailable_roots);
}

#[test]
fn unreadable_root_identity_falls_back_to_bounded_full_reconciliation() {
    let root = PathBuf::from("identity-unavailable");
    let started = Instant::now();
    let mut recovery = RootIdentityRecovery::default();

    assert_eq!(
        recovery.due_roots(std::slice::from_ref(&root), started),
        vec![root.clone()]
    );
    assert!(
        recovery
            .due_roots(
                std::slice::from_ref(&root),
                started + super::ROOT_IDENTITY_RETRY_MIN / 2,
            )
            .is_empty()
    );
    assert_eq!(
        recovery.due_roots(
            std::slice::from_ref(&root),
            started + super::ROOT_IDENTITY_RETRY_MIN,
        ),
        vec![root]
    );
    assert!(
        recovery
            .due_roots(&[], started + Duration::from_secs(1))
            .is_empty()
    );
}

#[test]
fn initial_watch_registration_does_not_queue_full_source_refresh() {
    let root = tempfile::tempdir().expect("source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("source_id::initial-watch"),
        root.path().to_path_buf(),
    );
    let mut state = GuiSourceWatchState {
        sources: vec![source],
        ..Default::default()
    };
    let (_unavailable, failed) = state.apply_root_watch_update(
        RootWatchUpdate {
            changed_roots: vec![root.path().to_path_buf()],
            has_unavailable_roots: false,
            watch_failed: false,
        },
        Instant::now(),
        false,
    );

    assert!(!failed);
    assert!(state.pending.is_empty());
}

#[test]
fn later_watch_registration_queues_authoritative_source_refresh() {
    let root = tempfile::tempdir().expect("source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("source_id::reappeared-watch"),
        root.path().to_path_buf(),
    );
    let mut state = GuiSourceWatchState {
        sources: vec![source],
        ..Default::default()
    };
    let (_unavailable, failed) = state.apply_root_watch_update(
        RootWatchUpdate {
            changed_roots: vec![root.path().to_path_buf()],
            has_unavailable_roots: false,
            watch_failed: false,
        },
        Instant::now(),
        true,
    );

    assert!(!failed);
    assert!(
        state
            .pending
            .get("source_id::reappeared-watch")
            .is_some_and(|pending| pending.overflowed)
    );
}

#[test]
fn path_churn_is_bounded_and_escalates_to_full_refresh() {
    let root = PathBuf::from(r"C:\samples");
    let source =
        SampleSource::new_with_id(SourceId::from_string("source_id::samples"), root.clone());
    let mut state = GuiSourceWatchState {
        sources: vec![source],
        ..Default::default()
    };
    let started = Instant::now();
    for index in 0..=super::MAX_PENDING_PATHS_PER_SOURCE {
        state.collect_event(
            &Event {
                kind: EventKind::Create(notify::event::CreateKind::File),
                paths: vec![root.join(format!("sample-{index}.wav"))],
                attrs: Default::default(),
            },
            started,
        );
    }

    let pending = state.pending.get("source_id::samples").unwrap();
    assert!(pending.overflowed);
    assert!(pending.paths.is_empty());
}

#[test]
fn watcher_restart_marks_every_source_for_authoritative_refresh() {
    let first = SampleSource::new_with_id(
        SourceId::from_string("source_id::first"),
        PathBuf::from(r"C:\first"),
    );
    let second = SampleSource::new_with_id(
        SourceId::from_string("source_id::second"),
        PathBuf::from(r"C:\second"),
    );
    let mut state = GuiSourceWatchState {
        sources: vec![first, second],
        ..Default::default()
    };
    let started = Instant::now();

    state.mark_all_overflowed(started);
    let mut ready = state.drain_ready_sources(
        started + super::SOURCE_CHANGE_DEBOUNCE,
        super::SOURCE_CHANGE_DEBOUNCE,
    );
    ready.sort_by(|left, right| left.source_id.cmp(&right.source_id));

    assert_eq!(ready.len(), 2);
    assert!(ready.iter().all(|event| event.overflowed));
    assert!(ready.iter().all(|event| event.paths.is_empty()));
}

#[test]
fn foreground_reconciliation_request_refreshes_every_configured_source() {
    let first_root = tempfile::tempdir().expect("first watched source root");
    let second_root = tempfile::tempdir().expect("second watched source root");
    let first = SampleSource::new_with_id(
        SourceId::from_string("source_id::foreground-first"),
        first_root.path().to_path_buf(),
    );
    let second = SampleSource::new_with_id(
        SourceId::from_string("source_id::foreground-second"),
        second_root.path().to_path_buf(),
    );
    let expected_source_ids = [
        first.id.as_str().to_string(),
        second.id.as_str().to_string(),
    ];
    let (sender, receiver) = std::sync::mpsc::channel();
    let watcher = GuiSourceWatcherHandle::spawn(vec![first, second], sender);
    watcher.wait_until_ready_for_tests();
    while !matches!(
        receiver
            .recv_timeout(super::WATCHER_START_TIMEOUT)
            .expect("watcher-ready message"),
        GuiMessage::SourceWatcherReady { .. }
    ) {}

    watcher.request_full_reconciliation();

    let deadline = Instant::now()
        + super::SOURCE_CHANGE_DEBOUNCE
        + super::WATCHER_POLL_INTERVAL.saturating_mul(4);
    let mut refreshed_source_ids = Vec::new();
    while refreshed_source_ids.len() < expected_source_ids.len() {
        let message = receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("foreground reconciliation event");
        if let GuiMessage::SourceFilesystemChanged {
            source_id,
            paths,
            overflowed,
            source_root_available,
            ..
        } = message
        {
            assert!(paths.is_empty());
            assert!(overflowed);
            assert!(source_root_available);
            refreshed_source_ids.push(source_id);
        }
    }
    refreshed_source_ids.sort();

    assert_eq!(refreshed_source_ids, expected_source_ids);
}

#[test]
fn watcher_restart_backoff_is_bounded() {
    assert_eq!(
        doubled_backoff(super::WATCHER_RESTART_MIN),
        super::WATCHER_RESTART_MIN * 2
    );
    assert_eq!(
        doubled_backoff(super::WATCHER_RESTART_MAX),
        super::WATCHER_RESTART_MAX
    );
}

#[test]
fn idempotent_startup_source_sync_does_not_refresh_every_source() {
    let root = tempfile::tempdir().expect("watched source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("source_id::startup-sync"),
        root.path().to_path_buf(),
    );
    let (sender, receiver) = std::sync::mpsc::channel();
    let watcher = GuiSourceWatcherHandle::spawn(vec![source.clone()], sender);

    watcher.replace_sources(vec![source]);
    watcher.wait_until_ready_for_tests();
    std::thread::sleep(
        super::SOURCE_CHANGE_DEBOUNCE + super::WATCHER_POLL_INTERVAL.saturating_mul(2),
    );

    let messages = receiver.try_iter().collect::<Vec<_>>();
    assert_eq!(
        messages
            .iter()
            .filter(|message| matches!(message, GuiMessage::SourceWatcherReady { .. }))
            .count(),
        1,
        "the startup audit boundary must be published exactly once"
    );
    let refreshes = messages
        .into_iter()
        .filter_map(|message| {
            let GuiMessage::SourceFilesystemChanged {
                source_id,
                paths,
                overflowed,
                source_root_available,
                ..
            } = message
            else {
                return None;
            };
            Some((source_id, paths, overflowed, source_root_available))
        })
        .collect::<Vec<_>>();
    assert!(
        refreshes.is_empty(),
        "repeating the configured source list during startup must not synthesize overflow scans: \
        {refreshes:?}"
    );
}

#[test]
fn filesystem_event_after_initial_watcher_ready_is_not_suppressed() {
    let root = tempfile::tempdir().expect("watched source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("source_id::post-ready-event"),
        root.path().to_path_buf(),
    );
    let source_id = source.id.as_str().to_string();
    let (sender, receiver) = std::sync::mpsc::channel();
    let watcher = GuiSourceWatcherHandle::spawn(vec![source], sender);
    watcher.wait_until_ready_for_tests();
    while !matches!(
        receiver
            .recv_timeout(super::WATCHER_START_TIMEOUT)
            .expect("watcher-ready message"),
        GuiMessage::SourceWatcherReady { .. }
    ) {}

    let created = root.path().join("recording.wav");
    std::fs::write(&created, [0_u8; 8]).expect("create watched audio file");
    watcher.inject_paths_for_tests(vec![created]);

    let deadline = Instant::now()
        + super::SOURCE_CHANGE_DEBOUNCE
        + super::WATCHER_POLL_INTERVAL.saturating_mul(4);
    let refresh = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let message = receiver
            .recv_timeout(remaining)
            .expect("post-ready filesystem refresh");
        if let GuiMessage::SourceFilesystemChanged {
            source_id,
            paths,
            overflowed,
            source_root_available,
            ..
        } = message
        {
            break (source_id, paths, overflowed, source_root_available);
        }
    };

    assert_eq!(refresh.0, source_id);
    assert_eq!(refresh.1, vec![PathBuf::from("recording.wav")]);
    assert!(!refresh.2);
    assert!(refresh.3);
}

#[test]
fn watcher_restarts_and_overflows_when_a_live_root_is_replaced_at_the_same_path() {
    let parent = tempfile::tempdir().expect("source parent");
    let root = parent.path().join("source");
    let retired = parent.path().join("retired");
    fs::create_dir(&root).expect("create watched source root");
    let source = SampleSource::new_with_id(
        SourceId::from_string("source_id::same-path-restart"),
        root.clone(),
    );
    let expected_source_id = source.id.as_str().to_string();
    let (sender, receiver) = std::sync::mpsc::channel();
    let watcher = GuiSourceWatcherHandle::spawn(vec![source], sender);
    watcher.wait_until_ready_for_tests();
    while !matches!(
        receiver
            .recv_timeout(super::WATCHER_START_TIMEOUT)
            .expect("watcher-ready message"),
        GuiMessage::SourceWatcherReady { .. }
    ) {}

    fs::rename(&root, &retired).expect("retire watched source root");
    fs::create_dir(&root).expect("create replacement source root");
    watcher.force_root_refresh_for_tests();

    let deadline = Instant::now() + super::WATCHER_START_TIMEOUT + super::SOURCE_CHANGE_DEBOUNCE;
    let refresh = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let message = receiver
            .recv_timeout(remaining)
            .expect("same-path replacement refresh");
        if let GuiMessage::SourceFilesystemChanged {
            source_id,
            paths,
            overflowed,
            source_root_available,
            ..
        } = message
            && source_id == expected_source_id
            && overflowed
        {
            break (paths, source_root_available);
        }
    };
    watcher.wait_until_ready_for_tests();

    assert!(refresh.0.is_empty());
    assert!(refresh.1);
}
