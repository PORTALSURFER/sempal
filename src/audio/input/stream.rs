use cpal;
use cpal::traits::DeviceTrait;
use tracing::warn;

use super::AudioInputError;

pub(crate) fn build_input_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    selection: StreamChannelSelection,
    mut on_samples: impl FnMut(Vec<f32>) + Send + 'static,
) -> Result<cpal::Stream, AudioInputError> {
    let err_fn = move |err| {
        warn!("Audio input stream error: {err}");
    };
    let selection = std::sync::Arc::new(selection);
    match sample_format {
        cpal::SampleFormat::F32 => device
            .build_input_stream(
                config,
                move |data: &[f32], _| {
                    let samples = extract_selected_samples(data, &selection, |sample| *sample);
                    on_samples(samples);
                },
                err_fn,
                None,
            )
            .map_err(|source| AudioInputError::OpenStream { source }),
        cpal::SampleFormat::I16 => device
            .build_input_stream(
                config,
                move |data: &[i16], _| {
                    let samples = extract_selected_samples(data, &selection, |sample| {
                        *sample as f32 / i16::MAX as f32
                    });
                    on_samples(samples);
                },
                err_fn,
                None,
            )
            .map_err(|source| AudioInputError::OpenStream { source }),
        cpal::SampleFormat::U16 => device
            .build_input_stream(
                config,
                move |data: &[u16], _| {
                    let samples = extract_selected_samples(data, &selection, |sample| {
                        (*sample as f32 - 32_768.0) / 32_768.0
                    });
                    on_samples(samples);
                },
                err_fn,
                None,
            )
            .map_err(|source| AudioInputError::OpenStream { source }),
        cpal::SampleFormat::I32 => device
            .build_input_stream(
                config,
                move |data: &[i32], _| {
                    let samples = extract_selected_samples(data, &selection, |sample| {
                        *sample as f32 / i32::MAX as f32
                    });
                    on_samples(samples);
                },
                err_fn,
                None,
            )
            .map_err(|source| AudioInputError::OpenStream { source }),
        cpal::SampleFormat::U32 => device
            .build_input_stream(
                config,
                move |data: &[u32], _| {
                    let samples = extract_selected_samples(data, &selection, |sample| {
                        (*sample as f32 - 2_147_483_648.0) / 2_147_483_648.0
                    });
                    on_samples(samples);
                },
                err_fn,
                None,
            )
            .map_err(|source| AudioInputError::OpenStream { source }),
        cpal::SampleFormat::F64 => device
            .build_input_stream(
                config,
                move |data: &[f64], _| {
                    let samples =
                        extract_selected_samples(data, &selection, |sample| *sample as f32);
                    on_samples(samples);
                },
                err_fn,
                None,
            )
            .map_err(|source| AudioInputError::OpenStream { source }),
        format => Err(AudioInputError::RecordingFailed {
            detail: format!("Unsupported input sample format {format:?}"),
        }),
    }
}

#[derive(Clone)]
pub(crate) struct StreamChannelSelection {
    stream_channels: usize,
    selected_channels: Vec<usize>,
}

impl StreamChannelSelection {
    pub(crate) fn new(stream_channels: u16, selected_channels: &[u16]) -> Self {
        let stream_channels = stream_channels.max(1) as usize;
        let mut selected_channels: Vec<usize> = selected_channels
            .iter()
            .copied()
            .filter(|channel| *channel >= 1)
            .map(|channel| (channel - 1) as usize)
            .collect();
        if selected_channels.is_empty() && stream_channels > 0 {
            selected_channels.push(0);
        }
        Self {
            stream_channels,
            selected_channels,
        }
    }
}

fn extract_selected_samples<T>(
    data: &[T],
    selection: &StreamChannelSelection,
    mut convert: impl FnMut(&T) -> f32,
) -> Vec<f32> {
    let mut samples = Vec::with_capacity(
        data.len() / selection.stream_channels.max(1) * selection.selected_channels.len(),
    );
    for frame in data.chunks(selection.stream_channels.max(1)) {
        for &channel_idx in &selection.selected_channels {
            if let Some(sample) = frame.get(channel_idx) {
                samples.push(convert(sample));
            }
        }
    }
    samples
}
