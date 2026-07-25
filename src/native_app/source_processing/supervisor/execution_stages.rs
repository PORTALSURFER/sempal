use super::{
    AtomicBool, ClaimedReadinessWork, DatabasePhase, DatabaseWriterGate, Ordering,
    ReadinessExecutionOutcome, ReadinessStage, ReadinessStore, SampleSource,
    SimilarityPublicationFence, SourceDatabase, SourceProcessingFailure,
    analysis_features_are_current, complete_pending_deep_hash_for_path,
    embedding_aspects_are_current, finalize_similarity_artifacts_if_ready,
    native_similarity_artifact_version, params, readiness_stage_is_unsupported,
    reconcile_stale_analysis_input, source_database_failure,
};
use rusqlite::OptionalExtension;

pub(super) fn run_readiness_stage(
    source: &SampleSource,
    connection: &mut rusqlite::Connection,
    database_writer: &DatabaseWriterGate,
    claim: &ClaimedReadinessWork,
    cancel: &AtomicBool,
) -> Result<ReadinessExecutionOutcome, SourceProcessingFailure> {
    let target = claim.target();
    match target.stage {
        ReadinessStage::IndexedIdentity => {
            let _writer = database_writer.lock(DatabasePhase::SerialCompatibility);
            let Some(relative_path) = target.relative_path.as_deref() else {
                return Ok(ReadinessExecutionOutcome::Permanent(
                    "indexed identity target has no relative path",
                ));
            };
            let current: bool = connection
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM wav_files
                        WHERE file_identity = ?1 AND path = ?2 AND missing = 0
                    )",
                    params![target.scope_id, relative_path],
                    |row| row.get(0),
                )
                .map_err(SourceProcessingFailure::from)?;
            Ok(if current {
                let has_content_hash: bool = connection
                    .query_row(
                        "SELECT EXISTS(
                            SELECT 1 FROM wav_files
                            WHERE file_identity = ?1 AND path = ?2 AND missing = 0
                              AND content_hash IS NOT NULL AND content_hash != ''
                        )",
                        params![target.scope_id, relative_path],
                        |row| row.get(0),
                    )
                    .map_err(SourceProcessingFailure::from)?;
                if !has_content_hash {
                    let database_root = source
                        .database_root()
                        .map_err(SourceProcessingFailure::from)?;
                    let db = SourceDatabase::open_for_background_job_with_database_root(
                        &source.root,
                        database_root,
                    )
                    .map_err(source_database_failure)?;
                    complete_pending_deep_hash_for_path(
                        &db,
                        std::path::Path::new(relative_path),
                        Some(cancel),
                    )
                    .map_err(SourceProcessingFailure::from)?;
                }
                let committed_content_hash = connection
                    .query_row(
                        "SELECT content_hash FROM wav_files
                         WHERE file_identity = ?1 AND path = ?2 AND missing = 0",
                        params![target.scope_id, relative_path],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .optional()
                    .map_err(SourceProcessingFailure::from)?
                    .flatten()
                    .filter(|content_hash| !content_hash.is_empty());
                if committed_content_hash.as_deref() == Some(target.content_generation.as_str()) {
                    ReadinessExecutionOutcome::Complete(None)
                } else if committed_content_hash.is_some() {
                    ReadinessExecutionOutcome::PrerequisiteInvalidated(
                        "indexed identity content generation changed",
                    )
                } else {
                    ReadinessExecutionOutcome::Retry(
                        "file is still changing; waiting for a stable content hash",
                    )
                }
            } else {
                ReadinessExecutionOutcome::Retry("indexed identity is not committed yet")
            })
        }
        ReadinessStage::AnalysisFeatures => {
            if target.content_generation.starts_with("pending-") {
                return Ok(ReadinessExecutionOutcome::PrerequisiteInvalidated(
                    "analysis target is waiting for a committed content generation",
                ));
            }
            let Some(relative_path) = target.relative_path.as_deref() else {
                return Ok(ReadinessExecutionOutcome::Permanent(
                    "analysis feature target has no relative path",
                ));
            };
            if target.required_version != wavecrate_analysis::analysis_version() {
                return Ok(ReadinessExecutionOutcome::Retry(
                    "feature executor version does not match target",
                ));
            }
            if analysis_features_are_current(connection, target)? {
                return Ok(ReadinessExecutionOutcome::Complete(None));
            }
            let produced = super::super::worker::run_readiness_feature_stage(
                connection,
                database_writer,
                source,
                std::path::Path::new(relative_path),
                target.content_generation.as_str(),
                target.required_version.as_str(),
                cancel,
            )?;
            if produced && analysis_features_are_current(connection, target)? {
                return Ok(ReadinessExecutionOutcome::Complete(None));
            }
            if !produced {
                {
                    let _writer = database_writer.lock(DatabasePhase::Publish);
                    reconcile_stale_analysis_input(
                        source,
                        std::path::Path::new(relative_path),
                        cancel,
                    )?;
                }
                return Ok(ReadinessExecutionOutcome::Retry(
                    "analysis input changed; targeted source reconciliation committed",
                ));
            }
            Ok(ReadinessExecutionOutcome::Retry(
                "analysis feature publication is not durable yet",
            ))
        }
        ReadinessStage::EmbeddingAspects => {
            if target.content_generation.starts_with("pending-") {
                return Ok(ReadinessExecutionOutcome::PrerequisiteInvalidated(
                    "embedding target is waiting for a committed content generation",
                ));
            }
            let expected_version = format!(
                "{}+{}",
                wavecrate_analysis::similarity::SIMILARITY_MODEL_ID,
                wavecrate_analysis::aspects::ASPECT_DESCRIPTOR_MODEL_ID,
            );
            if target.required_version != expected_version {
                return Ok(ReadinessExecutionOutcome::Retry(
                    "embedding executor version does not match target",
                ));
            }
            let Some(relative_path) = target.relative_path.as_deref() else {
                return Ok(ReadinessExecutionOutcome::Permanent(
                    "embedding target has no relative path",
                ));
            };
            if readiness_stage_is_unsupported(connection, target, "analysis_features")? {
                return Ok(ReadinessExecutionOutcome::Unsupported(
                    "analysis prerequisite is unsupported for this content generation",
                ));
            }
            let mut analysis_target = target.clone();
            analysis_target.stage = ReadinessStage::AnalysisFeatures;
            analysis_target.required_version = wavecrate_analysis::analysis_version().to_string();
            if !analysis_features_are_current(connection, &analysis_target)? {
                let invalidated = {
                    let _writer = database_writer.lock(DatabasePhase::Publish);
                    ReadinessStore::new(connection)
                        .invalidate_artifact(&analysis_target)
                        .map_err(|error| error.to_string())?
                };
                if invalidated {
                    tracing::warn!(
                        target: "wavecrate::source_processing",
                        source_id = target.source_id,
                        scope_id = target.scope_id,
                        "Invalidated an analysis readiness marker whose payload was missing"
                    );
                }
                return Ok(ReadinessExecutionOutcome::PrerequisiteInvalidated(
                    "analysis prerequisite artifact payload is missing",
                ));
            }
            Ok(if embedding_aspects_are_current(connection, target)? {
                ReadinessExecutionOutcome::Complete(None)
            } else {
                let produced = super::super::worker::run_readiness_embedding_stage(
                    connection,
                    database_writer,
                    source,
                    std::path::Path::new(relative_path),
                    target.content_generation.as_str(),
                    wavecrate_analysis::analysis_version(),
                    cancel,
                )?;
                if produced && embedding_aspects_are_current(connection, target)? {
                    ReadinessExecutionOutcome::Complete(None)
                } else {
                    ReadinessExecutionOutcome::Retry(
                        "embedding feature prerequisite is not durable yet",
                    )
                }
            })
        }
        ReadinessStage::SimilarityLayout => {
            let _writer = database_writer.lock(DatabasePhase::SerialCompatibility);
            if target.required_version != native_similarity_artifact_version() {
                return Ok(ReadinessExecutionOutcome::Retry(
                    "similarity finalizer version does not match target",
                ));
            }
            if cancel.load(Ordering::Acquire) {
                return Ok(ReadinessExecutionOutcome::Retry(
                    "similarity finalization cancelled",
                ));
            }
            let publication_fence = SimilarityPublicationFence::for_readiness_target(target)?;
            Ok(
                finalize_similarity_artifacts_if_ready(source, &publication_fence, cancel).map(
                    |finalized| {
                        if finalized {
                            ReadinessExecutionOutcome::Complete(None)
                        } else {
                            ReadinessExecutionOutcome::PrerequisiteInvalidated(
                                "similarity prerequisites changed before publication",
                            )
                        }
                    },
                )?,
            )
        }
    }
}
