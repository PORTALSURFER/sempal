use std::path::PathBuf;

use super::super::Rating;
use super::super::util::parse_relative_path_from_db;
use super::entry::{FileOpJournalEntry, FileOpKind, FileOpStage};

/// Result of decoding journal rows, partitioned by valid and malformed entries.
#[derive(Debug, Default)]
pub struct ListedJournalEntries {
    pub entries: Vec<FileOpJournalEntry>,
    pub malformed: Vec<MalformedJournalEntry>,
}

/// Description of one malformed journal row that cannot be reconciled safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalformedJournalEntry {
    pub id: Option<String>,
    pub detail: String,
}

impl MalformedJournalEntry {
    fn new(id: Option<String>, detail: impl Into<String>) -> Self {
        Self {
            id,
            detail: detail.into(),
        }
    }

    pub(super) fn describe(&self) -> String {
        match self.id.as_deref() {
            Some(id) => format!("Malformed file-ops journal entry {id}: {}", self.detail),
            None => format!("Malformed file-ops journal entry: {}", self.detail),
        }
    }
}

pub(super) fn decode_journal_row(
    row: &rusqlite::Row<'_>,
) -> Result<FileOpJournalEntry, MalformedJournalEntry> {
    let id: String = row
        .get(0)
        .map_err(|err| malformed_column_error(None, "id", err))?;
    let op_type: String = row
        .get(1)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "op_type", err))?;
    let stage_text: String = row
        .get(2)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "stage", err))?;
    let kind = FileOpKind::from_str(op_type.as_str()).ok_or_else(|| {
        MalformedJournalEntry::new(
            Some(id.clone()),
            format!("unknown op_type value `{op_type}`"),
        )
    })?;
    let stage = FileOpStage::from_str(stage_text.as_str()).ok_or_else(|| {
        MalformedJournalEntry::new(
            Some(id.clone()),
            format!("unknown stage value `{stage_text}`"),
        )
    })?;
    let source_root: Option<String> = row
        .get(3)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "source_root", err))?;
    let source_relative_raw: Option<String> = row
        .get(4)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "source_relative", err))?;
    let target_relative_raw: String = row
        .get(5)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "target_relative", err))?;
    let staged_relative_raw: Option<String> = row
        .get(6)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "staged_relative", err))?;
    let file_size = row
        .get::<_, Option<i64>>(7)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "file_size", err))?
        .map(|size| {
            if size < 0 {
                Err(MalformedJournalEntry::new(
                    Some(id.clone()),
                    format!("file_size must be non-negative, got {size}"),
                ))
            } else {
                Ok(size as u64)
            }
        })
        .transpose()?;
    let modified_ns: Option<i64> = row
        .get(8)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "modified_ns", err))?;
    let file_identity: Option<String> = row
        .get(15)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "file_identity", err))?;
    let target_root_identity: Option<String> = row
        .get(16)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "target_root_identity", err))?;
    let source_root_identity: Option<String> = row
        .get(17)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "source_root_identity", err))?;
    let tag = row
        .get::<_, Option<i64>>(9)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "tag", err))?
        .map(Rating::from_i64);
    let looped = row
        .get::<_, Option<i64>>(10)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "looped", err))?
        .map(|flag| flag != 0);
    let locked = row
        .get::<_, Option<i64>>(11)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "locked", err))?
        .map(|flag| flag != 0);
    let last_played_at = row
        .get(12)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "last_played_at", err))?;
    let last_curated_at = row
        .get(13)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "last_curated_at", err))?;
    let created_at = row
        .get(14)
        .map_err(|err| malformed_column_error(Some(id.as_str()), "created_at", err))?;
    Ok(FileOpJournalEntry {
        id: id.clone(),
        kind,
        stage,
        source_root: source_root.map(PathBuf::from),
        source_relative: parse_optional_path(&id, "source_relative", source_relative_raw)?,
        target_relative: parse_required_path(&id, "target_relative", target_relative_raw)?,
        staged_relative: parse_optional_path(&id, "staged_relative", staged_relative_raw)?,
        file_size,
        modified_ns,
        file_identity,
        target_root_identity,
        source_root_identity,
        tag,
        looped,
        locked,
        last_played_at,
        last_curated_at,
        created_at,
    })
}

fn malformed_column_error(
    id: Option<&str>,
    column: &str,
    error: rusqlite::Error,
) -> MalformedJournalEntry {
    MalformedJournalEntry::new(
        id.map(str::to_string),
        format!("invalid `{column}` column: {error}"),
    )
}

fn parse_optional_path(
    id: &str,
    column: &str,
    value: Option<String>,
) -> Result<Option<PathBuf>, MalformedJournalEntry> {
    value
        .map(|path| {
            parse_relative_path_from_db(&path).map_err(|error| {
                MalformedJournalEntry::new(
                    Some(id.to_string()),
                    format!("invalid `{column}` path `{path}`: {error}"),
                )
            })
        })
        .transpose()
}

fn parse_required_path(
    id: &str,
    column: &str,
    value: String,
) -> Result<PathBuf, MalformedJournalEntry> {
    parse_relative_path_from_db(&value).map_err(|error| {
        MalformedJournalEntry::new(
            Some(id.to_string()),
            format!("invalid `{column}` path `{value}`: {error}"),
        )
    })
}
