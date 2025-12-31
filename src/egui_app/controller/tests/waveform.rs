use super::super::selection_edits::SelectionEditRequest;
use super::super::test_support::{
    load_waveform_selection, prepare_with_source_and_wav_entries, sample_entry, write_test_wav,
};
use super::super::*;
use super::common::max_sample_amplitude;
use crate::egui_app::state::{DestructiveSelectionEdit, FocusContext};
use hound::WavReader;
use std::cell::RefCell;
use std::mem;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tempfile::tempdir;

#[test]
fn waveform_image_resizes_to_view() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "resize.wav",
        SampleTag::Neutral,
    )]);
    let wav_path = source.root.join("resize.wav");
    write_test_wav(&wav_path, &[0.0, 0.25, -0.5, 0.75]);

    controller
        .load_waveform_for_selection(&source, Path::new("resize.wav"))
        .unwrap();
    controller.update_waveform_size(24, 8);

    let size = controller.ui.waveform.image.as_ref().unwrap().image.size;
    assert_eq!(size, [24, 8]);
}

#[test]
fn removing_selected_source_clears_waveform_view() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "one.wav",
        SampleTag::Neutral,
    )]);
    let wav_path = source.root.join("one.wav");
    write_test_wav(&wav_path, &[0.1, -0.1]);
    controller
        .load_waveform_for_selection(&source, Path::new("one.wav"))
        .unwrap();

    controller.remove_source(0);

    assert!(controller.ui.waveform.image.is_none());
    assert!(controller.ui.waveform.selection.is_none());
    assert!(controller.sample_view.wav.loaded_audio.is_none());
    assert!(controller.sample_view.wav.loaded_wav.is_none());
}

#[test]
fn switching_sources_resets_waveform_state() {
    let (mut controller, first) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "a.wav",
        SampleTag::Neutral,
    )]);
    let wav_path = first.root.join("a.wav");
    write_test_wav(&wav_path, &[0.0, 0.1]);
    controller
        .load_waveform_for_selection(&first, Path::new("a.wav"))
        .unwrap();

    let second_dir = tempdir().unwrap();
    let second_root = second_dir.path().join("second");
    std::fs::create_dir_all(&second_root).unwrap();
    mem::forget(second_dir);
    let second = SampleSource::new(second_root);
    controller.library.sources.push(second.clone());

    controller.select_source(Some(second.id.clone()));

    assert!(controller.ui.waveform.image.is_none());
    assert!(controller.ui.waveform.notice.is_none());
    assert!(controller.sample_view.wav.loaded_audio.is_none());
}

#[test]
fn pruning_missing_selection_clears_waveform_view() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "gone.wav",
        SampleTag::Neutral,
    )]);
    let wav_path = source.root.join("gone.wav");
    write_test_wav(&wav_path, &[0.2, -0.2]);
    controller.sample_view.wav.selected_wav = Some(PathBuf::from("gone.wav"));
    controller
        .load_waveform_for_selection(&source, Path::new("gone.wav"))
        .unwrap();

    controller.set_wav_entries_for_tests(Vec::new());
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    assert!(controller.ui.waveform.image.is_none());
    assert!(controller.ui.waveform.selection.is_none());
    assert!(controller.sample_view.wav.loaded_audio.is_none());
    assert!(controller.sample_view.wav.loaded_wav.is_none());
}

#[test]
fn cropping_selection_overwrites_file() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "edit.wav",
        SampleTag::Neutral,
    )]);
    let wav_path = load_waveform_selection(
        &mut controller,
        &source,
        "edit.wav",
        &[0.1, 0.2, 0.3, 0.4],
        SelectionRange::new(0.25, 0.75),
    );

    controller.crop_waveform_selection().unwrap();

    let samples: Vec<f32> = WavReader::open(&wav_path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap())
        .collect();
    assert_eq!(samples, vec![0.2, 0.3]);
    assert!(controller.ui.waveform.selection.is_none());
    assert_eq!(controller.ui.status.badge_label, "Info");
}

#[test]
fn trimming_selection_removes_span() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "trim.wav",
        SampleTag::Neutral,
    )]);
    let wav_path = load_waveform_selection(
        &mut controller,
        &source,
        "trim.wav",
        &[0.0, 0.1, 0.2, 0.3],
        SelectionRange::new(0.25, 0.75),
    );

    controller.trim_waveform_selection().unwrap();

    let samples: Vec<f32> = WavReader::open(&wav_path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap())
        .collect();
    assert_eq!(samples, vec![0.0, 0.3]);
    assert!(controller.ui.waveform.selection.is_none());
    let entry = controller.wav_entry(0).unwrap();
    assert!(entry.file_size > 0);
}

#[test]
fn click_removal_interpolates_selected_span() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "click.wav",
        SampleTag::Neutral,
    )]);
    let wav_path = load_waveform_selection(
        &mut controller,
        &source,
        "click.wav",
        &[0.0, 1.0, 9.0, -1.0, 0.0],
        SelectionRange::new(0.4, 0.6),
    );

    controller.repair_clicks_selection().unwrap();

    let samples: Vec<f32> = WavReader::open(&wav_path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap())
        .collect();
    assert!(samples[2].abs() < 1e-6);
    assert!(controller.ui.waveform.selection.is_none());
    assert_eq!(controller.ui.status.badge_label, "Info");
}

#[test]
fn destructive_edit_request_prompts_without_yolo_mode() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "warn.wav",
        SampleTag::Neutral,
    )]);
    let wav_path = load_waveform_selection(
        &mut controller,
        &source,
        "warn.wav",
        &[0.0, 0.1, 0.2, 0.3],
        SelectionRange::new(0.25, 0.75),
    );

    let outcome = controller
        .request_destructive_selection_edit(DestructiveSelectionEdit::CropSelection)
        .unwrap();

    assert!(matches!(outcome, SelectionEditRequest::Prompted));
    assert!(controller.ui.waveform.pending_destructive.is_some());
    let samples: Vec<f32> = WavReader::open(&wav_path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap())
        .collect();
    assert_eq!(samples.len(), 4);
}

#[test]
fn yolo_mode_applies_destructive_edit_immediately() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "yolo.wav",
        SampleTag::Neutral,
    )]);
    let wav_path = load_waveform_selection(
        &mut controller,
        &source,
        "yolo.wav",
        &[0.1, 0.2, 0.3, 0.4],
        SelectionRange::new(0.25, 0.75),
    );
    controller.set_destructive_yolo_mode(true);

    let outcome = controller
        .request_destructive_selection_edit(DestructiveSelectionEdit::CropSelection)
        .unwrap();

    assert!(matches!(outcome, SelectionEditRequest::Applied));
    assert!(controller.ui.waveform.pending_destructive.is_none());
    let samples: Vec<f32> = hound::WavReader::open(&wav_path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap())
        .collect();
    assert_eq!(samples, vec![0.2, 0.3]);
}

#[test]
fn confirming_pending_destructive_edit_clears_prompt() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "confirm.wav",
        SampleTag::Neutral,
    )]);
    let wav_path = load_waveform_selection(
        &mut controller,
        &source,
        "confirm.wav",
        &[0.0, 0.1, 0.2, 0.3],
        SelectionRange::new(0.25, 0.75),
    );
    controller
        .request_destructive_selection_edit(DestructiveSelectionEdit::TrimSelection)
        .unwrap();
    let prompt = controller.ui.waveform.pending_destructive.clone().unwrap();

    controller.apply_confirmed_destructive_edit(prompt.edit);

    assert!(controller.ui.waveform.pending_destructive.is_none());
    let samples: Vec<f32> = hound::WavReader::open(&wav_path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap())
        .collect();
    assert_eq!(samples, vec![0.0, 0.3]);
}

#[test]
fn t_hotkey_prompts_trim_selection_in_waveform_focus() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "trim_hotkey.wav",
        SampleTag::Neutral,
    )]);
    load_waveform_selection(
        &mut controller,
        &source,
        "trim_hotkey.wav",
        &[0.0, 0.1, 0.2, 0.3],
        SelectionRange::new(0.25, 0.75),
    );

    let action = hotkeys::iter_actions()
        .find(|action| action.id == "trim-selection")
        .unwrap();
    controller.handle_hotkey(action, FocusContext::Waveform);

    assert!(controller.ui.waveform.pending_destructive.is_some());
    assert_eq!(
        controller
            .ui
            .waveform
            .pending_destructive
            .as_ref()
            .unwrap()
            .edit,
        DestructiveSelectionEdit::TrimSelection
    );
}

#[test]
fn slash_hotkeys_prompt_fade_selection_in_waveform_focus() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "fade_hotkey.wav",
        SampleTag::Neutral,
    )]);
    load_waveform_selection(
        &mut controller,
        &source,
        "fade_hotkey.wav",
        &[0.0, 0.1, 0.2, 0.3],
        SelectionRange::new(0.25, 0.75),
    );

    let backslash = hotkeys::iter_actions()
        .find(|action| action.id == "fade-selection-left-to-right")
        .unwrap();
    controller.handle_hotkey(backslash, FocusContext::Waveform);
    assert_eq!(
        controller
            .ui
            .waveform
            .pending_destructive
            .as_ref()
            .unwrap()
            .edit,
        DestructiveSelectionEdit::FadeLeftToRight
    );

    controller.ui.waveform.pending_destructive = None;

    let slash = hotkeys::iter_actions()
        .find(|action| action.id == "fade-selection-right-to-left")
        .unwrap();
    controller.handle_hotkey(slash, FocusContext::Waveform);
    assert_eq!(
        controller
            .ui
            .waveform
            .pending_destructive
            .as_ref()
            .unwrap()
            .edit,
        DestructiveSelectionEdit::FadeRightToLeft
    );
}

#[test]
fn m_hotkey_prompts_mute_selection_in_waveform_focus() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "mute_hotkey.wav",
        SampleTag::Neutral,
    )]);
    load_waveform_selection(
        &mut controller,
        &source,
        "mute_hotkey.wav",
        &[0.0, 0.1, 0.2, 0.3],
        SelectionRange::new(0.25, 0.75),
    );

    let action = hotkeys::iter_actions()
        .find(|action| action.id == "mute-selection")
        .unwrap();
    controller.handle_hotkey(action, FocusContext::Waveform);

    assert_eq!(
        controller
            .ui
            .waveform
            .pending_destructive
            .as_ref()
            .unwrap()
            .edit,
        DestructiveSelectionEdit::MuteSelection
    );
}

#[test]
fn n_hotkey_prompts_normalize_selection_when_selection_present() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "normalize_select_hotkey.wav",
        SampleTag::Neutral,
    )]);
    load_waveform_selection(
        &mut controller,
        &source,
        "normalize_select_hotkey.wav",
        &[0.0, 0.2, -0.6, 0.3],
        SelectionRange::new(0.25, 0.75),
    );

    let action = hotkeys::iter_actions()
        .find(|action| action.id == "normalize-waveform")
        .unwrap();
    controller.handle_hotkey(action, FocusContext::Waveform);

    assert_eq!(
        controller
            .ui
            .waveform
            .pending_destructive
            .as_ref()
            .unwrap()
            .edit,
        DestructiveSelectionEdit::NormalizeSelection
    );
}

#[test]
fn n_hotkey_normalizes_whole_loaded_sample_when_no_selection() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "normalize_full_hotkey.wav",
        SampleTag::Neutral,
    )]);
    let wav_path = load_waveform_selection(
        &mut controller,
        &source,
        "normalize_full_hotkey.wav",
        &[0.1, -0.5, 0.25],
        SelectionRange::new(0.0, 0.5),
    );
    controller.ui.waveform.selection = None;

    let action = hotkeys::iter_actions()
        .find(|action| action.id == "normalize-waveform")
        .unwrap();
    controller.handle_hotkey(action, FocusContext::Waveform);

    assert!(controller.ui.waveform.pending_destructive.is_none());
    let peak = max_sample_amplitude(&wav_path);
    assert!((peak - 1.0).abs() < 1e-4, "peak={peak}");
}

#[test]
fn normalize_selection_resumes_playback_when_playing() {
    let Some(player) = crate::audio::AudioPlayer::playing_for_tests() else {
        return;
    };
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "normalize_resume.wav",
        SampleTag::Neutral,
    )]);
    controller.audio.player = Some(Rc::new(RefCell::new(player)));
    load_waveform_selection(
        &mut controller,
        &source,
        "normalize_resume.wav",
        &[0.0, 0.2, -0.6, 0.3],
        SelectionRange::new(0.25, 0.75),
    );
    if controller.play_audio(false, None).is_err() || !controller.is_playing() {
        return;
    }
    controller.ui.waveform.playhead.position = 0.5;

    assert!(controller.normalize_waveform_selection().is_ok());

    assert!(controller.is_playing());
    assert!((controller.ui.waveform.playhead.position - 0.5).abs() < 1e-6);
}

#[test]
fn c_hotkey_prompts_crop_selection_in_waveform_focus() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "crop_hotkey.wav",
        SampleTag::Neutral,
    )]);
    load_waveform_selection(
        &mut controller,
        &source,
        "crop_hotkey.wav",
        &[0.0, 0.1, 0.2, 0.3],
        SelectionRange::new(0.25, 0.75),
    );

    let action = hotkeys::iter_actions()
        .find(|action| action.id == "crop-selection")
        .unwrap();
    controller.handle_hotkey(action, FocusContext::Waveform);

    assert_eq!(
        controller
            .ui
            .waveform
            .pending_destructive
            .as_ref()
            .unwrap()
            .edit,
        DestructiveSelectionEdit::CropSelection
    );
}

#[test]
fn shift_c_hotkey_crops_selection_to_new_sample() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
        "original.wav",
        SampleTag::Neutral,
    )]);
    let wav_path = load_waveform_selection(
        &mut controller,
        &source,
        "original.wav",
        &[0.0, 0.1, 0.2, 0.3],
        SelectionRange::new(0.25, 0.75),
    );

    let action = hotkeys::iter_actions()
        .find(|action| action.id == "crop-selection-new-sample")
        .unwrap();
    controller.handle_hotkey(action, FocusContext::Waveform);

    let cropped_path = source.root.join("original_crop001.wav");
    assert!(cropped_path.is_file());
    let original_samples: Vec<f32> = hound::WavReader::open(&wav_path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap())
        .collect();
    assert_eq!(original_samples, vec![0.0, 0.1, 0.2, 0.3]);

    let cropped_samples: Vec<f32> = hound::WavReader::open(&cropped_path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap())
        .collect();
    assert_eq!(cropped_samples, vec![0.1, 0.2]);
}
