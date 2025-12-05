mod audio;
mod waveform;

use std::{cell::RefCell, rc::Rc};

use audio::AudioPlayer;
use slint::winit_030::{
    self, CustomApplicationHandler, EventResult,
    winit::event::{ElementState, WindowEvent},
    winit::keyboard::{KeyCode, PhysicalKey},
};
use waveform::WaveformRenderer;

slint::slint! {
    export component HelloWorld inherits Window {
        width: 720px;
        height: 420px;
        in-out property <string> status_text: "Drop a .wav file";
        in-out property <image> waveform;
        in-out property <float> playhead_position: 0.0;
        in-out property <bool> playhead_visible: false;
        callback seek_requested(float);
        callback close_requested();

        VerticalLayout {
            spacing: 8px;
            padding: 0px;

            Rectangle {
                width: parent.width;
                height: 36px;
                background: #181818;
                border-width: 1px;
                border-color: #303030;

                HorizontalLayout {
                    padding: 8px;
                    spacing: 8px;

                    Text {
                        text: "Waveform Viewer";
                        color: #e0e0e0;
                        vertical-alignment: center;
                    }

                    Rectangle {
                        height: 1px;
                        horizontal-stretch: 1;
                        background: #00000000;
                    }

                    Rectangle {
                        width: 28px;
                        height: parent.height - 8px;
                        background: #2a2a2a;
                        border-width: 1px;
                        border-color: #404040;
                        border-radius: 4px;

                        Text {
                            text: "X";
                            horizontal-alignment: center;
                            vertical-alignment: center;
                            color: #e0e0e0;
                            width: parent.width;
                            height: parent.height;
                        }

                        TouchArea {
                            width: parent.width;
                            height: parent.height;
                            clicked => { root.close_requested(); }
                        }
                    }
                }
            }

            VerticalLayout {
                spacing: 8px;
                padding: 12px;

                Rectangle {
                    border-width: 1px;
                    border-color: #404040;
                    background: #101010;
                    VerticalLayout {
                        spacing: 8px;
                        padding: 8px;

                        Text {
                            text: "Waveform Viewer";
                            horizontal-alignment: center;
                            color: #e0e0e0;
                            font-size: 18px;
                            width: parent.width;
                        }

                        Rectangle {
                            width: parent.width;
                            height: 260px;
                            clip: true;

                            Image {
                                source: root.waveform;
                                width: parent.width;
                                height: parent.height;
                                image-fit: contain;
                                colorize: #00000000;
                            }

                            Rectangle {
                                visible: root.playhead_visible;
                                width: 2px;
                                height: parent.height;
                                x: (root.playhead_position * parent.width) - (self.width / 2);
                                background: #3399ff;
                                z: 1;
                            }

                            TouchArea {
                                width: parent.width;
                                height: parent.height;
                                clicked => {
                                    root.seek_requested(self.mouse-x / self.width);
                                }
                            }
                        }
                    }
                }

                Rectangle {
                    height: 32px;
                    background: #00000033;
                    border-width: 1px;
                    border-color: #303030;
                    Text {
                        text: root.status_text;
                        vertical-alignment: center;
                        width: parent.width;
                        height: parent.height;
                        color: #d0d0d0;
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
struct DropHandler {
    app: Rc<RefCell<Option<slint::Weak<HelloWorld>>>>,
    renderer: WaveformRenderer,
    player: Rc<RefCell<AudioPlayer>>,
    playhead_timer: Rc<slint::Timer>,
}

impl DropHandler {
    fn new(renderer: WaveformRenderer, player: Rc<RefCell<AudioPlayer>>) -> Self {
        Self {
            app: Rc::new(RefCell::new(None)),
            renderer,
            player,
            playhead_timer: Rc::new(slint::Timer::default()),
        }
    }

    fn set_app(&self, app: &HelloWorld) {
        self.app.replace(Some(app.as_weak()));
    }

    fn handle_drop(&self, path: &std::path::Path) {
        let Some(app) = self.app.borrow().as_ref().and_then(|a| a.upgrade()) else {
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

    fn is_wav(path: &std::path::Path) -> bool {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
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

    fn play_audio(&self) -> EventResult {
        let Some(app) = self.app.borrow().as_ref().and_then(|a| a.upgrade()) else {
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

    fn seek_to(&self, position: f32) {
        let Some(app) = self.app.borrow().as_ref().and_then(|a| a.upgrade()) else {
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let seek_handler = drop_handler.clone();
    app.on_seek_requested(move |position| seek_handler.seek_to(position));
    app.on_close_requested(|| {
        let _ = slint::quit_event_loop();
    });
    app.window().set_fullscreen(true);

    app.run()?;

    Ok(())
}
