use radiant::prelude as ui;
use wavecrate::audio::{
    AudioOutputConfig, available_devices, available_hosts, supported_sample_rates,
};

use crate::native_app::app::{
    AudioOptionsRefreshResult, GuiMessage, NativeAppState, format_sample_rate_label,
};

impl NativeAppState {
    pub(in crate::native_app) fn queue_audio_options_refresh(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.cancel_audio_options_refresh();
        let config = self.audio.output_config.clone();
        let cancellation = context
            .business()
            .blocking_io("gui-audio-options-discovery")
            .cancellable()
            .latest(&mut self.background.audio_options_refresh_task)
            .run(
                move |context| discover_audio_options(&config, || context.is_cancelled()),
                GuiMessage::AudioOptionsRefreshFinished,
            );
        self.background.audio_options_refresh_cancel = Some(cancellation);
    }

    pub(in crate::native_app) fn cancel_audio_options_refresh(&mut self) {
        self.background.audio_options_refresh_task.cancel();
        if let Some(cancellation) = self.background.audio_options_refresh_cancel.take() {
            cancellation.cancel();
        }
    }

    pub(in crate::native_app) fn finish_audio_options_refresh(
        &mut self,
        completion: ui::TaskCompletion<AudioOptionsRefreshResult>,
    ) {
        let Some(result) = self
            .background
            .audio_options_refresh_task
            .finish_completion(completion)
        else {
            return;
        };
        self.background.audio_options_refresh_cancel = None;
        self.audio.hosts = result.hosts;
        self.audio.devices = result.devices;
        self.audio.sample_rates = result.sample_rates;
        if let Some(error) = result.error {
            self.audio.settings_error = Some(error);
        }
    }

    fn selected_audio_host_id_from(
        config: &AudioOutputConfig,
        hosts: &[wavecrate::audio::AudioHostSummary],
    ) -> Option<String> {
        config.host.clone().or_else(|| {
            hosts
                .iter()
                .find(|host| host.is_default)
                .or_else(|| hosts.first())
                .map(|host| host.id.clone())
        })
    }

    fn selected_audio_device_name_from(
        config: &AudioOutputConfig,
        devices: &[wavecrate::audio::AudioDeviceSummary],
    ) -> Option<String> {
        config.device.clone().or_else(|| {
            devices
                .iter()
                .find(|device| device.is_default)
                .or_else(|| devices.first())
                .map(|device| device.name.clone())
        })
    }

    pub(in crate::native_app) fn audio_engine_pill_label(&self) -> String {
        if self.audio.settings_error.is_some() {
            return String::from("OFF");
        }
        if self.background.audio_open.active().is_some() {
            return String::from("starting");
        }
        if self
            .background
            .audio_options_refresh_task
            .active()
            .is_some()
        {
            return String::from("loading");
        }
        self.audio
            .output_resolved
            .as_ref()
            .map(|output| format_sample_rate_label(output.sample_rate))
            .unwrap_or_else(|| String::from("no audio"))
    }

    pub(in crate::native_app) fn audio_engine_pill_style(&self) -> ui::WidgetStyle {
        if self.audio.settings_error.is_some() {
            ui::WidgetStyle::strong(ui::WidgetTone::Danger)
        } else if self.audio.output_resolved.is_some() {
            ui::WidgetStyle::subtle(ui::WidgetTone::Neutral)
        } else {
            ui::WidgetStyle::subtle(ui::WidgetTone::Warning)
        }
    }

    pub(in crate::native_app) fn audio_engine_detail_label(&self) -> String {
        if self.background.audio_open.active().is_some() {
            return String::from("Audio output starting");
        }
        if self
            .background
            .audio_options_refresh_task
            .active()
            .is_some()
            && self.audio.output_resolved.is_none()
        {
            return String::from("Loading audio options");
        }
        self.audio
            .output_resolved
            .as_ref()
            .map(|output| {
                let running_host = self.audio_host_label(output.host_id.as_str());
                let selected_host = self
                    .audio
                    .output_config
                    .host
                    .as_deref()
                    .map(|host| self.audio_host_label(host));
                let host_label = selected_host
                    .filter(|host| *host != running_host)
                    .map(|host| format!("{host} selected | using {running_host}"))
                    .unwrap_or(running_host);
                format!(
                    "{} | {} | {}",
                    host_label,
                    output.device_name,
                    format_sample_rate_label(output.sample_rate)
                )
            })
            .or_else(|| self.audio.settings_error.clone())
            .unwrap_or_else(|| String::from("Audio output idle"))
    }

    pub(super) fn audio_host_label(&self, id: &str) -> String {
        self.audio
            .hosts
            .iter()
            .find(|host| host.id == id)
            .map(|host| host.label.clone())
            .unwrap_or_else(|| id.to_string())
    }
}

fn discover_audio_options(
    config: &AudioOutputConfig,
    is_cancelled: impl Fn() -> bool,
) -> AudioOptionsRefreshResult {
    discover_audio_options_with(
        config,
        is_cancelled,
        available_hosts,
        available_devices,
        supported_sample_rates,
    )
}

fn discover_audio_options_with(
    config: &AudioOutputConfig,
    is_cancelled: impl Fn() -> bool,
    enumerate_hosts: impl FnOnce() -> Vec<wavecrate::audio::AudioHostSummary>,
    enumerate_devices: impl FnOnce(
        &str,
    ) -> Result<
        Vec<wavecrate::audio::AudioDeviceSummary>,
        wavecrate::audio::AudioOutputError,
    >,
    enumerate_sample_rates: impl FnOnce(
        &str,
        &str,
    ) -> Result<Vec<u32>, wavecrate::audio::AudioOutputError>,
) -> AudioOptionsRefreshResult {
    let hosts = enumerate_hosts();
    if is_cancelled() {
        return AudioOptionsRefreshResult {
            hosts,
            ..AudioOptionsRefreshResult::default()
        };
    }

    let host_id = NativeAppState::selected_audio_host_id_from(config, &hosts);
    let mut error = None;
    let devices = host_id
        .as_deref()
        .and_then(|host_id| match enumerate_devices(host_id) {
            Ok(devices) => Some(devices),
            Err(err) => {
                error = Some(err.to_string());
                None
            }
        })
        .unwrap_or_default();
    if is_cancelled() {
        return AudioOptionsRefreshResult {
            hosts,
            devices,
            error,
            ..AudioOptionsRefreshResult::default()
        };
    }

    let device_name = NativeAppState::selected_audio_device_name_from(config, &devices);
    let sample_rates = match (host_id.as_deref(), device_name.as_deref()) {
        (Some(host_id), Some(device_name)) => match enumerate_sample_rates(host_id, device_name) {
            Ok(rates) => rates,
            Err(err) => {
                error = Some(err.to_string());
                Vec::new()
            }
        },
        _ => Vec::new(),
    };

    AudioOptionsRefreshResult {
        hosts,
        devices,
        sample_rates,
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::Duration;

    #[test]
    fn injected_slow_discovery_stops_before_device_and_rate_work_when_cancelled() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_for_hosts = Arc::clone(&cancelled);
        let result = discover_audio_options_with(
            &AudioOutputConfig::default(),
            || cancelled.load(Ordering::Acquire),
            move || {
                std::thread::sleep(Duration::from_millis(5));
                cancelled_for_hosts.store(true, Ordering::Release);
                vec![wavecrate::audio::AudioHostSummary {
                    id: String::from("slow-host"),
                    label: String::from("Slow host"),
                    is_default: true,
                }]
            },
            |_| panic!("cancelled discovery must not enumerate devices"),
            |_, _| panic!("cancelled discovery must not enumerate sample rates"),
        );

        assert_eq!(result.hosts.len(), 1);
        assert!(result.devices.is_empty());
        assert!(result.sample_rates.is_empty());
    }
}
