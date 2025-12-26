use std::time::{SystemTime, UNIX_EPOCH};

use crate::sample_sources::db::META_LAST_SCAN_COMPLETED_AT;
use crate::sample_sources::db::SourceWriteBatch;
use crate::sample_sources::SourceDatabase;

use super::scan::{ScanContext, ScanError};
use super::scan_diff::mark_missing;

pub(super) fn db_sync_phase(
    db: &SourceDatabase,
    batch: &mut SourceWriteBatch<'_>,
    context: &mut ScanContext,
) -> Result<(), ScanError> {
    let existing = std::mem::take(&mut context.existing);
    mark_missing(batch, existing, &mut context.stats, context.mode)?;
    batch.commit()?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    db.set_metadata(META_LAST_SCAN_COMPLETED_AT, &timestamp)?;
    Ok(())
}
