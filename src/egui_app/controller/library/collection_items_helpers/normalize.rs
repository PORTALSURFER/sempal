use super::super::*;
use super::io;
use hound::SampleFormat;
use std::path::Path;

impl EguiController {
    pub(crate) fn normalize_and_save(
        &mut self,
        ctx: &super::CollectionSampleContext,
    ) -> Result<(u64, i64, crate::sample_sources::Rating), String> {
        self.normalize_and_save_for_path(&ctx.source, &ctx.member.relative_path, &ctx.absolute_path)
    }

    pub(crate) fn normalize_and_save_for_path(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        absolute_path: &Path,
    ) -> Result<(u64, i64, crate::sample_sources::Rating), String> {
        let (samples, spec) = io::read_samples_for_normalization(absolute_path)?;
        if samples.is_empty() {
            return Err("No audio data to normalize".into());
        }
        let peak = samples
            .iter()
            .fold(0.0_f32, |acc, sample| acc.max(sample.abs()));
        if peak <= f32::EPSILON {
            return Err("Cannot normalize silent audio".into());
        }
        let scale = 1.0 / peak;
        let normalized: Vec<f32> = samples
            .iter()
            .map(|s| (s * scale).clamp(-1.0, 1.0))
            .collect();
        let target_spec = hound::WavSpec {
            channels: spec.channels.max(1),
            sample_rate: spec.sample_rate.max(1),
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        io::write_normalized_wav(absolute_path, &normalized, target_spec)?;
        let (file_size, modified_ns) = io::file_metadata(absolute_path)?;
        let tag = self.sample_tag_for(source, relative_path)?;
        Ok((file_size, modified_ns, tag))
    }

    pub(crate) fn sample_tag_for(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<crate::sample_sources::Rating, String> {
        if let Some(cache) = self.cache.wav.entries.get(&source.id) {
            if let Some(index) = cache.lookup.get(relative_path).copied()
                && let Some(entry) = cache.entry(index)
            {
                return Ok(entry.tag);
            }
        }
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id)
            && let Some(index) = self.wav_index_for_path(relative_path)
            && let Some(entry) = self.wav_entries.entry(index)
        {
            return Ok(entry.tag);
        }
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.tag_for_path(relative_path)
            .map_err(|err| format!("Failed to read database: {err}"))?
            .ok_or_else(|| "Sample not found in database".to_string())
    }
}
