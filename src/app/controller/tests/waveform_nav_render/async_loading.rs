use super::*;

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
        .clone();

    write_test_wav(&path, &[1.0, -1.0, 1.0, -1.0]);
    controller.refresh_waveform_for_sample(&source, Path::new("edit.wav"));
    for _ in 0..50 {
        controller.poll_background_jobs();
        if controller.ui.waveform.loading.is_none() && controller.ui.waveform.image.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let after = controller
        .ui
        .waveform
        .image
        .as_ref()
        .expect("refreshed waveform image")
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
    controller.set_wav_entries_for_tests(vec![sample_entry(
        "wait.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
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
    controller.sample_view.wav.selected_wav = Some(rel.clone());
    write_test_wav(&source.root.join(&rel), &[0.0, 0.5, -0.5]);
    controller.set_wav_entries_for_tests(vec![sample_entry(
        "load.wav",
        crate::sample_sources::Rating::NEUTRAL,
    )]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller
        .queue_audio_load_for(&source, &rel, AudioLoadIntent::Selection, None)
        .expect("queue load");
    assert_eq!(
        controller.ui.waveform.loading.as_deref(),
        Some(rel.as_path())
    );

    for _ in 0..250 {
        controller.poll_background_jobs();
        if controller.sample_view.wav.loaded_wav.as_deref() == Some(rel.as_path())
            && controller.ui.waveform.loading.is_none()
        {
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

#[test]
/// Queue failures must clear loading state so browser focus does not appear stuck.
fn queue_audio_load_failure_clears_loading_state() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    let rel = PathBuf::from("missing.wav");
    let (audio_job_tx, audio_job_rx) = std::sync::mpsc::channel();
    drop(audio_job_rx);
    controller.runtime.jobs.audio_job_tx = audio_job_tx;

    let result = controller.queue_audio_load_for(&source, &rel, AudioLoadIntent::Selection, None);

    assert_eq!(result, Err(String::from("Failed to queue audio load")));
    assert!(controller.ui.waveform.loading.is_none());
    assert!(controller.runtime.jobs.pending_audio.is_none());
    assert!(controller.runtime.jobs.pending_playback.is_none());
}
