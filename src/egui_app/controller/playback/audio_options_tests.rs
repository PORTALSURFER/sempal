use super::audio_options::normalize_audio_options;
use crate::audio::{AudioDeviceSummary, AudioHostSummary};

fn host(id: &str, is_default: bool) -> AudioHostSummary {
    AudioHostSummary {
        id: id.to_string(),
        label: id.to_string(),
        is_default,
    }
}

fn device(host_id: &str, name: &str, is_default: bool) -> AudioDeviceSummary {
    AudioDeviceSummary {
        host_id: host_id.to_string(),
        name: name.to_string(),
        is_default,
    }
}

#[test]
fn missing_host_falls_back_to_default() {
    let hosts = vec![host("default", true), host("alt", false)];
    let normalized = normalize_audio_options(
        Some("missing".to_string()),
        None,
        None,
        &hosts,
        |_host| Ok(Vec::new()),
        |_host, _device| Vec::new(),
        "system default output",
    );

    assert_eq!(normalized.host_id.as_deref(), Some("default"));
    assert_eq!(
        normalized.warning.as_deref(),
        Some("Host missing unavailable; using system default")
    );
}

#[test]
fn missing_device_falls_back_to_default_device() {
    let hosts = vec![host("default", true)];
    let normalized = normalize_audio_options(
        Some("default".to_string()),
        Some("gone".to_string()),
        None,
        &hosts,
        |_host| Ok(vec![device("default", "Built-in", true)]),
        |_host, _device| Vec::new(),
        "system default output",
    );

    assert_eq!(normalized.device_name.as_deref(), Some("Built-in"));
    assert_eq!(
        normalized.warning.as_deref(),
        Some("Device gone unavailable; using Built-in")
    );
}

#[test]
fn unsupported_sample_rate_falls_back_to_first_supported() {
    let hosts = vec![host("default", true)];
    let normalized = normalize_audio_options(
        Some("default".to_string()),
        Some("Built-in".to_string()),
        Some(44_100),
        &hosts,
        |_host| Ok(vec![device("default", "Built-in", true)]),
        |_host, _device| vec![48_000, 96_000],
        "system default output",
    );

    assert_eq!(normalized.sample_rate, Some(48_000));
    assert_eq!(
        normalized.warning.as_deref(),
        Some("Sample rate 44100 unsupported; using 48000")
    );
}
