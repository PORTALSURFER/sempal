use std::path::{Path, PathBuf};
use std::time::Instant;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use super::connection::LibraryDatabase;
use super::error::map_sql_error;
use super::owner::SourceRegistrySnapshot;
use super::telemetry::record_library_db_event;
use super::{KNOWN_SOURCES_KEY, LibraryError, LibraryState};
use crate::sample_sources::normalize_path;
use crate::sample_sources::{
    SampleSource, SourceId, SourceMetadataStorage, SourceRole, default_primary_import_folder,
};

impl LibraryDatabase {
    pub(super) fn load_state(&self) -> Result<LibraryState, LibraryError> {
        let started_at = Instant::now();
        let sources = self.load_sources()?;
        let state = LibraryState { sources };
        record_library_db_event("library.load_state", started_at, Ok(()));
        Ok(state)
    }

    pub(super) fn replace_state(&mut self, state: &LibraryState) -> Result<(), LibraryError> {
        self.replace_source_registry_with_event(
            &SourceRegistrySnapshot::from_sources(state.sources.clone()),
            "library.replace_state",
        )
    }

    pub(super) fn load_source_registry(&self) -> Result<SourceRegistrySnapshot, LibraryError> {
        Ok(SourceRegistrySnapshot::from_sources(self.load_sources()?))
    }

    pub(super) fn replace_source_registry(
        &mut self,
        snapshot: &SourceRegistrySnapshot,
    ) -> Result<(), LibraryError> {
        self.replace_source_registry_with_event(snapshot, "library.replace_source_registry")
    }

    fn replace_source_registry_with_event(
        &mut self,
        snapshot: &SourceRegistrySnapshot,
        event: &'static str,
    ) -> Result<(), LibraryError> {
        let started_at = Instant::now();
        let tx = self.connection.transaction().map_err(map_sql_error)?;
        let mut mappings = Self::load_known_sources_from(&tx)?;
        Self::replace_sources(&tx, snapshot.as_slice())?;
        Self::remember_known_sources_in_tx(&tx, &mut mappings, snapshot.as_slice())?;
        tx.commit().map_err(map_sql_error)?;
        record_library_db_event(event, started_at, Ok(()));
        Ok(())
    }

    pub(super) fn load_sources(&self) -> Result<Vec<SampleSource>, LibraryError> {
        let started_at = Instant::now();
        let mut stmt = self
            .connection
            .prepare(
                "SELECT id, root, role, metadata_storage, primary_import_folder
                 FROM sources
                 ORDER BY sort_order ASC, id ASC",
            )
            .map_err(map_sql_error)?;
        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let root: String = row.get(1)?;
                let role: String = row.get(2)?;
                let metadata_storage: String = row.get(3)?;
                let primary_import_folder: String = row.get(4)?;
                Ok(normalized_source(SampleSource {
                    id: SourceId::from_string(id),
                    root: PathBuf::from(root),
                    role: SourceRole::from_stored(&role),
                    metadata_storage: SourceMetadataStorage::from_stored(&metadata_storage),
                    primary_import_folder: primary_import_folder_path(primary_import_folder),
                }))
            })
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        record_library_db_event("library.load_sources", started_at, Ok(()));
        Ok(rows)
    }

    pub(super) fn lookup_known_source_id(
        &self,
        root: &Path,
    ) -> Result<Option<SourceId>, LibraryError> {
        let started_at = Instant::now();
        let normalized = normalize_path(root);
        let needle = normalized.to_string_lossy().to_string();
        let mappings = self.load_known_sources()?;
        let result = mappings
            .into_iter()
            .find(|entry| entry.root == needle)
            .map(|entry| SourceId::from_string(entry.source_id));
        record_library_db_event("library.lookup_known_source_id", started_at, Ok(()));
        Ok(result)
    }

    pub(super) fn lookup_retained_source(
        &self,
        root: &Path,
    ) -> Result<Option<SampleSource>, LibraryError> {
        let started_at = Instant::now();
        let normalized = normalize_path(root);
        let needle = normalized.to_string_lossy().to_string();
        let result = self
            .load_known_sources()?
            .into_iter()
            .find(|entry| entry.root == needle)
            .map(KnownSourceMapping::try_into_sample_source)
            .transpose()?;
        record_library_db_event("library.lookup_retained_source", started_at, Ok(()));
        Ok(result)
    }

    pub(super) fn load_retained_sources(&self) -> Result<Vec<SampleSource>, LibraryError> {
        self.load_known_sources()?
            .into_iter()
            .map(KnownSourceMapping::try_into_sample_source)
            .collect()
    }

    fn replace_sources(tx: &Transaction<'_>, sources: &[SampleSource]) -> Result<(), LibraryError> {
        tx.execute("DELETE FROM sources", [])
            .map_err(map_sql_error)?;
        if sources.is_empty() {
            return Ok(());
        }

        let mut stmt = tx
            .prepare(
                "INSERT INTO sources (
                    id, root, sort_order, role, metadata_storage, primary_import_folder
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(map_sql_error)?;
        for (idx, source) in sources.iter().enumerate() {
            let source = normalized_source(source.clone());
            stmt.execute(params![
                source.id.as_str(),
                source.root.to_string_lossy(),
                idx as i64,
                source.role.as_str(),
                source.metadata_storage.as_str(),
                source.primary_import_folder.to_string_lossy(),
            ])
            .map_err(map_sql_error)?;
        }
        Ok(())
    }

    fn remember_known_sources_in_tx(
        tx: &Transaction<'_>,
        mappings: &mut Vec<KnownSourceMapping>,
        sources: &[SampleSource],
    ) -> Result<(), LibraryError> {
        for source in sources {
            upsert_known_source_mapping(mappings, source);
        }
        mappings.sort_by(|a, b| a.root.cmp(&b.root));
        Self::set_metadata_in_tx(tx, KNOWN_SOURCES_KEY, &serde_json::to_string(&mappings)?)?;
        Ok(())
    }

    fn load_known_sources(&self) -> Result<Vec<KnownSourceMapping>, LibraryError> {
        let started_at = Instant::now();
        let result = Self::load_known_sources_from(&self.connection);
        record_library_db_event(
            "library.load_known_sources",
            started_at,
            result.as_ref().map(|_| ()),
        );
        result
    }

    fn load_known_sources_from(conn: &Connection) -> Result<Vec<KnownSourceMapping>, LibraryError> {
        let Some(value) = Self::get_metadata_from(conn, KNOWN_SOURCES_KEY)? else {
            return Ok(Vec::new());
        };
        serde_json::from_str::<Vec<KnownSourceMapping>>(&value).map_err(|source| {
            LibraryError::MetadataJson {
                key: KNOWN_SOURCES_KEY,
                source,
            }
        })
    }

    fn get_metadata_from(conn: &Connection, key: &str) -> Result<Option<String>, LibraryError> {
        conn.query_row("SELECT value FROM metadata WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()
        .map_err(map_sql_error)
    }

    fn set_metadata_in_tx(
        tx: &Transaction<'_>,
        key: &str,
        value: &str,
    ) -> Result<(), LibraryError> {
        tx.execute(
            "INSERT INTO metadata (key, value)
             VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )
        .map_err(map_sql_error)?;
        Ok(())
    }
}

fn normalized_source(mut source: SampleSource) -> SampleSource {
    if source.role == SourceRole::Protected {
        source.metadata_storage = SourceMetadataStorage::AppData;
    }
    if source.role == SourceRole::Primary {
        source.metadata_storage = SourceMetadataStorage::SourceFolder;
    }
    source.primary_import_folder =
        primary_import_folder_path(source.primary_import_folder.to_string_lossy().to_string());
    source
}

fn primary_import_folder_path(value: String) -> PathBuf {
    let path = PathBuf::from(value.trim());
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return default_primary_import_folder();
    }
    path
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KnownSourceMapping {
    root: String,
    source_id: String,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    metadata_storage: Option<String>,
    #[serde(default)]
    primary_import_folder: Option<String>,
}

impl KnownSourceMapping {
    fn try_into_sample_source(self) -> Result<SampleSource, LibraryError> {
        let root = PathBuf::from(&self.root);
        let role = match self.role.as_deref() {
            None | Some("normal") => SourceRole::Normal,
            Some("protected") => SourceRole::Protected,
            Some("primary") => SourceRole::Primary,
            Some(value) => {
                return Err(LibraryError::InvalidRetainedSourceDescriptor {
                    root,
                    field: "role",
                    value: value.to_string(),
                });
            }
        };
        let metadata_storage = match self.metadata_storage.as_deref() {
            None | Some("source_folder") => SourceMetadataStorage::SourceFolder,
            Some("app_data") => SourceMetadataStorage::AppData,
            Some(value) => {
                return Err(LibraryError::InvalidRetainedSourceDescriptor {
                    root,
                    field: "metadata_storage",
                    value: value.to_string(),
                });
            }
        };
        Ok(normalized_source(SampleSource {
            id: SourceId::from_string(self.source_id),
            root,
            role,
            metadata_storage,
            primary_import_folder: primary_import_folder_path(
                self.primary_import_folder
                    .unwrap_or_else(|| default_primary_import_folder().to_string_lossy().into()),
            ),
        }))
    }
}

fn upsert_known_source_mapping(mappings: &mut Vec<KnownSourceMapping>, source: &SampleSource) {
    let normalized = normalize_path(&source.root);
    let root = normalized.to_string_lossy().to_string();
    if let Some(existing) = mappings.iter_mut().find(|entry| entry.root == root) {
        existing.source_id = source.id.as_str().to_string();
        existing.role = Some(source.role.as_str().to_string());
        existing.metadata_storage = Some(source.metadata_storage.as_str().to_string());
        existing.primary_import_folder =
            Some(source.primary_import_folder.to_string_lossy().to_string());
        return;
    }
    mappings.push(KnownSourceMapping {
        root,
        source_id: source.id.as_str().to_string(),
        role: Some(source.role.as_str().to_string()),
        metadata_storage: Some(source.metadata_storage.as_str().to_string()),
        primary_import_folder: Some(source.primary_import_folder.to_string_lossy().to_string()),
    });
}
