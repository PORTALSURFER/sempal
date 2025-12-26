use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::sample_sources::db::SourceWriteBatch;

use super::scan::{ScanContext, ScanError};
use super::scan_diff_phase::diff_phase;
use super::scan_fs::visit_dir;

pub(super) fn walk_phase(
    root: &Path,
    cancel: Option<&AtomicBool>,
    mut on_progress: Option<&mut dyn FnMut(usize, &Path)>,
    context: &mut ScanContext,
    batch: &mut SourceWriteBatch<'_>,
) -> Result<(), ScanError> {
    visit_dir(root, cancel, &mut |path| {
        if let Some(cancel) = cancel
            && cancel.load(Ordering::Relaxed)
        {
            return Err(ScanError::Canceled);
        }
        diff_phase(batch, root, path, context)?;
        if let Some(on_progress) = on_progress.as_mut() {
            on_progress(context.stats.total_files, path);
        }
        Ok(())
    })
}
