use super::*;
use std::path::Path;

impl EguiController {
    /// Enqueue analysis for a newly created sample so similarity search stays fresh.
    pub(super) fn enqueue_similarity_for_new_sample(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
    ) {
        let source = source.clone();
        let relative_path = relative_path.to_path_buf();
        let content_hash = fast_content_hash(file_size, modified_ns);
        let tx = self.runtime.jobs.message_sender();
        std::thread::spawn(move || {
            let changed = crate::sample_sources::scanner::ChangedSample {
                relative_path,
                file_size,
                modified_ns,
                content_hash,
            };
            let result = super::analysis_jobs::enqueue_jobs_for_source(&source, &[changed]);
            match result {
                Ok((inserted, progress)) => {
                    let _ = tx.send(super::jobs::JobMessage::Analysis(
                        super::analysis_jobs::AnalysisJobMessage::EnqueueFinished {
                            inserted,
                            progress,
                        },
                    ));
                }
                Err(err) => {
                    let _ = tx.send(super::jobs::JobMessage::Analysis(
                        super::analysis_jobs::AnalysisJobMessage::EnqueueFailed(err),
                    ));
                }
            }
        });
    }

    pub fn backfill_missing_features_for_selected_source(&mut self) {
        let Some(source) = self.current_source() else {
            self.set_status_message(StatusMessage::SelectSourceFirst {
                tone: StatusTone::Warning,
            });
            return;
        };
        let tx = self.runtime.jobs.message_sender();
        std::thread::spawn(move || {
            let result = super::analysis_jobs::enqueue_jobs_for_source_missing_features(&source);
            match result {
                Ok((inserted, progress)) => {
                    let _ = tx.send(super::jobs::JobMessage::Analysis(
                        super::analysis_jobs::AnalysisJobMessage::EnqueueFinished {
                            inserted,
                            progress,
                        },
                    ));
                }
                Err(err) => {
                    let _ = tx.send(super::jobs::JobMessage::Analysis(
                        super::analysis_jobs::AnalysisJobMessage::EnqueueFailed(err),
                    ));
                }
            }
        });
    }

    pub fn backfill_embeddings_for_selected_source(&mut self) {
        let Some(source) = self.current_source() else {
            self.set_status_message(StatusMessage::SelectSourceFirst {
                tone: StatusTone::Warning,
            });
            return;
        };
        let tx = self.runtime.jobs.message_sender();
        std::thread::spawn(move || {
            let result = super::analysis_jobs::enqueue_jobs_for_embedding_backfill(&source);
            match result {
                Ok((inserted, progress)) => {
                    let _ = tx.send(super::jobs::JobMessage::Analysis(
                        super::analysis_jobs::AnalysisJobMessage::EmbeddingBackfillEnqueueFinished {
                            inserted,
                            progress,
                        },
                    ));
                }
                Err(err) => {
                    let _ = tx.send(super::jobs::JobMessage::Analysis(
                        super::analysis_jobs::AnalysisJobMessage::EmbeddingBackfillEnqueueFailed(
                            err,
                        ),
                    ));
                }
            }
        });
    }

    pub fn recalc_similarity_for_browser_rows(
        &mut self,
        rows: &[usize],
    ) -> Result<(), String> {
        let Some(source) = self.current_source() else {
            return Err("Select a source first".to_string());
        };

        let mut changed_samples = Vec::new();
        let mut sample_ids = Vec::new();
        for &row in rows {
            let Some(entry_index) = self.visible_browser_index(row) else {
                continue;
            };
            let Some(entry) = self.wav_entry(entry_index) else {
                continue;
            };
            if entry.missing {
                continue;
            }
            changed_samples.push(crate::sample_sources::scanner::ChangedSample {
                relative_path: entry.relative_path.clone(),
                file_size: entry.file_size,
                modified_ns: entry.modified_ns,
                content_hash: fast_content_hash(entry.file_size, entry.modified_ns),
            });
            sample_ids.push(super::analysis_jobs::db::build_sample_id(
                source.id.as_str(),
                &entry.relative_path,
            ));
        }

        if sample_ids.is_empty() {
            return Err("No valid samples selected".to_string());
        }

        sample_ids.sort();
        sample_ids.dedup();

        let tx = self.runtime.jobs.message_sender();
        let source = source.clone();
        std::thread::spawn(move || {
            if !changed_samples.is_empty() {
                let result = super::analysis_jobs::enqueue_jobs_for_source(&source, &changed_samples);
                match result {
                    Ok((inserted, progress)) => {
                        let _ = tx.send(super::jobs::JobMessage::Analysis(
                            super::analysis_jobs::AnalysisJobMessage::EnqueueFinished {
                                inserted,
                                progress,
                            },
                        ));
                    }
                    Err(err) => {
                        let _ = tx.send(super::jobs::JobMessage::Analysis(
                            super::analysis_jobs::AnalysisJobMessage::EnqueueFailed(err),
                        ));
                    }
                }
            }

            let result =
                super::analysis_jobs::enqueue_jobs_for_embedding_samples(&source, &sample_ids);
            match result {
                Ok((inserted, progress)) => {
                    let _ = tx.send(super::jobs::JobMessage::Analysis(
                        super::analysis_jobs::AnalysisJobMessage::EmbeddingBackfillEnqueueFinished {
                            inserted,
                            progress,
                        },
                    ));
                }
                Err(err) => {
                    let _ = tx.send(super::jobs::JobMessage::Analysis(
                        super::analysis_jobs::AnalysisJobMessage::EmbeddingBackfillEnqueueFailed(
                            err,
                        ),
                    ));
                }
            }
        });

        Ok(())
    }

    pub fn has_any_sources(&self) -> bool {
        !self.library.sources.is_empty()
    }
}

fn fast_content_hash(file_size: u64, modified_ns: i64) -> String {
    format!("fast-{}-{}", file_size, modified_ns)
}
