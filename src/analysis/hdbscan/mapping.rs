use rusqlite::{Connection, Transaction, params};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::HdbscanStats;

pub fn assign_all_points_to_clusters(data: &[Vec<f32>], labels: &mut [i32]) {
    if data.is_empty() || labels.is_empty() {
        return;
    }
    if labels.iter().all(|label| *label >= 0) {
        return;
    }
    let mut centroid_sums: HashMap<i32, (Vec<f32>, usize)> = HashMap::new();
    for (idx, label) in labels.iter().enumerate() {
        if *label < 0 {
            continue;
        }
        let point = match data.get(idx) {
            Some(point) => point,
            None => continue,
        };
        let entry = centroid_sums
            .entry(*label)
            .or_insert_with(|| (vec![0.0; point.len()], 0));
        for (dim, value) in point.iter().enumerate() {
            if let Some(sum) = entry.0.get_mut(dim) {
                *sum += *value;
            }
        }
        entry.1 += 1;
    }
    if centroid_sums.is_empty() {
        labels.fill(0);
        return;
    }
    let mut centroids: Vec<(i32, Vec<f32>)> = Vec::with_capacity(centroid_sums.len());
    for (label, (mut sums, count)) in centroid_sums {
        if count == 0 {
            continue;
        }
        let denom = count as f32;
        for value in &mut sums {
            *value /= denom;
        }
        centroids.push((label, sums));
    }
    if centroids.is_empty() {
        labels.fill(0);
        return;
    }
    for (idx, label) in labels.iter_mut().enumerate() {
        if *label >= 0 {
            continue;
        }
        let point = match data.get(idx) {
            Some(point) => point,
            None => continue,
        };
        let mut best: Option<(i32, f32)> = None;
        for (centroid_label, centroid) in &centroids {
            let dist = squared_distance(point, centroid);
            if best.map(|(_, best_dist)| dist < best_dist).unwrap_or(true) {
                best = Some((*centroid_label, dist));
            }
        }
        if let Some((centroid_label, _)) = best {
            *label = centroid_label;
        } else {
            *label = 0;
        }
    }
}

pub fn remap_labels_deterministic(sample_ids: &[String], labels: &mut [i32]) -> Result<(), String> {
    if sample_ids.len() != labels.len() {
        return Err("Cluster label length mismatch".to_string());
    }
    let mut next_label = 0;
    let mut mapping: HashMap<i32, i32> = HashMap::new();
    for (_sample_id, label) in sample_ids.iter().zip(labels.iter_mut()) {
        let canonical = mapping.entry(*label).or_insert_with(|| {
            let assigned = next_label;
            next_label += 1;
            assigned
        });
        *label = *canonical;
    }
    Ok(())
}

fn squared_distance(a: &[f32], b: &[f32]) -> f32 {
    let mut sum = 0.0;
    for (av, bv) in a.iter().zip(b.iter()) {
        sum += (*av - *bv) * (*av - *bv);
    }
    sum
}

pub fn summarize_labels(labels: &[i32]) -> HdbscanStats {
    let mut cluster_counts: HashMap<i32, usize> = HashMap::new();
    let mut noise = 0usize;
    for label in labels {
        if *label < 0 {
            noise += 1;
        } else {
            *cluster_counts.entry(*label).or_insert(0) += 1;
        }
    }
    let total = labels.len().max(1) as f32;
    let (min_cluster_size, max_cluster_size) = min_max_cluster_size(&cluster_counts);
    HdbscanStats {
        cluster_count: cluster_counts.len(),
        noise_count: noise,
        noise_ratio: noise as f32 / total,
        min_cluster_size,
        max_cluster_size,
    }
}

fn min_max_cluster_size(cluster_counts: &HashMap<i32, usize>) -> (usize, usize) {
    if cluster_counts.is_empty() {
        return (0, 0);
    }
    let mut min_size = usize::MAX;
    let mut max_size = 0usize;
    for size in cluster_counts.values() {
        min_size = min_size.min(*size);
        max_size = max_size.max(*size);
    }
    (min_size, max_size)
}

pub fn write_clusters(
    conn: &mut Connection,
    sample_ids: &[String],
    labels: &[i32],
    model_id: &str,
    method: &str,
    umap_version: &str,
) -> Result<(), String> {
    let now = now_epoch_seconds()?;
    let tx = start_cluster_tx(conn)?;
    {
        let mut stmt = prepare_cluster_insert(&tx)?;
        insert_cluster_rows(
            &mut stmt,
            sample_ids,
            labels,
            model_id,
            method,
            umap_version,
            now,
        )?;
    }
    tx.commit()
        .map_err(|err| format!("Commit clusters failed: {err}"))?;
    Ok(())
}

fn start_cluster_tx(conn: &mut Connection) -> Result<Transaction<'_>, String> {
    conn.transaction()
        .map_err(|err| format!("Start transaction failed: {err}"))
}

fn prepare_cluster_insert<'a>(tx: &'a Transaction<'a>) -> Result<rusqlite::Statement<'a>, String> {
    tx.prepare(
        "INSERT INTO hdbscan_clusters (
            sample_id,
            model_id,
            method,
            umap_version,
            cluster_id,
            created_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(sample_id, model_id, method, umap_version) DO UPDATE SET
            cluster_id = excluded.cluster_id,
            created_at = excluded.created_at",
    )
    .map_err(|err| format!("Prepare cluster insert failed: {err}"))
}

fn insert_cluster_rows(
    stmt: &mut rusqlite::Statement<'_>,
    sample_ids: &[String],
    labels: &[i32],
    model_id: &str,
    method: &str,
    umap_version: &str,
    now: i64,
) -> Result<(), String> {
    for (idx, sample_id) in sample_ids.iter().enumerate() {
        let label = labels
            .get(idx)
            .ok_or_else(|| "Cluster label length mismatch".to_string())?;
        stmt.execute(params![
            sample_id,
            model_id,
            method,
            umap_version,
            label,
            now
        ])
        .map_err(|err| format!("Insert cluster failed: {err}"))?;
    }
    Ok(())
}

fn now_epoch_seconds() -> Result<i64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Invalid system time".to_string())
        .map(|time| time.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigns_noise_points_to_nearest_centroid() {
        let data = vec![vec![0.0, 0.0], vec![1.0, 1.0], vec![10.0, 10.0]];
        let mut labels = vec![0, -1, 1];
        assign_all_points_to_clusters(&data, &mut labels);
        assert_eq!(labels.len(), 3);
        assert_eq!(labels[1], 0);
        assert!(labels.iter().all(|label| *label >= 0));
    }

    #[test]
    fn assigns_single_cluster_when_everything_is_noise() {
        let data = vec![vec![0.0], vec![1.0], vec![2.0]];
        let mut labels = vec![-1, -1, -1];
        assign_all_points_to_clusters(&data, &mut labels);
        assert_eq!(labels, vec![0, 0, 0]);
    }

    #[test]
    fn remaps_labels_deterministically() {
        let sample_ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut labels = vec![5, 5, 2];
        remap_labels_deterministic(&sample_ids, &mut labels).unwrap();
        assert_eq!(labels, vec![0, 0, 1]);
    }
}
