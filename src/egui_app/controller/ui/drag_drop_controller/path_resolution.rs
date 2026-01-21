use super::*;

impl DragDropController<'_> {
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
