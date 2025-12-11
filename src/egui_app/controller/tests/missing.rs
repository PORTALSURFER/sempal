use super::super::selection_edits::SelectionEditRequest;
use super::super::test_support::{dummy_controller, sample_entry, write_test_wav};
use super::super::*;
use super::common::*;
use crate::egui_app::controller::collection_export;
use crate::egui_app::controller::hotkeys;
use crate::egui_app::state::{
    DestructiveSelectionEdit, DragPayload, DragSource, DragTarget, FocusContext,
    SampleBrowserActionPrompt, TriageFlagColumn, TriageFlagFilter, WaveformView,
};
use crate::sample_sources::Collection;
use crate::sample_sources::collections::CollectionMember;
use crate::waveform::DecodedWaveform;
use egui;
use hound::WavReader;
use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::IteratorRandom;
use std::io::Cursor;
use std::mem;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::tempdir;

#[test]
fn selecting_missing_sample_sets_waveform_notice() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.wav_entries = vec![WavEntry {
        relative_path: PathBuf::from("one.wav"),
        file_size: 1,
        modified_ns: 1,
        tag: SampleTag::Neutral,
        missing: true,
    }];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.select_wav_by_path(Path::new("one.wav"));

    assert!(
        controller
            .ui
            .waveform
            .notice
            .as_ref()
            .is_some_and(|msg| msg.contains("one.wav"))
    );
    assert!(controller.loaded_audio.is_none());
}

#[test]
fn collection_views_flag_missing_members() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    controller.wav_entries = vec![WavEntry {
        relative_path: PathBuf::from("one.wav"),
        file_size: 1,
        modified_ns: 1,
        tag: SampleTag::Neutral,
        missing: true,
    }];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller.rebuild_missing_lookup_for_source(&source.id);

    let mut collection = Collection::new("Test");
    collection.members.push(CollectionMember {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
    });
    controller.selected_collection = Some(collection.id.clone());
    controller.collections.push(collection);
    controller.refresh_collections_ui();

    assert!(
        controller
            .ui
            .collections
            .rows
            .first()
            .is_some_and(|row| row.missing)
    );
    assert!(
        controller
            .ui
            .collections
            .samples
            .iter()
            .any(|sample| sample.missing)
    );
}

#[test]
fn read_failure_marks_sample_missing() {
    let (mut controller, source) = dummy_controller();
    controller.sources.push(source.clone());
    controller.selected_source = Some(source.id.clone());
    let rel = PathBuf::from("gone.wav");
    controller.wav_entries = vec![WavEntry {
        relative_path: rel.clone(),
        file_size: 1,
        modified_ns: 1,
        tag: SampleTag::Neutral,
        missing: false,
    }];
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    let err = controller
        .load_waveform_for_selection(&source, &rel)
        .unwrap_err();
    assert!(err.contains("Failed to read"));
    assert!(controller.sample_missing(&source.id, &rel));
    assert!(controller.wav_entries[0].missing);
    assert!(
        controller
            .missing_wavs
            .get(&source.id)
            .is_some_and(|set| set.contains(&rel))
    );
}
