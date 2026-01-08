use super::*;

impl DragDropController<'_> {
    pub(crate) fn selection_clip_root_for_collection(
        &self,
        collection_id: &CollectionId,
    ) -> Result<PathBuf, String> {
        let preferred = self
            .library
            .collections
            .iter()
            .find(|c| &c.id == collection_id)
            .and_then(|collection| {
                crate::egui_app::controller::library::collection_export::resolved_export_dir(
                    collection,
                    self.settings.collection_export_root.as_deref(),
                )
            });
        if let Some(path) = preferred {
            if path.exists() && !path.is_dir() {
                return Err(format!(
                    "Collection export path is not a directory: {}",
                    path.display()
                ));
            }
            std::fs::create_dir_all(&path).map_err(|err| {
                format!(
                    "Failed to create collection export path {}: {err}",
                    path.display()
                )
            })?;
            return Ok(path);
        }
        let fallback = crate::app_dirs::app_root_dir()
            .map_err(|err| err.to_string())?
            .join("collection_clips")
            .join(collection_id.as_str());
        std::fs::create_dir_all(&fallback)
            .map_err(|err| format!("Failed to create collection clip folder: {err}"))?;
        Ok(fallback)
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn sample_absolute_path(
        &self,
        source_id: &SourceId,
        relative_path: &Path,
    ) -> PathBuf {
        self.library
            .sources
            .iter()
            .find(|s| &s.id == source_id)
            .map(|source| source.root.join(relative_path))
            .unwrap_or_else(|| relative_path.to_path_buf())
    }
}
