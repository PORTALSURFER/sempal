use super::*;
use rusqlite::params;
use crate::egui_app::state::ProgressTaskKind;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub(super) struct ModelTrainingJob {
    pub(super) db_path: PathBuf,
    pub(super) source_ids: Vec<String>,
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
    let summary = crate::dataset::export::export_training_dataset_for_sources(&options, &job.source_ids)
        .map_err(|err| err.to_string())?;
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
        super::analysis_jobs::enqueue_inference_jobs_for_sources(&job.source_ids)?;

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
    let source_ids: Vec<String> = controller
        .library
        .sources
        .iter()
        .map(|source| source.id.as_str().to_string())
        .collect();
    controller.runtime.jobs.begin_model_training(ModelTrainingJob {
        db_path,
        source_ids,
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
        return Err("Dataset needs at least 2 labeled samples".to_string());
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

#[cfg(test)]
mod tests {
    use super::split_train_test;
    use crate::dataset::loader::{LoadedDataset, Manifest, ManifestFiles, SampleFeatures, SampleLabel, SampleRecord};

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
