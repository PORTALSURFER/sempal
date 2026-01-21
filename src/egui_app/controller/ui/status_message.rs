use crate::egui_app::ui::style::StatusTone;

#[derive(Clone, Debug)]
pub(crate) enum StatusMessage {
    SelectSourceFirst {
        tone: StatusTone,
    },
    SelectSourceToScan,
    ScanAlreadyRunning,
    SimilarityPrepAlreadyRunning,
    SimilarityScanAlreadyRunning,
    TsneBuildAlreadyRunning,
    ClusterBuildAlreadyRunning,
    BuildingTsneLayout,
    BuildingClusters,
    PreparingSimilarity {
        source: String,
    },
    FinalizingSimilarityPrep,
    SimilarityReady {
        cluster_count: usize,
        noise_ratio: f32,
    },
    SimilarityPrepFailed {
        err: String,
    },
    SimilarityAlreadyUpToDate,
    RandomHistoryEmpty,
    RandomHistoryStart,
    RandomNavOff,
    NoSamplesToRandomize,
    AddSourceFirst {
        tone: StatusTone,
    },
    AddSourceWithSamplesFirst,
    Custom {
        text: String,
        tone: StatusTone,
    },
}

impl StatusMessage {
    pub(crate) fn custom(text: impl Into<String>, tone: StatusTone) -> Self {
        Self::Custom {
            text: text.into(),
            tone,
        }
    }

    pub(crate) fn into_text_and_tone(self) -> (String, StatusTone) {
        match self {
            StatusMessage::SelectSourceFirst { tone } => ("Select a source first".into(), tone),
            StatusMessage::SelectSourceToScan => {
                ("Select a source to scan".into(), StatusTone::Warning)
            }
            StatusMessage::ScanAlreadyRunning => {
                ("Scan already in progress".into(), StatusTone::Info)
            }
            StatusMessage::SimilarityPrepAlreadyRunning => {
                ("Similarity prep already running".into(), StatusTone::Info)
            }
            StatusMessage::SimilarityScanAlreadyRunning => {
                ("Scan already in progress".into(), StatusTone::Info)
            }
            StatusMessage::TsneBuildAlreadyRunning => {
                ("t-SNE build already in progress".into(), StatusTone::Info)
            }
            StatusMessage::ClusterBuildAlreadyRunning => {
                ("Cluster build already running".into(), StatusTone::Info)
            }
            StatusMessage::BuildingTsneLayout => {
                ("Building t-SNE layout...".into(), StatusTone::Info)
            }
            StatusMessage::BuildingClusters => ("Building clusters...".into(), StatusTone::Info),
            StatusMessage::PreparingSimilarity { source } => (
                format!("Preparing similarity search for {}", source),
                StatusTone::Busy,
            ),
            StatusMessage::FinalizingSimilarityPrep => {
                ("Finalizing similarity prep...".into(), StatusTone::Busy)
            }
            StatusMessage::SimilarityReady {
                cluster_count,
                noise_ratio,
            } => (
                format!(
                    "Similarity ready: {} clusters (noise {:.1}%)",
                    cluster_count,
                    noise_ratio * 100.0
                ),
                StatusTone::Info,
            ),
            StatusMessage::SimilarityPrepFailed { err } => {
                (format!("Similarity prep failed: {err}"), StatusTone::Error)
            }
            StatusMessage::SimilarityAlreadyUpToDate => (
                "Similarity search is already up to date for this source".into(),
                StatusTone::Info,
            ),
            StatusMessage::RandomHistoryEmpty => ("No random history yet".into(), StatusTone::Info),
            StatusMessage::RandomHistoryStart => {
                ("Reached start of random history".into(), StatusTone::Info)
            }
            StatusMessage::RandomNavOff => ("Random navigation off".into(), StatusTone::Info),
            StatusMessage::NoSamplesToRandomize => {
                ("No samples available to randomize".into(), StatusTone::Info)
            }
            StatusMessage::AddSourceFirst { tone } => ("Add a source first".into(), tone),
            StatusMessage::AddSourceWithSamplesFirst => {
                ("Add a source with samples first".into(), StatusTone::Info)
            }
            StatusMessage::Custom { text, tone } => (text, tone),
        }
    }
}
