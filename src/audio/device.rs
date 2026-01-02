use cpal::traits::DeviceTrait;

pub(crate) fn device_label(device: &cpal::Device) -> Option<String> {
    device.name().ok()
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
