use super::super::*;
use super::CollectionSampleContext;
use std::path::{Path, PathBuf};

impl EguiController {
    pub(in crate::egui_app::controller) fn validate_new_sample_name(
        &self,
        ctx: &CollectionSampleContext,
        new_name: &str,
    ) -> Result<PathBuf, String> {
        self.validate_new_sample_name_in_parent(
            &ctx.member.relative_path,
            &ctx.source.root,
            new_name,
        )
    }

    pub(in crate::egui_app::controller) fn validate_new_sample_name_in_parent(
        &self,
        relative_path: &Path,
        root: &Path,
        new_name: &str,
    ) -> Result<PathBuf, String> {
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err("Name cannot be empty".into());
        }
        if trimmed.contains(['/', '\\']) {
            return Err("Name cannot contain path separators".into());
        }
        let parent = relative_path.parent().unwrap_or(Path::new(""));
        let new_relative = parent.join(trimmed);
        let new_absolute = root.join(&new_relative);
        if new_absolute.exists() {
            return Err(format!(
                "A file named {} already exists",
                new_relative.display()
            ));
        }
        Ok(new_relative)
    }

    /// Build a sanitized sample name that keeps the existing file extension.
    pub(in crate::egui_app::controller) fn name_with_preserved_extension(
        &self,
        current_relative: &Path,
        new_name: &str,
    ) -> Result<String, String> {
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err("Name cannot be empty".into());
        }
        let Some(ext) = current_relative.extension().and_then(|ext| ext.to_str()) else {
            return Ok(trimmed.to_string());
        };
        let ext_lower = ext.to_ascii_lowercase();
        let should_strip_suffix = |suffix: &str| -> bool {
            let suffix_lower = suffix.to_ascii_lowercase();
            suffix_lower == ext_lower
                || matches!(
                    suffix_lower.as_str(),
                    "wav" | "wave" | "flac" | "aif" | "aiff" | "mp3" | "ogg" | "opus"
                )
        };
        let stem = if let Some((stem, suffix)) = trimmed.rsplit_once('.') {
            if !stem.is_empty() && should_strip_suffix(suffix) {
                stem
            } else {
                trimmed
            }
        } else {
            trimmed
        };
        let stem = stem.trim_end_matches('.');
        if stem.trim().is_empty() {
            return Err("Name cannot be empty".into());
        }
        Ok(format!("{stem}.{ext}"))
    }
}
