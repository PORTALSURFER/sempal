use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use rusqlite::{Connection, params};
use sempal::analysis::{ann_index, embedding};
use sempal::analysis::vector::encode_f32_le_blob;
use std::path::Path;
use tempfile::tempdir;

const SAMPLE_COUNT: usize = 256;
const EXTRA_COUNT: usize = 32;

fn setup_tables(conn: &Connection) {
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
    .expect("create tables");
}

fn seed_embeddings(conn: &Connection, start: usize, count: usize) {
    let dim = embedding::EMBEDDING_DIM;
    for i in start..start + count {
        let vec = unit_vec(dim, i);
        let blob = encode_f32_le_blob(&vec);
        conn.execute(
            "INSERT INTO embeddings (sample_id, model_id, dim, dtype, l2_normed, vec, created_at)
             VALUES (?1, ?2, ?3, 'f32', 1, ?4, 0)",
            params![format!("s{i}"), embedding::EMBEDDING_MODEL_ID, dim as i64, blob],
        )
        .expect("seed embedding");
    }
}

fn unit_vec(dim: usize, idx: usize) -> Vec<f32> {
    let mut vec = vec![0.0; dim];
    vec[idx % dim] = 1.0;
    vec
}

fn set_config_home(path: &Path) {
    unsafe {
        std::env::set_var("SEMPAL_CONFIG_HOME", path);
    }
}

fn bench_ann_index(c: &mut Criterion) {
    let config_dir = tempdir().expect("tempdir");
    set_config_home(config_dir.path());

    c.bench_function("ann_index_full_rebuild", |b| {
        b.iter_batched(
            || {
                let conn = Connection::open_in_memory().expect("open db");
                setup_tables(&conn);
                seed_embeddings(&conn, 0, SAMPLE_COUNT);
                conn
            },
            |conn| {
                ann_index::rebuild_index(&conn).expect("rebuild index");
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("ann_index_incremental_update", |b| {
        b.iter_batched(
            || {
                let conn = Connection::open_in_memory().expect("open db");
                setup_tables(&conn);
                seed_embeddings(&conn, 0, SAMPLE_COUNT);
                conn
            },
            |conn| {
                ann_index::rebuild_index(&conn).expect("rebuild index");
                seed_embeddings(&conn, SAMPLE_COUNT, EXTRA_COUNT);
                for i in SAMPLE_COUNT..SAMPLE_COUNT + EXTRA_COUNT {
                    let vec = unit_vec(embedding::EMBEDDING_DIM, i);
                    ann_index::upsert_embedding(&conn, &format!("s{i}"), &vec)
                        .expect("incremental upsert");
                }
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_ann_index);
criterion_main!(benches);
