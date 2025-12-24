use crate::analysis::vector::encode_f32_le_blob;
use crate::analysis::{ann_index, embedding};
use crate::app_dirs::ConfigBaseGuard;
use rusqlite::{Connection, params};
use std::sync::{LazyLock, Mutex};
use tempfile::tempdir;

static ANN_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[test]
fn ann_index_matches_bruteforce_neighbors_on_fixture() {
    let _lock = ANN_TEST_LOCK.lock().expect("ann test lock poisoned");
    let temp = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());

    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE embeddings (
            sample_id TEXT PRIMARY KEY,
            model_id TEXT NOT NULL,
            dim INTEGER NOT NULL,
            dtype TEXT NOT NULL,
            l2_normed INTEGER NOT NULL,
            vec BLOB NOT NULL,
            created_at INTEGER NOT NULL
        ) WITHOUT ROWID;
         CREATE TABLE ann_index_meta (
            model_id TEXT PRIMARY KEY,
            index_path TEXT NOT NULL,
            count INTEGER NOT NULL,
            params_json TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        ) WITHOUT ROWID;",
    )
    .unwrap();

    let dim = embedding::EMBEDDING_DIM;
    let samples = vec![
        ("s1", normalize(unit_vec(dim, 0))),
        ("s2", normalize(blend_unit(dim, 0, 1, 0.08))),
        ("s3", normalize(unit_vec(dim, 1))),
        ("s4", normalize(unit_vec(dim, 2))),
    ];
    for (sample_id, vec) in &samples {
        let blob = encode_f32_le_blob(vec);
        conn.execute(
            "INSERT INTO embeddings (sample_id, model_id, dim, dtype, l2_normed, vec, created_at)
             VALUES (?1, ?2, ?3, 'f32', 1, ?4, 0)",
            params![sample_id, embedding::EMBEDDING_MODEL_ID, dim as i64, blob],
        )
        .unwrap();
    }

    ann_index::rebuild_index(&conn).expect("ANN rebuild");
    let results = ann_index::find_similar(&conn, "s1", 2).expect("ANN search");
    let expected = brute_force_neighbors("s1", &samples, 2);
    let result_ids: Vec<_> = results
        .iter()
        .map(|entry| entry.sample_id.as_str())
        .collect();
    assert_eq!(result_ids.first().copied(), expected.first().copied());
    assert_results_within_top_k("s1", &samples, 2, &result_ids);
}

fn unit_vec(dim: usize, idx: usize) -> Vec<f32> {
    let mut vec = vec![0.0; dim];
    if idx < dim {
        vec[idx] = 1.0;
    }
    vec
}

fn blend_unit(dim: usize, a: usize, b: usize, mix: f32) -> Vec<f32> {
    let mut vec = unit_vec(dim, a);
    if b < dim {
        vec[b] = mix;
    }
    vec
}

fn normalize(mut vec: Vec<f32>) -> Vec<f32> {
    let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut vec {
            *value /= norm;
        }
    }
    vec
}

fn brute_force_neighbors<'a>(
    target: &str,
    samples: &'a [(&'a str, Vec<f32>)],
    k: usize,
) -> Vec<&'a str> {
    let target_vec = samples
        .iter()
        .find(|(id, _)| *id == target)
        .expect("target sample")
        .1
        .as_slice();
    let mut scored: Vec<(&'a str, f32)> = samples
        .iter()
        .filter(|(id, _)| *id != target)
        .map(|(id, vec)| (*id, cosine_distance(target_vec, vec)))
        .collect();
    scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    scored.into_iter().take(k).map(|(id, _)| id).collect()
}

fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    let mut dot = 0.0;
    for i in 0..len {
        dot += a[i] * b[i];
    }
    1.0 - dot
}

fn assert_results_within_top_k(
    target: &str,
    samples: &[(&str, Vec<f32>)],
    k: usize,
    result_ids: &[&str],
) {
    let target_vec = samples
        .iter()
        .find(|(id, _)| *id == target)
        .expect("target sample")
        .1
        .as_slice();
    let mut scored: Vec<(&str, f32)> = samples
        .iter()
        .filter(|(id, _)| *id != target)
        .map(|(id, vec)| (*id, cosine_distance(target_vec, vec)))
        .collect();
    scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    let threshold = scored
        .get(k.saturating_sub(1))
        .map(|entry| entry.1)
        .unwrap_or(f32::INFINITY);
    for id in result_ids {
        let distance = scored
            .iter()
            .find(|(entry_id, _)| entry_id == id)
            .map(|entry| entry.1)
            .expect("result id present");
        assert!(
            distance <= threshold + 1e-6,
            "result {id} distance {distance} exceeds threshold {threshold}"
        );
    }
}
