use std::{
    cell::RefCell,
    collections::HashMap,
    mem,
    path::PathBuf,
    rc::Rc,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

use crate::audio::AudioPlayer;
use crate::sample_sources::config::{self, AppConfig};
use crate::sample_sources::scanner::scan_once;
use crate::sample_sources::{
    SampleSource, SampleTag, ScanError, ScanStats, ScanTracker, SourceDatabase, SourceDbError,
    SourceId, WavEntry,
};
use crate::selection::{SelectionEdge, SelectionRange, SelectionState};
use crate::ui::{HelloWorld, SourceRow, WavRow};
use crate::waveform::LoadedWaveform;
use crate::waveform::WaveformRenderer;
use rfd::FileDialog;
use slint::winit_030::{
    self, CustomApplicationHandler, EventResult,
    winit::event::{ElementState, WindowEvent},
    winit::keyboard::{KeyCode, ModifiersState, PhysicalKey},
};
use slint::{Color, ComponentHandle, SharedString};

mod callbacks;
mod loading;
mod navigation;
mod playback;
mod scan;
mod sources;
mod tags;
mod view;
mod wavs;
use self::loading::{WaveformCache, WaveformJob, WaveformJobResult};
use self::tags::TagStep;
use self::wavs::WavModels;

/// Minimum normalized width for a selection to count as usable.
const MIN_SELECTION_WIDTH: f32 = 0.001;
const WAVEFORM_CACHE_CAPACITY: usize = 6;

struct ScanJobResult {
    source_id: SourceId,
    result: Result<ScanStats, ScanError>,
}

#[derive(Clone, Copy, Debug)]
enum StatusState {
    #[allow(dead_code)]
    Idle,
    Busy,
    Info,
    Warning,
    Error,
}

/// Coordinates UI callbacks, playback, scanning, and tag persistence.
#[derive(Clone)]
pub struct DropHandler {
    app: Rc<RefCell<Option<slint::Weak<HelloWorld>>>>,
    renderer: WaveformRenderer,
    player: Rc<RefCell<AudioPlayer>>,
    playhead_timer: Rc<slint::Timer>,
    sources: Rc<RefCell<Vec<SampleSource>>>,
    db_cache: Rc<RefCell<HashMap<SourceId, Rc<SourceDatabase>>>>,
    wav_entries: Rc<RefCell<Vec<WavEntry>>>,
    selected_source: Rc<RefCell<Option<SourceId>>>,
    selected_wav: Rc<RefCell<Option<PathBuf>>>,
    loaded_wav: Rc<RefCell<Option<PathBuf>>>,
    pending_tags: Rc<RefCell<HashMap<SourceId, Vec<(PathBuf, SampleTag)>>>>,
    scan_tracker: Rc<RefCell<ScanTracker>>,
    scan_tx: Sender<ScanJobResult>,
    scan_rx: Rc<RefCell<Receiver<ScanJobResult>>>,
    scan_poll_timer: Rc<slint::Timer>,
    tag_flush_timer: Rc<slint::Timer>,
    shutting_down: Rc<RefCell<bool>>,
    selection: Rc<RefCell<SelectionState>>,
    loop_enabled: Rc<RefCell<bool>>,
    selection_drag_looping: Rc<RefCell<bool>>,
    modifiers: Rc<RefCell<ModifiersState>>,
    waveform_tx: Sender<WaveformJob>,
    waveform_rx: Rc<RefCell<Receiver<WaveformJobResult>>>,
    waveform_poll_timer: Rc<slint::Timer>,
    waveform_cache: Rc<RefCell<WaveformCache>>,
    wav_models: Rc<RefCell<WavModels>>,
}

impl DropHandler {
    /// Create a new handler with shared waveform renderer and audio player.
    pub fn new(renderer: WaveformRenderer, player: Rc<RefCell<AudioPlayer>>) -> Self {
        let (scan_tx, scan_rx) = mpsc::channel();
        let (waveform_tx, waveform_rx_raw) = mpsc::channel();
        let (waveform_result_tx, waveform_result_rx) = mpsc::channel();
        let worker_renderer = renderer.clone();
        thread::spawn(move || {
            loading::run_waveform_worker(worker_renderer, waveform_rx_raw, waveform_result_tx)
        });
        Self {
            app: Rc::new(RefCell::new(None)),
            renderer,
            player,
            playhead_timer: Rc::new(slint::Timer::default()),
            sources: Rc::new(RefCell::new(Vec::new())),
            db_cache: Rc::new(RefCell::new(HashMap::new())),
            wav_entries: Rc::new(RefCell::new(Vec::new())),
            selected_source: Rc::new(RefCell::new(None)),
            selected_wav: Rc::new(RefCell::new(None)),
            loaded_wav: Rc::new(RefCell::new(None)),
            pending_tags: Rc::new(RefCell::new(HashMap::new())),
            scan_tracker: Rc::new(RefCell::new(ScanTracker::default())),
            scan_tx,
            scan_rx: Rc::new(RefCell::new(scan_rx)),
            scan_poll_timer: Rc::new(slint::Timer::default()),
            tag_flush_timer: Rc::new(slint::Timer::default()),
            shutting_down: Rc::new(RefCell::new(false)),
            selection: Rc::new(RefCell::new(SelectionState::new())),
            loop_enabled: Rc::new(RefCell::new(false)),
            selection_drag_looping: Rc::new(RefCell::new(false)),
            modifiers: Rc::new(RefCell::new(ModifiersState::empty())),
            waveform_tx,
            waveform_rx: Rc::new(RefCell::new(waveform_result_rx)),
            waveform_poll_timer: Rc::new(slint::Timer::default()),
            waveform_cache: Rc::new(RefCell::new(WaveformCache::new(WAVEFORM_CACHE_CAPACITY))),
            wav_models: Rc::new(RefCell::new(WavModels::new())),
        }
    }

    /// Connect to the UI instance, preload sources, and begin scan polling.
    pub fn set_app(&self, app: &HelloWorld) {
        self.app.replace(Some(app.as_weak()));
        app.set_selection_visible(false);
        app.set_selection_start(0.0);
        app.set_selection_end(0.0);
        app.set_loop_enabled(*self.loop_enabled.borrow());
        self.load_sources(app);
        self.start_scan_polling();
        self.start_waveform_polling();
    }

    fn app(&self) -> Option<HelloWorld> {
        self.app.borrow().as_ref().and_then(|a| a.upgrade())
    }

    fn shutdown(&self) {
        *self.shutting_down.borrow_mut() = true;
        self.flush_pending_tags();
        self.scan_poll_timer.stop();
        self.waveform_poll_timer.stop();
        self.playhead_timer.stop();
        self.player.borrow_mut().stop();
        mem::forget(self.player.clone());
    }
}

impl CustomApplicationHandler for DropHandler {
    fn window_event(
        &mut self,
        _event_loop: &winit_030::winit::event_loop::ActiveEventLoop,
        _window_id: winit_030::winit::window::WindowId,
        _winit_window: Option<&winit_030::winit::window::Window>,
        _slint_window: Option<&slint::Window>,
        event: &WindowEvent,
    ) -> EventResult {
        match event {
            WindowEvent::DroppedFile(path_buf) => {
                self.handle_drop(path_buf.as_path());
                EventResult::Propagate
            }
            WindowEvent::CloseRequested => {
                self.shutdown();
                let _ = slint::quit_event_loop();
                EventResult::PreventDefault
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers.replace(modifiers.state());
                EventResult::Propagate
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                let modifiers = *self.modifiers.borrow();
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Space) => {
                        if modifiers.contains(ModifiersState::CONTROL) {
                            if self.player.borrow().is_playing() && *self.loop_enabled.borrow() {
                                self.player.borrow_mut().stop();
                                self.playhead_timer.stop();
                                self.set_loop_enabled(false);
                                if let Some(app) = self.app() {
                                    app.set_playhead_visible(false);
                                    self.set_status(&app, "Loop stopped", StatusState::Info);
                                }
                                EventResult::PreventDefault
                            } else {
                                self.set_loop_enabled(true);
                                self.play_audio(true)
                            }
                        } else {
                            self.play_audio(*self.loop_enabled.borrow())
                        }
                    }
                    PhysicalKey::Code(KeyCode::ArrowUp) => {
                        if self.move_selection(-1) {
                            EventResult::PreventDefault
                        } else {
                            EventResult::Propagate
                        }
                    }
                    PhysicalKey::Code(KeyCode::ArrowDown) => {
                        if self.move_selection(1) {
                            EventResult::PreventDefault
                        } else {
                            EventResult::Propagate
                        }
                    }
                    PhysicalKey::Code(KeyCode::ArrowLeft) => {
                        if self.apply_tag_step(TagStep::Left) {
                            EventResult::PreventDefault
                        } else {
                            EventResult::Propagate
                        }
                    }
                    PhysicalKey::Code(KeyCode::ArrowRight) => {
                        if self.apply_tag_step(TagStep::Right) {
                            EventResult::PreventDefault
                        } else {
                            EventResult::Propagate
                        }
                    }
                    _ => EventResult::Propagate,
                }
            }
            _ => EventResult::Propagate,
        }
    }
}

/// Configure the UI and enter the event loop.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let renderer = WaveformRenderer::new(680, 260);
    let audio_player = AudioPlayer::new()
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
    let player = Rc::new(RefCell::new(audio_player));
    let drop_handler = DropHandler::new(renderer.clone(), player.clone());

    slint::BackendSelector::new()
        .require_wgpu_27(slint::wgpu_27::WGPUConfiguration::default())
        .with_winit_custom_application_handler(drop_handler.clone())
        .select()?;

    let app = HelloWorld::new()?;
    app.set_source_menu_index(-1);
    app.set_waveform(renderer.empty_image());
    drop_handler.set_app(&app);
    callbacks::attach_callbacks(&app, &drop_handler);
    app.window().set_fullscreen(true);
    app.run()?;
    Ok(())
}
