use super::enqueue_helpers::now_epoch_seconds;
use super::{invalidate, persist, scan};
use crate::egui_app::controller::analysis_jobs::db;
use crate::egui_app::controller::analysis_jobs::types::AnalysisProgress;
use rusqlite::params;
use tracing::info;

struct EnqueueSamplesRequest<'a> {
    source: &'a crate::sample_sources::SampleSource,
    changed_samples: &'a [crate::sample_sources::scanner::ChangedSample],
}

pub(in crate::egui_app::controller) fn enqueue_jobs_for_source(
    source: &crate::sample_sources::SampleSource,
    changed_samples: &[crate::sample_sources::scanner::ChangedSample],
) -> Result<(usize, AnalysisProgress), String> {
    let request = EnqueueSamplesRequest {
        source,
        changed_samples,
    };
    enqueue_samples(request)
}

fn enqueue_samples(
    request: EnqueueSamplesRequest<'_>,
) -> Result<(usize, AnalysisProgress), String> {
    if request.changed_samples.is_empty() {
        let conn = db::open_source_db(&request.source.root)?;
        info!(
            "Analysis enqueue skipped: no changed samples (source_id={})",
            request.source.id.as_str()
        );
        return Ok((0, db::current_progress(&conn)?));
    }

    let sample_metadata =
        scan::sample_metadata_for_changed_samples(request.source, request.changed_samples);
    let mut conn = db::open_source_db(&request.source.root)?;
    let sample_ids: Vec<String> = sample_metadata
        .iter()
        .map(|sample| sample.sample_id.clone())
        .collect();
    let current_version = crate::analysis::version::analysis_version();
    let existing_states = db::sample_analysis_states(&conn, &sample_ids)?;
    let (invalidate, jobs) = invalidate::collect_changed_sample_updates(
        &sample_metadata,
        &existing_states,
        current_version,
    );

    let created_at = now_epoch_seconds();
    persist::write_changed_samples(
        &mut conn,
        &sample_metadata,
        &invalidate,
        &jobs,
        request.source.id.as_str(),
        created_at,
    )
}

struct EnqueueSourceRequest<'a> {
    source: &'a crate::sample_sources::SampleSource,
}

pub(in crate::egui_app::controller) fn enqueue_jobs_for_source_backfill(
    source: &crate::sample_sources::SampleSource,
) -> Result<(usize, AnalysisProgress), String> {
    let request = EnqueueSourceRequest { source };
    enqueue_source_backfill(request, false)
}

pub(in crate::egui_app::controller) fn enqueue_jobs_for_source_backfill_full(
    source: &crate::sample_sources::SampleSource,
) -> Result<(usize, AnalysisProgress), String> {
    let request = EnqueueSourceRequest { source };
    enqueue_source_backfill(request, true)
}

fn enqueue_source_backfill(
    request: EnqueueSourceRequest<'_>,
    force_full: bool,
) -> Result<(usize, AnalysisProgress), String> {
    let mut conn = db::open_source_db(&request.source.root)?;
    let existing_jobs_total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM analysis_jobs WHERE source_id = ?1",
            params![request.source.id.as_str()],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if existing_jobs_total > 0 {
        let active_jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM analysis_jobs
                 WHERE source_id = ?1 AND status IN ('pending','running')",
                params![request.source.id.as_str()],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if active_jobs > 0 {
            info!(
                "Analysis backfill skipped: active jobs exist (active={}, total={}, source_id={}, force_full={})",
                active_jobs,
                existing_jobs_total,
                request.source.id.as_str(),
                force_full
            );
            return Ok((0, db::current_progress(&conn)?));
        }
    }
    let staged_samples = scan::stage_samples_for_source(request.source, true)?;
    if staged_samples.is_empty() {
        info!(
            "Analysis backfill skipped: no staged samples (source_id={}, force_full={})",
            request.source.id.as_str(),
            force_full
        );
        return Ok((0, db::current_progress(&conn)?));
    }
    enqueue_from_staged_samples(
        &mut conn,
        staged_samples,
        db::ANALYZE_SAMPLE_JOB_TYPE,
        force_full,
        false,
        request.source.id.as_str(),
    )
}

struct EnqueueMissingFeaturesRequest<'a> {
    source: &'a crate::sample_sources::SampleSource,
}

pub(in crate::egui_app::controller) fn enqueue_jobs_for_source_missing_features(
    source: &crate::sample_sources::SampleSource,
) -> Result<(usize, AnalysisProgress), String> {
    let request = EnqueueMissingFeaturesRequest { source };
    enqueue_missing_features(request)
}

fn enqueue_missing_features(
    request: EnqueueMissingFeaturesRequest<'_>,
) -> Result<(usize, AnalysisProgress), String> {
    let mut conn = db::open_source_db(&request.source.root)?;

    let staged_samples = scan::stage_samples_for_source(request.source, false)?;
    if staged_samples.is_empty() {
        return Ok((0, db::current_progress(&conn)?));
    }
    enqueue_from_staged_samples(
        &mut conn,
        staged_samples,
        db::ANALYZE_SAMPLE_JOB_TYPE,
        false,
        true,
        request.source.id.as_str(),
    )
}

fn enqueue_from_staged_samples(
    conn: &mut rusqlite::Connection,
    staged_samples: Vec<db::SampleMetadata>,
    job_type: &str,
    force_full: bool,
    skip_when_no_jobs: bool,
    source_id: &str,
) -> Result<(usize, AnalysisProgress), String> {
    if staged_samples.is_empty() {
        return Ok((0, db::current_progress(conn)?));
    }
    let staged_index: std::collections::HashMap<String, db::SampleMetadata> = staged_samples
        .iter()
        .map(|sample| (sample.sample_id.clone(), sample.clone()))
        .collect();
    persist::stage_backfill_samples(conn, &staged_samples)?;
    let (mut sample_metadata, mut jobs, mut invalidate) =
        invalidate::collect_backfill_updates(conn, job_type, force_full)?;
    let include_failed = force_full;
    let failed_jobs = if include_failed {
        invalidate::fetch_failed_backfill_jobs(conn, job_type, source_id)?
    } else {
        Vec::new()
    };
    let failed_count = invalidate::merge_failed_backfill_jobs(
        &staged_index,
        &mut sample_metadata,
        &mut jobs,
        &mut invalidate,
        &failed_jobs,
    );
    if !invalidate.is_empty() {
        invalidate.sort();
        invalidate.dedup();
    }

    if skip_when_no_jobs && jobs.is_empty() {
        info!(
            "Analysis backfill: no jobs to enqueue (staged={}, failed_requeued={}, source_id={}, job_type={}, force_full={})",
            staged_samples.len(),
            failed_count,
            source_id,
            job_type,
            force_full
        );
        return Ok((0, db::current_progress(conn)?));
    }
    info!(
        "Analysis backfill prepared (staged={}, jobs={}, failed_requeued={}, invalidate={}, source_id={}, job_type={}, force_full={})",
        staged_samples.len(),
        jobs.len(),
        failed_count,
        invalidate.len(),
        source_id,
        job_type,
        force_full
    );
    let created_at = now_epoch_seconds();
    let (inserted, progress) = persist::write_backfill_samples(
        conn,
        &sample_metadata,
        &invalidate,
        &jobs,
        job_type,
        source_id,
        created_at,
    )?;
    info!(
        "Analysis backfill enqueued (inserted={}, staged={}, jobs={}, failed_requeued={}, source_id={}, job_type={})",
        inserted,
        staged_samples.len(),
        jobs.len(),
        failed_count,
        source_id,
        job_type
    );
    Ok((inserted, progress))
}
