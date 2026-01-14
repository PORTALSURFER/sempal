mod cache_updates;
mod db;
pub(crate) mod io;
mod naming;
mod normalize;

use super::*;
use crate::sample_sources::collections::CollectionMember;
use std::path::{Path, PathBuf};

pub(crate) struct CollectionSampleContext {
    pub(crate) collection_id: CollectionId,
    pub(crate) member: CollectionMember,
    pub(crate) source: SampleSource,
    pub(crate) absolute_path: PathBuf,
    pub(crate) row: usize,
}

pub(crate) fn read_samples_for_normalization(
    path: &Path,
) -> Result<(Vec<f32>, hound::WavSpec), String> {
    io::read_samples_for_normalization(path)
}

pub(crate) fn file_metadata(path: &Path) -> Result<(u64, i64), String> {
    io::file_metadata(path)
}
