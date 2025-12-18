use super::*;
use crate::egui_app::state::PredictedCategory;
use rusqlite::{Connection, OptionalExtension, params};

impl EguiController {
    pub(super) fn prepare_prediction_filter_cache(&mut self) {
        let category = self.ui.browser.category_filter.as_deref();
        let threshold = self.ui.browser.confidence_threshold.clamp(0.0, 1.0);
        if category.is_none() && threshold <= 0.0 && self.ui.browser.include_unknowns {
            return;
        }
        let Some(source_id) = self.selection_state.ctx.selected_source.clone() else {
            return;
        };
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

    fn prediction_filter_accepts_cached(&self, entry_index: usize) -> bool {
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

    fn ensure_prediction_categories(&mut self) -> Result<(), String> {
        let db_path = crate::app_dirs::app_root_dir()
            .map_err(|err| err.to_string())?
            .join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);
        let conn = open_library_db(&db_path)?;
        let latest: Option<(String, String)> = conn
            .query_row(
                "SELECT model_id, classes_json
                 FROM models
                 ORDER BY created_at DESC, model_id DESC
                 LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|err| format!("Failed to query latest model: {err}"))?;
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
                },
            );
        }

        let db_path = crate::app_dirs::app_root_dir()
            .map_err(|err| err.to_string())?
            .join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);
        let conn = open_library_db(&db_path)?;

        let latest_model_id: Option<String> = conn
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

        let cache = self
            .ui_cache
            .browser
            .predictions
            .get_mut(source_id)
            .expect("cache inserted above");
        if cache.rows.len() != needs_len {
            cache.rows = vec![None; needs_len];
        }
        if cache.model_id == latest_model_id {
            return Ok(());
        }
        cache.model_id = latest_model_id.clone();
        cache.rows.fill(None);

        let Some(model_id) = latest_model_id else {
            return Ok(());
        };

        let prefix = format!("{}::", source_id.as_str());
        let prefix_end = format!("{prefix}\u{10FFFF}");
        let mut stmt = conn
            .prepare(
                "SELECT sample_id, top_class, confidence
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
            let Some(relative_path) = sample_id.split_once("::").map(|(_, p)| p) else {
                continue;
            };
            let path = PathBuf::from(relative_path);
            let Some(&idx) = self.wav_entries.lookup.get(&path) else {
                continue;
            };
            if idx >= cache.rows.len() {
                continue;
            }
            cache.rows[idx] = Some(PredictedCategory {
                class_id: top_class,
                confidence: confidence as f32,
            });
        }

        Ok(())
    }
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

fn open_library_db(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|err| format!("Open library DB failed: {err}"))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys=ON;",
    )
    .map_err(|err| format!("Failed to set library DB pragmas: {err}"))?;
    Ok(conn)
}
