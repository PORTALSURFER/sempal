//! Entry point for the egui-based Sempal UI.
use eframe::egui;
use egui::viewport::IconData;
use sempal::audio::AudioPlayer;
use sempal::egui_app::ui::{EguiApp, MIN_VIEWPORT_SIZE};
use sempal::logging;
use sempal::waveform::WaveformRenderer;

/// Launch the egui UI.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Err(err) = logging::init() {
        eprintln!("Logging disabled: {err}");
    }

    let renderer = WaveformRenderer::new(680, 260);
    let player = None::<std::rc::Rc<std::cell::RefCell<AudioPlayer>>>;

    let mut viewport = egui::ViewportBuilder::default()
        .with_min_inner_size(MIN_VIEWPORT_SIZE)
        .with_maximized(true)
        .with_drag_and_drop(true);
    if let Some(icon) = load_app_icon() {
        viewport = viewport.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Sempal",
        native_options,
        Box::new(
            move |_cc| match EguiApp::new(renderer.clone(), player.clone()) {
                Ok(app) => Ok(Box::new(app)),
                Err(err) => Ok(Box::new(LaunchError { message: err })),
            },
        ),
    )?;
    Ok(())
}

fn load_app_icon() -> Option<IconData> {
    decode_icon(include_bytes!("../assets/logo3.ico")).or_else(|| {
        eprintln!("Failed to decode logo3.ico; falling back to PNG icon.");
        let fallback = decode_icon(include_bytes!("../assets/logo3.png"));
        if fallback.is_none() {
            eprintln!("Failed to decode logo3.png fallback for window icon.");
        }
        fallback
    })
}

/// Convert raw embedded bytes into icon-friendly RGBA data.
fn decode_icon(bytes: &[u8]) -> Option<IconData> {
    let image = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    Some(IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

/// Minimal fallback app to display initialization errors.
struct LaunchError {
    message: String,
}

impl eframe::App for LaunchError {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Failed to start UI");
                ui.label(&self.message);
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_icons_decode() {
        assert!(decode_icon(include_bytes!("../assets/logo3.ico")).is_some());
        assert!(decode_icon(include_bytes!("../assets/logo3.png")).is_some());
    }
}
