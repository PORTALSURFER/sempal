//! File system watcher for source roots that reports audio-relevant changes.

use crate::egui_app::controller::jobs::JobMessage;
use crate::sample_sources::{SourceId, db::DB_FILE_NAME, is_supported_audio};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Result as NotifyResult, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use std::time::Duration;

const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(200);

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
}

/// Event emitted when a watched source sees an on-disk change worth syncing.
#[derive(Debug, Clone)]
pub(crate) struct SourceWatchEvent {
    pub(crate) source_id: SourceId,
}

/// Spawn the watcher thread and return a sender used to update watched sources.
pub(crate) fn spawn_source_watcher(
    message_tx: Sender<JobMessage>,
) -> Sender<SourceWatchCommand> {
    let (command_tx, command_rx) = std::sync::mpsc::channel();
    thread::spawn(move || run_source_watcher(command_rx, message_tx));
    command_tx
}

fn run_source_watcher(command_rx: Receiver<SourceWatchCommand>, message_tx: Sender<JobMessage>) {
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

    loop {
        match command_rx.recv_timeout(COMMAND_POLL_INTERVAL) {
            Ok(command) => match command {
                SourceWatchCommand::ReplaceSources(next_sources) => {
                    update_watched_sources(&mut watcher, &mut watched_roots, &next_sources);
                    sources = next_sources;
                }
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
                let _ = message_tx.send(JobMessage::SourceWatch(SourceWatchEvent { source_id }));
            }
        }
    }
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
    path.extension().is_none()
}

fn path_is_ignored(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.starts_with(DB_FILE_NAME)
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
    fn path_is_candidate_allows_extensionless_paths() {
        assert!(path_is_candidate(Path::new("Samples")));
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
}
