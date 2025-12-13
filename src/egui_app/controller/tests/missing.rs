use super::super::test_support::dummy_controller;
use super::super::*;
use crate::sample_sources::Collection;
use crate::sample_sources::collections::CollectionMember;
use std::path::{Path, PathBuf};

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
        clip_root: None,
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
