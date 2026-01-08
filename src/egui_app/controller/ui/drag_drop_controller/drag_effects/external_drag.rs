use super::super::DragDropController;
use std::path::PathBuf;

impl DragDropController<'_> {
    #[cfg(target_os = "windows")]
    pub(crate) fn start_external_drag(
        &self,
        paths: &[PathBuf],
    ) -> Result<(), String> {
        let hwnd = self
            .drag_hwnd
            .ok_or_else(|| "Window handle unavailable for external drag".to_string())?;
        crate::external_drag::start_file_drag(hwnd, paths)
    }
}
