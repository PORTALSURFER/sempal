use std::path::PathBuf;

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use sempal::sample_sources::{SampleTag, SourceDatabase};
use tempfile::tempdir;

const SAMPLE_COUNT: usize = 1_000;

fn setup_db() -> SourceDatabase {
    let dir = tempdir().expect("tempdir");
    let db = SourceDatabase::open(dir.path()).expect("db open");
    let mut batch = db.write_batch().expect("write batch");
    for i in 0..SAMPLE_COUNT {
        let path = PathBuf::from(format!("{i}.wav"));
        batch.upsert_file(&path, 1, i as i64).expect("seed upsert");
    }
    batch.commit().expect("seed commit");
    db
}

fn tag_updates() -> Vec<(PathBuf, SampleTag)> {
    (0..SAMPLE_COUNT)
        .map(|i| {
            let tag = if i % 2 == 0 {
                SampleTag::Keep
            } else {
                SampleTag::Trash
            };
            (PathBuf::from(format!("{i}.wav")), tag)
        })
        .collect()
}

fn bench_tag_batch(c: &mut Criterion) {
    let db = setup_db();
    let updates = tag_updates();
    c.bench_with_input(
        BenchmarkId::new("tag_batch", SAMPLE_COUNT),
        &updates,
        |b, updates| {
            b.iter(|| {
                db.set_tags_batch(black_box(updates))
                    .expect("set_tags_batch");
            });
        },
    );
}

criterion_group!(benches, bench_tag_batch);
criterion_main!(benches);
