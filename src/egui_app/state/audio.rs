use crate::audio::{AudioInputConfig, AudioOutputConfig, ResolvedInput, ResolvedOutput};

/// UI state for audio host/device selection.
#[derive(Clone, Debug, Default)]
pub struct AudioOptionsState {
    /// Available output hosts for selection.
    pub hosts: Vec<AudioHostView>,
    /// Available output devices for the selected host.
    pub devices: Vec<AudioDeviceView>,
    /// Supported output sample rates.
    pub sample_rates: Vec<u32>,
    /// User-selected output configuration.
    pub selected: AudioOutputConfig,
    /// Output configuration currently applied to the player.
    pub applied: Option<ActiveAudioOutput>,
    /// Warning message for output selection, if any.
    pub warning: Option<String>,
    /// Available input hosts for selection.
    pub input_hosts: Vec<AudioHostView>,
    /// Available input devices for the selected host.
    pub input_devices: Vec<AudioDeviceView>,
    /// Supported input sample rates.
    pub input_sample_rates: Vec<u32>,
    /// Channel count supported by the selected input device.
    pub input_channel_count: u16,
    /// User-selected input configuration.
    pub input_selected: AudioInputConfig,
    /// Input configuration currently applied to the recorder.
    pub input_applied: Option<ActiveAudioInput>,
    /// Warning message for input selection, if any.
    pub input_warning: Option<String>,
    /// Whether the audio options panel is open.
    pub panel_open: bool,
}

/// Render-friendly audio host descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioHostView {
    /// Host identifier reported by cpal.
    pub id: String,
    /// User-facing label for the host.
    pub label: String,
    /// Whether this host is the system default.
    pub is_default: bool,
}

/// Render-friendly audio device descriptor scoped to a host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDeviceView {
    /// Host identifier that owns the device.
    pub host_id: String,
    /// User-facing device name.
    pub name: String,
    /// Whether this device is the host default.
    pub is_default: bool,
}

/// Active audio output the player is currently using.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveAudioOutput {
    /// Host identifier used to open the stream.
    pub host_id: String,
    /// Device name used to open the stream.
    pub device_name: String,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Buffer size in frames, if configured.
    pub buffer_size_frames: Option<u32>,
    /// Channel count for the output stream.
    pub channel_count: u16,
}

impl From<&ResolvedOutput> for ActiveAudioOutput {
    fn from(output: &ResolvedOutput) -> Self {
        Self {
            host_id: output.host_id.clone(),
            device_name: output.device_name.clone(),
            sample_rate: output.sample_rate,
            buffer_size_frames: output.buffer_size_frames,
            channel_count: output.channel_count,
        }
    }
}

/// Active audio input the recorder is currently using.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveAudioInput {
    /// Host identifier used to open the stream.
    pub host_id: String,
    /// Device name used to open the stream.
    pub device_name: String,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Buffer size in frames, if configured.
    pub buffer_size_frames: Option<u32>,
    /// Channel count for the input stream.
    pub channel_count: u16,
}

impl From<&ResolvedInput> for ActiveAudioInput {
    fn from(input: &ResolvedInput) -> Self {
        Self {
            host_id: input.host_id.clone(),
            device_name: input.device_name.clone(),
            sample_rate: input.sample_rate,
            buffer_size_frames: input.buffer_size_frames,
            channel_count: input.channel_count,
        }
    }
}
