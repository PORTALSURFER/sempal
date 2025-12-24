use super::options::BenchOptions;
use super::stats;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rusqlite::{Connection, params};
use sempal::analysis::vector::encode_f32_le_blob;
use sempal::analysis::{ann_index, embedding};
use serde::Serialize;
use tempfile::tempdir;

#[derive(Clone, Debug, Serialize)]
pub(super) struct SimilarityBenchResult {
    pub(super) seeded_rows: usize,
    pub(super) similarity_query: stats::LatencySummary,
}

pub(super) fn run(options: &BenchOptions) -> Result<SimilarityBenchResult, String> {
    let _temp = tempdir().map_err(|err| format!("Tempdir failed: {err}"))?;
    let mut conn =
        Connection::open_in_memory().map_err(|err| format!("Open sqlite failed: {err}"))?;
    conn.execute_batch(
        "PRAGMA journal_mode=OFF;
         PRAGMA synchronous=OFF;
         PRAGMA foreign_keys=ON;
         CREATE TABLE embeddings (
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
    .map_err(|err| format!("Create schema failed: {err}"))?;

    seed_embeddings(&mut conn, options.similarity_rows, options.seed)?;
    ann_index::rebuild_index(&conn)?;
    let target_id = "sample-000000";
    let similarity_query = stats::bench_action(options, || {
        ann_index::find_similar(&conn, target_id, 10)?;
        Ok(())
    })?;

    Ok(SimilarityBenchResult {
        seeded_rows: options.similarity_rows,
        similarity_query,
    })
}

fn seed_embeddings(conn: &mut Connection, rows: usize, seed: u64) -> Result<(), String> {
    let mut rng = StdRng::seed_from_u64(seed);
    let dim = embedding::EMBEDDING_DIM;
    let tx = conn
        .transaction()
        .map_err(|err| format!("Start seed transaction failed: {err}"))?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO embeddings (sample_id, model_id, dim, dtype, l2_normed, vec, created_at)
                 VALUES (?1, ?2, ?3, 'f32', 1, ?4, 0)",
            )
            .map_err(|err| format!("Prepare seed embeddings failed: {err}"))?;
        for i in 0..rows.max(2) {
            let sample_id = format!("sample-{i:06}");
            let mut vec: Vec<f32> = (0..dim).map(|_| rng.random::<f32>() * 2.0 - 1.0).collect();
            let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
            if norm > 0.0 {
                for value in &mut vec {
                    *value /= norm;
                }
            }
            let blob = encode_f32_le_blob(&vec);
            stmt.execute(params![
                sample_id,
                embedding::EMBEDDING_MODEL_ID,
                dim as i64,
                blob
            ])
            .map_err(|err| format!("Seed embeddings failed: {err}"))?;
        }
    }
    tx.commit()
        .map_err(|err| format!("Commit seed transaction failed: {err}"))?;
    Ok(())
}
