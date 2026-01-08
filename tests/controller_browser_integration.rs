mod support;

use support::{sempal_env::SempalEnvGuard, wav::write_test_wav};

use sempal::{
    egui_app::controller::EguiController, sample_sources::Rating, waveform::WaveformRenderer,
};
use std::{path::PathBuf, time::Duration};
use tempfile::TempDir;

struct ControllerHarness {
    _config: SempalEnvGuard,
    _temp: TempDir,
    pub controller: EguiController,
}

impl ControllerHarness {
    fn new_with_wavs(names: &[&str]) -> Self {
        let temp = tempfile::tempdir().expect("create tempdir");
        let config_home = temp.path().join("config");
        std::fs::create_dir_all(&config_home).expect("create config dir");
        let env = SempalEnvGuard::set_config_home(config_home);

        let source_root = temp.path().join("source");
        std::fs::create_dir_all(&source_root).expect("create source dir");
        for &name in names {
            write_test_wav(&source_root.join(name), &[0.0, 0.1, -0.1, 0.2]);
        }

        let renderer = WaveformRenderer::new(32, 16);
        let mut controller = EguiController::new(renderer, None);
        controller
            .add_source_from_path(source_root.clone())
            .expect("add source");

        let expected = names.len();
        for _ in 0..200 {
            controller.tick_playhead();
            if controller.visible_browser_len() == expected {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(controller.visible_browser_len(), expected);

        Self {
            _config: env,
            _temp: temp,
            controller,
        }
    }
}

fn visible_path(controller: &mut EguiController, visible_row: usize) -> PathBuf {
    let entry_index = controller
        .visible_browser_index(visible_row)
        .expect("visible row exists");
    controller
        .wav_entry(entry_index)
        .expect("wav entry exists")
        .relative_path
        .clone()
}

#[test]
fn click_clears_selection_and_focuses_row() {
    let mut h = ControllerHarness::new_with_wavs(&["one.wav", "two.wav", "three.wav"]);
    let controller = &mut h.controller;

    controller.focus_browser_row(0);
    controller.toggle_browser_row_selection(1);
    assert_eq!(controller.ui.browser.selected_paths.len(), 2);

    controller.clear_browser_selection();
    controller.focus_browser_row_only(2);

    assert!(controller.ui.browser.selected_paths.is_empty());
    assert_eq!(controller.ui.browser.selected_visible, Some(2));
    assert_eq!(controller.ui.browser.selection_anchor_visible, Some(2));
}

#[test]
fn ctrl_click_toggles_selection_and_focuses_row() {
    let mut h = ControllerHarness::new_with_wavs(&["one.wav", "two.wav", "three.wav"]);
    let controller = &mut h.controller;

    let row0 = visible_path(controller, 0);
    let row2 = visible_path(controller, 2);

    controller.focus_browser_row(0);
    assert_eq!(controller.ui.browser.selected_paths.len(), 1);
    assert_eq!(controller.ui.browser.selection_anchor_visible, Some(0));

    controller.toggle_browser_row_selection(2);

    let selected: Vec<_> = controller.ui.browser.selected_paths.to_vec();
    assert!(selected.contains(&row0));
    assert!(selected.contains(&row2));
    assert_eq!(controller.ui.browser.selected_visible, Some(2));
}

#[test]
fn shift_click_extends_selection_range() {
    let mut h = ControllerHarness::new_with_wavs(&["one.wav", "two.wav", "three.wav"]);
    let controller = &mut h.controller;

    let row0 = visible_path(controller, 0);
    let row1 = visible_path(controller, 1);
    let row2 = visible_path(controller, 2);

    controller.focus_browser_row(0);
    controller.toggle_browser_row_selection(2);

    controller.extend_browser_selection_to_row(1);

    let selected: Vec<_> = controller.ui.browser.selected_paths.to_vec();
    assert_eq!(selected.len(), 2);
    assert!(selected.contains(&row0));
    assert!(selected.contains(&row1));
    assert!(!selected.contains(&row2));
    assert_eq!(controller.ui.browser.selected_visible, Some(1));
    assert_eq!(controller.ui.browser.selection_anchor_visible, Some(0));
}

#[test]
fn ctrl_shift_click_adds_range_without_resetting_anchor() {
    let mut h = ControllerHarness::new_with_wavs(&[
        "one.wav",
        "two.wav",
        "three.wav",
        "four.wav",
        "five.wav",
        "six.wav",
    ]);
    let controller = &mut h.controller;

    let row0 = visible_path(controller, 0);
    let row1 = visible_path(controller, 1);
    let row2 = visible_path(controller, 2);
    let row5 = visible_path(controller, 5);

    controller.focus_browser_row(0);
    controller.toggle_browser_row_selection(5);

    controller.add_range_browser_selection(2);

    let selected: Vec<_> = controller
        .ui
        .browser
        .selected_paths
        .iter()
        .cloned()
        .collect();
    assert_eq!(selected.len(), 4);
    assert!(selected.contains(&row0));
    assert!(selected.contains(&row1));
    assert!(selected.contains(&row2));
    assert!(selected.contains(&row5));
    assert_eq!(controller.ui.browser.selection_anchor_visible, Some(0));
    assert_eq!(controller.ui.browser.selected_visible, Some(2));
}

#[test]
fn browser_tagging_via_controller_updates_rows() {
    let mut h = ControllerHarness::new_with_wavs(&["one.wav", "two.wav"]);
    let controller = &mut h.controller;

    controller.focus_browser_row_only(0);
    controller.toggle_browser_row_selection(1);
    controller
        .tag_browser_samples(&[0, 1], Rating::TRASH_3, 0)
        .expect("tag browser samples");

    assert_eq!(controller.ui.browser.trash.len(), 2);
}
