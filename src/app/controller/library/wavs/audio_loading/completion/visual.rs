use super::super::super::*;
use crate::app::controller::playback::audio_loader::AudioVisualResult;
use crate::app::controller::playback::telemetry::{log_audio_start_stage, stage_timer};
use std::{path::PathBuf, sync::Arc};

struct ValidatedVisualHandoff {
    decoded: Arc<DecodedWaveform>,
    loaded_bytes: Arc<[u8]>,
    audio_path: Option<PathBuf>,
    load_kind: &'static str,
    byte_count: usize,
    sample_count: usize,
}

impl AppController {
    pub(crate) fn handle_audio_visual_loaded(&mut self, result: AudioVisualResult) {
        let started_at = stage_timer();
        let Some(handoff) = self.validated_visual_handoff(&result) else {
            return;
        };
        self.publish_audio_visuals(&result, &handoff.decoded);
        self.cache_loaded_waveform_transients(
            &result.source_id,
            &result.relative_path,
            result.metadata,
            &handoff.decoded,
            handoff.loaded_bytes,
            handoff.audio_path,
            result.transients,
            result.stretched,
        );
        self.finalize_staged_audio_handoff(result.cache_token);
        log_audio_start_stage(
            "handle_audio_visual_loaded",
            Some(&result.source_id),
            Some(&result.relative_path),
            started_at,
            Some(handoff.load_kind),
            None,
            Some(handoff.byte_count),
            Some(handoff.sample_count),
        );
    }

    fn validated_visual_handoff(
        &self,
        result: &AudioVisualResult,
    ) -> Option<ValidatedVisualHandoff> {
        if !self.audio_target_matches_current(&result.source_id, &result.relative_path) {
            return None;
        }
        let staged = self.runtime.jobs.staged_audio_handoff()?;
        if staged.request_id != result.request_id
            || staged.source_id != result.source_id
            || staged.relative_path != result.relative_path
        {
            return None;
        }
        if staged.decoded.cache_token != result.cache_token {
            return None;
        }
        Some(ValidatedVisualHandoff {
            decoded: Arc::clone(&staged.decoded),
            loaded_bytes: Arc::clone(&staged.bytes),
            audio_path: staged.audio_path.clone(),
            load_kind: if staged.bytes.is_empty() {
                "file"
            } else {
                "bytes"
            },
            byte_count: staged.bytes.len(),
            sample_count: staged.decoded.samples.len(),
        })
    }

    fn publish_audio_visuals(
        &mut self,
        result: &AudioVisualResult,
        decoded: &Arc<DecodedWaveform>,
    ) {
        self.publish_loaded_transients(result.cache_token, result.transients.clone());
        if self.visual_render_matches_current_state(result) {
            self.store_prepared_waveform_image(
                result.image.clone(),
                result.projected_image.clone(),
                result.render_meta,
            );
        } else {
            self.sample_view.waveform.render_meta = None;
            self.sample_view.waveform.decoded = Some(Arc::clone(decoded));
            self.refresh_waveform_image();
        }
    }

    fn visual_render_matches_current_state(&self, result: &AudioVisualResult) -> bool {
        let expected_transient_visual_token = self
            .ui
            .waveform
            .transient_cache_token
            .filter(|_| self.ui.waveform.transient_markers_enabled);
        result.render_meta.as_ref().is_some_and(|meta| {
            meta.transient_visual_token == expected_transient_visual_token
                && meta.matches_view_identity(self.ui.waveform.view)
        })
    }
}
