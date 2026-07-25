use super::*;

#[cfg(target_os = "macos")]
pub(super) fn seed_durable_watcher_checkpoint(connection: &mut Connection, source: &SampleSource) {
    let metadata = fs::metadata(&source.root).expect("read source root metadata");
    let root_identity =
        wavecrate_library::filesystem_identity::stable_filesystem_identity(&source.root, &metadata)
            .expect("temporary source root should expose stable identity");
    connection
        .execute(
            "INSERT INTO metadata (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![
                wavecrate_library::sample_sources::db::META_SOURCE_WATCHER_CHECKPOINT,
                serde_json::json!({
                    "root_identity": root_identity,
                    "event_id": 1_u64,
                })
                .to_string(),
            ],
        )
        .expect("persist durable watcher checkpoint");
}

pub(super) fn assert_exact_artifact_coverage(source: &SampleSource, snapshot: &ReadinessSnapshot) {
    let connection = open_connection(source).expect("open exact coverage database");
    let eligible_count = connection
        .query_row(
            "SELECT COUNT(*)
             FROM source_readiness_targets
             WHERE source_id = ?1
               AND scope_kind = 'file'
               AND stage = 'embedding_aspects'
               AND eligibility = 'eligible'",
            [source.id.as_str()],
            |row| row.get::<_, usize>(0),
        )
        .expect("count exact eligible manifest");
    assert!(eligible_count > 0, "liveness fixture must contain audio");
    for table in [
        "samples",
        "features",
        "embeddings",
        "similarity_aspect_descriptors",
        "layout_umap",
        "hdbscan_clusters",
    ] {
        assert_exact_sample_table(&connection, source.id.as_str(), table, eligible_count);
    }
    let ann_count = connection
        .query_row("SELECT count FROM ann_index_meta LIMIT 1", [], |row| {
            row.get::<_, usize>(0)
        })
        .expect("read exact ANN count");
    assert_eq!(ann_count, eligible_count, "ANN membership must be exact");

    let playback_rows = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM source_readiness_targets
                 WHERE source_id = ?1 AND stage = 'playback_summary')
              + (SELECT COUNT(*) FROM source_readiness_artifacts
                 WHERE source_id = ?1 AND stage = 'playback_summary')",
            [source.id.as_str()],
            |row| row.get::<_, usize>(0),
        )
        .expect("count retired playback readiness");
    assert_eq!(
        playback_rows, 0,
        "persistent playback cache residency must not be a source-readiness target"
    );
    assert!(snapshot.is_fully_ready());
}

fn assert_exact_sample_table(
    connection: &Connection,
    source_id: &str,
    table: &str,
    eligible_count: usize,
) {
    let prefix = format!("{source_id}::%");
    let sql = format!(
        "SELECT COUNT(*)
         FROM {table}
         WHERE sample_id LIKE ?1 ESCAPE '\\'"
    );
    let table_count = connection
        .query_row(&sql, [&prefix], |row| row.get::<_, usize>(0))
        .unwrap_or_else(|error| panic!("count {table}: {error}"));
    assert_eq!(
        table_count, eligible_count,
        "{table} must not retain stale source membership"
    );
    let missing_sql = format!(
        "SELECT EXISTS(
            SELECT source_id || '::' || relative_path
            FROM source_readiness_targets
            WHERE source_id = ?1
              AND scope_kind = 'file'
              AND stage = 'embedding_aspects'
              AND eligibility = 'eligible'
            EXCEPT
            SELECT sample_id FROM {table}
            WHERE sample_id LIKE ?2 ESCAPE '\\'
         )"
    );
    let missing = connection
        .query_row(&missing_sql, params![source_id, prefix], |row| {
            row.get::<_, bool>(0)
        })
        .unwrap_or_else(|error| panic!("compare exact {table} membership: {error}"));
    assert!(!missing, "{table} is missing current eligible membership");
}

pub(super) fn write_test_wav(path: &Path, phase_offset: f32) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create test WAV parent");
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: wavecrate_analysis::ANALYSIS_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("create test WAV");
    for index in 0..4_096 {
        let phase = index as f32 / 32.0 + phase_offset;
        writer
            .write_sample((phase.sin() * i16::MAX as f32 * 0.25) as i16)
            .expect("write test WAV sample");
    }
    writer.finalize().expect("finalize test WAV");
}

pub(super) fn seed_profile_manifest(connection: &mut Connection, file_count: usize) {
    let transaction = connection.transaction().expect("start profile seed");
    {
        let mut insert = transaction
            .prepare(
                "INSERT INTO wav_files (
                    path, file_size, modified_ns, file_identity, content_hash, missing,
                    extension
                 ) VALUES (?1, 1024, 1, ?2, ?3, 0, 'wav')",
            )
            .expect("prepare profile insert");
        for index in 0..file_count {
            insert
                .execute(params![
                    format!("profile/sample-{index:05}.wav"),
                    format!("identity-{index:05}"),
                    format!("content-{index:05}"),
                ])
                .expect("insert profile row");
        }
    }
    transaction.commit().expect("commit profile seed");
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ProcessResourceSnapshot {
    pub(super) memory_bytes: u64,
    pub(super) heap_bytes_in_use: u64,
    pub(super) cpu_time_ms: u64,
    pub(super) disk_read_bytes: u64,
    pub(super) disk_written_bytes: u64,
}

pub(super) fn process_resource_snapshot() -> ProcessResourceSnapshot {
    let pid = sysinfo::get_current_pid().expect("resolve liveness profile process");
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    let process = system
        .process(pid)
        .expect("refresh liveness profile process metrics");
    let disk = process.disk_usage();
    ProcessResourceSnapshot {
        memory_bytes: process.memory(),
        heap_bytes_in_use: heap_bytes_in_use(),
        cpu_time_ms: process.accumulated_cpu_time(),
        disk_read_bytes: disk.total_read_bytes,
        disk_written_bytes: disk.total_written_bytes,
    }
}

#[cfg(target_os = "macos")]
fn heap_bytes_in_use() -> u64 {
    #[repr(C)]
    #[derive(Default)]
    struct MallocStatistics {
        blocks_in_use: usize,
        size_in_use: usize,
        max_size_in_use: usize,
        size_allocated: usize,
    }

    unsafe extern "C" {
        fn malloc_default_zone() -> *mut libc::c_void;
        fn malloc_zone_statistics(zone: *mut libc::c_void, statistics: *mut MallocStatistics);
    }

    let mut statistics = MallocStatistics::default();
    // SAFETY: Both functions are stable macOS malloc-zone APIs. The default zone is process-owned,
    // and the out pointer remains valid for the duration of the synchronous call.
    unsafe {
        malloc_zone_statistics(malloc_default_zone(), &mut statistics);
    }
    u64::try_from(statistics.size_in_use).unwrap_or(u64::MAX)
}

#[cfg(not(target_os = "macos"))]
fn heap_bytes_in_use() -> u64 {
    0
}
