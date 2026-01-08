use super::*;

impl DragDropController<'_> {
    pub(crate) fn selection_drag_label(
        &self,
        audio: &LoadedAudio,
        bounds: SelectionRange,
    ) -> String {
        let name = audio
            .relative_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Selection");
        let seconds = (audio.duration_seconds * bounds.width()).max(0.0);
        format!("{name} ({seconds:.2}s)")
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn export_selection_for_drag(
        &mut self,
        bounds: SelectionRange,
    ) -> Result<(PathBuf, String), String> {
        let audio = self
            .sample_view
            .wav
            .loaded_audio
            .as_ref()
            .ok_or_else(|| "Load a sample before dragging a selection".to_string())?;
        let clip = self.selection_audio(&audio.source_id, &audio.relative_path)?;
        let entry = self.export_selection_clip(
            &clip.source_id,
            &clip.relative_path,
            bounds,
            None,
            true,
            true,
        )?;
        let source = self
            .library
            .sources
            .iter()
            .find(|s| s.id == clip.source_id)
            .cloned()
            .ok_or_else(|| "Source not available for selection export".to_string())?;
        let absolute = source.root.join(&entry.relative_path);
        let label = format!(
            "Drag {} to an external target",
            entry.relative_path.display()
        );
        Ok((absolute, label))
    }
}
