mod cache_updates;
mod db;
mod io;
mod naming;
mod normalize;

use super::*;
use crate::sample_sources::collections::CollectionMember;
use std::path::{Path, PathBuf};

pub(in crate::egui_app::controller) struct CollectionSampleContext {
    pub(in crate::egui_app::controller) collection_id: CollectionId,
    pub(in crate::egui_app::controller) member: CollectionMember,
    pub(in crate::egui_app::controller) source: SampleSource,
    pub(in crate::egui_app::controller) absolute_path: PathBuf,
    pub(in crate::egui_app::controller) row: usize,
}

pub(in crate::egui_app::controller) fn read_samples_for_normalization(
    path: &Path,
) -> Result<(Vec<f32>, hound::WavSpec), String> {
    io::read_samples_for_normalization(path)
}

pub(in crate::egui_app::controller) fn file_metadata(path: &Path) -> Result<(u64, i64), String> {
    io::file_metadata(path)
}
