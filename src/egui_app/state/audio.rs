use crate::audio::{AudioInputConfig, AudioOutputConfig, ResolvedInput, ResolvedOutput};

/// UI state for audio host/device selection.
#[derive(Clone, Debug, Default)]
pub struct AudioOptionsState {
    pub hosts: Vec<AudioHostView>,
    pub devices: Vec<AudioDeviceView>,
    pub sample_rates: Vec<u32>,
    pub selected: AudioOutputConfig,
    pub applied: Option<ActiveAudioOutput>,
    pub warning: Option<String>,
    pub input_hosts: Vec<AudioHostView>,
    pub input_devices: Vec<AudioDeviceView>,
    pub input_sample_rates: Vec<u32>,
    pub input_channel_count: u16,
    pub input_selected: AudioInputConfig,
    pub input_applied: Option<ActiveAudioInput>,
    pub input_warning: Option<String>,
    pub panel_open: bool,
}

/// Render-friendly audio host descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioHostView {
    pub id: String,
    pub label: String,
    pub is_default: bool,
}

/// Render-friendly audio device descriptor scoped to a host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDeviceView {
    pub host_id: String,
    pub name: String,
    pub is_default: bool,
}

/// Active audio output the player is currently using.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveAudioOutput {
    pub host_id: String,
    pub device_name: String,
    pub sample_rate: u32,
    pub buffer_size_frames: Option<u32>,
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
    pub host_id: String,
    pub device_name: String,
    pub sample_rate: u32,
    pub buffer_size_frames: Option<u32>,
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
