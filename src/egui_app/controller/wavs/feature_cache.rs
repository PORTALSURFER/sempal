use super::*;
use crate::egui_app::controller::controller_state::{
    AnalysisJobStatus, FeatureCache, FeatureStatus, WeakLabelInfo,
};
use rusqlite::{OptionalExtension, params};
use std::path::{Path, PathBuf};

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

        let mut stmt = conn
            .prepare(
                "WITH best_weak AS (
                    SELECT l.sample_id, l.class_id, l.confidence, l.rule_id
                    FROM labels_weak l
                    WHERE l.ruleset_version = ?1
                      AND l.class_id = (
                        SELECT l2.class_id
                        FROM labels_weak l2
                        WHERE l2.sample_id = l.sample_id
                          AND l2.ruleset_version = ?1
                        ORDER BY l2.confidence DESC, l2.class_id ASC
                        LIMIT 1
                      )
                )
                 SELECT s.sample_id,
                        s.duration_seconds,
                        s.sr_used,
                        CASE WHEN f.sample_id IS NULL THEN 0 ELSE 1 END AS has_features_v1,
                        j.status,
                        w.class_id,
                        w.confidence,
                        w.rule_id
                 FROM samples s
                 LEFT JOIN features f ON f.sample_id = s.sample_id AND f.feat_version = 1
                 LEFT JOIN analysis_jobs j ON j.sample_id = s.sample_id AND j.job_type = ?2
                 LEFT JOIN best_weak w ON w.sample_id = s.sample_id
                 WHERE s.sample_id >= ?3 AND s.sample_id < ?4",
            )
            .map_err(|err| format!("Prepare feature cache query failed: {err}"))?;
        let mut rows = stmt
            .query(params![
                WEAK_LABEL_RULESET_VERSION,
                ANALYSIS_JOB_TYPE,
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
            let status: Option<String> = row.get(4).optional().map_err(|err| err.to_string())?;
            let weak_class_id: Option<String> =
                row.get(5).optional().map_err(|err| err.to_string())?;
            let weak_confidence: Option<f64> =
                row.get(6).optional().map_err(|err| err.to_string())?;
            let weak_rule_id: Option<String> =
                row.get(7).optional().map_err(|err| err.to_string())?;

            let Some(relative_path) = sample_id.split_once("::").map(|(_, p)| p) else {
                continue;
            };
            let Some(&idx) = lookup_entry_index(&self.wav_entries.lookup, relative_path) else {
                continue;
            };
            let analysis_status = status.as_deref().and_then(parse_job_status);
            let weak_label = match (weak_class_id, weak_confidence, weak_rule_id) {
                (Some(class_id), Some(confidence), Some(rule_id)) => Some(WeakLabelInfo {
                    class_id,
                    confidence: confidence as f32,
                    rule_id,
                }),
                _ => None,
            };
            cache.rows[idx] = Some(FeatureStatus {
                has_features_v1: has_features_v1 != 0,
                duration_seconds: duration_seconds.map(|s| s as f32),
                sr_used,
                analysis_status,
                weak_label,
            });
        }

        Ok(())
    }
}

fn lookup_entry_index<'a>(
    lookup: &'a std::collections::HashMap<PathBuf, usize>,
    relative_path: &str,
) -> Option<&'a usize> {
    let path = Path::new(relative_path);
    if let Some(idx) = lookup.get(path) {
        return Some(idx);
    }
    if relative_path.contains('\\') {
        let normalized = relative_path.replace('\\', "/");
        if let Some(idx) = lookup.get(Path::new(&normalized)) {
            return Some(idx);
        }
    }
    if relative_path.contains('/') {
        let normalized = relative_path.replace('/', "\\");
        if let Some(idx) = lookup.get(Path::new(&normalized)) {
            return Some(idx);
        }
    }
    None
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
