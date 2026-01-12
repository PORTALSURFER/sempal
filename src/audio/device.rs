use cpal::traits::DeviceTrait;

pub(crate) fn device_label(device: &cpal::Device) -> Option<String> {
    device
        .description()
        .ok()
        .map(|description| description.name().to_string())
}

pub(crate) fn host_label(id: &str) -> String {
    match id.to_ascii_lowercase().as_str() {
        "asio" => "ASIO".into(),
        "wasapi" => "WASAPI".into(),
        "coreaudio" => "Core Audio".into(),
        "alsa" => "ALSA".into(),
        "jack" => "JACK".into(),
        _ => id.to_uppercase(),
    }
}
