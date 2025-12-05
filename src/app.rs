use std::mem;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

use crate::audio::AudioPlayer;
use crate::sample_sources::config::{self, AppConfig};
use crate::sample_sources::scanner::scan_once;
use crate::sample_sources::{
    SampleSource, ScanError, ScanStats, ScanTracker, SourceDatabase, SourceId, WavEntry,
};
use crate::ui::{HelloWorld, SourceRow, WavRow};
use crate::waveform::WaveformRenderer;
use rfd::FileDialog;
use slint::winit_030::{
    self, CustomApplicationHandler, EventResult,
    winit::event::{ElementState, WindowEvent},
    winit::keyboard::{KeyCode, PhysicalKey},
};
use slint::{Color, ComponentHandle, SharedString};

#[derive(Clone)]
pub struct DropHandler {
    app: Rc<RefCell<Option<slint::Weak<HelloWorld>>>>,
    renderer: WaveformRenderer,
    player: Rc<RefCell<AudioPlayer>>,
    playhead_timer: Rc<slint::Timer>,
    sources: Rc<RefCell<Vec<SampleSource>>>,
    wav_entries: Rc<RefCell<Vec<WavEntry>>>,
    selected_source: Rc<RefCell<Option<SourceId>>>,
    scan_tracker: Rc<RefCell<ScanTracker>>,
    scan_tx: Sender<ScanJobResult>,
    scan_rx: Rc<RefCell<Receiver<ScanJobResult>>>,
    scan_poll_timer: Rc<slint::Timer>,
    shutting_down: Rc<RefCell<bool>>,
}

struct ScanJobResult {
    source_id: SourceId,
    result: Result<ScanStats, ScanError>,
}

#[derive(Clone, Copy, Debug)]
enum StatusState {
    Idle,
    Busy,
    Info,
    Warning,
    Error,
}

impl DropHandler {
    pub fn new(renderer: WaveformRenderer, player: Rc<RefCell<AudioPlayer>>) -> Self {
        let (scan_tx, scan_rx) = mpsc::channel();
        Self {
            app: Rc::new(RefCell::new(None)),
            renderer,
            player,
            playhead_timer: Rc::new(slint::Timer::default()),
            sources: Rc::new(RefCell::new(Vec::new())),
            wav_entries: Rc::new(RefCell::new(Vec::new())),
            selected_source: Rc::new(RefCell::new(None)),
            scan_tracker: Rc::new(RefCell::new(ScanTracker::default())),
            scan_tx,
            scan_rx: Rc::new(RefCell::new(scan_rx)),
            scan_poll_timer: Rc::new(slint::Timer::default()),
            shutting_down: Rc::new(RefCell::new(false)),
        }
    }

    pub fn set_app(&self, app: &HelloWorld) {
        self.app.replace(Some(app.as_weak()));
        self.load_sources(app);
        self.start_scan_polling();
    }

    pub fn handle_drop(&self, path: &std::path::Path) {
        let Some(app) = self.app() else {
            return;
        };
        if !Self::is_wav(path) {
            self.set_status(
                &app,
                "Unsupported file type (please drop a .wav)",
                StatusState::Warning,
            );
            return;
        }
        match self.renderer.load_waveform(path) {
            Ok(loaded) => {
                app.set_waveform(loaded.image);
                let mut player = self.player.borrow_mut();
                player.stop();
                player.set_audio(loaded.audio_bytes, loaded.duration_seconds);
                self.playhead_timer.stop();
                app.set_playhead_position(0.0);
                app.set_playhead_visible(false);
                self.set_status(
                    &app,
                    format!("Loaded {}", path.display()),
                    StatusState::Info,
                );
            }
            Err(error) => self.set_status(&app, error, StatusState::Error),
        }
    }

    pub fn play_audio(&self) -> EventResult {
        let Some(app) = self.app() else {
            return EventResult::Propagate;
        };
        match self.player.borrow_mut().play() {
            Ok(_) => {
                self.set_status(&app, "Playing audio", StatusState::Info);
                self.start_playhead_updates();
                EventResult::PreventDefault
            }
            Err(error) => {
                self.set_status(&app, error, StatusState::Error);
                EventResult::PreventDefault
            }
        }
    }

    pub fn seek_to(&self, position: f32) {
        let Some(app) = self.app() else {
            return;
        };
        let progress = position.clamp(0.0, 1.0);
        self.playhead_timer.stop();
        match self.player.borrow_mut().play_from_fraction(progress) {
            Ok(_) => {
                app.set_playhead_position(progress);
                app.set_playhead_visible(true);
                self.set_status(&app, "Playing audio", StatusState::Info);
                self.start_playhead_updates();
            }
            Err(error) => {
                self.set_status(&app, error, StatusState::Error);
                app.set_playhead_visible(false);
            }
        }
    }

    pub fn handle_add_source(&self) {
        let Some(app) = self.app() else {
            return;
        };
        let Some(path) = FileDialog::new().pick_folder() else {
            return;
        };
        let normalized = config::normalize_path(path.as_path());
        if !normalized.is_dir() {
            self.set_status(&app, "Please select a directory", StatusState::Warning);
            return;
        }
        let mut sources = self.sources.borrow_mut();
        if sources.iter().any(|s| s.root == normalized) {
            self.set_status(&app, "Source already added", StatusState::Info);
            return;
        }
        let source = SampleSource::new(normalized.clone());
        if let Err(error) = SourceDatabase::open(&normalized) {
            self.set_status(
                &app,
                format!("Failed to create database: {error}"),
                StatusState::Error,
            );
            return;
        }
        sources.push(source.clone());
        drop(sources);
        if let Err(error) = self.save_sources() {
            self.set_status(
                &app,
                format!("Failed to save config: {error}"),
                StatusState::Error,
            );
        }
        self.refresh_sources(&app);
        self.select_source_by_id(&app, &source.id);
        self.start_scan_for(source, true);
    }

    pub fn handle_source_selected(&self, index: i32) {
        if index < 0 {
            return;
        }
        let Some(app) = self.app() else {
            return;
        };
        let Some(source) = self.sources.borrow().get(index as usize).cloned() else {
            return;
        };
        self.select_source_by_id(&app, &source.id);
        self.start_scan_for(source, false);
    }

    pub fn handle_wav_clicked(&self, index: i32) {
        if index < 0 {
            return;
        }
        let Some(source) = self.current_source() else {
            return;
        };
        let Some(entry) = self.wav_entries.borrow().get(index as usize).cloned() else {
            return;
        };
        let full_path = source.root.join(&entry.relative_path);
        if !full_path.exists() {
            self.prune_missing_entry(&source, &entry);
            if let Some(app) = self.app() {
                self.refresh_wavs(&app);
                self.set_status(
                    &app,
                    "File missing on disk. Removed from library.",
                    StatusState::Warning,
                );
            }
            return;
        }
        self.handle_drop(full_path.as_path());
    }

    pub fn handle_update_source(&self, index: i32) {
        if index < 0 {
            return;
        }
        let Some(source) = self.sources.borrow().get(index as usize).cloned() else {
            return;
        };
        self.start_scan_for(source, true);
    }

    pub fn handle_remove_source(&self, index: i32) {
        if index < 0 {
            return;
        }
        let Some(app) = self.app() else {
            return;
        };
        let removed = {
            let mut sources = self.sources.borrow_mut();
            if (index as usize) >= sources.len() {
                return;
            }
            sources.remove(index as usize)
        };
        self.scan_tracker.borrow_mut().forget(&removed.id);
        let mut selected = self.selected_source.borrow_mut();
        if selected.as_ref().is_some_and(|id| id == &removed.id) {
            *selected = None;
        }
        drop(selected);
        if let Err(error) = self.save_sources() {
            self.set_status(
                &app,
                format!("Failed to save config: {error}"),
                StatusState::Error,
            );
            return;
        }
        self.refresh_sources(&app);
        if self.selected_source.borrow().is_none() {
            self.select_first_source(&app);
        } else {
            self.refresh_wavs(&app);
        }
        self.set_status(&app, "Source removed", StatusState::Info);
    }

    fn app(&self) -> Option<HelloWorld> {
        self.app.borrow().as_ref().and_then(|a| a.upgrade())
    }

    fn load_sources(&self, app: &HelloWorld) {
        match config::load_or_default() {
            Ok(cfg) => {
                self.sources.replace(cfg.sources);
                self.refresh_sources(app);
                self.select_first_source(app);
            }
            Err(error) => self.set_status(
                app,
                format!("Config load failed: {error}"),
                StatusState::Error,
            ),
        }
    }

    fn save_sources(&self) -> Result<(), config::ConfigError> {
        config::save(&AppConfig {
            sources: self.sources.borrow().clone(),
        })
    }

    fn refresh_sources(&self, app: &HelloWorld) {
        let rows = self
            .sources
            .borrow()
            .iter()
            .map(Self::source_row)
            .collect::<Vec<_>>();
        let model = Rc::new(slint::VecModel::from(rows));
        app.set_sources(model.into());
        let index = self
            .selected_source
            .borrow()
            .as_ref()
            .and_then(|id| self.sources.borrow().iter().position(|s| &s.id == id))
            .map(|i| i as i32)
            .unwrap_or(-1);
        app.set_selected_source(index);
        app.set_source_menu_index(-1);
    }

    fn select_first_source(&self, app: &HelloWorld) {
        if let Some(first) = self.sources.borrow().first().cloned() {
            self.select_source_by_id(app, &first.id);
            self.start_scan_for(first, false);
        } else {
            app.set_wavs(Rc::<slint::VecModel<WavRow>>::default().into());
        }
    }

    fn select_source_by_id(&self, app: &HelloWorld, id: &SourceId) {
        self.selected_source.replace(Some(id.clone()));
        self.refresh_sources(app);
        self.refresh_wavs(app);
    }

    fn current_source(&self) -> Option<SampleSource> {
        let selected = self.selected_source.borrow().clone()?;
        self.sources
            .borrow()
            .iter()
            .find(|s| s.id == selected)
            .cloned()
    }

    fn prune_missing_entry(&self, source: &SampleSource, entry: &WavEntry) {
        if let Ok(db) = SourceDatabase::open(&source.root) {
            let _ = db.remove_file(&entry.relative_path);
        }
        self.wav_entries
            .borrow_mut()
            .retain(|e| e.relative_path != entry.relative_path);
    }

    fn refresh_wavs(&self, app: &HelloWorld) {
        let Some(source) = self.current_source() else {
            app.set_wavs(Rc::<slint::VecModel<WavRow>>::default().into());
            return;
        };
        match SourceDatabase::open(&source.root).and_then(|db| db.list_files()) {
            Ok(entries) => {
                let unchanged = Self::wav_entries_match(&self.wav_entries.borrow(), &entries);
                if !unchanged {
                    self.wav_entries.replace(entries.clone());
                    let rows = entries.iter().map(Self::wav_row).collect::<Vec<_>>();
                    let model = Rc::new(slint::VecModel::from(rows));
                    app.set_wavs(model.into());
                }
                self.set_status(
                    app,
                    format!("{} wav files loaded", entries.len()),
                    StatusState::Info,
                );
            }
            Err(error) => self.set_status(
                app,
                format!("Failed to load wavs: {error}"),
                StatusState::Error,
            ),
        }
    }

    fn start_scan_for(&self, source: SampleSource, force: bool) {
        if *self.shutting_down.borrow() {
            return;
        }
        {
            let tracker = self.scan_tracker.borrow();
            if !tracker.can_start(&source.id, force) {
                if tracker.is_active(&source.id) {
                    if let Some(app) = self.app() {
                        self.set_status(&app, "Scan already in progress", StatusState::Info);
                    }
                } else if let Some(app) = self.app() {
                    self.set_status(&app, "Using existing scan results", StatusState::Info);
                }
                return;
            }
        }
        self.scan_tracker.borrow_mut().mark_started(&source.id);
        let tx = self.scan_tx.clone();
        if let Some(app) = self.app() {
            self.set_status(
                &app,
                format!("Scanning {}", source.root.display()),
                StatusState::Busy,
            );
        }
        thread::spawn(move || {
            let result = (|| -> Result<ScanStats, ScanError> {
                let db = SourceDatabase::open(&source.root)?;
                scan_once(&db)
            })();
            let _ = tx.send(ScanJobResult {
                source_id: source.id,
                result,
            });
        });
    }

    fn start_scan_polling(&self) {
        if *self.shutting_down.borrow() {
            return;
        }
        let poller = self.clone();
        self.scan_poll_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(200),
            move || poller.process_scan_queue(),
        );
    }

    fn process_scan_queue(&self) {
        let Some(app) = self.app() else {
            return;
        };
        while let Ok(message) = self.scan_rx.borrow().try_recv() {
            self.handle_scan_result(&app, message);
        }
    }

    fn handle_scan_result(&self, app: &HelloWorld, message: ScanJobResult) {
        if !self
            .sources
            .borrow()
            .iter()
            .any(|source| source.id == message.source_id)
        {
            self.scan_tracker.borrow_mut().forget(&message.source_id);
            return;
        }
        match message.result {
            Ok(stats) => {
                self.scan_tracker
                    .borrow_mut()
                    .mark_completed(&message.source_id);
                let state = if self.scan_tracker.borrow().has_active() {
                    StatusState::Busy
                } else {
                    StatusState::Info
                };
                self.set_status(
                    app,
                    format!(
                        "Scan complete: {} added, {} updated, {} removed",
                        stats.added, stats.updated, stats.removed
                    ),
                    state,
                );
                if self
                    .selected_source
                    .borrow()
                    .as_ref()
                    .is_some_and(|id| id == &message.source_id)
                {
                    self.refresh_wavs(app);
                }
            }
            Err(error) => {
                self.scan_tracker
                    .borrow_mut()
                    .mark_failed(&message.source_id);
                self.set_status(app, format!("Scan failed: {error}"), StatusState::Error);
            }
        }
    }

    fn shutdown(&self) {
        *self.shutting_down.borrow_mut() = true;
        self.scan_poll_timer.stop();
        self.playhead_timer.stop();
        self.player.borrow_mut().stop();
        mem::forget(self.player.clone());
    }

    fn start_playhead_updates(&self) {
        self.playhead_timer.stop();
        let timer = self.playhead_timer.clone();
        let app = self.app.clone();
        let player = self.player.clone();
        let timer_for_tick = timer.clone();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(30),
            move || Self::tick_playhead(&app, &player, &timer_for_tick),
        );
    }

    fn tick_playhead(
        app_handle: &Rc<RefCell<Option<slint::Weak<HelloWorld>>>>,
        player: &Rc<RefCell<AudioPlayer>>,
        timer: &slint::Timer,
    ) {
        let Some(app) = app_handle.borrow().as_ref().and_then(|a| a.upgrade()) else {
            timer.stop();
            return;
        };
        let mut player = player.borrow_mut();
        let Some(progress) = player.progress() else {
            app.set_playhead_visible(false);
            timer.stop();
            return;
        };
        if !player.is_playing() {
            app.set_playhead_visible(false);
            timer.stop();
            return;
        }
        app.set_playhead_visible(true);
        app.set_playhead_position(progress);
        if progress >= 1.0 {
            player.stop();
            app.set_playhead_visible(false);
            timer.stop();
        }
    }

    fn source_row(source: &SampleSource) -> SourceRow {
        let name = source
            .root
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.to_string())
            .unwrap_or_else(|| source.root.to_string_lossy().to_string());
        SourceRow {
            name: name.into(),
            path: source.root.to_string_lossy().to_string().into(),
        }
    }

    fn wav_row(entry: &WavEntry) -> WavRow {
        WavRow {
            name: entry.relative_path.to_string_lossy().to_string().into(),
            path: entry.relative_path.to_string_lossy().to_string().into(),
        }
    }

    fn wav_entries_match(existing: &[WavEntry], incoming: &[WavEntry]) -> bool {
        if existing.len() != incoming.len() {
            return false;
        }
        existing.iter().zip(incoming.iter()).all(|(old, new)| {
            old.relative_path == new.relative_path
                && old.file_size == new.file_size
                && old.modified_ns == new.modified_ns
        })
    }

    fn set_status(&self, app: &HelloWorld, text: impl Into<SharedString>, state: StatusState) {
        let (badge, color) = Self::status_badge(state);
        app.set_status_badge_text(badge);
        app.set_status_badge_color(color);
        app.set_status_text(text.into());
    }

    fn status_badge(state: StatusState) -> (SharedString, Color) {
        match state {
            StatusState::Idle => ("Idle".into(), Color::from_rgb_u8(42, 42, 42)),
            StatusState::Busy => ("Scanning".into(), Color::from_rgb_u8(31, 139, 255)),
            StatusState::Info => ("Info".into(), Color::from_rgb_u8(64, 140, 112)),
            StatusState::Warning => ("Warning".into(), Color::from_rgb_u8(192, 138, 43)),
            StatusState::Error => ("Error".into(), Color::from_rgb_u8(192, 57, 43)),
        }
    }

    fn is_wav(path: &std::path::Path) -> bool {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
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
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && !event.repeat
                    && event.physical_key == PhysicalKey::Code(KeyCode::Space) =>
            {
                self.play_audio()
            }
            _ => EventResult::Propagate,
        }
    }
}

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
    attach_callbacks(&app, &drop_handler);
    app.run()?;
    Ok(())
}

fn attach_callbacks(app: &HelloWorld, drop_handler: &DropHandler) {
    let seek_handler = drop_handler.clone();
    app.on_seek_requested(move |position| seek_handler.seek_to(position));
    let add_handler = drop_handler.clone();
    app.on_add_source(move || add_handler.handle_add_source());
    let source_handler = drop_handler.clone();
    app.on_source_selected(move |index| source_handler.handle_source_selected(index));
    let update_handler = drop_handler.clone();
    app.on_source_update_requested(move |index| update_handler.handle_update_source(index));
    let remove_handler = drop_handler.clone();
    app.on_source_remove_requested(move |index| remove_handler.handle_remove_source(index));
    let wav_handler = drop_handler.clone();
    app.on_wav_clicked(move |index| wav_handler.handle_wav_clicked(index));
    let close_handler = drop_handler.clone();
    app.on_close_requested(move || {
        close_handler.shutdown();
        let _ = slint::quit_event_loop();
    });
    app.window().set_fullscreen(true);
}
