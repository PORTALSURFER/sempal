use super::*;
use crate::egui_app::controller::controller_state::{
    AnalysisJobStatus, FeatureCache, FeatureStatus,
};
use rusqlite::params;
use std::collections::HashMap;

const ANALYSIS_JOB_TYPE: &str = "wav_metadata_v1";

impl EguiController {
    pub(crate) fn prepare_feature_cache_for_browser(&mut self) {
        let Some(source_id) = self.selection_state.ctx.selected_source.clone() else {
            return;
        };
        if let Err(err) = self.ensure_feature_cache(&source_id) {
            self.ui_cache.browser.features.remove(&source_id);
            self.set_status(
                format!("Failed to load analysis metadata: {err}"),
                crate::egui_app::ui::style::StatusTone::Error,
            );
        }
    }

    pub(crate) fn cached_feature_status_for_entry(
        &self,
        entry_index: usize,
    ) -> Option<&FeatureStatus> {
        let source_id = self.selection_state.ctx.selected_source.as_ref()?;
        self.ui_cache
            .browser
            .features
            .get(source_id)
            .and_then(|cache| cache.rows.get(entry_index))
            .and_then(|row| row.as_ref())
    }

    fn ensure_feature_cache(&mut self, source_id: &SourceId) -> Result<(), String> {
        let needs_len = self.wav_entries_len();
        let existing = self.ui_cache.browser.features.get(source_id);
        if existing.is_some_and(|cache| {
            cache.rows.len() == needs_len && cache.rows.iter().all(|row| row.is_some())
        }) {
            return Ok(());
        }
        let source = self
            .library
            .sources
            .iter()
            .find(|source| &source.id == source_id)
            .ok_or_else(|| "Source not found".to_string())?;
        let conn = super::analysis_jobs::open_source_db(&source.root)?;
        let mut rows = vec![None; needs_len];

        let prefix = format!("{}::", source_id.as_str());
        let prefix_end = format!("{prefix}\u{10FFFF}");

        let mut sample_map: HashMap<String, FeatureStatus> = HashMap::new();
        {
            let mut stmt = conn
                .prepare(
                    "SELECT s.sample_id,
                            s.duration_seconds,
                            s.sr_used,
                            CASE WHEN f.sample_id IS NULL THEN 0 ELSE 1 END AS has_features_v1,
                            CASE WHEN e.sample_id IS NULL THEN 0 ELSE 1 END AS has_embedding,
                            j.status
                     FROM samples s
                     LEFT JOIN features f ON f.sample_id = s.sample_id AND f.feat_version = 1
                     LEFT JOIN embeddings e ON e.sample_id = s.sample_id AND e.model_id = ?2
                     LEFT JOIN analysis_jobs j ON j.sample_id = s.sample_id AND j.job_type = ?1
                     WHERE s.sample_id >= ?3 AND s.sample_id < ?4",
                )
                .map_err(|err| format!("Prepare feature cache query failed: {err}"))?;
            let mut rows = stmt
                .query(params![
                    ANALYSIS_JOB_TYPE,
                    crate::analysis::embedding::EMBEDDING_MODEL_ID,
                    prefix,
                    prefix_end
                ])
                .map_err(|err| format!("Query feature cache failed: {err}"))?;
            while let Some(row) = rows
                .next()
                .map_err(|err| format!("Query feature cache failed: {err}"))?
            {
                let sample_id: String = row.get(0).map_err(|err| err.to_string())?;
                let duration_seconds: Option<f64> = row.get(1).map_err(|err| err.to_string())?;
                let sr_used: Option<i64> = row.get(2).map_err(|err| err.to_string())?;
                let has_features_v1: i64 = row.get(3).map_err(|err| err.to_string())?;
                let has_embedding: i64 = row.get(4).map_err(|err| err.to_string())?;
                let status: Option<String> = row.get(5).map_err(|err| err.to_string())?;
                let analysis_status = status.as_deref().and_then(parse_job_status);
                let Some(relative_path) = sample_id.split_once("::").map(|(_, p)| p) else {
                    continue;
                };
                sample_map.insert(
                    normalize_relative_key(relative_path),
                    FeatureStatus {
                        has_features_v1: has_features_v1 != 0,
                        has_embedding: has_embedding != 0,
                        duration_seconds: duration_seconds.map(|s| s as f32),
                        sr_used,
                        analysis_status,
                    },
                );
            }
        }

        for idx in 0..self.wav_entries_len() {
            let Some(entry) = self.wav_entry(idx) else {
                continue;
            };
            let key = normalize_relative_key(&entry.relative_path.to_string_lossy());
            let status = sample_map.remove(&key).unwrap_or(FeatureStatus {
                has_features_v1: false,
                has_embedding: false,
                duration_seconds: None,
                sr_used: None,
                analysis_status: None,
            });
            rows[idx] = Some(status);
        }
        self.ui_cache
            .browser
            .features
            .insert(source_id.clone(), FeatureCache { rows });

        Ok(())
    }
}

fn parse_job_status(status: &str) -> Option<AnalysisJobStatus> {
    match status {
        "pending" => Some(AnalysisJobStatus::Pending),
        "running" => Some(AnalysisJobStatus::Running),
        "done" => Some(AnalysisJobStatus::Done),
        "failed" => Some(AnalysisJobStatus::Failed),
        "canceled" => Some(AnalysisJobStatus::Canceled),
        _ => None,
    }
}

fn normalize_relative_key(relative_path: &str) -> String {
    relative_path
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_ascii_lowercase()
}
