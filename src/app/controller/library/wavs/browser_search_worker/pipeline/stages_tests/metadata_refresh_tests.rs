use super::super::stages::{
    ensure_search_cache_ready_for_job, ensure_search_entries_loaded_for_job,
};
use super::super::*;
use super::support::make_search_job;
use crate::sample_sources::{SourceDatabase, SourceId};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn metadata_only_refresh_reuses_cached_paths_and_scores() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).expect("create source root");

    let db = SourceDatabase::open_for_test_fixture_source_write(&root).expect("open source db");
    db.upsert_file(Path::new("drums/kick.wav"), 1, 1)
        .expect("insert kick");
    db.upsert_file(Path::new("drums/snare.wav"), 1, 2)
        .expect("insert snare");

    let job = SearchJob {
        source_id: SourceId::new(),
        source_root: root.clone(),
        ..make_search_job("kick")
    };
    let source_id = job.source_id.as_str().to_string();
    let queue = SearchJobQueue::new();
    queue.send(SearchJob {
        source_id: job.source_id.clone(),
        source_root: job.source_root.clone(),
        ..make_search_job("kick")
    });
    let generation = queue
        .take_blocking()
        .expect("expected queued search job generation")
        .generation;
    let mut cache = SearchWorkerCache::default();

    assert!(ensure_search_cache_ready_for_job(
        &mut cache, &job, &source_id
    ));
    assert!(ensure_search_entries_loaded_for_job(
        &mut cache, &job, &queue, generation
    ));

    let entries = cache.entries.as_ref().expect("entries loaded");
    let initial_path_ptrs: Vec<*const u8> = entries
        .iter()
        .map(|entry| entry.relative_path.as_ptr())
        .collect();
    let initial_label_ptrs: Vec<*const u8> = entries
        .iter()
        .map(|entry| entry.display_label.as_ptr())
        .collect();
    let path_fingerprint = cache.path_fingerprint;
    cache.query_score_cache.push(WorkerQueryScoreCacheEntry {
        scope: WorkerQueryScoreCacheScope {
            source_id: source_id.clone(),
            path_fingerprint,
        },
        query: "kick".to_string(),
        scores: Arc::from([Some(10), None]),
        matched_indices: Arc::from([0]),
    });

    db.set_tag(Path::new("drums/kick.wav"), Rating::KEEP_1)
        .expect("update tag");
    db.set_last_played_at(Path::new("drums/snare.wav"), 42)
        .expect("update last played");

    assert!(ensure_search_cache_ready_for_job(
        &mut cache, &job, &source_id
    ));
    assert!(ensure_search_entries_loaded_for_job(
        &mut cache, &job, &queue, generation
    ));

    let refreshed = cache.entries.as_ref().expect("entries refreshed");
    let refreshed_path_ptrs: Vec<*const u8> = refreshed
        .iter()
        .map(|entry| entry.relative_path.as_ptr())
        .collect();
    let refreshed_label_ptrs: Vec<*const u8> = refreshed
        .iter()
        .map(|entry| entry.display_label.as_ptr())
        .collect();

    assert_eq!(refreshed_path_ptrs, initial_path_ptrs);
    assert_eq!(refreshed_label_ptrs, initial_label_ptrs);
    assert_eq!(cache.path_fingerprint, path_fingerprint);
    assert_eq!(cache.query_score_cache.len(), 1);
    assert_eq!(refreshed[0].tag, Rating::KEEP_1);
    assert_eq!(refreshed[1].last_played_at, Some(42));
}

#[test]
fn metadata_delta_refresh_updates_only_targeted_rows() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).expect("create source root");

    let db = SourceDatabase::open_for_test_fixture_source_write(&root).expect("open source db");
    db.upsert_file(Path::new("drums/kick.wav"), 1, 1)
        .expect("insert kick");
    db.upsert_file(Path::new("drums/snare.wav"), 1, 2)
        .expect("insert snare");

    let base_job = SearchJob {
        source_id: SourceId::new(),
        source_root: root.clone(),
        ..make_search_job("kick")
    };
    let source_id = base_job.source_id.as_str().to_string();
    let queue = SearchJobQueue::new();
    queue.send(SearchJob {
        source_id: base_job.source_id.clone(),
        source_root: base_job.source_root.clone(),
        ..make_search_job("kick")
    });
    let generation = queue
        .take_blocking()
        .expect("expected queued search job generation")
        .generation;
    let mut cache = SearchWorkerCache::default();

    assert!(ensure_search_cache_ready_for_job(
        &mut cache, &base_job, &source_id
    ));
    assert!(ensure_search_entries_loaded_for_job(
        &mut cache, &base_job, &queue, generation
    ));

    db.set_last_played_at(Path::new("drums/snare.wav"), 77)
        .expect("update snare playback age");
    let mut batch = db.write_batch().expect("open tag batch");
    batch
        .replace_tags_for_path(Path::new("drums/snare.wav"), &[String::from("Layer Clap")])
        .expect("update snare normal tags");
    batch.commit().expect("commit snare normal tags");
    let delta_job = SearchJob {
        metadata_delta_paths: vec![PathBuf::from("drums/snare.wav")],
        source_id: base_job.source_id.clone(),
        source_root: base_job.source_root.clone(),
        ..make_search_job("kick")
    };

    assert!(ensure_search_cache_ready_for_job(
        &mut cache, &delta_job, &source_id
    ));
    assert!(ensure_search_entries_loaded_for_job(
        &mut cache, &delta_job, &queue, generation
    ));

    let refreshed = cache.entries.as_ref().expect("entries refreshed");
    assert_eq!(refreshed[0].last_played_at, None);
    assert_eq!(refreshed[1].last_played_at, Some(77));
    assert_eq!(refreshed[1].display_label.as_ref(), "snare Layer Clap");
}

#[test]
fn metadata_delta_refresh_reuses_same_source_db_connection_after_db_write() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).expect("create source root");

    let db = SourceDatabase::open_for_test_fixture_source_write(&root).expect("open source db");
    db.upsert_file(Path::new("drums/kick.wav"), 1, 1)
        .expect("insert kick");
    db.upsert_file(Path::new("drums/snare.wav"), 1, 2)
        .expect("insert snare");

    let base_job = SearchJob {
        source_id: SourceId::new(),
        source_root: root.clone(),
        ..make_search_job("")
    };
    let source_id = base_job.source_id.as_str().to_string();
    let queue = SearchJobQueue::new();
    queue.send(SearchJob {
        source_id: base_job.source_id.clone(),
        source_root: base_job.source_root.clone(),
        ..make_search_job("")
    });
    let generation = queue
        .take_blocking()
        .expect("expected queued search job generation")
        .generation;
    let mut cache = SearchWorkerCache::default();
    let _open_count_scope = crate::sample_sources::db::test_scope_source_db_open_total_count(&root);

    assert!(ensure_search_cache_ready_for_job(
        &mut cache, &base_job, &source_id
    ));
    assert!(ensure_search_entries_loaded_for_job(
        &mut cache, &base_job, &queue, generation
    ));
    assert_eq!(
        crate::sample_sources::db::test_source_db_open_total_count(&root),
        1
    );

    db.set_last_played_at(Path::new("drums/snare.wav"), 77)
        .expect("update snare playback age");
    let delta_job = SearchJob {
        metadata_delta_paths: vec![PathBuf::from("drums/snare.wav")],
        source_id: base_job.source_id.clone(),
        source_root: base_job.source_root.clone(),
        ..make_search_job("")
    };

    assert!(ensure_search_cache_ready_for_job(
        &mut cache, &delta_job, &source_id
    ));
    assert!(ensure_search_entries_loaded_for_job(
        &mut cache, &delta_job, &queue, generation
    ));

    let refreshed = cache.entries.as_ref().expect("entries refreshed");
    assert_eq!(refreshed[1].last_played_at, Some(77));
    assert_eq!(
        crate::sample_sources::db::test_source_db_open_total_count(&root),
        1,
        "same-source metadata deltas should not reopen the source DB after hot playback writes"
    );
}

#[test]
/// A coalesced metadata-delta refresh can cross revisions when it carries every changed path.
fn metadata_delta_revision_gap_refreshes_all_provided_paths() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).expect("create source root");

    let db = SourceDatabase::open_for_test_fixture_source_write(&root).expect("open source db");
    db.upsert_file(Path::new("one.wav"), 1, 1)
        .expect("insert one");
    db.upsert_file(Path::new("two.wav"), 1, 2)
        .expect("insert two");

    let base_job = SearchJob {
        source_id: SourceId::new(),
        source_root: root.clone(),
        ..make_search_job("")
    };
    let source_id = base_job.source_id.as_str().to_string();
    let queue = SearchJobQueue::new();
    queue.send(SearchJob {
        source_id: base_job.source_id.clone(),
        source_root: base_job.source_root.clone(),
        ..make_search_job("")
    });
    let generation = queue
        .take_blocking()
        .expect("expected queued search job generation")
        .generation;
    let mut cache = SearchWorkerCache::default();

    assert!(ensure_search_cache_ready_for_job(
        &mut cache, &base_job, &source_id
    ));
    assert!(ensure_search_entries_loaded_for_job(
        &mut cache, &base_job, &queue, generation
    ));

    db.set_tag(Path::new("one.wav"), Rating::TRASH_3)
        .expect("update first skipped delta");
    db.set_tag(Path::new("two.wav"), Rating::TRASH_3)
        .expect("update second delta");
    let delta_job = SearchJob {
        metadata_delta_paths: vec![PathBuf::from("one.wav"), PathBuf::from("two.wav")],
        source_id: base_job.source_id.clone(),
        source_root: base_job.source_root.clone(),
        ..make_search_job("")
    };

    assert!(ensure_search_cache_ready_for_job(
        &mut cache, &delta_job, &source_id
    ));
    assert!(ensure_search_entries_loaded_for_job(
        &mut cache, &delta_job, &queue, generation
    ));

    let refreshed = cache.entries.as_ref().expect("entries refreshed");
    assert_eq!(refreshed[0].tag, Rating::TRASH_3);
    assert_eq!(refreshed[1].tag, Rating::TRASH_3);
}
