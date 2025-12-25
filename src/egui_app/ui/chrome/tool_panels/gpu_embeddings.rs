use eframe::egui::{self, RichText};

use crate::egui_app::ui::EguiApp;
use crate::egui_app::ui::style;
use super::section_label;

impl EguiApp {
    pub(super) fn render_gpu_embeddings_panel(&mut self, ui: &mut egui::Ui) {
        let palette = style::palette();
        section_label(ui, "GPU embeddings");
        let mut backend = self.controller.panns_backend();
        egui::ComboBox::from_id_salt("panns_backend_combo")
            .selected_text(match backend {
                crate::sample_sources::config::PannsBackendChoice::Wgpu => "WGPU (Vulkan)",
                crate::sample_sources::config::PannsBackendChoice::Cuda => "CUDA",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut backend,
                    crate::sample_sources::config::PannsBackendChoice::Wgpu,
                    "WGPU (Vulkan)",
                );
                let cuda_enabled = cfg!(feature = "panns-cuda");
                ui.add_enabled(
                    cuda_enabled,
                    egui::SelectableLabel::new(
                        backend == crate::sample_sources::config::PannsBackendChoice::Cuda,
                        "CUDA",
                    ),
                )
                .on_disabled_hover_text("CUDA backend not enabled in this build");
            });
        if backend != self.controller.panns_backend() {
            self.controller.set_panns_backend(backend);
        }

        let wgpu_active = backend == crate::sample_sources::config::PannsBackendChoice::Wgpu;
        ui.add_enabled_ui(wgpu_active, |ui| {
            let mut power = self.controller.wgpu_power_preference();
            let power_combo = egui::ComboBox::from_id_salt("wgpu_power_combo")
                .selected_text(match power {
                    crate::sample_sources::config::WgpuPowerPreference::Default => "Default",
                    crate::sample_sources::config::WgpuPowerPreference::Low => "Low power",
                    crate::sample_sources::config::WgpuPowerPreference::High => "High performance",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut power,
                        crate::sample_sources::config::WgpuPowerPreference::Default,
                        "Default",
                    );
                    ui.selectable_value(
                        &mut power,
                        crate::sample_sources::config::WgpuPowerPreference::Low,
                        "Low power",
                    );
                    ui.selectable_value(
                        &mut power,
                        crate::sample_sources::config::WgpuPowerPreference::High,
                        "High performance",
                    );
                })
                .response;
            if power_combo.changed() {
                self.controller.set_wgpu_power_preference(power);
            }

            let mut adapter_name = self
                .controller
                .wgpu_adapter_name()
                .unwrap_or_default()
                .to_string();
            let adapter_edit = ui
                .add(
                    egui::TextEdit::singleline(&mut adapter_name)
                        .hint_text("Adapter name filter (optional)"),
                )
                .on_hover_text("Match a GPU adapter name substring (WGPU only)");
            if adapter_edit.changed() {
                self.controller.set_wgpu_adapter_name(adapter_name);
            }
        });

        ui.label(
            RichText::new("Changes apply on next start.")
                .color(palette.text_muted),
        );
        if let Ok(value) = std::env::var("SEMPAL_PANNS_BACKEND") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                ui.label(
                    RichText::new(format!("Env override: SEMPAL_PANNS_BACKEND={}", trimmed))
                        .color(palette.text_muted),
                );
            }
        }
        if let Ok(value) = std::env::var("WGPU_ADAPTER_NAME") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                ui.label(
                    RichText::new(format!("Env override: WGPU_ADAPTER_NAME={}", trimmed))
                        .color(palette.text_muted),
                );
            }
        }
        if let Ok(value) = std::env::var("WGPU_POWER_PREFERENCE") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "Env override: WGPU_POWER_PREFERENCE={}",
                        trimmed
                    ))
                    .color(palette.text_muted),
                );
            }
        }
    }
}
