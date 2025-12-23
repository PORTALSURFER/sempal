mod metadata;
mod scan;

pub use scan::{
    scan_in_background, scan_once, scan_with_progress, hard_rescan, ChangedSample, ScanError,
    ScanMode, ScanStats,
};
