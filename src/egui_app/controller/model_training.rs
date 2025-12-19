use super::*;
use crate::egui_app::state::ProgressTaskKind;
use rusqlite::{params, OptionalExtension};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub(super) struct ModelTrainingJob {
    pub(super) db_path: PathBuf,
    pub(super) source_ids: Vec<String>,
    pub(super) min_confidence: f32,
    pub(super) pack_depth: usize,
    pub(super) use_user_labels: bool,
    pub(super) model_kind: crate::sample_sources::config::TrainingModelKind,
    pub(super) train_options: crate::ml::gbdt_stump::TrainOptions,
    pub(super) mlp_options: crate::ml::mlp::TrainOptions,
    pub(super) logreg_options: crate::ml::logreg::TrainOptions,
}

#[derive(Clone, Debug)]
pub(super) struct ModelTrainingResult {
    pub(super) model_id: String,
    pub(super) exported_samples: usize,
    pub(super) inference_jobs_enqueued: usize,
}

#[derive(Clone, Debug)]
pub(super) enum ModelTrainingMessage {
    Progress {
        completed: usize,
        total: usize,
        detail: String,
    },
    Finished {
        result: Result<ModelTrainingResult, String>,
    },
}

pub(super) fn run_model_training(
    job: ModelTrainingJob,
    tx: &Sender<super::jobs::JobMessage>,
) -> Result<ModelTrainingResult, String> {
    let total_steps = 4usize;
    send_progress(
        tx,
        0,
        total_steps,
        format!("Exporting dataset (min_conf={:.2})…", job.min_confidence),
    )?;
    let temp = tempfile::tempdir().map_err(|err| err.to_string())?;
    let out_dir = temp.path().join("dataset");
    let mut options = crate::dataset::export::ExportOptions {
        out_dir,
        db_path: Some(job.db_path.clone()),
        min_confidence: job.min_confidence,
        pack_depth: job.pack_depth,
        use_user_labels: job.use_user_labels,
        seed: "sempal-dataset-v1".to_string(),
        test_fraction: 0.1,
        val_fraction: 0.1,
    };

    let (summary, used_min_confidence) =
        export_with_confidence_fallback(&mut options, &job, tx)?;
    if summary.total_exported < 2 {
        return Err(format!(
            "Not enough labeled samples to train (need >=2, got {}). {}",
            summary.total_exported,
            training_diagnostics_hint(
                &job.db_path,
                &job.source_ids,
                used_min_confidence,
                job.use_user_labels,
                job.model_kind.clone()
            )?
        ));
    }

    send_progress(
        tx,
        1,
        total_steps,
        format!("Training model on {} samples…", summary.total_exported),
    )?;
    let loaded =
        crate::dataset::loader::load_dataset(&options.out_dir).map_err(|err| err.to_string())?;
    let (model_json, classes_json, kind, feat_version, feature_len_f32) = match job.model_kind {
        crate::sample_sources::config::TrainingModelKind::GbdtStumpV1 => {
            let (train, test) = split_train_test(&loaded)?;
            let model = crate::ml::gbdt_stump::train_gbdt_stump(&train, &job.train_options)?;
            let _ = evaluate_accuracy(&model, &test);
            (
                serde_json::to_string(&model).map_err(|err| err.to_string())?,
                serde_json::to_string(&model.classes).map_err(|err| err.to_string())?,
                "gbdt_stump_v1",
                crate::analysis::FEATURE_VERSION_V1,
                crate::analysis::FEATURE_VECTOR_LEN_V1 as i64,
            )
        }
        crate::sample_sources::config::TrainingModelKind::MlpV1 => {
            let (train, test) = split_train_test(&loaded)?;
            let model = crate::ml::mlp::train_mlp(&train, &job.mlp_options)?;
            let _ = evaluate_mlp_accuracy(&model, &test);
            (
                serde_json::to_string(&model).map_err(|err| err.to_string())?,
                serde_json::to_string(&model.classes).map_err(|err| err.to_string())?,
                "mlp_v1",
                crate::analysis::FEATURE_VERSION_V1,
                crate::analysis::FEATURE_VECTOR_LEN_V1 as i64,
            )
        }
        crate::sample_sources::config::TrainingModelKind::LogRegV1 => {
            let (train_logreg, test_logreg) = split_logreg_train_test(&loaded)?;
            let model =
                crate::ml::logreg::train_logreg(&train_logreg, &job.logreg_options)?;
            let _ = evaluate_logreg_accuracy(&model, &test_logreg);
            (
                serde_json::to_string(&model).map_err(|err| err.to_string())?,
                serde_json::to_string(&model.classes).map_err(|err| err.to_string())?,
                "logreg_v1",
                0,
                crate::analysis::embedding::EMBEDDING_DIM as i64,
            )
        }
    };

    send_progress(tx, 2, total_steps, "Importing model…")?;
    let model_id = import_model_json_into_db(
        &job.db_path,
        kind,
        &model_json,
        &classes_json,
        feat_version,
        feature_len_f32,
    )?;

    send_progress(tx, 3, total_steps, "Enqueueing inference…")?;
    let (mut inference_jobs_enqueued, _progress) =
        super::analysis_jobs::enqueue_inference_jobs_for_sources(
            &job.source_ids,
            Some(&model_id),
        )?;
    if inference_jobs_enqueued < summary.total_exported {
        let (extra, _progress) =
            super::analysis_jobs::enqueue_inference_jobs_for_all_features(Some(&model_id))?;
        inference_jobs_enqueued = inference_jobs_enqueued.saturating_add(extra);
    }

    Ok(ModelTrainingResult {
        model_id,
        exported_samples: summary.total_exported,
        inference_jobs_enqueued,
    })
}

pub(super) fn begin_retrain_from_app(controller: &mut EguiController) {
    if controller.runtime.jobs.model_training_in_progress() {
        controller.set_status("Model training already running", StatusTone::Info);
        return;
    }
    let db_path = match crate::app_dirs::app_root_dir() {
        Ok(root) => root.join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME),
        Err(err) => {
            controller.set_status(
                format!("Resolve library DB failed: {err}"),
                StatusTone::Error,
            );
            return;
        }
    };
    let source_ids: Vec<String> = controller
        .library
        .sources
        .iter()
        .map(|source| source.id.as_str().to_string())
        .collect();
    let model_kind = controller.training_model_kind();
    if let Ok(conn) = super::analysis_jobs::open_library_db(&db_path) {
        if let Ok(diag) = training_diagnostics_for_sources(
            &conn,
            &source_ids,
            controller.retrain_min_confidence(),
            controller.retrain_use_user_labels(),
            &model_kind,
        ) {
            if diag.samples_total > 100
                && diag.features_v1 < (diag.samples_total / 4).max(50)
            {
                let mut inserted = 0usize;
                for source in controller.library.sources.iter() {
                    if let Ok((count, _)) =
                        super::analysis_jobs::enqueue_jobs_for_source_missing_features(source)
                    {
                        inserted += count;
                    }
                }
                controller.set_status(
                    format!(
                        "Queued {inserted} feature jobs; retrain once analysis completes"
                    ),
                    StatusTone::Info,
                );
                return;
            }
        }
    }
    controller.show_status_progress(ProgressTaskKind::ModelTraining, "Training model", 4, false);
    let train_options = crate::ml::gbdt_stump::TrainOptions::default();
    let mlp_options = crate::ml::mlp::TrainOptions::default();
    let logreg_options = crate::ml::logreg::TrainOptions::default();
    controller
        .runtime
        .jobs
        .begin_model_training(ModelTrainingJob {
            db_path,
            source_ids,
            min_confidence: controller.retrain_min_confidence(),
            pack_depth: controller.retrain_pack_depth(),
            use_user_labels: controller.retrain_use_user_labels(),
            model_kind,
            train_options,
            mlp_options,
            logreg_options,
        });
}

impl EguiController {
    pub fn refresh_training_summary(&mut self) {
        let db_path = match crate::app_dirs::app_root_dir() {
            Ok(root) => root.join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME),
            Err(err) => {
                self.ui.training.summary = None;
                self.ui.training.summary_error = Some(format!("Resolve library DB failed: {err}"));
                return;
            }
        };
        let source_ids: Vec<String> = self
            .library
            .sources
            .iter()
            .map(|source| source.id.as_str().to_string())
            .collect();
        if source_ids.is_empty() {
            self.ui.training.summary = None;
            self.ui.training.summary_error = Some("No sources loaded".to_string());
            return;
        }

        let conn = match super::analysis_jobs::open_library_db(&db_path) {
            Ok(conn) => conn,
            Err(err) => {
                self.ui.training.summary = None;
                self.ui.training.summary_error = Some(err);
                return;
            }
        };
        let min_confidence = self.retrain_min_confidence();
        let model_kind = self.training_model_kind();
        let diagnostics = match training_diagnostics_for_sources(
            &conn,
            &source_ids,
            min_confidence,
            self.retrain_use_user_labels(),
            &model_kind,
        ) {
            Ok(diag) => diag,
            Err(err) => {
                self.ui.training.summary = None;
                self.ui.training.summary_error = Some(err);
                return;
            }
        };
        let exportable = match training_exportable_count(
            &conn,
            &source_ids,
            min_confidence,
            self.retrain_use_user_labels(),
            model_kind,
        ) {
            Ok(count) => count,
            Err(err) => {
                self.ui.training.summary = None;
                self.ui.training.summary_error = Some(err);
                return;
            }
        };
        let prediction_stats = match training_prediction_stats(&conn, &source_ids) {
            Ok(stats) => stats,
            Err(err) => {
                self.ui.training.summary = None;
                self.ui.training.summary_error = Some(err);
                return;
            }
        };
        let (predictions_total, predictions_unknown, predictions_min_conf, predictions_avg_conf, predictions_max_conf) =
            prediction_stats
                .map(|stats| {
                    (
                        Some(stats.total),
                        Some(stats.unknown),
                        stats.min_confidence,
                        stats.avg_confidence,
                        stats.max_confidence,
                    )
                })
                .unwrap_or((None, None, None, None, None));

        self.ui.training.summary = Some(crate::egui_app::state::TrainingSummary {
            updated_at: now_epoch_seconds(),
            sources: source_ids.len(),
            samples_total: diagnostics.samples_total,
            features_v1: diagnostics.features_v1,
            user_labeled: diagnostics.user_join,
            weak_labeled: diagnostics.weak_join,
            exportable,
            predictions_total,
            predictions_unknown,
            predictions_min_conf,
            predictions_avg_conf,
            predictions_max_conf,
            min_confidence,
        });
        self.ui.training.summary_error = None;
    }

    pub fn retrain_model_from_app(&mut self) {
        begin_retrain_from_app(self);
    }

    pub fn rerun_inference_for_loaded_sources(&mut self) {
        let source_ids: Vec<String> = self
            .library
            .sources
            .iter()
            .map(|source| source.id.as_str().to_string())
            .collect();
        if source_ids.is_empty() {
            self.set_status("No sources loaded", StatusTone::Info);
            return;
        }
        let db_path = match crate::app_dirs::app_root_dir() {
            Ok(root) => root.join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME),
            Err(err) => {
                self.set_status(
                    format!("Resolve library DB failed: {err}"),
                    StatusTone::Error,
                );
                return;
            }
        };
        let conn = match super::analysis_jobs::open_library_db(&db_path) {
            Ok(conn) => conn,
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
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
            .map_err(|err| err.to_string())
            .ok()
            .flatten();
        let Some(model_id) = latest_model_id else {
            self.set_status("No model available for inference", StatusTone::Info);
            return;
        };
        let deleted = match delete_predictions_for_sources(&conn, &model_id, &source_ids) {
            Ok(count) => count,
            Err(err) => {
                self.set_status(format!("Failed to clear predictions: {err}"), StatusTone::Error);
                return;
            }
        };
        let preferred_model_id = self.classifier_model_id();
        match super::analysis_jobs::enqueue_inference_jobs_for_sources(
            &source_ids,
            preferred_model_id.as_deref(),
        ) {
            Ok((count, _progress)) => {
                self.set_status(
                    format!("Cleared {deleted} predictions; queued {count} inference jobs"),
                    StatusTone::Info,
                );
            }
            Err(err) => {
                self.set_status(format!("Failed to queue inference: {err}"), StatusTone::Error);
            }
        }
    }

    pub fn model_training_in_progress(&self) -> bool {
        self.runtime.jobs.model_training_in_progress()
    }
}

fn send_progress(
    tx: &Sender<super::jobs::JobMessage>,
    completed: usize,
    total: usize,
    detail: impl Into<String>,
) -> Result<(), String> {
    tx.send(super::jobs::JobMessage::ModelTraining(
        ModelTrainingMessage::Progress {
            completed,
            total,
            detail: detail.into(),
        },
    ))
    .map_err(|_| "Model training channel dropped".to_string())
}

fn import_model_json_into_db(
    db_path: &PathBuf,
    kind: &str,
    model_json: &str,
    classes_json: &str,
    feat_version: i64,
    feature_len_f32: i64,
) -> Result<String, String> {
    let created_at = now_epoch_seconds();
    let model_id = uuid::Uuid::new_v4().to_string();
    let conn = super::analysis_jobs::open_library_db(db_path)?;
    conn.execute(
        "INSERT INTO models (
            model_id, kind, model_version, feat_version, feature_len_f32, classes_json, model_json, created_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            model_id,
            kind,
            1,
            feat_version,
            feature_len_f32,
            classes_json,
            model_json,
            created_at
        ],
    )
    .map_err(|err| format!("Failed to insert model: {err}"))?;
    Ok(model_id)
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64
}

fn split_train_test(
    loaded: &crate::dataset::loader::LoadedDataset,
) -> Result<
    (
        crate::ml::gbdt_stump::TrainDataset,
        crate::ml::gbdt_stump::TrainDataset,
    ),
    String,
> {
    if loaded.manifest.feat_version != crate::analysis::FEATURE_VERSION_V1 {
        return Err(format!(
            "Unsupported feat_version {} (expected {})",
            loaded.manifest.feat_version,
            crate::analysis::FEATURE_VERSION_V1
        ));
    }
    if loaded.manifest.feature_len_f32 != crate::analysis::FEATURE_VECTOR_LEN_V1 {
        return Err(format!(
            "Unsupported feature_len_f32 {} (expected {})",
            loaded.manifest.feature_len_f32,
            crate::analysis::FEATURE_VECTOR_LEN_V1
        ));
    }

    let class_map = loaded.class_index_map();
    let classes: Vec<String> = class_map.iter().map(|(name, _)| name.clone()).collect();

    #[derive(Clone)]
    struct LabeledRow {
        sample_id: String,
        class_idx: usize,
        split: String,
        row: Vec<f32>,
    }

    let mut rows = Vec::new();
    for sample in &loaded.samples {
        let Some(row) = loaded.feature_row(sample) else {
            continue;
        };
        let Some(&class_idx) = class_map.get(&sample.label.class_id) else {
            continue;
        };
        rows.push(LabeledRow {
            sample_id: sample.sample_id.clone(),
            class_idx,
            split: sample.split.clone(),
            row: row.to_vec(),
        });
    }

    if rows.len() < 2 {
        return Err(format!(
            "Dataset needs at least 2 labeled samples (got {})",
            rows.len()
        ));
    }

    let mut train_x = Vec::new();
    let mut train_y = Vec::new();
    let mut test_x = Vec::new();
    let mut test_y = Vec::new();

    for item in &rows {
        match item.split.as_str() {
            "train" => {
                train_x.push(item.row.clone());
                train_y.push(item.class_idx);
            }
            "test" => {
                test_x.push(item.row.clone());
                test_y.push(item.class_idx);
            }
            _ => {}
        }
    }

    if train_x.is_empty() || test_x.is_empty() {
        // Fallback when pack-based splitting yields no test set (e.g., all samples in one pack).
        // Use a deterministic sample-level split keyed by sample_id to ensure both sets exist.
        train_x.clear();
        train_y.clear();
        test_x.clear();
        test_y.clear();

        for item in &rows {
            let u = split_u01(&item.sample_id);
            if u < 0.1 {
                test_x.push(item.row.clone());
                test_y.push(item.class_idx);
            } else {
                train_x.push(item.row.clone());
                train_y.push(item.class_idx);
            }
        }

        if test_x.is_empty() {
            if let Some(row) = train_x.pop()
                && let Some(y) = train_y.pop()
            {
                test_x.push(row);
                test_y.push(y);
            }
        } else if train_x.is_empty() {
            if let Some(row) = test_x.pop()
                && let Some(y) = test_y.pop()
            {
                train_x.push(row);
                train_y.push(y);
            }
        }

        if train_x.is_empty() || test_x.is_empty() {
            return Err("Dataset needs both train and test samples".to_string());
        }
    }

    Ok((
        crate::ml::gbdt_stump::TrainDataset {
            feature_len_f32: crate::analysis::FEATURE_VECTOR_LEN_V1,
            feat_version: crate::analysis::FEATURE_VERSION_V1,
            classes: classes.clone(),
            x: train_x,
            y: train_y,
        },
        crate::ml::gbdt_stump::TrainDataset {
            feature_len_f32: crate::analysis::FEATURE_VECTOR_LEN_V1,
            feat_version: crate::analysis::FEATURE_VERSION_V1,
            classes,
            x: test_x,
            y: test_y,
        },
    ))
}

fn split_logreg_train_test(
    loaded: &crate::dataset::loader::LoadedDataset,
) -> Result<
    (
        crate::ml::logreg::TrainDataset,
        crate::ml::logreg::TrainDataset,
    ),
    String,
> {
    if loaded.manifest.feature_len_f32 != crate::analysis::embedding::EMBEDDING_DIM {
        return Err(format!(
            "Unsupported embedding_len {} (expected {})",
            loaded.manifest.feature_len_f32,
            crate::analysis::embedding::EMBEDDING_DIM
        ));
    }

    let class_map = loaded.class_index_map();
    let classes: Vec<String> = class_map.iter().map(|(name, _)| name.clone()).collect();

    #[derive(Clone)]
    struct LabeledRow {
        sample_id: String,
        class_idx: usize,
        split: String,
        row: Vec<f32>,
    }

    let mut rows = Vec::new();
    for sample in &loaded.samples {
        let Some(row) = loaded.feature_row(sample) else {
            continue;
        };
        let Some(&class_idx) = class_map.get(&sample.label.class_id) else {
            continue;
        };
        rows.push(LabeledRow {
            sample_id: sample.sample_id.clone(),
            class_idx,
            split: sample.split.clone(),
            row: row.to_vec(),
        });
    }

    if rows.len() < 2 {
        return Err(format!(
            "Dataset needs at least 2 labeled samples (got {})",
            rows.len()
        ));
    }

    let mut train_x = Vec::new();
    let mut train_y = Vec::new();
    let mut test_x = Vec::new();
    let mut test_y = Vec::new();

    for item in &rows {
        match item.split.as_str() {
            "train" => {
                train_x.push(item.row.clone());
                train_y.push(item.class_idx);
            }
            "test" => {
                test_x.push(item.row.clone());
                test_y.push(item.class_idx);
            }
            _ => {}
        }
    }

    if train_x.is_empty() || test_x.is_empty() {
        train_x.clear();
        train_y.clear();
        test_x.clear();
        test_y.clear();

        for item in &rows {
            let u = split_u01(&item.sample_id);
            if u < 0.1 {
                test_x.push(item.row.clone());
                test_y.push(item.class_idx);
            } else {
                train_x.push(item.row.clone());
                train_y.push(item.class_idx);
            }
        }

        if test_x.is_empty() {
            if let Some(row) = train_x.pop()
                && let Some(y) = train_y.pop()
            {
                test_x.push(row);
                test_y.push(y);
            }
        } else if train_x.is_empty() {
            if let Some(row) = test_x.pop()
                && let Some(y) = test_y.pop()
            {
                train_x.push(row);
                train_y.push(y);
            }
        }

        if train_x.is_empty() || test_x.is_empty() {
            return Err("Dataset needs both train and test samples".to_string());
        }
    }

    Ok((
        crate::ml::logreg::TrainDataset {
            classes: classes.clone(),
            x: train_x,
            y: train_y,
        },
        crate::ml::logreg::TrainDataset {
            classes,
            x: test_x,
            y: test_y,
        },
    ))
}

fn export_with_confidence_fallback(
    options: &mut crate::dataset::export::ExportOptions,
    job: &ModelTrainingJob,
    tx: &Sender<super::jobs::JobMessage>,
) -> Result<(crate::dataset::export::ExportSummary, f32), String> {
    let candidates = confidence_candidates(job.min_confidence);
    let mut last_summary: Option<crate::dataset::export::ExportSummary> = None;
    let mut last_conf = job.min_confidence;

    for (idx, conf) in candidates.iter().copied().enumerate() {
        options.min_confidence = conf;
        if idx > 0 {
            send_progress(
                tx,
                0,
                4,
                format!("Exporting dataset (min_conf={:.2})…", conf),
            )?;
        }
        let summary = match job.model_kind {
            crate::sample_sources::config::TrainingModelKind::LogRegV1 => {
                crate::dataset::export::export_embedding_dataset_for_sources(
                    options,
                    &job.source_ids,
                )
                .map_err(|err| err.to_string())?
            }
            _ => crate::dataset::export::export_training_dataset_for_sources(
                options,
                &job.source_ids,
            )
            .map_err(|err| err.to_string())?,
        };
        last_conf = conf;
        last_summary = Some(summary.clone());
        if summary.total_exported >= 2 {
            return Ok((summary, conf));
        }
    }

    Ok((
        last_summary.unwrap_or(crate::dataset::export::ExportSummary {
            total_exported: 0,
            total_packs: 0,
            db_path: job.db_path.clone(),
        }),
        last_conf,
    ))
}

fn confidence_candidates(initial: f32) -> Vec<f32> {
    let mut values = vec![initial.clamp(0.0, 1.0), 0.75, 0.6, 0.5, 0.4, 0.3, 0.2, 0.0];
    values.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    values.dedup_by(|a, b| (*a - *b).abs() < 0.0001);
    values
}

fn training_diagnostics_hint(
    db_path: &PathBuf,
    source_ids: &[String],
    min_confidence: f32,
    include_user_labels: bool,
    model_kind: crate::sample_sources::config::TrainingModelKind,
) -> Result<String, String> {
    let conn = super::analysis_jobs::open_library_db(db_path)?;
    let diag = training_diagnostics_for_sources(
        &conn,
        source_ids,
        min_confidence,
        include_user_labels,
        &model_kind,
    )?;
    let vector_label = match model_kind {
        crate::sample_sources::config::TrainingModelKind::LogRegV1 => "Embeddings",
        _ => "Features(v1)",
    };
    let user_hint = if include_user_labels {
        "User-labeled"
    } else {
        "User-labeled (ignored)"
    };
    Ok(format!(
        "Samples: {}. {}: {}. {}: {}. Name-labeled(conf>={:.2}): {}. Tip: assign categories to a few samples (dropdown) or lower the weak-label threshold.",
        diag.samples_total,
        vector_label,
        diag.features_v1,
        user_hint,
        diag.user_join,
        min_confidence,
        diag.weak_join
    ))
}

struct TrainingDiagnostics {
    samples_total: i64,
    features_v1: i64,
    user_join: i64,
    weak_join: i64,
}

struct TrainingPredictionStats {
    total: i64,
    unknown: i64,
    min_confidence: Option<f32>,
    avg_confidence: Option<f32>,
    max_confidence: Option<f32>,
}

fn training_diagnostics_for_sources(
    conn: &rusqlite::Connection,
    source_ids: &[String],
    min_confidence: f32,
    include_user_labels: bool,
    model_kind: &crate::sample_sources::config::TrainingModelKind,
) -> Result<TrainingDiagnostics, String> {
    let ruleset_version = crate::labeling::weak::WEAK_LABEL_RULESET_VERSION;
    let (where_sql_samples, params_samples) = source_id_where_clause("s", source_ids);
    let (where_sql_vectors, params_vectors) = if matches!(
        model_kind,
        crate::sample_sources::config::TrainingModelKind::LogRegV1
    ) {
        source_id_where_clause("e", source_ids)
    } else {
        source_id_where_clause("f", source_ids)
    };

    let samples_sql = format!(
        "SELECT COUNT(*)
         FROM samples s
         WHERE ({})",
        where_sql_samples
    );
    let samples_total: i64 = conn
        .query_row(
            &samples_sql,
            rusqlite::params_from_iter(params_samples),
            |row| row.get(0),
        )
        .map_err(|err| err.to_string())?;

    let features_v1 = if matches!(
        model_kind,
        crate::sample_sources::config::TrainingModelKind::LogRegV1
    ) {
        let sql = format!(
            "SELECT COUNT(*)
             FROM embeddings e
             WHERE e.model_id = ?1 AND ({})",
            where_sql_vectors
        );
        let mut params_vec = vec![rusqlite::types::Value::Text(
            crate::analysis::embedding::EMBEDDING_MODEL_ID.to_string(),
        )];
        params_vec.extend(params_vectors.iter().cloned());
        conn.query_row(&sql, rusqlite::params_from_iter(params_vec), |row| row.get(0))
            .map_err(|err| err.to_string())?
    } else {
        let sql = format!(
            "SELECT COUNT(*)
             FROM features f
             WHERE f.feat_version = 1 AND ({})",
            where_sql_vectors
        );
        conn.query_row(&sql, rusqlite::params_from_iter(params_vectors.clone()), |row| row.get(0))
            .map_err(|err| err.to_string())?
    };

    let user_join = if include_user_labels {
        if matches!(
            model_kind,
            crate::sample_sources::config::TrainingModelKind::LogRegV1
        ) {
            let sql = format!(
                "SELECT COUNT(*)
                 FROM embeddings e
                 JOIN labels_user u ON u.sample_id = e.sample_id
                 WHERE e.model_id = ?1 AND ({})",
                where_sql_vectors
            );
            let mut params_vec = vec![rusqlite::types::Value::Text(
                crate::analysis::embedding::EMBEDDING_MODEL_ID.to_string(),
            )];
            params_vec.extend(params_vectors.iter().cloned());
            conn.query_row(&sql, rusqlite::params_from_iter(params_vec), |row| row.get(0))
                .map_err(|err| err.to_string())?
        } else {
            let sql = format!(
                "SELECT COUNT(*)
                 FROM features f
                 JOIN labels_user u ON u.sample_id = f.sample_id
                 WHERE f.feat_version = 1 AND ({})",
                where_sql_vectors
            );
            conn.query_row(&sql, rusqlite::params_from_iter(params_vectors.clone()), |row| {
                row.get(0)
            })
            .map_err(|err| err.to_string())?
        }
    } else {
        0
    };

    let weak_join = if matches!(
        model_kind,
        crate::sample_sources::config::TrainingModelKind::LogRegV1
    ) {
        let sql = format!(
            "SELECT COUNT(*)
             FROM embeddings e
             JOIN labels_weak w ON w.sample_id = e.sample_id
             WHERE e.model_id = ?1
               AND w.ruleset_version = ?2
               AND w.confidence >= ?3
               AND ({})",
            where_sql_vectors
        );
        let mut params_vec = vec![
            rusqlite::types::Value::Text(crate::analysis::embedding::EMBEDDING_MODEL_ID.to_string()),
            rusqlite::types::Value::Integer(ruleset_version),
            rusqlite::types::Value::Real(min_confidence as f64),
        ];
        params_vec.extend(params_vectors.iter().cloned());
        conn.query_row(&sql, rusqlite::params_from_iter(params_vec), |row| row.get(0))
            .map_err(|err| err.to_string())?
    } else {
        let sql = format!(
            "SELECT COUNT(*)
             FROM features f
             JOIN labels_weak w ON w.sample_id = f.sample_id
             WHERE f.feat_version = 1
               AND w.ruleset_version = ?1
               AND w.confidence >= ?2
               AND ({})",
            where_sql_vectors
        );
        let mut params_vec = vec![
            rusqlite::types::Value::Integer(ruleset_version),
            rusqlite::types::Value::Real(min_confidence as f64),
        ];
        params_vec.extend(params_vectors.iter().cloned());
        conn.query_row(&sql, rusqlite::params_from_iter(params_vec), |row| row.get(0))
            .map_err(|err| err.to_string())?
    };

    Ok(TrainingDiagnostics {
        samples_total,
        features_v1,
        user_join,
        weak_join,
    })
}

fn source_id_where_clause(
    alias: &str,
    source_ids: &[String],
) -> (String, Vec<rusqlite::types::Value>) {
    if source_ids.is_empty() {
        return ("1=0".to_string(), Vec::new());
    }
    let mut params = Vec::new();
    let mut parts = Vec::new();
    for source_id in source_ids {
        params.push(rusqlite::types::Value::Text(format!("{source_id}::%")));
        parts.push(format!("{alias}.sample_id LIKE ?{}", params.len()));
    }
    (parts.join(" OR "), params)
}

fn training_exportable_count(
    conn: &rusqlite::Connection,
    source_ids: &[String],
    min_confidence: f32,
    include_user_labels: bool,
    model_kind: crate::sample_sources::config::TrainingModelKind,
) -> Result<i64, String> {
    if source_ids.is_empty() {
        return Ok(0);
    }
    let ruleset_version = crate::labeling::weak::WEAK_LABEL_RULESET_VERSION;
    let (table, extra_where, model_param) = if matches!(
        model_kind,
        crate::sample_sources::config::TrainingModelKind::LogRegV1
    ) {
        (
            "embeddings",
            "t.model_id = ?3",
            Some(crate::analysis::embedding::EMBEDDING_MODEL_ID),
        )
    } else {
        ("features", "t.feat_version = ?3", None)
    };
    let mut params: Vec<rusqlite::types::Value> = vec![
        rusqlite::types::Value::Integer(ruleset_version),
        rusqlite::types::Value::Real(min_confidence as f64),
    ];
    if let Some(model_id) = model_param {
        params.push(rusqlite::types::Value::Text(model_id.to_string()));
    } else {
        params.push(rusqlite::types::Value::Integer(crate::analysis::FEATURE_VERSION_V1));
    }
    let mut where_parts = Vec::new();
    for source_id in source_ids {
        where_parts.push(format!("t.sample_id LIKE ?{}", params.len() + 1));
        params.push(rusqlite::types::Value::Text(format!("{source_id}::%")));
    }
    let where_sql = where_parts.join(" OR ");
    let join_user = if include_user_labels {
        "LEFT JOIN labels_user u ON u.sample_id = t.sample_id"
    } else {
        ""
    };
    let user_filter = if include_user_labels {
        "(u.class_id IS NOT NULL OR w.class_id IS NOT NULL)"
    } else {
        "w.class_id IS NOT NULL"
    };
    let sql = format!(
        "WITH best_weak AS (
            SELECT l.sample_id, l.class_id, l.confidence
            FROM labels_weak l
            WHERE l.ruleset_version = ?1
              AND l.confidence >= ?2
              AND l.class_id = (
                SELECT l2.class_id
                FROM labels_weak l2
                WHERE l2.sample_id = l.sample_id
                  AND l2.ruleset_version = ?1
                  AND l2.confidence >= ?2
                ORDER BY l2.confidence DESC, l2.class_id ASC
                LIMIT 1
              )
        )
        SELECT COUNT(*)
        FROM {table} t
        {join_user}
        LEFT JOIN best_weak w ON w.sample_id = t.sample_id
        WHERE {extra_where}
          AND {user_filter}
          AND ({where_sql})"
    );
    conn.query_row(&sql, rusqlite::params_from_iter(params), |row| row.get(0))
        .map_err(|err| err.to_string())
}

fn training_prediction_stats(
    conn: &rusqlite::Connection,
    source_ids: &[String],
) -> Result<Option<TrainingPredictionStats>, String> {
    if source_ids.is_empty() {
        return Ok(None);
    }
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
        .map_err(|err| err.to_string())?;
    let Some(model_id) = latest_model_id else {
        return Ok(None);
    };
    let mut params = vec![rusqlite::types::Value::Text(model_id)];
    let mut parts = Vec::new();
    for source_id in source_ids {
        params.push(rusqlite::types::Value::Text(format!("{source_id}::%")));
        parts.push(format!("p.sample_id LIKE ?{}", params.len()));
    }
    let where_sql = parts.join(" OR ");
    let sql = format!(
        "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN top_class = 'UNKNOWN' THEN 1 ELSE 0 END), 0),
                MIN(confidence),
                AVG(confidence),
                MAX(confidence)
         FROM predictions p
         WHERE p.model_id = ?1
           AND ({where_sql})"
    );
    let (total, unknown, min_conf, avg_conf, max_conf): (
        i64,
        i64,
        Option<f64>,
        Option<f64>,
        Option<f64>,
    ) = conn
        .query_row(&sql, rusqlite::params_from_iter(params), |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
        })
        .map_err(|err| err.to_string())?;
    Ok(Some(TrainingPredictionStats {
        total,
        unknown,
        min_confidence: min_conf.map(|v| v as f32),
        avg_confidence: avg_conf.map(|v| v as f32),
        max_confidence: max_conf.map(|v| v as f32),
    }))
}

fn delete_predictions_for_sources(
    conn: &rusqlite::Connection,
    model_id: &str,
    source_ids: &[String],
) -> Result<usize, String> {
    if source_ids.is_empty() {
        return Ok(0);
    }
    let mut params: Vec<rusqlite::types::Value> =
        vec![rusqlite::types::Value::Text(model_id.to_string())];
    let mut parts = Vec::new();
    for source_id in source_ids {
        params.push(rusqlite::types::Value::Text(format!("{source_id}::%")));
        parts.push(format!("sample_id LIKE ?{}", params.len()));
    }
    let where_sql = parts.join(" OR ");
    let sql = format!(
        "DELETE FROM predictions
         WHERE model_id = ?1
           AND ({where_sql})"
    );
    conn.execute(&sql, rusqlite::params_from_iter(params))
        .map_err(|err| err.to_string())
}

fn split_u01(sample_id: &str) -> f64 {
    let hash = blake3::hash(format!("sempal-train-test-v1|{sample_id}").as_bytes());
    let bytes = hash.as_bytes();
    let u = u64::from_le_bytes(bytes[0..8].try_into().expect("slice size verified"));
    (u as f64) / (u64::MAX as f64)
}

fn evaluate_accuracy(
    model: &crate::ml::gbdt_stump::GbdtStumpModel,
    dataset: &crate::ml::gbdt_stump::TrainDataset,
) -> f32 {
    let mut cm = crate::ml::metrics::ConfusionMatrix::new(model.classes.len());
    for (row, &truth) in dataset.x.iter().zip(dataset.y.iter()) {
        let predicted = model.predict_class_index(row);
        cm.add(truth, predicted);
    }
    crate::ml::metrics::accuracy(&cm)
}

fn evaluate_mlp_accuracy(
    model: &crate::ml::mlp::MlpModel,
    dataset: &crate::ml::gbdt_stump::TrainDataset,
) -> f32 {
    let mut cm = crate::ml::metrics::ConfusionMatrix::new(model.classes.len());
    for (row, &truth) in dataset.x.iter().zip(dataset.y.iter()) {
        let predicted = model.predict_class_index(row);
        cm.add(truth, predicted);
    }
    crate::ml::metrics::accuracy(&cm)
}

fn evaluate_logreg_accuracy(
    model: &crate::ml::logreg::LogRegModel,
    dataset: &crate::ml::logreg::TrainDataset,
) -> f32 {
    let mut cm = crate::ml::metrics::ConfusionMatrix::new(model.classes.len());
    for (row, &truth) in dataset.x.iter().zip(dataset.y.iter()) {
        let predicted = model.predict_class_index(row);
        cm.add(truth, predicted);
    }
    crate::ml::metrics::accuracy(&cm)
}

#[cfg(test)]
mod tests {
    use super::split_train_test;
    use crate::dataset::loader::{
        LoadedDataset, Manifest, ManifestFiles, SampleFeatures, SampleLabel, SampleRecord,
    };

    fn minimal_loaded_dataset(split_a: &str, split_b: &str) -> LoadedDataset {
        let len = crate::analysis::FEATURE_VECTOR_LEN_V1;
        LoadedDataset {
            manifest: Manifest {
                format_version: 1,
                feat_version: crate::analysis::FEATURE_VERSION_V1,
                feature_len_f32: len,
                files: ManifestFiles {
                    samples: "samples.jsonl".to_string(),
                    features: "features.f32le".to_string(),
                },
            },
            samples: vec![
                SampleRecord {
                    sample_id: "s::a.wav".to_string(),
                    pack_id: "s/Pack".to_string(),
                    split: split_a.to_string(),
                    label: SampleLabel {
                        class_id: "kick".to_string(),
                        confidence: 1.0,
                        rule_id: "x".to_string(),
                        ruleset_version: 1,
                    },
                    features: SampleFeatures {
                        feat_version: crate::analysis::FEATURE_VERSION_V1,
                        offset_bytes: 0,
                        len_f32: len,
                        encoding: "f32le".to_string(),
                    },
                },
                SampleRecord {
                    sample_id: "s::b.wav".to_string(),
                    pack_id: "s/Pack".to_string(),
                    split: split_b.to_string(),
                    label: SampleLabel {
                        class_id: "snare".to_string(),
                        confidence: 1.0,
                        rule_id: "y".to_string(),
                        ruleset_version: 1,
                    },
                    features: SampleFeatures {
                        feat_version: crate::analysis::FEATURE_VERSION_V1,
                        offset_bytes: (len * 4) as u64,
                        len_f32: len,
                        encoding: "f32le".to_string(),
                    },
                },
            ],
            features_f32: (0..(len * 2)).map(|idx| idx as f32).collect(),
        }
    }

    #[test]
    fn split_train_test_accepts_pack_split() {
        let loaded = minimal_loaded_dataset("train", "test");
        let (train, test) = split_train_test(&loaded).unwrap();
        assert!(!train.x.is_empty());
        assert!(!test.x.is_empty());
    }

    #[test]
    fn split_train_test_falls_back_when_test_empty() {
        let loaded = minimal_loaded_dataset("train", "train");
        let (train, test) = split_train_test(&loaded).unwrap();
        assert!(!train.x.is_empty());
        assert!(!test.x.is_empty());
    }
}
