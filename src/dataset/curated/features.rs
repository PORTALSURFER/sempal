use std::collections::HashMap;
use std::path::PathBuf;

use tracing::warn;

use super::classes::{class_index_map, collect_class_ids};
use super::progress::progress_tick;
use super::samples::TrainingSample;

/// Build GBDT datasets from curated samples and a split map.
pub fn build_feature_dataset_from_samples(
    samples: &[TrainingSample],
    split_map: &HashMap<PathBuf, String>,
) -> Result<
    (
        crate::ml::gbdt_stump::TrainDataset,
        crate::ml::gbdt_stump::TrainDataset,
    ),
    String,
> {
    let mut progress = None;
    build_feature_dataset_from_samples_impl(samples, split_map, &mut progress)
}

/// Build GBDT datasets with progress updates during feature extraction.
pub fn build_feature_dataset_from_samples_with_progress(
    samples: &[TrainingSample],
    split_map: &HashMap<PathBuf, String>,
    mut progress: Option<&mut dyn FnMut(super::TrainingProgress)>,
) -> Result<
    (
        crate::ml::gbdt_stump::TrainDataset,
        crate::ml::gbdt_stump::TrainDataset,
    ),
    String,
> {
    build_feature_dataset_from_samples_impl(samples, split_map, &mut progress)
}

fn build_feature_dataset_from_samples_impl(
    samples: &[TrainingSample],
    split_map: &HashMap<PathBuf, String>,
    progress: &mut Option<&mut dyn FnMut(super::TrainingProgress)>,
) -> Result<
    (
        crate::ml::gbdt_stump::TrainDataset,
        crate::ml::gbdt_stump::TrainDataset,
    ),
    String,
> {
    let classes = collect_class_ids(samples);
    let class_map = class_index_map(&classes);
    let mut train_x = Vec::new();
    let mut train_y = Vec::new();
    let mut test_x = Vec::new();
    let mut test_y = Vec::new();
    let mut skipped = 0usize;
    let mut skipped_errors = Vec::new();

    let total = samples.len();
    let mut processed = 0usize;
    for sample in samples {
        processed += 1;
        let vector = match crate::analysis::compute_feature_vector_v1_for_path(&sample.path) {
            Ok(vector) => vector,
            Err(err) => {
                skipped += 1;
                if skipped_errors.len() < 3 {
                    skipped_errors.push(err);
                }
                progress_tick(progress, "features", processed, total, skipped);
                continue;
            }
        };
        let Some(&class_idx) = class_map.get(&sample.class_id) else {
            progress_tick(progress, "features", processed, total, skipped);
            continue;
        };
        let split = split_map
            .get(&sample.path)
            .map(|s| s.as_str())
            .unwrap_or("train");
        if split == "test" {
            test_x.push(vector);
            test_y.push(class_idx);
        } else {
            train_x.push(vector);
            train_y.push(class_idx);
        }
        progress_tick(progress, "features", processed, total, skipped);
    }

    if skipped > 0 {
        warn!(
            "Skipped {skipped} training samples during feature extraction; first errors: {:?}",
            skipped_errors
        );
    }
    if train_x.is_empty() || test_x.is_empty() {
        let hint = if skipped_errors.is_empty() {
            String::new()
        } else {
            format!(" First errors: {:?}", skipped_errors)
        };
        return Err(format!(
            "Training dataset needs both train and test samples. Skipped {skipped}.{hint}"
        ));
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
