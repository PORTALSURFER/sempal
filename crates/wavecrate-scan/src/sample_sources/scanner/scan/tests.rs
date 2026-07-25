use super::*;
use crate::sample_sources::{Rating, SourceDatabase};
use std::path::Path;
use std::time::Duration;
use tempfile::tempdir;

mod basics;
#[cfg(unix)]
mod filesystem_edges;
mod hard_rescan;
mod index_entries;
mod rename_reconciliation;
mod rename_retention;
mod targeted_sync;
mod timestamps;

#[cfg(unix)]
fn set_file_times(path: &Path, seconds: i64, nanos: i64) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).unwrap();
    let times = [
        libc::timespec {
            tv_sec: seconds,
            tv_nsec: nanos,
        },
        libc::timespec {
            tv_sec: seconds,
            tv_nsec: nanos,
        },
    ];
    let result = unsafe { libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), times.as_ptr(), 0) };
    assert_eq!(result, 0);
}
