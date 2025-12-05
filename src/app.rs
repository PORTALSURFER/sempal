use std::{cell::RefCell, rc::Rc};

use crate::audio::AudioPlayer;
use crate::file_browser::{EntryAction, FileBrowser, FileEntry};
use crate::ui::{FileRow, HelloWorld};
use crate::waveform::WaveformRenderer;
use slint::winit_030::{
    self, CustomApplicationHandler, EventResult,
    winit::event::{ElementState, WindowEvent},
    winit::keyboard::{KeyCode, PhysicalKey},
};
use slint::ComponentHandle;

#[derive(Clone)]
pub struct DropHandler {
    app: Rc<RefCell<Option<slint::Weak<HelloWorld>>>>,
    renderer: WaveformRenderer,
    player: Rc<RefCell<AudioPlayer>>,
    playhead_timer: Rc<slint::Timer>,
    file_browser: Rc<RefCell<FileBrowser>>,
    file_entries: Rc<RefCell<Vec<FileEntry>>>,
}

impl DropHandler {
    pub fn new(renderer: WaveformRenderer, player: Rc<RefCell<AudioPlayer>>) -> Self {
        Self {
            app: Rc::new(RefCell::new(None)),
            renderer,
            player,
            playhead_timer: Rc::new(slint::Timer::default()),
            file_browser: Rc::new(RefCell::new(FileBrowser::new())),
            file_entries: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub fn set_app(&self, app: &HelloWorld) {
        self.app.replace(Some(app.as_weak()));
        self.refresh_disks(app);
        self.refresh_files(app);
    }

    pub fn handle_drop(&self, path: &std::path::Path) {
        let Some(app) = self.app() else {
            return;
        };

        if !Self::is_wav(path) {
            app.set_status_text("Unsupported file type (please drop a .wav)".into());
            return;
        }

        match self.renderer.load_waveform(path) {
            Ok(loaded) => {
                let message = format!("Loaded {}", path.display());
                app.set_waveform(loaded.image);
                {
                    let mut player = self.player.borrow_mut();
                    player.stop();
                    player.set_audio(loaded.audio_bytes, loaded.duration_seconds);
                }
                self.playhead_timer.stop();
                app.set_playhead_position(0.0);
                app.set_playhead_visible(false);
                app.set_status_text(message.into());
            }
            Err(error) => app.set_status_text(error.into()),
        }
    }

    pub fn handle_disk_selected(&self, index: i32) {
        let Some(app) = self.app() else {
            return;
        };
        if index < 0 {
            return;
        }
        self.file_browser
            .borrow_mut()
            .select_disk(index as usize);
        self.refresh_disks(&app);
        self.refresh_files(&app);
    }

    pub fn handle_file_clicked(&self, index: i32) {
        let Some(app) = self.app() else {
            return;
        };
        let Some(entry) = self.file_entries.borrow().get(index as usize).cloned() else {
            return;
        };

        let action = {
            let mut browser = self.file_browser.borrow_mut();
            browser.activate_entry(&entry)
        };

        match action {
            EntryAction::OpenDir => self.refresh_files(&app),
            EntryAction::PlayFile(path) => self.handle_drop(&path),
            EntryAction::None => app.set_status_text("Double-click a folder to open it".into()),
        }
    }

    pub fn go_up_directory(&self) {
        let Some(app) = self.app() else {
            return;
        };
        self.file_browser.borrow_mut().go_up();
        self.refresh_disks(&app);
        self.refresh_files(&app);
    }

    pub fn play_audio(&self) -> EventResult {
        let Some(app) = self.app() else {
            return EventResult::Propagate;
        };

        match self.player.borrow_mut().play() {
            Ok(_) => {
                app.set_status_text("Playing audio".into());
                self.start_playhead_updates();
                EventResult::PreventDefault
            }
            Err(error) => {
                app.set_status_text(error.into());
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
                app.set_status_text("Playing audio".into());
                self.start_playhead_updates();
            }
            Err(error) => {
                app.set_status_text(error.into());
                app.set_playhead_visible(false);
            }
        }
    }

    fn app(&self) -> Option<HelloWorld> {
        self.app
            .borrow()
            .as_ref()
            .and_then(|a| a.upgrade())
    }

    fn refresh_disks(&self, app: &HelloWorld) {
        let browser = self.file_browser.borrow();
        let model = Rc::new(slint::VecModel::from(browser.mounts()));
        app.set_disks(model.into());
        app.set_selected_disk(browser.selected_disk());
    }

    fn refresh_files(&self, app: &HelloWorld) {
        let entries = self.file_browser.borrow().entries();
        let model = Rc::new(slint::VecModel::from(
            entries
                .iter()
                .map(|e| FileRow {
                    name: e.name.clone().into(),
                    is_dir: e.is_dir,
                })
                .collect::<Vec<_>>(),
        ));
        app.set_files(model.into());
        self.file_entries.replace(entries);
    }

    fn start_playhead_updates(&self) {
        self.playhead_timer.stop();
        let timer = self.playhead_timer.clone();
        let app = self.app.clone();
        let player = self.player.clone();
        let timer_for_tick = timer.clone();

        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(30),
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
    app.set_waveform(renderer.empty_image());
    drop_handler.set_app(&app);
    attach_callbacks(&app, &drop_handler);

    app.run()?;

    Ok(())
}

fn attach_callbacks(app: &HelloWorld, drop_handler: &DropHandler) {
    let seek_handler = drop_handler.clone();
    app.on_seek_requested(move |position| seek_handler.seek_to(position));
    let disk_handler = drop_handler.clone();
    app.on_disk_selected(move |index| disk_handler.handle_disk_selected(index));
    let file_handler = drop_handler.clone();
    app.on_file_clicked(move |index| file_handler.handle_file_clicked(index));
    let go_up_handler = drop_handler.clone();
    app.on_go_up_directory(move || go_up_handler.go_up_directory());
    app.on_close_requested(|| {
        let _ = slint::quit_event_loop();
    });
    app.window().set_fullscreen(true);
}
