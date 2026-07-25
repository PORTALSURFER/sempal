use std::time::Duration;

mod classification;
mod debounce;
mod handle;
mod journal;
mod path_mapping;
mod roots;
mod state;

const WATCHER_POLL_INTERVAL: Duration = Duration::from_millis(200);
const SOURCE_CHANGE_DEBOUNCE: Duration = Duration::from_millis(400);
const MAX_PENDING_PATHS_PER_SOURCE: usize = 512;
const WATCHER_EVENT_QUEUE_CAPACITY: usize = 256;
const WATCHER_RESTART_MIN: Duration = Duration::from_secs(1);
const WATCHER_RESTART_MAX: Duration = Duration::from_secs(60);
const WATCHER_START_TIMEOUT: Duration = Duration::from_secs(5);
const ROOT_REFRESH_AVAILABLE: Duration = Duration::from_secs(10);
const ROOT_REFRESH_UNAVAILABLE: Duration = Duration::from_secs(30);
const ROOT_IDENTITY_RETRY_MIN: Duration = Duration::from_secs(30);
const ROOT_IDENTITY_RETRY_MAX: Duration = Duration::from_secs(60 * 60);

fn doubled_duration(current: Duration, maximum: Duration) -> Duration {
    current.saturating_mul(2).min(maximum)
}

pub(in crate::native_app) use handle::GuiSourceWatcherHandle;

#[cfg(test)]
#[path = "source_watcher/tests.rs"]
mod tests;
