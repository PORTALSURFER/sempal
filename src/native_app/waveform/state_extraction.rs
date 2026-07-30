use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use wavecrate::sample_sources::{SourceFileEvidence, capture_source_file_evidence};
use wavecrate::selection::SelectionRange;

use super::{
    WaveformState,
    audio_file::{
        InterleavedF32FileExtractionSource, PersistedPlaybackCacheFile,
        extract_interleaved_f32_file_range_to_folder, extract_interleaved_f32_range_to_folder,
        extract_wav_file_range_to_folder, extract_wav_range_to_folder, is_wav_path,
    },
};

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct WaveformExtractionCompletion {
    pub(in crate::native_app) source_path: PathBuf,
    pub(in crate::native_app) selection: SelectionRange,
    pub(in crate::native_app) result: Result<PathBuf, String>,
    pub(in crate::native_app) evidence: Option<SourceFileEvidence>,
}

#[derive(Clone, Debug)]
pub(in crate::native_app) struct WaveformExtractionRequest {
    source_path: PathBuf,
    target_folder: Option<PathBuf>,
    source: WaveformExtractionSource,
    sample_rate: u32,
    channels: usize,
    loaded_frames: usize,
    selection: SelectionRange,
    gain: f32,
}

#[derive(Clone, Debug)]
enum WaveformExtractionSource {
    WavBytes(Arc<[u8]>),
    WavFile {
        fallback: Option<LoadedPlaybackExtractionSource>,
    },
    LoadedPlayback(LoadedPlaybackExtractionSource),
}

#[derive(Clone, Debug)]
enum LoadedPlaybackExtractionSource {
    InterleavedF32Samples(Arc<[f32]>),
    InterleavedF32File(PersistedPlaybackCacheFile),
}

impl WaveformExtractionRequest {
    pub(in crate::native_app) fn source_path(&self) -> &Path {
        &self.source_path
    }

    pub(in crate::native_app) fn selection(&self) -> SelectionRange {
        self.selection
    }

    pub(in crate::native_app) fn with_target_folder(mut self, target_folder: PathBuf) -> Self {
        self.target_folder = Some(target_folder);
        self
    }

    pub(in crate::native_app) fn with_gain(mut self, gain: f32) -> Self {
        self.gain = normalize_extraction_gain(gain);
        self
    }

    pub(in crate::native_app) fn target_folder(&self) -> Result<&Path, String> {
        self.target_folder
            .as_deref()
            .or_else(|| self.source_path.parent())
            .ok_or_else(|| String::from("Source sample has no parent folder"))
    }

    pub(in crate::native_app) fn has_explicit_target_folder(&self) -> bool {
        self.target_folder.is_some()
    }

    fn execute(&self) -> Result<PathBuf, String> {
        self.source.extract_to_folder(
            &self.source_path,
            self.target_folder()?,
            self.sample_rate,
            self.channels,
            self.loaded_frames,
            self.selection,
            self.gain,
        )
    }
}

impl WaveformExtractionSource {
    fn extract_to_folder(
        &self,
        source_path: &Path,
        target_folder: &Path,
        sample_rate: u32,
        channels: usize,
        loaded_frames: usize,
        selection: SelectionRange,
        gain: f32,
    ) -> Result<PathBuf, String> {
        match self {
            Self::WavBytes(audio_bytes) => extract_wav_range_to_folder(
                source_path,
                target_folder,
                audio_bytes,
                loaded_frames,
                selection,
                gain,
            ),
            Self::WavFile { fallback } => match extract_wav_file_range_to_folder(
                source_path,
                target_folder,
                loaded_frames,
                selection,
                gain,
            ) {
                Ok(path) => Ok(path),
                Err(error) => match fallback {
                    Some(fallback) => fallback
                        .extract_to_folder(
                            source_path,
                            target_folder,
                            sample_rate,
                            channels,
                            loaded_frames,
                            selection,
                            gain,
                        )
                        .map_err(|fallback_error| {
                            format!("{error}; loaded playback fallback failed: {fallback_error}")
                        }),
                    None => Err(error),
                },
            },
            Self::LoadedPlayback(source) => source.extract_to_folder(
                source_path,
                target_folder,
                sample_rate,
                channels,
                loaded_frames,
                selection,
                gain,
            ),
        }
    }
}

impl LoadedPlaybackExtractionSource {
    fn extract_to_folder(
        &self,
        source_path: &Path,
        target_folder: &Path,
        sample_rate: u32,
        channels: usize,
        loaded_frames: usize,
        selection: SelectionRange,
        gain: f32,
    ) -> Result<PathBuf, String> {
        match self {
            Self::InterleavedF32Samples(samples) => extract_interleaved_f32_range_to_folder(
                source_path,
                target_folder,
                samples,
                sample_rate,
                channels,
                loaded_frames,
                selection,
                gain,
            ),
            Self::InterleavedF32File(cache_file) => extract_interleaved_f32_file_range_to_folder(
                source_path,
                target_folder,
                InterleavedF32FileExtractionSource {
                    cache_path: &cache_file.path,
                    sample_count: cache_file.sample_count,
                    sample_rate,
                    channels,
                    loaded_frames,
                },
                selection,
                gain,
            ),
        }
    }
}

impl WaveformState {
    #[cfg(test)]
    pub(in crate::native_app) fn extract_play_selection_to_sibling(
        &mut self,
    ) -> Result<PathBuf, String> {
        let request = self.play_selection_extraction_request(None)?;
        let selection = request.selection();
        let completion = execute_waveform_extraction(request);
        let path = completion.result?;
        self.mark_extracted_range(selection);
        Ok(path)
    }

    #[cfg(test)]
    pub(in crate::native_app) fn extract_play_selection_to_folder(
        &mut self,
        target_folder: &Path,
    ) -> Result<PathBuf, String> {
        let request = self.play_selection_extraction_request(Some(target_folder.to_path_buf()))?;
        let selection = request.selection();
        let completion = execute_waveform_extraction(request);
        let path = completion.result?;
        self.mark_extracted_range(selection);
        Ok(path)
    }

    pub(in crate::native_app) fn play_selection_extraction_request(
        &self,
        target_folder: Option<PathBuf>,
    ) -> Result<WaveformExtractionRequest, String> {
        let selection = self.extractable_play_selection()?;
        self.selection_extraction_request(target_folder, selection)
    }

    pub(in crate::native_app) fn selection_extraction_request(
        &self,
        target_folder: Option<PathBuf>,
        selection: SelectionRange,
    ) -> Result<WaveformExtractionRequest, String> {
        let selection = self.extractable_selection(selection)?;
        let source = self.extraction_source()?;
        Ok(WaveformExtractionRequest {
            source_path: self.file.path.clone(),
            target_folder,
            source,
            sample_rate: self.file.sample_rate,
            channels: self.file.channels,
            loaded_frames: self.file.frames,
            selection,
            gain: 1.0,
        })
    }

    pub(in crate::native_app) fn mark_extracted_play_selection(
        &mut self,
        source_path: &Path,
        selection: SelectionRange,
    ) {
        if self.file.path == source_path {
            self.mark_extracted_range(selection);
        }
    }

    fn mark_extracted_range(&mut self, selection: SelectionRange) {
        super::state_range_history::insert_merged_range(&mut self.extracted_ranges, selection);
    }

    fn extractable_play_selection(&self) -> Result<SelectionRange, String> {
        let selection = self
            .play_selection
            .filter(|selection| selection.width() > 0.0)
            .ok_or_else(|| String::from("Mark a play range before extracting"))?;
        self.extractable_selection(selection)
    }

    fn extractable_selection(&self, selection: SelectionRange) -> Result<SelectionRange, String> {
        if selection.width() <= 0.0 {
            return Err(String::from("Mark a range before extracting"));
        }
        if !self.has_loaded_sample() {
            return Err(String::from("Load a sample before extracting"));
        }
        Ok(selection)
    }

    fn extraction_source(&self) -> Result<WaveformExtractionSource, String> {
        if !self.file.audio_bytes.is_empty() && is_wav_path(&self.file.path) {
            return Ok(WaveformExtractionSource::WavBytes(Arc::clone(
                &self.file.audio_bytes,
            )));
        }
        let loaded_playback_source = self.loaded_playback_extraction_source();
        if is_wav_path(&self.file.path) {
            return Ok(WaveformExtractionSource::WavFile {
                fallback: loaded_playback_source,
            });
        }
        if let Some(source) = loaded_playback_source {
            return Ok(WaveformExtractionSource::LoadedPlayback(source));
        }
        if !is_wav_path(&self.file.path) {
            return Err(String::from("Extraction currently supports WAV files"));
        }
        Err(String::from("Reload the sample before extracting"))
    }

    fn loaded_playback_extraction_source(&self) -> Option<LoadedPlaybackExtractionSource> {
        if let Some(samples) = self.file.playback_samples.as_ref() {
            return Some(LoadedPlaybackExtractionSource::InterleavedF32Samples(
                Arc::clone(samples),
            ));
        }
        self.file.playback_cache_file.as_ref().map(|cache_file| {
            LoadedPlaybackExtractionSource::InterleavedF32File(cache_file.clone())
        })
    }

    pub(in crate::native_app) fn play_selection(&self) -> Option<SelectionRange> {
        self.play_selection
    }

    pub(in crate::native_app) fn edit_selection(&self) -> Option<SelectionRange> {
        self.edit_selection
    }

    pub(in crate::native_app) fn extracted_ranges(&self) -> &[SelectionRange] {
        &self.extracted_ranges
    }
}

pub(in crate::native_app) fn execute_waveform_extraction(
    request: WaveformExtractionRequest,
) -> WaveformExtractionCompletion {
    tracing::info!(
        target: "wavecrate::debug::sample_identity",
        event = "browser.extraction.execute_before",
        trigger = "execute_waveform_extraction",
        source_path = %request.source_path.display(),
        target_folder = %request.target_folder().map(|path| path.display().to_string()).unwrap_or_else(|error| error),
        selection_start = request.selection.start_f64(),
        selection_end = request.selection.end_f64(),
        sample_rate = request.sample_rate,
        channels = request.channels,
        loaded_frames = request.loaded_frames,
        gain = request.gain,
        "Sample identity extraction before execute"
    );
    let result = request.execute();
    match &result {
        Ok(path) => {
            super::audio_file::invalidate_persisted_waveform_cache_path(path);
            crate::native_app::sample_identity_diagnostics::log_sample_identity_path_event(
                "browser.extraction.result_path",
                "execute_waveform_extraction",
                path,
                Some("extracted"),
            );
            tracing::info!(
            target: "wavecrate::debug::sample_identity",
                    event = "browser.extraction.execute_after",
                    trigger = "execute_waveform_extraction",
                    source_path = %request.source_path.display(),
                    result_path = %path.display(),
                    selection_start = request.selection.start_f64(),
                    selection_end = request.selection.end_f64(),
                    "Sample identity extraction after execute"
                );
        }
        Err(error) => {
            tracing::info!(
            target: "wavecrate::debug::sample_identity",
                    event = "browser.extraction.execute_after",
                    trigger = "execute_waveform_extraction",
                    source_path = %request.source_path.display(),
                    error,
                    selection_start = request.selection.start_f64(),
                    selection_end = request.selection.end_f64(),
                    "Sample identity extraction after execute"
                );
        }
    }
    WaveformExtractionCompletion {
        source_path: request.source_path,
        selection: request.selection,
        evidence: result
            .as_ref()
            .ok()
            .map(|path| capture_source_file_evidence(path)),
        result,
    }
}

fn normalize_extraction_gain(gain: f32) -> f32 {
    if gain.is_finite() && gain > 0.0 {
        gain
    } else {
        1.0
    }
}
