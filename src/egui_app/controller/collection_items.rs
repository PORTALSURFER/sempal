use super::*;

impl EguiController {
    /// Delete a collection sample by row index.
    pub fn delete_collection_sample(&mut self, row: usize) -> Result<(), String> {
        let result: Result<(), String> = (|| {
            let ctx = self.resolve_collection_sample(row)?;
            if !self.drop_collection_member(&ctx) {
                return Err("Sample not found in collection".into());
            }
            self.persist_config("Failed to save collection after delete")?;
            self.refresh_collections_ui();
            self.set_status(
                format!(
                    "Removed {} from collection",
                    ctx.member.relative_path.display()
                ),
                StatusTone::Info,
            );
            Ok(())
        })();
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Rename a collection sample file and update references.
    pub fn rename_collection_sample(&mut self, row: usize, new_name: &str) -> Result<(), String> {
        let result: Result<(), String> = (|| {
            let ctx = self.resolve_collection_sample(row)?;
            let tag = self.sample_tag_for(&ctx.source, &ctx.member.relative_path)?;
            let new_relative = self.validate_new_sample_name(&ctx, new_name)?;
            let (file_size, modified_ns) = self.apply_rename(&ctx, &new_relative, tag)?;
            self.update_collection_member_path(&ctx, &new_relative)?;
            self.update_cached_entry(
                &ctx.source,
                &ctx.member.relative_path,
                WavEntry {
                    relative_path: new_relative.clone(),
                    file_size,
                    modified_ns,
                    tag,
                },
            );
            self.refresh_waveform_after_change(&ctx, &new_relative);
            self.update_export_after_change(&ctx, &new_relative);
            self.persist_config("Failed to save collection after rename")?;
            self.refresh_collections_ui();
            self.set_status(
                format!(
                    "Renamed {} to {}",
                    ctx.member.relative_path.display(),
                    new_relative.display()
                ),
                StatusTone::Info,
            );
            Ok(())
        })();
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Apply a keep/trash tag to a collection sample.
    pub fn tag_collection_sample(&mut self, row: usize, tag: SampleTag) -> Result<(), String> {
        let result: Result<(), String> = (|| {
            let ctx = self.resolve_collection_sample(row)?;
            self.set_sample_tag_for_source(&ctx.source, &ctx.member.relative_path, tag, false)?;
            if self.selected_source.as_ref() == Some(&ctx.source.id) {
                self.rebuild_triage_lists();
            }
            self.set_status(
                format!("Tagged {} as {:?}", ctx.member.relative_path.display(), tag),
                StatusTone::Info,
            );
            Ok(())
        })();
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Normalize a wav in-place to full scale and refresh metadata.
    pub fn normalize_collection_sample(&mut self, row: usize) -> Result<(), String> {
        let result: Result<(), String> = (|| {
            let ctx = self.resolve_collection_sample(row)?;
            let (file_size, modified_ns, tag) = self.normalize_and_save(&ctx)?;
            self.upsert_metadata(&ctx, file_size, modified_ns)?;
            self.update_cached_entry(
                &ctx.source,
                &ctx.member.relative_path,
                WavEntry {
                    relative_path: ctx.member.relative_path.clone(),
                    file_size,
                    modified_ns,
                    tag,
                },
            );
            if self.selected_source.as_ref() == Some(&ctx.source.id) {
                self.rebuild_triage_lists();
            }
            self.refresh_waveform_after_change(&ctx, &ctx.member.relative_path);
            self.update_export_after_change(&ctx, &ctx.member.relative_path);
            self.set_status(
                format!("Normalized {}", ctx.member.relative_path.display()),
                StatusTone::Info,
            );
            Ok(())
        })();
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }
}
