//! Helpers for training from curated folder datasets.

mod builders;
mod classes;
mod export;
mod embeddings;
mod features;
mod progress;
mod samples;

pub use builders::{
    build_logreg_dataset_from_samples, build_logreg_dataset_from_samples_with_progress,
    build_mlp_dataset_from_samples, build_mlp_dataset_from_samples_with_progress,
};
pub use progress::TrainingProgress;
pub use export::{
    CuratedExportOptions, CuratedExportSummary, export_curated_embedding_dataset,
    export_curated_embedding_dataset_with_progress,
};
pub use features::{
    build_feature_dataset_from_samples, build_feature_dataset_from_samples_with_progress,
};
pub use samples::{
    collect_training_samples, filter_training_samples, stratified_split_map, TrainingSample,
};
