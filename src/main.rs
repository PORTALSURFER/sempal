use std::{cell::RefCell, rc::Rc};

use slint::winit_030::{self, CustomApplicationHandler, EventResult};

slint::slint! {
    // Simple hello world window with centered text and a status bar.
    export component HelloWorld inherits Window {
        width: 360px;
        height: 220px;
        in-out property <string> status_text: "Drop a .wav file";

        VerticalLayout {
            spacing: 0px;

            Rectangle {
                Text {
                    text: "Hello, world!";
                    horizontal-alignment: center;
                    vertical-alignment: center;
                    width: parent.width;
                    height: parent.height;
                }
            }

            Rectangle {
                height: 32px;
                background: #00000033;
                Text {
                    text: root.status_text;
                    vertical-alignment: center;
                    width: parent.width;
                    height: parent.height;
                }
            }
        }
    }
}

#[derive(Clone, Default)]
struct DropHandler {
    app: Rc<RefCell<Option<slint::Weak<HelloWorld>>>>,
}

impl DropHandler {
    fn set_app(&self, app: &HelloWorld) {
        self.app.replace(Some(app.as_weak()));
    }

    fn handle_drop(&self, path: &std::path::Path) {
        let Some(app) = self.app.borrow().as_ref().and_then(|a| a.upgrade()) else {
            return;
        };

        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
        {
            app.set_status_text("Unsupported file type".into());
            return;
        }

        let status = match std::fs::read(path) {
            Ok(_) => format!("Loaded {}", path.display()),
            Err(error) => format!("Failed to load {}: {error}", path.display()),
        };

        app.set_status_text(status.into());
    }
}

impl CustomApplicationHandler for DropHandler {
    fn window_event(
        &mut self,
        _event_loop: &winit_030::winit::event_loop::ActiveEventLoop,
        _window_id: winit_030::winit::window::WindowId,
        _winit_window: Option<&winit_030::winit::window::Window>,
        _slint_window: Option<&slint::Window>,
        event: &winit_030::winit::event::WindowEvent,
    ) -> EventResult {
        if let winit_030::winit::event::WindowEvent::DroppedFile(path_buf) = event {
            self.handle_drop(path_buf.as_path());
        }

        EventResult::Propagate
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let drop_handler = DropHandler::default();

    slint::BackendSelector::new()
        .require_wgpu_27(slint::wgpu_27::WGPUConfiguration::default())
        .with_winit_custom_application_handler(drop_handler.clone())
        .select()?;

    let app = HelloWorld::new()?;
    drop_handler.set_app(&app);

    app.run()?;

    Ok(())
}
