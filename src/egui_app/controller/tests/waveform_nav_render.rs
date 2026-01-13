use super::super::test_support::{dummy_controller, sample_entry, write_test_wav};
use crate::egui_app::controller::library::wavs;
use super::super::*;
use crate::egui_app::state::WaveformView;
use crate::waveform::DecodedWaveform;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

#[test]
fn waveform_refresh_respects_view_slice_and_caps_width() {
    let (mut controller, _source) = dummy_controller();
    controller.sample_view.waveform.size = [100, 10];
    controller.ui.waveform.view = WaveformView {
        start: 0.25,
        end: 0.5,
    };
    controller.sample_view.waveform.decoded = Some(DecodedWaveform {
        cache_token: 1,
        samples: std::sync::Arc::from((0..1000).map(|i| i as f32).collect::<Vec<_>>()),
        peaks: None,
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.sample_view.waveform.render_meta = None;
    controller.refresh_waveform_image();
    let image = controller
        .ui
        .waveform
        .image
        .as_ref()
        .expect("waveform image");
    assert!((image.view_start - 0.25).abs() < 1e-6);
    assert!((image.view_end - 0.5).abs() < 1e-6);
    let expected_width = (controller.sample_view.waveform.size[0] as f32)
        .min(crate::egui_app::controller::library::wavs::MAX_TEXTURE_WIDTH as f32)
        .ceil() as usize;
    let samples_in_view = (0.5 - 0.25) * 1000.0;
    let upper = (samples_in_view as usize)
        .min(crate::egui_app::controller::library::wavs::MAX_TEXTURE_WIDTH as usize)
        .max(1);
    let lower = controller.sample_view.waveform.size[0]
        .min(crate::egui_app::controller::library::wavs::MAX_TEXTURE_WIDTH) as usize;
    let clamped = expected_width.min(upper).max(lower);
    assert_eq!(image.image.size[0], clamped);
    assert_eq!(image.image.size[1], 10);
}

#[test]
fn waveform_render_meta_rejects_small_shifts_when_zoomed_in() {
    let base = wavs::WaveformRenderMeta {
        view_start: 0.10,
        view_end: 0.1009,
        size: [240, 32],
        samples_len: 10_000,
        texture_width: 8_000,
        channel_view: crate::waveform::WaveformChannelView::Mono,
        channels: 2,
    };
    let shifted = wavs::WaveformRenderMeta {
        view_start: 0.10095,
        view_end: 0.10185,
        ..base
    };
    assert!(!base.matches(&shifted));
}

#[test]
fn waveform_render_meta_allows_small_shifts_on_full_view() {
    let base = wavs::WaveformRenderMeta {
        view_start: 0.0,
        view_end: 1.0,
        size: [240, 32],
        samples_len: 10_000,
        texture_width: 2_000,
        channel_view: crate::waveform::WaveformChannelView::Mono,
        channels: 1,
    };
    let minor_shift = wavs::WaveformRenderMeta {
        view_start: 0.0005,
        view_end: 1.0005,
        ..base
    };
    assert!(base.matches(&minor_shift));
}

#[test]
fn waveform_rerenders_after_same_length_edit() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.sample_view.waveform.size = [32, 8];
    let path = source.root.join("edit.wav");
    write_test_wav(&path, &[0.1, 0.1, 0.1, 0.1]);

    controller
        .load_waveform_for_selection(&source, Path::new("edit.wav"))
        .unwrap();
    let before = controller
        .ui
        .waveform
        .image
        .as_ref()
        .expect("waveform image")
        .image
        .clone();

    write_test_wav(&path, &[1.0, -1.0, 1.0, -1.0]);
    controller.refresh_waveform_for_sample(&source, Path::new("edit.wav"));
    let after = controller
        .ui
        .waveform
        .image
        .as_ref()
        .expect("refreshed waveform image")
        .image
        .clone();

    assert_ne!(before.pixels, after.pixels);
}

#[test]
fn stale_audio_results_are_ignored() {
    let (mut controller, source) = dummy_controller();
    controller.settings.feature_flags.autoplay_selection = false;
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    write_test_wav(&source.root.join("a.wav"), &[0.0, 0.1]);
    write_test_wav(&source.root.join("b.wav"), &[0.0, -0.1]);
    controller.set_wav_entries_for_tests(vec![
        sample_entry("a.wav", crate::sample_sources::Rating::NEUTRAL),
        sample_entry("b.wav", crate::sample_sources::Rating::NEUTRAL),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.select_wav_by_path(Path::new("a.wav"));
    controller.select_wav_by_path(Path::new("b.wav"));

    for _ in 0..20 {
        controller.poll_background_jobs();
        if controller.sample_view.wav.loaded_wav.as_deref() == Some(Path::new("b.wav")) {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(
        controller.sample_view.wav.loaded_wav.as_deref(),
        Some(Path::new("b.wav"))
    );
    assert_eq!(
        controller.ui.loaded_wav.as_deref(),
        Some(Path::new("b.wav"))
    );
    assert!(controller.runtime.jobs.pending_audio.is_none());
}

#[test]
fn play_request_is_deferred_until_audio_ready() {
    let (mut controller, source) = dummy_controller();
    controller.settings.feature_flags.autoplay_selection = false;
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    write_test_wav(&source.root.join("wait.wav"), &[0.0, 0.2, -0.2]);
    controller.set_wav_entries_for_tests(vec![sample_entry("wait.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.select_wav_by_path(Path::new("wait.wav"));
    assert!(controller.runtime.jobs.pending_playback.is_none());
    let result = controller.play_audio(false, None);
    assert!(result.is_ok());
    let pending = controller
        .runtime
        .jobs
        .pending_playback
        .as_ref()
        .expect("pending playback to be queued");
    assert_eq!(pending.relative_path, PathBuf::from("wait.wav"));
    assert_eq!(pending.source_id, source.id);
    assert!(!pending.looped);
}

#[test]
fn loading_flag_clears_after_audio_load() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    let rel = PathBuf::from("load.wav");
    write_test_wav(&source.root.join(&rel), &[0.0, 0.5, -0.5]);
    controller.set_wav_entries_for_tests(vec![sample_entry("load.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller
        .queue_audio_load_for(&source, &rel, AudioLoadIntent::Selection, None)
        .expect("queue load");
    assert_eq!(
        controller.ui.waveform.loading.as_deref(),
        Some(rel.as_path())
    );

    for _ in 0..50 {
        controller.poll_background_jobs();
        if controller.sample_view.wav.loaded_wav.as_deref() == Some(rel.as_path()) {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(
        controller.sample_view.wav.loaded_wav.as_deref(),
        Some(rel.as_path())
    );
    assert!(controller.runtime.jobs.pending_audio.is_none());
    assert!(controller.ui.waveform.loading.is_none());
    assert!(controller.sample_view.wav.loaded_audio.is_some());
}
