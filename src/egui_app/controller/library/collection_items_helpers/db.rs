use super::super::*;
use super::CollectionSampleContext;
use super::io;
use std::path::Path;

impl EguiController {
    pub(crate) fn apply_rename(
        &mut self,
        ctx: &CollectionSampleContext,
        new_relative: &Path,
        tag: crate::sample_sources::Rating,
    ) -> Result<(u64, i64), String> {
        let new_absolute = ctx.source.root.join(new_relative);
        std::fs::rename(&ctx.absolute_path, &new_absolute)
            .map_err(|err| format!("Failed to rename file: {err}"))?;
        let (file_size, modified_ns) = io::file_metadata(&new_absolute)?;
        if let Err(err) = self.rewrite_db_entry(ctx, new_relative, file_size, modified_ns, tag) {
            let _ = std::fs::rename(&new_absolute, &ctx.absolute_path);
            return Err(err);
        }
        Ok((file_size, modified_ns))
    }

    pub(crate) fn rewrite_db_entry(
        &mut self,
        ctx: &CollectionSampleContext,
        new_relative: &Path,
        file_size: u64,
        modified_ns: i64,
        tag: crate::sample_sources::Rating,
    ) -> Result<(), String> {
        self.rewrite_db_entry_for_source(
            &ctx.source,
            &ctx.member.relative_path,
            new_relative,
            file_size,
            modified_ns,
            tag,
        )
    }

    pub(crate) fn rewrite_db_entry_for_source(
        &mut self,
        source: &SampleSource,
        old_relative: &Path,
        new_relative: &Path,
        file_size: u64,
        modified_ns: i64,
        tag: crate::sample_sources::Rating,
    ) -> Result<(), String> {
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        let mut batch = db
            .write_batch()
            .map_err(|err| format!("Failed to start database update: {err}"))?;
        batch
            .remove_file(old_relative)
            .map_err(|err| format!("Failed to drop old entry: {err}"))?;
        batch
            .upsert_file(new_relative, file_size, modified_ns)
            .map_err(|err| format!("Failed to register renamed file: {err}"))?;
        batch
            .set_tag(new_relative, tag)
            .map_err(|err| format!("Failed to copy tag: {err}"))?;
        batch
            .commit()
            .map_err(|err| format!("Failed to save rename: {err}"))
    }

    pub(crate) fn upsert_metadata(
        &mut self,
        ctx: &CollectionSampleContext,
        file_size: u64,
        modified_ns: i64,
    ) -> Result<(), String> {
        self.upsert_metadata_for_source(
            &ctx.source,
            &ctx.member.relative_path,
            file_size,
            modified_ns,
        )
    }

    pub(crate) fn upsert_metadata_for_source(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
    ) -> Result<(), String> {
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.upsert_file(relative_path, file_size, modified_ns)
            .map_err(|err| format!("Failed to refresh metadata: {err}"))
    }
}
