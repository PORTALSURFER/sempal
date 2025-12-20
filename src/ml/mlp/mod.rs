//! Lightweight MLP classifier for feature vectors.

mod model;
mod train;

pub use model::{MlpInputKind, MlpModel};
pub use train::{TrainOptions, train_mlp};
