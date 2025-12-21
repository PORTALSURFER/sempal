use std::collections::HashMap;
use std::path::PathBuf;

use tracing::warn;

use crate::sample_sources::config::TrainingAugmentation;

use super::classes::{class_index_map, collect_class_ids};
use super::embeddings::{augment_rng, build_embedding_variants};
use super::samples::TrainingSample;

#[derive(Clone)]
struct TrainingRow {
    path: PathBuf,
    class_idx: usize,
    row: Vec<f32>,
}

/// Build logreg datasets from curated samples and a split map.
pub fn build_logreg_dataset_from_samples(
    samples: &[TrainingSample],
    split_map: &HashMap<PathBuf, String>,
    min_class_samples: usize,
    augmentation: &TrainingAugmentation,
    seed: u64,
) -> Result<
    (
        crate::ml::logreg::TrainDataset,
        crate::ml::logreg::TrainDataset,
        crate::ml::logreg::TrainDataset,
    ),
    String,
> {
    let classes = collect_class_ids(samples);
    let class_map = class_index_map(&classes);
    let mut train_rows = Vec::new();
    let mut val_rows = Vec::new();
    let mut test_rows = Vec::new();
    let mut augment_rng = augment_rng(augmentation, seed);
    let mut skipped = 0usize;
    let mut skipped_errors = Vec::new();

    for sample in samples {
        let decoded = match crate::analysis::audio::decode_for_analysis(&sample.path) {
            Ok(decoded) => decoded,
            Err(err) => {
                skipped += 1;
                if skipped_errors.len() < 3 {
                    skipped_errors.push(err);
                }
                continue;
            }
        };
        let embeddings = match build_embedding_variants(&decoded, augmentation, &mut augment_rng)
        {
            Ok(values) => values,
            Err(err) => {
                skipped += 1;
                if skipped_errors.len() < 3 {
                    skipped_errors.push(err);
                }
                continue;
            }
        };
        let Some(&class_idx) = class_map.get(&sample.class_id) else {
            continue;
        };
        let split = split_map
            .get(&sample.path)
            .map(|s| s.as_str())
            .unwrap_or("train");
        for embedding in embeddings {
            let row = TrainingRow {
                path: sample.path.clone(),
                class_idx,
                row: embedding.embedding,
            };
            match split {
                "test" => test_rows.push(row),
                "val" => val_rows.push(row),
                _ => train_rows.push(row),
            }
        }
    }

    if skipped > 0 {
        warn!(
            "Skipped {skipped} training samples during embedding; first errors: {:?}",
            skipped_errors
        );
    }
    if val_rows.is_empty() {
        let (train_keep, val_fallback) = split_val_fallback(train_rows);
        train_rows = train_keep;
        val_rows = val_fallback;
    }
    ensure_train_val_test(
        &train_rows,
        &val_rows,
        &test_rows,
        skipped,
        min_class_samples,
        &skipped_errors,
    )?;

    let (train_x, train_y) = split_rows(train_rows);
    let (val_x, val_y) = split_rows(val_rows);
    let (test_x, test_y) = split_rows(test_rows);

    Ok((
        crate::ml::logreg::TrainDataset {
            classes: classes.clone(),
            x: train_x,
            y: train_y,
        },
        crate::ml::logreg::TrainDataset {
            classes: classes.clone(),
            x: val_x,
            y: val_y,
        },
        crate::ml::logreg::TrainDataset {
            classes,
            x: test_x,
            y: test_y,
        },
    ))
}

/// Build MLP datasets from curated samples and a split map.
pub fn build_mlp_dataset_from_samples(
    samples: &[TrainingSample],
    split_map: &HashMap<PathBuf, String>,
    use_hybrid: bool,
    min_class_samples: usize,
    augmentation: &TrainingAugmentation,
    seed: u64,
) -> Result<
    (
        crate::ml::gbdt_stump::TrainDataset,
        crate::ml::gbdt_stump::TrainDataset,
        crate::ml::gbdt_stump::TrainDataset,
    ),
    String,
> {
    let classes = collect_class_ids(samples);
    let class_map = class_index_map(&classes);
    let mut train_rows = Vec::new();
    let mut val_rows = Vec::new();
    let mut test_rows = Vec::new();
    let mut skipped = 0usize;
    let mut skipped_errors = Vec::new();
    let mut augment_rng = augment_rng(augmentation, seed);

    for sample in samples {
        let decoded = match crate::analysis::audio::decode_for_analysis(&sample.path) {
            Ok(decoded) => decoded,
            Err(err) => {
                skipped += 1;
                if skipped_errors.len() < 3 {
                    skipped_errors.push(err);
                }
                continue;
            }
        };
        let embeddings = match build_embedding_variants(&decoded, augmentation, &mut augment_rng)
        {
            Ok(values) => values,
            Err(err) => {
                skipped += 1;
                if skipped_errors.len() < 3 {
                    skipped_errors.push(err);
                }
                continue;
            }
        };
        let Some(&class_idx) = class_map.get(&sample.class_id) else {
            continue;
        };
        let split = split_map
            .get(&sample.path)
            .map(|s| s.as_str())
            .unwrap_or("train");
        for embedding in embeddings {
            let row = if use_hybrid {
                let mut combined = embedding.embedding;
                combined.extend_from_slice(&embedding.light_features);
                combined
            } else {
                embedding.embedding
            };
            let labeled = TrainingRow {
                path: sample.path.clone(),
                class_idx,
                row,
            };
            match split {
                "test" => test_rows.push(labeled),
                "val" => val_rows.push(labeled),
                _ => train_rows.push(labeled),
            }
        }
    }

    if skipped > 0 {
        warn!(
            "Skipped {skipped} training samples during embedding; first errors: {:?}",
            skipped_errors
        );
    }
    if val_rows.is_empty() {
        let (train_keep, val_fallback) = split_val_fallback(train_rows);
        train_rows = train_keep;
        val_rows = val_fallback;
    }
    ensure_train_val_test(
        &train_rows,
        &val_rows,
        &test_rows,
        skipped,
        min_class_samples,
        &skipped_errors,
    )?;

    let feature_len_f32 = if use_hybrid {
        crate::analysis::embedding::EMBEDDING_DIM + crate::analysis::LIGHT_DSP_VECTOR_LEN
    } else {
        crate::analysis::embedding::EMBEDDING_DIM
    };
    let (train_x, train_y) = split_rows(train_rows);
    let (val_x, val_y) = split_rows(val_rows);
    let (test_x, test_y) = split_rows(test_rows);

    Ok((
        crate::ml::gbdt_stump::TrainDataset {
            feature_len_f32,
            feat_version: 0,
            classes: classes.clone(),
            x: train_x,
            y: train_y,
        },
        crate::ml::gbdt_stump::TrainDataset {
            feature_len_f32,
            feat_version: 0,
            classes: classes.clone(),
            x: val_x,
            y: val_y,
        },
        crate::ml::gbdt_stump::TrainDataset {
            feature_len_f32,
            feat_version: 0,
            classes,
            x: test_x,
            y: test_y,
        },
    ))
}


fn ensure_train_val_test(
    train_rows: &[TrainingRow],
    val_rows: &[TrainingRow],
    test_rows: &[TrainingRow],
    skipped: usize,
    min_class_samples: usize,
    skipped_errors: &[String],
) -> Result<(), String> {
    if !train_rows.is_empty() && !val_rows.is_empty() && !test_rows.is_empty() {
        return Ok(());
    }
    let hint = if skipped_errors.is_empty() {
        String::new()
    } else {
        format!(" First errors: {:?}", skipped_errors)
    };
    Err(format!(
        "Training dataset needs train/val/test samples. Skipped {skipped}. Min/class={min_class_samples}.{hint}"
    ))
}

fn split_rows(rows: Vec<TrainingRow>) -> (Vec<Vec<f32>>, Vec<usize>) {
    let mut x = Vec::with_capacity(rows.len());
    let mut y = Vec::with_capacity(rows.len());
    for row in rows {
        x.push(row.row);
        y.push(row.class_idx);
    }
    (x, y)
}

fn split_val_fallback(rows: Vec<TrainingRow>) -> (Vec<TrainingRow>, Vec<TrainingRow>) {
    let mut keep_train = Vec::new();
    let mut val_rows = Vec::new();
    for row in rows {
        let key = row.path.to_string_lossy();
        if split_u01(&key) < 0.1 {
            val_rows.push(row);
        } else {
            keep_train.push(row);
        }
    }
    if val_rows.is_empty() {
        if let Some(row) = keep_train.pop() {
            val_rows.push(row);
        }
    }
    (keep_train, val_rows)
}

fn split_u01(value: &str) -> f64 {
    let hash = blake3::hash(format!("sempal-train-test-v1|{value}").as_bytes());
    let bytes = hash.as_bytes();
    let u = u64::from_le_bytes(bytes[0..8].try_into().expect("slice size verified"));
    (u as f64) / (u64::MAX as f64)
}
