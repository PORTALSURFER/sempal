//! Entry point for the egui-based Sempal UI.
use eframe::egui;
use egui::viewport::IconData;
use sempal::audio::AudioPlayer;
use sempal::egui_app::ui::{EguiApp, MIN_VIEWPORT_SIZE};
use sempal::waveform::WaveformRenderer;
use std::env;

/// Launch the egui UI.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // SAFETY: wgpu picks its backend via this env var; we want to force Vulkan for all runs.
    unsafe {
        env::set_var("WGPU_BACKEND", "vulkan");
    }

    let renderer = WaveformRenderer::new(680, 260);
    let player = None::<std::rc::Rc<std::cell::RefCell<AudioPlayer>>>;

    let mut viewport = egui::ViewportBuilder::default()
        .with_min_inner_size(MIN_VIEWPORT_SIZE)
        .with_maximized(true);
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
    let bytes = include_bytes!("../assets/logo3.png");
    let image = image::load_from_memory(bytes).ok()?;
    let image = image.to_rgba8();
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
