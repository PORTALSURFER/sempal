//! File system watcher for source roots that reports audio-relevant changes.

use crate::egui_app::controller::jobs::{JobMessage, JobMessageSender};
use crate::sample_sources::{SourceId, db::DB_FILE_NAME, is_supported_audio};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Result as NotifyResult, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(200);
const SOURCE_WATCH_DEBOUNCE: Duration = Duration::from_millis(400);

/// Input used to configure which source roots are actively watched.
#[derive(Clone, Debug)]
pub(crate) struct SourceWatchEntry {
    pub(crate) source_id: SourceId,
    pub(crate) root: PathBuf,
}

impl SourceWatchEntry {
    /// Create a watch entry for a source root.
    pub(crate) fn new(source_id: SourceId, root: PathBuf) -> Self {
        Self { source_id, root }
    }
}

/// Commands sent to the watcher thread to update its configuration.
#[derive(Debug)]
pub(crate) enum SourceWatchCommand {
    /// Replace the watched sources with a new list of source roots.
    ReplaceSources(Vec<SourceWatchEntry>),
    /// Tell the watcher whether a scan job is currently running.
    SetScanInProgress { in_progress: bool },
    /// Signal the watcher thread to exit.
    Shutdown,
}

/// Event emitted when a watched source sees an on-disk change worth syncing.
#[derive(Debug, Clone)]
pub(crate) struct SourceWatchEvent {
    pub(crate) source_id: SourceId,
}

/// Join handle and command sender for the source watcher thread.
pub(crate) struct SourceWatcherHandle {
    command_tx: Sender<SourceWatchCommand>,
    join_handle: Option<thread::JoinHandle<()>>,
}

impl SourceWatcherHandle {
    /// Send a command to the watcher thread.
    pub(crate) fn send(&self, command: SourceWatchCommand) {
        let _ = self.command_tx.send(command);
    }

    /// Signal the watcher thread to exit and wait for it to finish.
    pub(crate) fn shutdown(&mut self) {
        let _ = self.command_tx.send(SourceWatchCommand::Shutdown);
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

/// Spawn the watcher thread and return a handle used to update watched sources.
pub(crate) fn spawn_source_watcher(message_tx: JobMessageSender) -> SourceWatcherHandle {
    let (command_tx, command_rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || run_source_watcher(command_rx, message_tx));
    SourceWatcherHandle {
        command_tx,
        join_handle: Some(handle),
    }
}

fn run_source_watcher(command_rx: Receiver<SourceWatchCommand>, message_tx: JobMessageSender) {
    let (event_tx, event_rx) = std::sync::mpsc::channel::<NotifyResult<Event>>();
    let mut watcher = match notify::recommended_watcher(move |event| {
        let _ = event_tx.send(event);
    }) {
        Ok(watcher) => watcher,
        Err(err) => {
            tracing::warn!("Failed to initialize source watcher: {err}");
            return;
        }
    };
    let mut watched_roots: HashSet<PathBuf> = HashSet::new();
    let mut sources: Vec<SourceWatchEntry> = Vec::new();
    let mut pending: HashMap<SourceId, PendingSourceWatch> = HashMap::new();
    let mut scan_in_progress = false;

    loop {
        match command_rx.recv_timeout(COMMAND_POLL_INTERVAL) {
            Ok(command) => match command {
                SourceWatchCommand::ReplaceSources(next_sources) => {
                    update_watched_sources(&mut watcher, &mut watched_roots, &next_sources);
                    sources = next_sources;
                    prune_pending_sources(&mut pending, &sources);
                }
                SourceWatchCommand::SetScanInProgress { in_progress } => {
                    scan_in_progress = in_progress;
                }
                SourceWatchCommand::Shutdown => break,
            },
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        while let Ok(event) = event_rx.try_recv() {
            let event = match event {
                Ok(event) => event,
                Err(err) => {
                    tracing::warn!("Source watcher error: {err}");
                    continue;
                }
            };
            if !event_triggers_sync(&event) {
                continue;
            }
            let mut impacted = HashSet::new();
            for path in &event.paths {
                if !path_is_candidate(path) {
                    continue;
                }
                if let Some(source_id) = select_source_for_path(&sources, path) {
                    impacted.insert(source_id);
                }
            }
            for source_id in impacted {
                update_pending_watch(&mut pending, source_id, Instant::now());
            }
        }

        let ready = drain_ready_sources(
            &mut pending,
            Instant::now(),
            SOURCE_WATCH_DEBOUNCE,
            scan_in_progress,
        );
        for source_id in ready {
            let _ = message_tx.send(JobMessage::SourceWatch(SourceWatchEvent { source_id }));
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct PendingSourceWatch {
    last_event: Instant,
}

fn update_pending_watch(
    pending: &mut HashMap<SourceId, PendingSourceWatch>,
    source_id: SourceId,
    now: Instant,
) {
    pending.insert(source_id, PendingSourceWatch { last_event: now });
}

fn drain_ready_sources(
    pending: &mut HashMap<SourceId, PendingSourceWatch>,
    now: Instant,
    debounce: Duration,
    scan_in_progress: bool,
) -> Vec<SourceId> {
    if scan_in_progress {
        return Vec::new();
    }
    let ready: Vec<SourceId> = pending
        .iter()
        .filter_map(|(source_id, entry)| {
            (now.saturating_duration_since(entry.last_event) >= debounce)
                .then(|| source_id.clone())
        })
        .collect();
    for source_id in &ready {
        pending.remove(source_id);
    }
    ready
}

fn prune_pending_sources(
    pending: &mut HashMap<SourceId, PendingSourceWatch>,
    sources: &[SourceWatchEntry],
) {
    let allowed: HashSet<&SourceId> = sources.iter().map(|entry| &entry.source_id).collect();
    pending.retain(|source_id, _| allowed.contains(source_id));
}

fn update_watched_sources(
    watcher: &mut RecommendedWatcher,
    watched_roots: &mut HashSet<PathBuf>,
    sources: &[SourceWatchEntry],
) {
    let desired: HashSet<PathBuf> = sources
        .iter()
        .map(|entry| entry.root.clone())
        .filter(|root| root.is_dir())
        .collect();
    for root in watched_roots.difference(&desired).cloned().collect::<Vec<_>>() {
        if let Err(err) = watcher.unwatch(&root) {
            tracing::warn!("Failed to unwatch source root {}: {err}", root.display());
        }
        watched_roots.remove(&root);
    }
    for root in desired.difference(watched_roots).cloned().collect::<Vec<_>>() {
        if let Err(err) = watcher.watch(&root, RecursiveMode::Recursive) {
            tracing::warn!("Failed to watch source root {}: {err}", root.display());
            continue;
        }
        watched_roots.insert(root);
    }
}

fn event_triggers_sync(event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
    )
}

fn select_source_for_path(
    sources: &[SourceWatchEntry],
    path: &Path,
) -> Option<SourceId> {
    sources
        .iter()
        .filter(|entry| path.starts_with(&entry.root))
        .max_by_key(|entry| entry.root.as_os_str().len())
        .map(|entry| entry.source_id.clone())
}

fn path_is_candidate(path: &Path) -> bool {
    if path_is_ignored(path) {
        return false;
    }
    if is_supported_audio(path) {
        return true;
    }
    path_extensionless_is_directory(path)
}

fn path_is_ignored(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.starts_with(DB_FILE_NAME)
}

fn path_extensionless_is_directory(path: &Path) -> bool {
    if path.extension().is_some() {
        return false;
    }
    match path.metadata() {
        Ok(metadata) => metadata.is_dir(),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_is_candidate_filters_db_files() {
        assert!(!path_is_candidate(Path::new(DB_FILE_NAME)));
        assert!(!path_is_candidate(Path::new(&format!("{DB_FILE_NAME}-wal"))));
    }

    #[test]
    fn path_is_candidate_allows_supported_audio() {
        assert!(path_is_candidate(Path::new("kick.wav")));
        assert!(path_is_candidate(Path::new("loop.flac")));
    }

    #[test]
    fn path_is_candidate_allows_extensionless_directories() {
        let root = std::env::temp_dir().join("sempal_source_watch_dir");
        std::fs::create_dir_all(&root).unwrap();
        assert!(path_is_candidate(&root));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn select_source_for_path_picks_longest_root() {
        let first = SourceWatchEntry::new(
            SourceId::from_string("a"),
            PathBuf::from("/music"),
        );
        let second = SourceWatchEntry::new(
            SourceId::from_string("b"),
            PathBuf::from("/music/drums"),
        );
        let path = Path::new("/music/drums/kicks/kick.wav");
        let selected = select_source_for_path(&[first, second], path).unwrap();
        assert_eq!(selected.as_str(), "b");
    }

    #[test]
    fn drain_ready_sources_waits_for_debounce() {
        let mut pending = HashMap::new();
        let source_id = SourceId::from_string("a");
        let start = Instant::now();
        update_pending_watch(&mut pending, source_id.clone(), start);
        assert!(drain_ready_sources(
            &mut pending,
            start + Duration::from_millis(200),
            Duration::from_millis(400),
            false
        )
        .is_empty());
        let ready = drain_ready_sources(
            &mut pending,
            start + Duration::from_millis(500),
            Duration::from_millis(400),
            false,
        );
        assert_eq!(ready, vec![source_id]);
    }

    #[test]
    fn drain_ready_sources_honors_scan_in_progress() {
        let mut pending = HashMap::new();
        let source_id = SourceId::from_string("a");
        let start = Instant::now();
        update_pending_watch(&mut pending, source_id, start);
        let ready = drain_ready_sources(
            &mut pending,
            start + Duration::from_millis(500),
            Duration::from_millis(400),
            true,
        );
        assert!(ready.is_empty());
        assert_eq!(pending.len(), 1);
    }
}
