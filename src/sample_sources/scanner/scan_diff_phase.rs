use std::path::Path;
use std::sync::atomic::AtomicBool;

use crate::sample_sources::db::SourceWriteBatch;

use super::scan::{ScanContext, ScanError};
use super::scan_diff::apply_diff;
use super::scan_fs::read_facts;

pub(super) fn diff_phase(
    batch: &mut SourceWriteBatch<'_>,
    root: &Path,
    path: &Path,
    context: &mut ScanContext,
    cancel: Option<&AtomicBool>,
) -> Result<(), ScanError> {
    let facts = read_facts(root, path)?;
    apply_diff(
        batch,
        facts,
        &mut context.existing,
        &mut context.existing_by_hash,
        &mut context.existing_by_facts,
        &mut context.stats,
        root,
        context.mode,
        cancel,
    )?;
    context.stats.total_files += 1;
    Ok(())
}
