use super::model::{GbdtStumpModel, Stump, softmax};

/// Training hyperparameters for stump boosting.
#[derive(Debug, Clone)]
pub struct TrainOptions {
    /// Number of boosting rounds.
    pub rounds: usize,
    /// Learning rate applied per round.
    pub learning_rate: f32,
    /// Number of bins used for split search.
    pub bins: usize,
}

impl Default for TrainOptions {
    fn default() -> Self {
        Self {
            rounds: 100,
            learning_rate: 0.1,
            bins: 32,
        }
    }
}

/// In-memory dataset used for training and evaluation.
#[derive(Debug, Clone)]
pub struct TrainDataset {
    /// Number of `f32` values in each feature vector.
    pub feature_len_f32: usize,
    /// Feature vector version.
    pub feat_version: i64,
    /// Ordered list of class identifiers.
    pub classes: Vec<String>,
    /// Feature matrix, row-major.
    pub x: Vec<Vec<f32>>,
    /// Class indices aligned with `x`.
    pub y: Vec<usize>,
}

/// Train a multi-class stump-GBDT model using softmax gradient boosting.
pub fn train_gbdt_stump(
    dataset: &TrainDataset,
    options: &TrainOptions,
) -> Result<GbdtStumpModel, String> {
    if dataset.x.len() != dataset.y.len() {
        return Err("Mismatched X/Y lengths".to_string());
    }
    if dataset.x.is_empty() {
        return Err("Empty dataset".to_string());
    }
    let n_classes = dataset.classes.len();
    if n_classes < 2 {
        return Err("Need at least 2 classes".to_string());
    }

    let n = dataset.x.len();
    let d = dataset.feature_len_f32;
    let (mins, maxs) = compute_feature_min_max(&dataset.x, d);
    let binned = bin_features(&dataset.x, &mins, &maxs, options.bins);

    let priors = class_priors(&dataset.y, n_classes);
    let init_raw: Vec<f32> = priors
        .iter()
        .map(|&p| (p.max(1e-6)).ln())
        .collect();
    let mut raw = vec![init_raw.clone(); n];

    let mut rounds_out: Vec<Vec<Stump>> = Vec::with_capacity(options.rounds);
    for _round in 0..options.rounds {
        let probs: Vec<Vec<f32>> = raw.iter().map(|r| softmax(r)).collect();
        let residuals = compute_residuals(&dataset.y, &probs, n_classes);

        let mut stumps_for_round = Vec::with_capacity(n_classes);
        for class_idx in 0..n_classes {
            let stump = fit_best_stump_for_class(
                &binned,
                &dataset.x,
                &mins,
                &maxs,
                options.bins,
                &residuals[class_idx],
            );
            for i in 0..n {
                raw[i][class_idx] += options.learning_rate * stump.predict(&dataset.x[i]);
            }
            stumps_for_round.push(stump);
        }
        rounds_out.push(stumps_for_round);
    }

    Ok(GbdtStumpModel {
        model_version: 1,
        feat_version: dataset.feat_version,
        feature_len_f32: dataset.feature_len_f32,
        classes: dataset.classes.clone(),
        learning_rate: options.learning_rate,
        init_raw,
        stumps: rounds_out,
    })
}

fn class_priors(y: &[usize], n_classes: usize) -> Vec<f32> {
    let mut counts = vec![0usize; n_classes];
    for &label in y {
        if label < n_classes {
            counts[label] += 1;
        }
    }
    let total = y.len().max(1) as f32;
    counts.into_iter().map(|c| c as f32 / total).collect()
}

fn compute_residuals(y: &[usize], probs: &[Vec<f32>], n_classes: usize) -> Vec<Vec<f32>> {
    let n = y.len();
    let mut residuals = vec![vec![0.0f32; n]; n_classes];
    for i in 0..n {
        let yi = y[i];
        for k in 0..n_classes {
            let target = if yi == k { 1.0 } else { 0.0 };
            residuals[k][i] = target - probs[i][k];
        }
    }
    residuals
}

fn compute_feature_min_max(x: &[Vec<f32>], feature_len: usize) -> (Vec<f32>, Vec<f32>) {
    let mut mins = vec![f32::INFINITY; feature_len];
    let mut maxs = vec![f32::NEG_INFINITY; feature_len];
    for row in x {
        for (j, &v) in row.iter().take(feature_len).enumerate() {
            if v.is_finite() {
                mins[j] = mins[j].min(v);
                maxs[j] = maxs[j].max(v);
            }
        }
    }
    for j in 0..feature_len {
        if !mins[j].is_finite() || !maxs[j].is_finite() {
            mins[j] = 0.0;
            maxs[j] = 0.0;
        }
        if mins[j] == maxs[j] {
            maxs[j] = mins[j] + 1.0;
        }
    }
    (mins, maxs)
}

fn bin_features(x: &[Vec<f32>], mins: &[f32], maxs: &[f32], bins: usize) -> Vec<Vec<u8>> {
    let bins = bins.clamp(2, 256) as f32;
    let mut out: Vec<Vec<u8>> = Vec::with_capacity(x.len());
    for row in x {
        let mut binned = Vec::with_capacity(mins.len());
        for (j, &min) in mins.iter().enumerate() {
            let max = maxs[j];
            let v = row.get(j).copied().unwrap_or(0.0);
            let t = if max > min {
                ((v - min) / (max - min)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let b = (t * (bins - 1.0)).round() as u8;
            binned.push(b);
        }
        out.push(binned);
    }
    out
}

fn fit_best_stump_for_class(
    binned: &[Vec<u8>],
    x: &[Vec<f32>],
    mins: &[f32],
    maxs: &[f32],
    bins: usize,
    residuals: &[f32],
) -> Stump {
    let n_features = mins.len();
    let bins = bins.clamp(2, 256);

    let mut best = BestSplit::default();
    for feature_idx in 0..n_features {
        let split = best_split_for_feature(binned, residuals, feature_idx, bins);
        if split.score < best.score {
            best = split;
        }
    }

    let feature_idx = best.feature_index;
    let threshold = threshold_for_bin(mins[feature_idx], maxs[feature_idx], best.split_bin, bins);
    let (left_value, right_value) = leaf_means_for_threshold(x, residuals, feature_idx, threshold);
    Stump {
        feature_index: feature_idx as u16,
        threshold,
        left_value,
        right_value,
    }
}

#[derive(Debug, Clone)]
struct BestSplit {
    score: f64,
    feature_index: usize,
    split_bin: usize,
}

impl Default for BestSplit {
    fn default() -> Self {
        Self {
            score: f64::INFINITY,
            feature_index: 0,
            split_bin: 0,
        }
    }
}

fn best_split_for_feature(
    binned: &[Vec<u8>],
    residuals: &[f32],
    feature_idx: usize,
    bins: usize,
) -> BestSplit {
    let mut counts = vec![0u32; bins];
    let mut sums = vec![0f64; bins];
    let mut sums_sq = vec![0f64; bins];
    for (i, row) in binned.iter().enumerate() {
        let b = row.get(feature_idx).copied().unwrap_or(0) as usize;
        let r = residuals[i] as f64;
        counts[b] += 1;
        sums[b] += r;
        sums_sq[b] += r * r;
    }
    let total_count: u32 = counts.iter().sum();
    if total_count == 0 {
        return BestSplit::default();
    }
    let total_sum: f64 = sums.iter().sum();
    let total_sum_sq: f64 = sums_sq.iter().sum();

    let mut best_score = f64::INFINITY;
    let mut best_bin = 0usize;

    let mut left_count = 0u32;
    let mut left_sum = 0f64;
    let mut left_sum_sq = 0f64;

    for split_bin in 0..(bins - 1) {
        left_count += counts[split_bin];
        left_sum += sums[split_bin];
        left_sum_sq += sums_sq[split_bin];
        let right_count = total_count - left_count;
        if left_count == 0 || right_count == 0 {
            continue;
        }
        let right_sum = total_sum - left_sum;
        let right_sum_sq = total_sum_sq - left_sum_sq;
        let left_sse = left_sum_sq - (left_sum * left_sum) / left_count as f64;
        let right_sse = right_sum_sq - (right_sum * right_sum) / right_count as f64;
        let score = left_sse + right_sse;
        if score < best_score {
            best_score = score;
            best_bin = split_bin;
        }
    }

    BestSplit {
        score: best_score,
        feature_index: feature_idx,
        split_bin: best_bin,
    }
}

fn threshold_for_bin(min: f32, max: f32, split_bin: usize, bins: usize) -> f32 {
    let bins_f = bins as f32;
    let t = ((split_bin + 1) as f32) / bins_f;
    min + t * (max - min)
}

fn leaf_means_for_threshold(
    x: &[Vec<f32>],
    residuals: &[f32],
    feature_idx: usize,
    threshold: f32,
) -> (f32, f32) {
    let mut left_sum = 0.0f32;
    let mut left_count = 0u32;
    let mut right_sum = 0.0f32;
    let mut right_count = 0u32;
    for (i, row) in x.iter().enumerate() {
        let v = row.get(feature_idx).copied().unwrap_or(0.0);
        if v <= threshold {
            left_sum += residuals[i];
            left_count += 1;
        } else {
            right_sum += residuals[i];
            right_count += 1;
        }
    }
    let left_mean = if left_count == 0 {
        0.0
    } else {
        left_sum / left_count as f32
    };
    let right_mean = if right_count == 0 {
        0.0
    } else {
        right_sum / right_count as f32
    };
    (left_mean, right_mean)
}

