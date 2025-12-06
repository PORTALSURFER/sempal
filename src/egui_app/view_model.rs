#![allow(dead_code)]
//! Helpers to convert domain data into egui-facing view structs.
// Transitional helpers; wiring into the egui renderer will consume these.

use crate::egui_app::state::{
    CollectionRowView, CollectionSampleView, SourceRowView, WavRowView,
};
use crate::sample_sources::{Collection, CollectionId, SampleSource, SampleTag, WavEntry};
use std::path::Path;

/// Convert a sample source into a UI row.
pub fn source_row(source: &SampleSource) -> SourceRowView {
    let name = source
        .root
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.to_string())
        .unwrap_or_else(|| source.root.to_string_lossy().to_string());
    SourceRowView {
        id: source.id.clone(),
        name,
        path: source.root.to_string_lossy().to_string(),
    }
}

/// Convert a wav entry into its UI row representation.
pub fn wav_row(entry: &WavEntry, selected: bool, loaded: bool) -> WavRowView {
    let name = entry.relative_path.to_string_lossy().to_string();
    WavRowView {
        path: entry.relative_path.clone(),
        name,
        tag: entry.tag,
        selected,
        loaded,
    }
}

/// Build display rows for the collection list.
pub fn collection_rows(
    collections: &[Collection],
    selected: Option<&CollectionId>,
) -> Vec<CollectionRowView> {
    collections
        .iter()
        .map(|collection| CollectionRowView {
            id: collection.id.clone(),
            name: collection.name.clone(),
            selected: selected.is_some_and(|id| id == &collection.id),
            count: collection.members.len(),
        })
        .collect()
}

/// Convert collection members into UI rows with source labels.
pub fn collection_samples(
    collection: Option<&Collection>,
    sources: &[SampleSource],
) -> Vec<CollectionSampleView> {
    let Some(collection) = collection else {
        return Vec::new();
    };
    collection
        .members
        .iter()
        .map(|member| CollectionSampleView {
            source: source_label(sources, member.source_id.as_str()),
            path: member.relative_path.to_string_lossy().to_string(),
        })
        .collect()
}

fn source_label(sources: &[SampleSource], id: &str) -> String {
    sources
        .iter()
        .find(|s| s.id.as_str() == id)
        .and_then(|source| {
            source
                .root
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.to_string())
        })
        .unwrap_or_else(|| "Unknown source".to_string())
}

/// Helper to derive a triage index from a tag and absolute row position.
pub fn triage_index_for(tag: SampleTag, index: usize) -> crate::egui_app::state::TriageIndex {
    use crate::egui_app::state::TriageColumn::*;
    crate::egui_app::state::TriageIndex {
        column: match tag {
            SampleTag::Trash => Trash,
            SampleTag::Neutral => Neutral,
            SampleTag::Keep => Keep,
        },
        row: index,
    }
}

/// Locate the entry index by path for reuse in selection bookkeeping.
pub fn locate_entry(entries: &[WavEntry], target: &Path) -> Option<usize> {
    entries
        .iter()
        .position(|entry| entry.relative_path == target)
}
