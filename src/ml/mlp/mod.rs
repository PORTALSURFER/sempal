//! Lightweight MLP classifier for feature vectors.

mod model;
mod train;

pub use model::MlpModel;
pub use train::{TrainOptions, train_mlp};
