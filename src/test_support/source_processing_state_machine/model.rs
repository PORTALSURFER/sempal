use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

const GENERATED_SLOTS: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ScanCause {
    Watcher,
    WatcherOverflow,
    Focus,
    Foreground,
    Restart,
    Lifecycle,
    Retry,
}

impl ScanCause {
    pub(super) const fn publication_reason(self) -> &'static str {
        match self {
            Self::Watcher => "state_machine_watcher",
            Self::WatcherOverflow => "state_machine_watcher_overflow",
            Self::Focus => "state_machine_focus",
            Self::Foreground => "state_machine_foreground",
            Self::Restart => "state_machine_restart",
            Self::Lifecycle => "state_machine_lifecycle",
            Self::Retry => "state_machine_retry",
        }
    }

    pub(super) fn from_publication_reason(reason: &str) -> Option<Self> {
        [
            Self::Watcher,
            Self::WatcherOverflow,
            Self::Focus,
            Self::Foreground,
            Self::Restart,
            Self::Lifecycle,
            Self::Retry,
        ]
        .into_iter()
        .find(|cause| cause.publication_reason() == reason)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FailureBoundary {
    Transaction,
    Publication,
    WatcherDelivery,
    Hashing,
    Lifecycle,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub(super) enum Event {
    Create { slot: u8 },
    SameSizeModify { slot: u8 },
    Move { slot: u8, nested: bool },
    Delete { slot: u8 },
    NestedDirectoryChange { slot: u8 },
    WatcherBatch,
    WatcherOverflow,
    FocusChanged { active: bool },
    ExplicitRefresh,
    Cancel,
    ShutdownRestart,
    SourceRemoveReadd,
    RootOfflineOnline,
    RootReplacement,
    PartialEnumeration,
    SymlinkEscape,
    DatabaseBusy,
    InjectFailure { boundary: FailureBoundary },
    Quiesce,
}

impl Event {
    pub(super) fn preserves_lifecycle_semantics(&self) -> bool {
        matches!(
            self,
            Self::ShutdownRestart
                | Self::SourceRemoveReadd
                | Self::RootOfflineOnline
                | Self::RootReplacement
        )
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub(super) struct ReferenceModel {
    pub(super) files: BTreeMap<String, String>,
    pub(super) queued_causes: BTreeSet<ScanCause>,
    pub(super) watcher_paths: BTreeSet<String>,
    pub(super) source_configured: bool,
    pub(super) root_online: bool,
    pub(super) focused: bool,
    pub(super) lifecycle_generation: u64,
    pub(super) restart_count: u64,
    pub(super) retry_count: u64,
}

impl ReferenceModel {
    pub(super) fn new(files: BTreeMap<String, String>) -> Self {
        Self {
            files,
            source_configured: true,
            root_online: true,
            focused: true,
            lifecycle_generation: 1,
            ..Self::default()
        }
    }

    pub(super) fn queue_path(&mut self, path: String) {
        self.watcher_paths.insert(path);
        self.queued_causes.insert(ScanCause::Watcher);
    }

    pub(super) fn queue(&mut self, cause: ScanCause) {
        self.queued_causes.insert(cause);
    }
}

pub(super) fn generated_path(slot: u8, nested: bool) -> String {
    let slot = slot % GENERATED_SLOTS;
    if nested {
        format!("generated/nested/sample-{slot}.wav")
    } else {
        format!("generated/sample-{slot}.wav")
    }
}

pub(super) fn generate(seed: u64, length: usize) -> Vec<Event> {
    let mut random = SplitMix64::new(seed);
    let mut events = regression_prefix(seed);
    while events.len() < length {
        let slot = (random.next() % u64::from(GENERATED_SLOTS)) as u8;
        let event = match random.next() % 16 {
            0 => Event::Create { slot },
            1 => Event::SameSizeModify { slot },
            2 => Event::Move {
                slot,
                nested: random.next() & 1 == 0,
            },
            3 => Event::Delete { slot },
            4 => Event::NestedDirectoryChange { slot },
            5 => Event::WatcherBatch,
            6 => Event::WatcherOverflow,
            7 => Event::FocusChanged {
                active: random.next() & 1 == 0,
            },
            8 => Event::ExplicitRefresh,
            9 => Event::Cancel,
            10 => Event::ShutdownRestart,
            11 => Event::SourceRemoveReadd,
            12 => Event::RootOfflineOnline,
            13 => Event::RootReplacement,
            14 => Event::InjectFailure {
                boundary: failure_boundary(random.next()),
            },
            _ => Event::WatcherBatch,
        };
        events.push(event);
    }
    events.push(Event::Quiesce);
    events
}

fn regression_prefix(seed: u64) -> Vec<Event> {
    match seed % 6 {
        0 => vec![
            Event::FocusChanged { active: false },
            Event::FocusChanged { active: true },
            Event::WatcherBatch,
        ],
        1 => vec![Event::Create { slot: 0 }, Event::RootReplacement],
        2 => vec![
            Event::Create { slot: 1 },
            Event::PartialEnumeration,
            Event::WatcherBatch,
        ],
        3 => vec![Event::SymlinkEscape, Event::WatcherOverflow],
        4 => vec![Event::SameSizeModify { slot: 3 }, Event::DatabaseBusy],
        _ => vec![
            Event::Create { slot: 4 },
            Event::Cancel,
            Event::SourceRemoveReadd,
        ],
    }
}

fn failure_boundary(value: u64) -> FailureBoundary {
    match value % 5 {
        0 => FailureBoundary::Transaction,
        1 => FailureBoundary::Publication,
        2 => FailureBoundary::WatcherDelivery,
        3 => FailureBoundary::Hashing,
        _ => FailureBoundary::Lifecycle,
    }
}

struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
}
