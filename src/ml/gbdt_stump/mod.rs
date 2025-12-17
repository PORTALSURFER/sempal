//! Deterministic gradient-boosted decision-stump classifier.
//!
//! This is a lightweight baseline that avoids external ML dependencies while still supporting:
//! - Multi-class classification via softmax boosting.
//! - Pack-based leakage-free splits (provided by the dataset export).
//! - Reproducible JSON model export/load.

mod model;
mod train;

pub use model::{GbdtStumpModel, Stump, softmax};
pub use train::{TrainDataset, TrainOptions, train_gbdt_stump};

