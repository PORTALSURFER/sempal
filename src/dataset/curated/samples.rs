use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

/// Represents a labeled audio file under a curated dataset root.
#[derive(Clone, Debug)]
pub struct TrainingSample {
    /// Class label derived from the folder name.
    pub class_id: String,
    /// Absolute path to the audio file.
    pub path: PathBuf,
}

/// Recursively collect training samples from a root folder with class subfolders.
pub fn collect_training_samples(root: &Path) -> Result<Vec<TrainingSample>, String> {
    let mut samples = Vec::new();
    let entries = fs::read_dir(root).map_err(|err| format!("Read training dataset root: {err}"))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("Read training dataset entry: {err}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let class_id = entry
            .file_name()
            .to_string_lossy()
            .trim()
            .to_string();
        if class_id.is_empty() {
            continue;
        }
        let mut files = Vec::new();
        collect_files_recursive(&path, &mut files)?;
        for file in files {
            samples.push(TrainingSample {
                class_id: class_id.clone(),
                path: file,
            });
        }
    }
    Ok(samples)
}

/// Filter samples by class count and remove generic "unknown" style buckets.
pub fn filter_training_samples(
    samples: Vec<TrainingSample>,
    min_class_samples: usize,
) -> Vec<TrainingSample> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for sample in &samples {
        *counts.entry(sample.class_id.clone()).or_default() += 1;
    }
    let min_required = min_class_samples.max(1);
    samples
        .into_iter()
        .filter(|sample| {
            let class_id = sample.class_id.trim().to_ascii_lowercase();
            if matches!(class_id.as_str(), "unknown" | "misc" | "other") {
                return false;
            }
            counts
                .get(&sample.class_id)
                .copied()
                .unwrap_or(0)
                >= min_required
        })
        .collect()
}

/// Build a deterministic per-sample split map using a hash of class + path.
pub fn stratified_split_map(
    samples: &[TrainingSample],
    seed: &str,
    test_fraction: f64,
    val_fraction: f64,
) -> Result<HashMap<PathBuf, String>, String> {
    if test_fraction + val_fraction > 1.0 + f64::EPSILON {
        return Err("Invalid split fractions".to_string());
    }
    let mut by_class: BTreeMap<String, Vec<(u128, PathBuf)>> = BTreeMap::new();
    for sample in samples {
        let hash = blake3::hash(
            format!("{seed}|{}|{}", sample.class_id, sample.path.display()).as_bytes(),
        );
        let key = u128::from_le_bytes(hash.as_bytes()[0..16].try_into().expect("slice size"));
        by_class
            .entry(sample.class_id.clone())
            .or_default()
            .push((key, sample.path.clone()));
    }
    let mut splits = HashMap::new();
    for (_class_id, mut entries) in by_class {
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let n = entries.len();
        if n == 0 {
            continue;
        }
        let mut test_n = ((n as f64) * test_fraction).round() as usize;
        let mut val_n = ((n as f64) * val_fraction).round() as usize;
        if n == 1 {
            test_n = 0;
            val_n = 0;
        } else {
            while test_n + val_n >= n {
                if val_n > 0 {
                    val_n -= 1;
                } else if test_n > 0 {
                    test_n -= 1;
                } else {
                    break;
                }
            }
        }
        for (idx, (_hash, path)) in entries.into_iter().enumerate() {
            let split = if idx < test_n {
                "test"
            } else if idx < test_n + val_n {
                "val"
            } else {
                "train"
            };
            splits.insert(path, split.to_string());
        }
    }
    Ok(splits)
}

fn collect_files_recursive(root: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(root).map_err(|err| format!("Read dir {}: {err}", root.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("Read dir entry: {err}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}
