use super::*;
use crate::egui_app::controller::controller_state::{
    AnalysisJobStatus, FeatureCache, FeatureStatus, WeakLabelInfo,
};
use rusqlite::{OptionalExtension, params};
use std::collections::HashMap;

const ANALYSIS_JOB_TYPE: &str = "wav_metadata_v1";
const WEAK_LABEL_RULESET_VERSION: i64 = crate::labeling::weak::WEAK_LABEL_RULESET_VERSION;

impl EguiController {
    pub(crate) fn prepare_feature_cache_for_browser(&mut self) {
        let Some(source_id) = self.selection_state.ctx.selected_source.clone() else {
            return;
        };
        let _ = self.ensure_feature_cache(&source_id);
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
        let needs_len = self.wav_entries.entries.len();
        let existing = self.ui_cache.browser.features.get(source_id);
        if existing.is_some_and(|cache| cache.rows.len() == needs_len) {
            return Ok(());
        }
        self.ui_cache.browser.features.insert(
            source_id.clone(),
            FeatureCache {
                rows: vec![None; needs_len],
            },
        );

        let db_path = crate::app_dirs::app_root_dir()
            .map_err(|err| err.to_string())?
            .join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);
        let conn = super::analysis_jobs::open_library_db(&db_path)?;

        let cache = self
            .ui_cache
            .browser
            .features
            .get_mut(source_id)
            .expect("cache inserted above");
        if cache.rows.len() != needs_len {
            cache.rows = vec![None; needs_len];
        }
        cache.rows.fill(None);

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
                            j.status
                     FROM samples s
                     LEFT JOIN features f ON f.sample_id = s.sample_id AND f.feat_version = 1
                     LEFT JOIN analysis_jobs j ON j.sample_id = s.sample_id AND j.job_type = ?1
                     WHERE s.sample_id >= ?2 AND s.sample_id < ?3",
                )
                .map_err(|err| format!("Prepare feature cache query failed: {err}"))?;
            let mut rows = stmt
                .query(params![ANALYSIS_JOB_TYPE, prefix, prefix_end])
                .map_err(|err| format!("Query feature cache failed: {err}"))?;
            while let Some(row) = rows
                .next()
                .map_err(|err| format!("Query feature cache failed: {err}"))?
            {
                let sample_id: String = row.get(0).map_err(|err| err.to_string())?;
                let duration_seconds: Option<f64> = row.get(1).map_err(|err| err.to_string())?;
                let sr_used: Option<i64> = row.get(2).map_err(|err| err.to_string())?;
                let has_features_v1: i64 = row.get(3).map_err(|err| err.to_string())?;
                let status: Option<String> =
                    row.get(4).optional().map_err(|err| err.to_string())?;
                let analysis_status = status.as_deref().and_then(parse_job_status);
                sample_map.insert(
                    normalize_sample_id_key(&sample_id),
                    FeatureStatus {
                        has_features_v1: has_features_v1 != 0,
                        duration_seconds: duration_seconds.map(|s| s as f32),
                        sr_used,
                        analysis_status,
                        weak_label: None,
                    },
                );
            }
        }

        let mut weak_map: HashMap<String, WeakLabelInfo> = HashMap::new();
        {
            let mut stmt = conn
                .prepare(
                    "SELECT sample_id, class_id, confidence, rule_id
                     FROM labels_weak
                     WHERE ruleset_version = ?1
                       AND sample_id >= ?2
                       AND sample_id < ?3",
                )
                .map_err(|err| format!("Prepare weak label cache query failed: {err}"))?;
            let mut rows = stmt
                .query(params![WEAK_LABEL_RULESET_VERSION, prefix, prefix_end])
                .map_err(|err| format!("Query weak label cache failed: {err}"))?;
            while let Some(row) = rows
                .next()
                .map_err(|err| format!("Query weak label cache failed: {err}"))?
            {
                let sample_id: String = row.get(0).map_err(|err| err.to_string())?;
                let class_id: String = row.get(1).map_err(|err| err.to_string())?;
                let confidence: f64 = row.get(2).map_err(|err| err.to_string())?;
                let rule_id: String = row.get(3).map_err(|err| err.to_string())?;
                let key = normalize_sample_id_key(&sample_id);
                let candidate = WeakLabelInfo {
                    class_id,
                    confidence: confidence as f32,
                    rule_id,
                };
                match weak_map.get(&key) {
                    Some(existing) if existing.confidence >= candidate.confidence => {}
                    _ => {
                        weak_map.insert(key, candidate);
                    }
                }
            }
        }

        for (idx, entry) in self.wav_entries.entries.iter().enumerate() {
            let sample_id = build_sample_id(source_id.as_str(), &entry.relative_path);
            let key = normalize_sample_id_key(&sample_id);
            let mut status = sample_map.remove(&key).unwrap_or(FeatureStatus {
                has_features_v1: false,
                duration_seconds: None,
                sr_used: None,
                analysis_status: None,
                weak_label: None,
            });
            if let Some(weak) = weak_map.get(&key) {
                status.weak_label = Some(weak.clone());
            }
            cache.rows[idx] = Some(status);
        }

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

fn normalize_sample_id_key(sample_id: &str) -> String {
    sample_id.replace('\\', "/")
}

fn build_sample_id(source_id: &str, relative_path: &std::path::Path) -> String {
    let rel = relative_path.to_string_lossy().replace('\\', "/");
    format!("{source_id}::{rel}")
}
