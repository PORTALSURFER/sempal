//! egui preview binary for Sempal.

use sempal::egui_app::ui::EguiApp;
use sempal::waveform::WaveformRenderer;
use std::cell::RefCell;
use std::rc::Rc;

/// Launch the egui-based UI.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let renderer = WaveformRenderer::new(680, 260);
    let app_renderer = renderer.clone();
    let app_player: Option<Rc<RefCell<_>>> = None;
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 560.0])
            .with_min_inner_size([640.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Sempal (egui preview)",
        native_options,
        Box::new(move |_cc| match EguiApp::new(app_renderer.clone(), app_player.clone()) {
            Ok(app) => Box::new(app),
            Err(err) => Box::new(LaunchError { message: err }),
        }),
    )?;
    Ok(())
}

/// Minimal fallback app to display initialization errors.
struct LaunchError {
    message: String,
}

impl eframe::App for LaunchError {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Failed to start egui UI");
                ui.label(&self.message);
            });
        });
    }
}
