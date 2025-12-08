#![allow(dead_code)]
//! Helpers to convert domain data into egui-facing view structs.
// Transitional helpers; wiring into the egui renderer will consume these.

use crate::egui_app::state::{CollectionRowView, CollectionSampleView, SourceRowView};
use crate::sample_sources::collections::CollectionMember;
use crate::sample_sources::{Collection, CollectionId, SampleSource, SampleTag, WavEntry};
use std::path::Path;

/// Convert a sample source into a UI row.
pub fn source_row(source: &SampleSource, missing: bool) -> SourceRowView {
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
        missing,
    }
}

/// Build display rows for the collection list.
pub fn collection_rows(
    collections: &[Collection],
    selected: Option<&CollectionId>,
    missing_flags: &[bool],
) -> Vec<CollectionRowView> {
    collections
        .iter()
        .enumerate()
        .map(|(index, collection)| CollectionRowView {
            id: collection.id.clone(),
            name: collection.name.clone(),
            selected: selected.is_some_and(|id| id == &collection.id),
            count: collection.members.len(),
            export_path: collection.export_path.clone(),
            missing: missing_flags.get(index).copied().unwrap_or(false),
        })
        .collect()
}

/// Convert collection members into UI rows with source labels.
pub fn collection_samples(
    collection: Option<&Collection>,
    sources: &[SampleSource],
    missing_flags: Option<&[bool]>,
    mut tag_lookup: impl FnMut(&CollectionMember) -> SampleTag,
) -> Vec<CollectionSampleView> {
    let Some(collection) = collection else {
        return Vec::new();
    };
    collection
        .members
        .iter()
        .enumerate()
        .map(|(index, member)| CollectionSampleView {
            source_id: member.source_id.clone(),
            source: source_label(sources, member.source_id.as_str()),
            path: member.relative_path.clone(),
            label: sample_display_label(&member.relative_path),
            tag: tag_lookup(member),
            missing: missing_flags
                .and_then(|flags| flags.get(index))
                .copied()
                .unwrap_or(false),
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

/// Helper to derive a browser index from a tag and absolute row position.
pub fn sample_browser_index_for(
    tag: SampleTag,
    index: usize,
) -> crate::egui_app::state::SampleBrowserIndex {
    use crate::egui_app::state::TriageFlagColumn::*;
    crate::egui_app::state::SampleBrowserIndex {
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

/// Produce a user-facing sample label that omits folders and extensions.
pub fn sample_display_label(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.to_string())
        .or_else(|| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_string())
        })
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn sample_display_label_strips_directories_and_extension() {
        let label = sample_display_label(Path::new("kits/sub/hihat_open.WAV"));
        assert_eq!(label, "hihat_open");
    }

    #[test]
    fn sample_display_label_handles_files_without_extension() {
        let label = sample_display_label(Path::new("clips/snare_roll"));
        assert_eq!(label, "snare_roll");
    }
}
