mod context;
mod errors;
mod runner;
mod stats;

pub use errors::ScanError;
pub use runner::{hard_rescan, scan_in_background, scan_once, scan_with_progress, ScanMode};
pub use stats::{ChangedSample, ScanStats};
pub(crate) use context::ScanContext;

#[cfg(test)]
mod tests;
