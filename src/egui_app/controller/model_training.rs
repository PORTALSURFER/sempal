use super::*;
use rusqlite::params;
use crate::egui_app::state::ProgressTaskKind;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub(super) struct ModelTrainingJob {
    pub(super) db_path: PathBuf,
    pub(super) min_confidence: f32,
    pub(super) pack_depth: usize,
    pub(super) train_options: crate::ml::gbdt_stump::TrainOptions,
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
    send_progress(tx, 0, total_steps, "Exporting dataset…")?;
    let temp = tempfile::tempdir().map_err(|err| err.to_string())?;
    let out_dir = temp.path().join("dataset");
    let options = crate::dataset::export::ExportOptions {
        out_dir,
        db_path: Some(job.db_path.clone()),
        min_confidence: job.min_confidence,
        pack_depth: job.pack_depth,
        seed: "sempal-dataset-v1".to_string(),
        test_fraction: 0.1,
        val_fraction: 0.1,
    };
    let summary =
        crate::dataset::export::export_training_dataset(&options).map_err(|err| err.to_string())?;
    if summary.total_exported == 0 {
        return Err("No samples exported for training (need features and labels)".to_string());
    }

    send_progress(
        tx,
        1,
        total_steps,
        format!("Training model on {} samples…", summary.total_exported),
    )?;
    let loaded = crate::dataset::loader::load_dataset(&options.out_dir)
        .map_err(|err| err.to_string())?;
    let (train, test) = split_train_test(&loaded)?;
    let model = crate::ml::gbdt_stump::train_gbdt_stump(&train, &job.train_options)?;
    let _ = evaluate_accuracy(&model, &test);

    send_progress(tx, 2, total_steps, "Importing model…")?;
    let model_id = import_model_into_db(&job.db_path, &model)?;

    send_progress(tx, 3, total_steps, "Enqueueing inference…")?;
    let (inference_jobs_enqueued, _progress) =
        super::analysis_jobs::enqueue_inference_jobs_for_all_sources()?;

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
            controller.set_status(format!("Resolve library DB failed: {err}"), StatusTone::Error);
            return;
        }
    };
    controller.show_status_progress(
        ProgressTaskKind::ModelTraining,
        "Training model",
        4,
        false,
    );
    let train_options = crate::ml::gbdt_stump::TrainOptions::default();
    controller.runtime.jobs.begin_model_training(ModelTrainingJob {
        db_path,
        min_confidence: 0.75,
        pack_depth: 1,
        train_options,
    });
}

impl EguiController {
    pub fn retrain_model_from_app(&mut self) {
        begin_retrain_from_app(self);
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

fn import_model_into_db(
    db_path: &PathBuf,
    model: &crate::ml::gbdt_stump::GbdtStumpModel,
) -> Result<String, String> {
    let model_json = serde_json::to_string(model).map_err(|err| err.to_string())?;
    let classes_json = serde_json::to_string(&model.classes).map_err(|err| err.to_string())?;
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
            "gbdt_stump_v1",
            model.model_version,
            model.feat_version,
            model.feature_len_f32 as i64,
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
) -> Result<(crate::ml::gbdt_stump::TrainDataset, crate::ml::gbdt_stump::TrainDataset), String> {
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

    let mut train_x = Vec::new();
    let mut train_y = Vec::new();
    let mut test_x = Vec::new();
    let mut test_y = Vec::new();

    for sample in &loaded.samples {
        let Some(row) = loaded.feature_row(sample) else {
            continue;
        };
        let Some(&class_idx) = class_map.get(&sample.label.class_id) else {
            continue;
        };
        match sample.split.as_str() {
            "train" => {
                train_x.push(row.to_vec());
                train_y.push(class_idx);
            }
            "test" => {
                test_x.push(row.to_vec());
                test_y.push(class_idx);
            }
            _ => {}
        }
    }

    if train_x.is_empty() || test_x.is_empty() {
        return Err("Dataset needs both train and test samples".to_string());
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
