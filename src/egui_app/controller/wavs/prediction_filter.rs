use super::*;
use crate::egui_app::controller::analysis_jobs::types::TopKProbability;
use crate::egui_app::state::PredictedCategory;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

impl EguiController {
    pub fn prepare_prediction_cache_for_browser(&mut self) {
        let Some(source_id) = self.selection_state.ctx.selected_source.clone() else {
            return;
        };
        let _ = self.ensure_prediction_cache(&source_id);
        let _ = self.ensure_prediction_categories();
        self.ui_cache.browser.prediction_categories_checked = true;
    }

    pub(super) fn prepare_prediction_filter_cache(&mut self) {
        let Some(source_id) = self.selection_state.ctx.selected_source.clone() else {
            return;
        };
        if self.ui.browser.review_mode {
            let _ = self.ensure_prediction_cache(&source_id);
            let _ = self.ensure_prediction_categories();
            self.ui_cache.browser.prediction_categories_checked = true;
            return;
        }
        let category = self.ui.browser.category_filter.as_deref();
        let threshold = self.ui.browser.confidence_threshold.clamp(0.0, 1.0);
        if category.is_none() && threshold <= 0.0 && self.ui.browser.include_unknowns {
            return;
        }
        let _ = self.ensure_prediction_cache(&source_id);
        let _ = self.ensure_prediction_categories();
        self.ui_cache.browser.prediction_categories_checked = true;
    }

    pub(super) fn prediction_filter_accepts(&self, entry_index: usize) -> bool {
        self.prediction_filter_accepts_cached(entry_index)
    }

    pub fn cached_prediction_for_entry(
        &self,
        entry_index: usize,
    ) -> Option<&PredictedCategory> {
        let source_id = self.selection_state.ctx.selected_source.as_ref()?;
        self.ui_cache
            .browser
            .predictions
            .get(source_id)
            .and_then(|cache| cache.rows.get(entry_index))
            .and_then(|pred| pred.as_ref())
    }

    pub fn cached_category_for_entry(&self, entry_index: usize) -> Option<(String, bool)> {
        let source_id = self.selection_state.ctx.selected_source.as_ref()?;
        let cache = self.ui_cache.browser.predictions.get(source_id)?;
        let pred = cache.rows.get(entry_index).and_then(|pred| pred.as_ref())?;
        let is_override = cache.user_overrides.get(entry_index).copied().unwrap_or(false);
        Some((pred.class_id.clone(), is_override))
    }

    fn prediction_filter_accepts_cached(&self, entry_index: usize) -> bool {
        if self.ui.browser.review_mode {
            return self.review_filter_accepts_cached(entry_index);
        }
        let category = self.ui.browser.category_filter.as_deref();
        let threshold = self.ui.browser.confidence_threshold.clamp(0.0, 1.0);
        if category.is_none() && threshold <= 0.0 && self.ui.browser.include_unknowns {
            return true;
        }
        let Some(source_id) = self.selection_state.ctx.selected_source.clone() else {
            return true;
        };
        let Some(cache) = self.ui_cache.browser.predictions.get(&source_id) else {
            return true;
        };
        let Some(prediction) = cache.rows.get(entry_index).and_then(|p| p.as_ref()) else {
            return false;
        };
        if !self.ui.browser.include_unknowns
            && category != Some("UNKNOWN")
            && prediction.class_id == "UNKNOWN"
        {
            return false;
        }
        if let Some(category) = category
            && prediction.class_id != category
        {
            return false;
        }
        if threshold > 0.0 && prediction.confidence < threshold {
            return false;
        }
        true
    }

    fn review_filter_accepts_cached(&self, entry_index: usize) -> bool {
        let category = self.ui.browser.category_filter.as_deref();
        let max_confidence = self.ui.browser.review_max_confidence.clamp(0.0, 1.0);
        let Some(source_id) = self.selection_state.ctx.selected_source.clone() else {
            return true;
        };
        let Some(cache) = self.ui_cache.browser.predictions.get(&source_id) else {
            return self.ui.browser.review_include_unpredicted;
        };
        let prediction = cache.rows.get(entry_index).and_then(|p| p.as_ref());
        let Some(prediction) = prediction else {
            return category.is_none() && self.ui.browser.review_include_unpredicted;
        };
        if let Some(category) = category
            && prediction.class_id != category
        {
            return false;
        }
        if prediction.class_id == "UNKNOWN" {
            return true;
        }
        prediction.confidence < max_confidence
    }

    pub fn prediction_categories(&mut self) -> Vec<String> {
        if self.ui_cache.browser.prediction_categories.is_none()
            && !self.ui_cache.browser.prediction_categories_checked
        {
            let _ = self.ensure_prediction_categories();
            self.ui_cache.browser.prediction_categories_checked = true;
        }
        self.ui_cache
            .browser
            .prediction_categories
            .as_ref()
            .map(|cats| {
                let mut classes = cats.classes.clone();
                if !classes.iter().any(|c| c == "UNKNOWN") {
                    classes.insert(0, "UNKNOWN".to_string());
                }
                classes
            })
            .unwrap_or_default()
    }

    pub fn label_override_categories(&mut self) -> Vec<String> {
        let mut categories = self.prediction_categories();
        if categories.is_empty() {
            categories = label_rules_categories();
        }
        categories
    }

    fn ensure_prediction_categories(&mut self) -> Result<(), String> {
        let db_path = crate::app_dirs::app_root_dir()
            .map_err(|err| err.to_string())?
            .join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);
        let conn = open_library_db(&db_path)?;
        let model_id = select_active_model_id(&conn, self.classifier_model_id())?;
        let Some(model_id) = model_id else {
            self.ui_cache.browser.prediction_categories = None;
            return Ok(());
        };
        let latest: Option<(String, String)> = conn
            .query_row(
                "SELECT model_id, classes_json
                 FROM models
                 WHERE model_id = ?1",
                params![&model_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|err| format!("Failed to query model classes: {err}"))?;
        let Some((model_id, classes_json)) = latest else {
            self.ui_cache.browser.prediction_categories = None;
            return Ok(());
        };
        if self
            .ui_cache
            .browser
            .prediction_categories
            .as_ref()
            .is_some_and(|cached| cached.model_id == model_id)
        {
            return Ok(());
        }
        let classes: Vec<String> =
            serde_json::from_str(&classes_json).map_err(|err| err.to_string())?;
        self.ui_cache.browser.prediction_categories = Some(
            super::super::controller_state::PredictionCategories { model_id, classes },
        );
        Ok(())
    }

    fn ensure_prediction_cache(&mut self, source_id: &SourceId) -> Result<(), String> {
        let needs_len = self.wav_entries.entries.len();
        let existing = self.ui_cache.browser.predictions.get(source_id);
        if existing.is_some_and(|cache| cache.rows.len() == needs_len) {
            // Might still need refresh if model changed; handled below.
        } else {
            self.ui_cache.browser.predictions.insert(
                source_id.clone(),
                super::super::controller_state::PredictionCache {
                    model_id: None,
                    rows: vec![None; needs_len],
                    user_overrides: vec![false; needs_len],
                },
            );
        }

        let db_path = crate::app_dirs::app_root_dir()
            .map_err(|err| err.to_string())?
            .join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);
        let conn = open_library_db(&db_path)?;
        let latest_model_id = select_active_model_id(&conn, self.classifier_model_id())?;

        let unknown_threshold = self.unknown_confidence_threshold().clamp(0.0, 1.0);
        let use_overrides = self.use_user_overrides_in_browser();
        let cache = self
            .ui_cache
            .browser
            .predictions
            .get_mut(source_id)
            .expect("cache inserted above");
        if cache.rows.len() != needs_len {
            cache.rows = vec![None; needs_len];
            cache.user_overrides = vec![false; needs_len];
        }
        if cache.model_id == latest_model_id {
            return Ok(());
        }
        cache.model_id = latest_model_id.clone();
        cache.rows.fill(None);
        cache.user_overrides.fill(false);

        let Some(model_id) = latest_model_id else {
            return Ok(());
        };

        let prefix = format!("{}::", source_id.as_str());
        let prefix_end = format!("{prefix}\u{10FFFF}");

        if use_overrides {
            apply_user_labels(&conn, &prefix, &prefix_end, &self.wav_entries.lookup, cache)?;
        }

        let mut stmt = conn
            .prepare(
                "SELECT sample_id, top_class, confidence, topk_json
                 FROM predictions
                 WHERE model_id = ?1 AND sample_id >= ?2 AND sample_id < ?3",
            )
            .map_err(|err| format!("Failed to prepare predictions query: {err}"))?;
        let mut rows = stmt
            .query(params![model_id, prefix, prefix_end])
            .map_err(|err| format!("Failed to query predictions: {err}"))?;
        while let Some(row) = rows
            .next()
            .map_err(|err| format!("Failed to query predictions: {err}"))?
        {
            let sample_id: String = row.get(0).map_err(|err| err.to_string())?;
            let top_class: String = row.get(1).map_err(|err| err.to_string())?;
            let confidence: f64 = row.get(2).map_err(|err| err.to_string())?;
            let topk_json: String = row.get(3).map_err(|err| err.to_string())?;
            let class_id = if confidence < unknown_threshold as f64 {
                "UNKNOWN".to_string()
            } else {
                top_class
            };
            let margin = parse_margin_from_topk(&topk_json);
            let Some(relative_path) = sample_id.split_once("::").map(|(_, p)| p) else {
                continue;
            };
            let Some(&idx) = lookup_entry_index(&self.wav_entries.lookup, relative_path) else {
                continue;
            };
            if idx >= cache.rows.len() {
                continue;
            }
            if cache.rows[idx].is_none() {
                cache.rows[idx] = Some(PredictedCategory {
                    class_id,
                    confidence: confidence as f32,
                    margin,
                });
                cache.user_overrides[idx] = false;
            }
        }

        Ok(())
    }
}

fn select_active_model_id(
    conn: &Connection,
    preferred: Option<String>,
) -> Result<Option<String>, String> {
    if let Some(model_id) = preferred {
        let exists: Option<String> = conn
            .query_row(
                "SELECT model_id FROM models WHERE model_id = ?1",
                params![&model_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| format!("Failed to query preferred model id: {err}"))?;
        if exists.is_some() {
            return Ok(Some(model_id));
        }
    }
    let latest: Option<String> = conn
        .query_row(
            "SELECT model_id
             FROM models
             ORDER BY created_at DESC, model_id DESC
             LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| format!("Failed to query latest model id: {err}"))?;
    Ok(latest)
}

fn label_rules_categories() -> Vec<String> {
    crate::labeling::weak_config::load_label_rules_from_app_dir()
        .map(|cfg| cfg.categories.keys().cloned().collect())
        .unwrap_or_default()
}

pub(super) fn set_category_filter(controller: &mut EguiController, category: Option<String>) {
    if controller.ui.browser.category_filter == category {
        return;
    }
    controller.ui.browser.category_filter = category;
    controller.rebuild_browser_lists();
}

pub(super) fn set_confidence_threshold(controller: &mut EguiController, threshold: f32) {
    let threshold = threshold.clamp(0.0, 1.0);
    if (controller.ui.browser.confidence_threshold - threshold).abs() < f32::EPSILON {
        return;
    }
    controller.ui.browser.confidence_threshold = threshold;
    controller.rebuild_browser_lists();
}

pub(super) fn set_include_unknowns(controller: &mut EguiController, include: bool) {
    if controller.ui.browser.include_unknowns == include {
        return;
    }
    controller.ui.browser.include_unknowns = include;
    controller.rebuild_browser_lists();
}

pub(super) fn set_review_mode(controller: &mut EguiController, enabled: bool) {
    if controller.ui.browser.review_mode == enabled {
        return;
    }
    controller.ui.browser.review_mode = enabled;
    controller.rebuild_browser_lists();
}

pub(super) fn set_review_max_confidence(controller: &mut EguiController, value: f32) {
    let value = value.clamp(0.0, 1.0);
    if (controller.ui.browser.review_max_confidence - value).abs() < f32::EPSILON {
        return;
    }
    controller.ui.browser.review_max_confidence = value;
    controller.rebuild_browser_lists();
}

pub(super) fn set_review_include_unpredicted(controller: &mut EguiController, include: bool) {
    if controller.ui.browser.review_include_unpredicted == include {
        return;
    }
    controller.ui.browser.review_include_unpredicted = include;
    controller.rebuild_browser_lists();
}

fn open_library_db(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|err| format!("Open library DB failed: {err}"))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=5000;
         PRAGMA temp_store=MEMORY;
         PRAGMA cache_size=-64000;
         PRAGMA mmap_size=268435456;",
    )
    .map_err(|err| format!("Failed to set library DB pragmas: {err}"))?;
    if let Err(err) = crate::sqlite_ext::try_load_optional_extension(&conn) {
        tracing::debug!("SQLite extension not loaded: {err}");
    }
    Ok(conn)
}

fn apply_user_labels(
    conn: &Connection,
    prefix: &str,
    prefix_end: &str,
    lookup: &HashMap<PathBuf, usize>,
    cache: &mut super::super::controller_state::PredictionCache,
) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "SELECT sample_id, class_id
             FROM labels_user
             WHERE sample_id >= ?1 AND sample_id < ?2",
        )
        .map_err(|err| format!("Failed to prepare user labels query: {err}"))?;
    let mut rows = stmt
        .query(params![prefix, prefix_end])
        .map_err(|err| format!("Failed to query user labels: {err}"))?;
    while let Some(row) = rows
        .next()
        .map_err(|err| format!("Failed to query user labels: {err}"))?
    {
        let sample_id: String = row.get(0).map_err(|err| err.to_string())?;
        let class_id: String = row.get(1).map_err(|err| err.to_string())?;
        let Some(relative_path) = sample_id.split_once("::").map(|(_, p)| p) else {
            continue;
        };
        let Some(&idx) = lookup_entry_index(lookup, relative_path) else {
            continue;
        };
        if idx >= cache.rows.len() {
            continue;
        }
        cache.rows[idx] = Some(PredictedCategory {
            class_id,
            confidence: 1.0,
            margin: None,
        });
        cache.user_overrides[idx] = true;
    }
    Ok(())
}

fn parse_margin_from_topk(topk_json: &str) -> Option<f32> {
    let parsed: Vec<TopKProbability> = serde_json::from_str(topk_json).ok()?;
    let mut probs: Vec<f32> = parsed.iter().map(|p| p.probability).collect();
    if probs.is_empty() {
        return None;
    }
    probs.sort_by(|a, b| b.total_cmp(a));
    let top1 = probs.get(0).copied().unwrap_or(0.0);
    let top2 = probs.get(1).copied().unwrap_or(0.0);
    Some(top1 - top2)
}

fn lookup_entry_index<'a>(
    lookup: &'a HashMap<PathBuf, usize>,
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
