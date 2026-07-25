use super::{
    ACTIVE_RECORDING_QUIET_SECONDS, BTreeSet, META_READINESS_DUPLICATE_IDENTITY, ReadinessStore,
    earliest_deadline, params,
};
use rusqlite::OptionalExtension;

const MAX_DUPLICATE_IDENTITY_GROUPS: usize = 8;
const MAX_DUPLICATE_IDENTITY_PATHS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DuplicateIdentityDiagnostic {
    pub(super) identity: String,
    pub(super) paths: Vec<String>,
}

/// Return the durable duplicate-identity diagnostic only while it belongs to this identity
/// revision. A changed identity revision invalidates the old terminal diagnosis automatically.
pub(super) fn duplicate_identity_diagnostic_for_revision(
    connection: &rusqlite::Connection,
    identity_revision: i64,
) -> Result<Option<Vec<DuplicateIdentityDiagnostic>>, String> {
    let Some(raw) = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [META_READINESS_DUPLICATE_IDENTITY],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Ok(None);
    };
    if value
        .get("identity_revision")
        .and_then(serde_json::Value::as_i64)
        != Some(identity_revision)
    {
        return Ok(None);
    }
    let Some(identities) = value
        .get("identities")
        .and_then(serde_json::Value::as_array)
    else {
        return Ok(None);
    };
    let diagnostics = identities
        .iter()
        .filter_map(|identity| {
            Some(DuplicateIdentityDiagnostic {
                identity: identity.get("identity")?.as_str()?.to_string(),
                paths: identity
                    .get("paths")?
                    .as_array()?
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string)
                    .take(MAX_DUPLICATE_IDENTITY_PATHS)
                    .collect(),
            })
        })
        .filter(|diagnostic| !diagnostic.identity.is_empty() && diagnostic.paths.len() > 1)
        .take(MAX_DUPLICATE_IDENTITY_GROUPS)
        .collect::<Vec<_>>();
    Ok((!diagnostics.is_empty()).then_some(diagnostics))
}

/// Find supported live manifest rows that share one stable filesystem identity.
pub(super) fn find_duplicate_identity_diagnostics(
    connection: &rusqlite::Connection,
) -> Result<Vec<DuplicateIdentityDiagnostic>, String> {
    let filter = wavecrate_library::sample_sources::supported_audio_where_clause();
    let mut groups = connection
        .prepare(&format!(
            "SELECT file_identity
             FROM wav_files
             WHERE missing = 0
               AND file_identity IS NOT NULL
               AND TRIM(file_identity) != ''
               AND {filter}
             GROUP BY file_identity
             HAVING COUNT(*) > 1
             ORDER BY file_identity
             LIMIT {MAX_DUPLICATE_IDENTITY_GROUPS}"
        ))
        .map_err(|error| error.to_string())?;
    let identities = groups
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(groups);

    identities
        .into_iter()
        .map(|identity| {
            let mut paths = connection
                .prepare(&format!(
                    "SELECT path FROM wav_files
                     WHERE missing = 0 AND file_identity = ?1 AND {filter}
                     ORDER BY path
                     LIMIT ?2"
                ))
                .map_err(|error| error.to_string())?;
            let paths = paths
                .query_map(params![identity, MAX_DUPLICATE_IDENTITY_PATHS], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            Ok(DuplicateIdentityDiagnostic { identity, paths })
        })
        .collect()
}

pub(super) fn persist_duplicate_identity_diagnostic(
    connection: &rusqlite::Connection,
    identity_revision: i64,
    diagnostics: &[DuplicateIdentityDiagnostic],
) -> Result<(), String> {
    let value = serde_json::json!({
        "identity_revision": identity_revision,
        "identities": diagnostics.iter().map(|diagnostic| serde_json::json!({
            "identity": diagnostic.identity,
            "paths": diagnostic.paths,
        })).collect::<Vec<_>>(),
    });
    connection
        .execute(
            "INSERT INTO metadata (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![META_READINESS_DUPLICATE_IDENTITY, value.to_string()],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) fn clear_duplicate_identity_diagnostic(
    connection: &rusqlite::Connection,
) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM metadata WHERE key = ?1",
            [META_READINESS_DUPLICATE_IDENTITY],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[derive(Debug, Default)]
pub(super) struct ActiveRecordingDeferrals {
    pub(super) scope_ids: BTreeSet<String>,
    pub(super) retry_at: Option<i64>,
}

pub(super) fn active_recording_deferrals(
    connection: &rusqlite::Connection,
    now: i64,
) -> Result<ActiveRecordingDeferrals, String> {
    const NANOS_PER_SECOND: i64 = 1_000_000_000;
    let end_of_current_second_ns = now
        .saturating_add(1)
        .saturating_mul(NANOS_PER_SECOND)
        .saturating_sub(1);
    let cutoff_ns = now
        .saturating_sub(ACTIVE_RECORDING_QUIET_SECONDS)
        .saturating_mul(NANOS_PER_SECOND);
    let mut statement = connection
        .prepare(
            "SELECT file_identity, modified_ns
             FROM wav_files
             WHERE missing = 0
               AND file_identity IS NOT NULL
               AND TRIM(file_identity) != ''
               AND modified_ns BETWEEN ?1 AND ?2",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![cutoff_ns, end_of_current_second_ns], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| error.to_string())?;
    let mut deferrals = ActiveRecordingDeferrals::default();
    for row in rows {
        let (scope_id, modified_ns) = row.map_err(|error| error.to_string())?;
        deferrals.scope_ids.insert(scope_id);
        let modified_second = modified_ns.div_euclid(NANOS_PER_SECOND);
        let stable_at = modified_second
            .saturating_add(ACTIVE_RECORDING_QUIET_SECONDS)
            .saturating_add(1);
        deferrals.retry_at = earliest_deadline(deferrals.retry_at, Some(stable_at));
    }
    Ok(deferrals)
}

pub(super) fn source_processing_schema_available(
    connection: &mut rusqlite::Connection,
) -> Result<bool, String> {
    for (table, required_columns) in [
        (
            "wav_files",
            &[
                "path",
                "file_identity",
                "content_hash",
                "file_size",
                "modified_ns",
                "missing",
            ][..],
        ),
        ("metadata", &["key", "value"][..]),
    ] {
        let pragma = format!("PRAGMA table_info({table})");
        let mut statement = connection
            .prepare(&pragma)
            .map_err(|error| error.to_string())?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|error| error.to_string())?
            .collect::<Result<std::collections::BTreeSet<_>, _>>()
            .map_err(|error| error.to_string())?;
        if required_columns
            .iter()
            .any(|column| !columns.contains(*column))
        {
            return Ok(false);
        }
    }
    ReadinessStore::new(connection)
        .processing_schema_available()
        .map_err(|error| error.to_string())
}

// Compatibility-only migration for rows persisted by versions that discarded the execution
// failure type. Live execution always receives `SourceProcessingFailure` from its owner.
pub(super) fn legacy_unsupported_decode_failure_text(reason: &str) -> bool {
    let reason = reason.to_ascii_lowercase();
    reason.contains("failed to decode audio file:")
        || reason.contains("audio decode failed for")
        || reason.contains("audio file contains no complete frames")
        || reason.contains("unsupported codec")
        || reason.contains("no suitable format reader found")
}
