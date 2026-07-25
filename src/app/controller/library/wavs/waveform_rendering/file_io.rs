use super::*;
use crate::app::controller::playback::audio_cache::FileMetadata;
use std::fs;
use std::path::Path;
use wavecrate_library::timestamps::system_time_to_unix_nanos;

impl AppController {
    pub(crate) fn read_waveform_bytes(
        &self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<Vec<u8>, String> {
        let full_path = source.root.join(relative_path);
        let bytes = fs::read(&full_path)
            .map_err(|err| format!("Failed to read {}: {err}", full_path.display()))?;
        Ok(crate::wav_sanitize::sanitize_wav_bytes(bytes))
    }

    pub(crate) fn current_file_metadata(
        &self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<FileMetadata, String> {
        let full_path = source.root.join(relative_path);
        let metadata = fs::metadata(&full_path)
            .map_err(|err| format!("Failed to read {}: {err}", full_path.display()))?;
        let modified_ns =
            system_time_to_unix_nanos(metadata.modified().map_err(|err| {
                format!("Missing modified time for {}: {err}", full_path.display())
            })?);
        Ok(FileMetadata {
            file_size: metadata.len(),
            modified_ns,
        })
    }
}
