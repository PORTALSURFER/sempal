use super::*;
use crate::egui_app::state::ProgressTaskKind;
use crate::sample_sources::scanner::ScanMode;

const SCAN_PROGRESS_DETAIL: &str = "Scanning audio files…";
const SIMILARITY_PREP_LABEL: &str = "Preparing similarity search";
const SIMILARITY_FINALIZE_LABEL: &str = "Finalizing similarity prep";
const SIMILARITY_SCAN_DETAIL: &str = "Scanning source…";
const SIMILARITY_ANALYSIS_DETAIL: &str = "Analyzing…";
const SIMILARITY_EMBEDDING_DETAIL: &str = "Embedding backfill…";
const SIMILARITY_FINALIZE_DETAIL: &str = "Finalizing…";
const WAV_LOAD_LABEL: &str = "Loading samples";

impl EguiController {
    pub(super) fn scan_status_label(mode: ScanMode) -> &'static str {
        match mode {
            ScanMode::Quick => "Quick sync",
            ScanMode::Hard => "Hard sync",
        }
    }

    pub(super) fn begin_scan_progress(&mut self, mode: ScanMode, source: &SampleSource) {
        let status_label = Self::scan_status_label(mode);
        self.set_status_message(StatusMessage::custom(
            format!("{status_label} on {}", source.root.display()),
            StatusTone::Busy,
        ));
        self.show_status_progress(ProgressTaskKind::Scan, status_label, 0, true);
        self.update_progress_detail(SCAN_PROGRESS_DETAIL);
    }

    pub(super) fn ensure_wav_load_progress(&mut self, source: &SampleSource) {
        if !self.ui.progress.visible || self.ui.progress.task == Some(ProgressTaskKind::WavLoad) {
            self.show_status_progress(ProgressTaskKind::WavLoad, WAV_LOAD_LABEL, 0, false);
            self.update_progress_detail(format!("Loading wavs for {}", source.root.display()));
        }
        self.set_status(
            format!("Loading wavs for {}", source.root.display()),
            StatusTone::Info,
        );
    }

    pub(super) fn show_similarity_prep_progress(&mut self, total: usize, cancelable: bool) {
        self.show_status_progress(
            ProgressTaskKind::Analysis,
            SIMILARITY_PREP_LABEL,
            total,
            cancelable,
        );
    }

    pub(super) fn set_similarity_scan_detail(&mut self) {
        self.update_progress_detail(SIMILARITY_SCAN_DETAIL);
    }

    pub(super) fn set_similarity_embedding_detail(&mut self) {
        self.update_progress_detail(SIMILARITY_EMBEDDING_DETAIL);
    }

    pub(super) fn set_similarity_analysis_detail(&mut self) {
        self.update_progress_detail(SIMILARITY_ANALYSIS_DETAIL);
    }

    pub(super) fn set_similarity_finalize_detail(&mut self) {
        self.update_progress_detail(SIMILARITY_FINALIZE_DETAIL);
    }

    pub(super) fn ensure_similarity_prep_progress(&mut self, total: usize, cancelable: bool) {
        if !self.ui.progress.visible || self.ui.progress.task != Some(ProgressTaskKind::Analysis) {
            self.show_similarity_prep_progress(total, cancelable);
        }
    }

    pub(super) fn show_similarity_finalize_progress(&mut self) {
        self.show_status_progress(
            ProgressTaskKind::Analysis,
            SIMILARITY_FINALIZE_LABEL,
            0,
            true,
        );
    }

    pub(super) fn ensure_similarity_finalize_progress(&mut self) {
        if !self.ui.progress.visible || self.ui.progress.task != Some(ProgressTaskKind::Analysis) {
            self.show_similarity_finalize_progress();
        }
    }
}
