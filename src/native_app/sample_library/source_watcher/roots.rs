use notify::{RecursiveMode, Watcher};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use wavecrate::sample_sources::SampleSource;
use wavecrate_library::filesystem_identity::stable_filesystem_identity;

pub(super) use crate::native_app::sample_library::committed_file_mutations::observed_watcher_path_state;

pub(super) fn source_root_is_available(source: &SampleSource) -> bool {
    source.root.is_dir()
}

pub(super) type WatchedRootIdentities = HashMap<PathBuf, Option<String>>;

pub(super) struct RootWatchUpdate {
    pub(super) changed_roots: Vec<PathBuf>,
    pub(super) has_unavailable_roots: bool,
    pub(super) watch_failed: bool,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub(super) struct RootWatchStatus {
    pub(super) changed_roots: Vec<PathBuf>,
    pub(super) uncertain_roots: Vec<PathBuf>,
    pub(super) has_unavailable_roots: bool,
}

#[derive(Debug)]
struct RootIdentityRetry {
    next_reconcile: Instant,
    delay: Duration,
}

#[derive(Debug, Default)]
pub(super) struct RootIdentityRecovery {
    pending: HashMap<PathBuf, RootIdentityRetry>,
}

impl RootIdentityRecovery {
    pub(super) fn due_roots(&mut self, uncertain_roots: &[PathBuf], now: Instant) -> Vec<PathBuf> {
        let uncertain = uncertain_roots.iter().cloned().collect::<HashSet<_>>();
        self.pending.retain(|root, _| uncertain.contains(root));

        let mut due = Vec::new();
        for root in uncertain_roots {
            let retry = self
                .pending
                .entry(root.clone())
                .or_insert(RootIdentityRetry {
                    next_reconcile: now,
                    delay: super::ROOT_IDENTITY_RETRY_MIN,
                });
            if now >= retry.next_reconcile {
                due.push(root.clone());
                retry.next_reconcile = now + retry.delay;
                retry.delay = super::doubled_duration(retry.delay, super::ROOT_IDENTITY_RETRY_MAX);
            }
        }
        due
    }
}

pub(super) fn update_watched_roots(
    watcher: &mut dyn Watcher,
    watched_roots: &mut WatchedRootIdentities,
    sources: &[SampleSource],
) -> RootWatchUpdate {
    let (desired, has_unavailable_roots) = observed_available_roots(sources);

    let mut changed_roots = Vec::new();
    let mut watch_failed = false;
    for root in watched_roots
        .keys()
        .filter(|root| !desired.contains_key(*root))
        .cloned()
        .collect::<Vec<_>>()
    {
        if let Err(error) = watcher.unwatch(&root) {
            tracing::warn!(
                "Failed to unwatch GUI source root {}: {error}",
                root.display()
            );
        }
        watched_roots.remove(&root);
        changed_roots.push(root);
    }

    let roots_to_watch = desired
        .iter()
        .filter(|(root, _)| !watched_roots.contains_key(*root))
        .map(|(root, identity)| (root.clone(), identity.clone()))
        .collect::<Vec<_>>();
    for (root, identity_before_watch) in roots_to_watch {
        if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
            tracing::warn!(
                "Failed to watch GUI source root {}: {error}",
                root.display()
            );
            changed_roots.push(root.clone());
            watch_failed = true;
            continue;
        }
        let identity_after_watch = observed_available_root_identity(&root);
        let registered_identity = match (&identity_before_watch, identity_after_watch) {
            (Some(before), RootObservation::Available(Some(after))) if before == &after => {
                Some(after)
            }
            (Some(before), RootObservation::Available(Some(after))) => {
                tracing::warn!(
                    root = %root.display(),
                    identity_before = before,
                    identity_after = after,
                    "Source root changed identity while its watcher was being registered"
                );
                let _ = watcher.unwatch(&root);
                changed_roots.push(root.clone());
                watch_failed = true;
                continue;
            }
            (_, RootObservation::Unavailable) => {
                tracing::warn!(
                    root = %root.display(),
                    "Source root became unavailable while its watcher was being registered"
                );
                let _ = watcher.unwatch(&root);
                changed_roots.push(root.clone());
                watch_failed = true;
                continue;
            }
            _ => None,
        };
        watched_roots.insert(root.clone(), registered_identity);
        changed_roots.push(root);
    }
    RootWatchUpdate {
        changed_roots,
        has_unavailable_roots,
        watch_failed,
    }
}

pub(super) fn root_watch_status(
    watched_roots: &WatchedRootIdentities,
    sources: &[SampleSource],
) -> RootWatchStatus {
    let (desired, has_unavailable_roots) = observed_available_roots(sources);
    let configured = sources
        .iter()
        .map(|source| source.root.clone())
        .collect::<HashSet<_>>();
    let mut status = RootWatchStatus {
        has_unavailable_roots,
        ..Default::default()
    };

    for root in watched_roots
        .keys()
        .filter(|root| !configured.contains(*root))
    {
        status.changed_roots.push(root.clone());
    }
    for root in configured {
        let Some(observed_identity) = desired.get(&root) else {
            if watched_roots.contains_key(&root) {
                status.changed_roots.push(root);
            }
            continue;
        };
        match (watched_roots.get(&root), observed_identity) {
            (None, _) => status.changed_roots.push(root),
            (Some(Some(watched)), Some(observed)) if watched == observed => {}
            (Some(Some(_)), Some(_)) => status.changed_roots.push(root),
            (Some(None), Some(_)) => status.changed_roots.push(root),
            (Some(_), None) => status.uncertain_roots.push(root),
        }
    }

    status.changed_roots.sort();
    status.changed_roots.dedup();
    status.uncertain_roots.sort();
    status.uncertain_roots.dedup();
    status
}

pub(super) fn root_identity_is_current(
    watched_roots: &WatchedRootIdentities,
    root: &Path,
) -> Option<bool> {
    let watched = registered_root_identity(watched_roots, root)?;
    match observed_available_root_identity(root) {
        RootObservation::Available(Some(observed)) => {
            Some(watched.as_bytes() == observed.as_bytes())
        }
        RootObservation::Available(None) => None,
        RootObservation::Unavailable => Some(false),
    }
}

pub(super) fn registered_root_identity(
    watched_roots: &WatchedRootIdentities,
    root: &Path,
) -> Option<wavecrate_library::sample_sources::reconciliation::RootIdentity> {
    watched_roots.get(root)?.as_ref().map(|identity| {
        wavecrate_library::sample_sources::reconciliation::RootIdentity::from_bytes(
            identity.as_bytes().to_vec(),
        )
    })
}

fn observed_available_roots(sources: &[SampleSource]) -> (WatchedRootIdentities, bool) {
    let mut roots = HashMap::new();
    let mut has_unavailable_roots = false;
    for source in sources {
        match observed_available_root_identity(&source.root) {
            RootObservation::Available(identity) => {
                roots.insert(source.root.clone(), identity);
            }
            RootObservation::Unavailable => has_unavailable_roots = true,
        }
    }
    (roots, has_unavailable_roots)
}

enum RootObservation {
    Available(Option<String>),
    Unavailable,
}

fn observed_available_root_identity(root: &Path) -> RootObservation {
    let Ok(metadata) = fs::metadata(root) else {
        return RootObservation::Unavailable;
    };
    if !metadata.is_dir() {
        return RootObservation::Unavailable;
    }
    RootObservation::Available(stable_filesystem_identity(root, &metadata))
}
