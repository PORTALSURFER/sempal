//! Time-stretch helpers for BPM-synced playback.

use std::f64::consts::PI;

const MIN_STRETCH_RATIO: f64 = 0.5;
const MAX_STRETCH_RATIO: f64 = 2.0;
const SILENCE_ENERGY: f32 = 1e-6;
const SIMILARITY_THRESHOLD: f32 = 0.2;

/// WSOLA time-stretcher tuned for rhythmic material.
pub(crate) struct Wsola {
    window_size: usize,
    hop_s: usize,
    search_radius: usize,
    window: Vec<f32>,
}

impl Wsola {
    /// Build a WSOLA helper for a given sample rate.
    pub(crate) fn new(sample_rate: u32) -> Self {
        let mut window_size = ((sample_rate.max(1) as f32) * 0.025).round() as usize;
        window_size = window_size.clamp(256, 4096);
        if window_size % 2 != 0 {
            window_size += 1;
        }
        let hop_s = window_size / 2;
        let search_radius = hop_s / 2;
        let window = hann_window(window_size);
        Self {
            window_size,
            hop_s,
            search_radius,
            window,
        }
    }

    /// Stretch interleaved samples using WSOLA, preserving pitch.
    pub(crate) fn stretch(&self, input: &[f32], channels: usize, ratio: f64) -> Vec<f32> {
        let channels = channels.max(1);
        if input.is_empty() || self.hop_s == 0 {
            return input.to_vec();
        }
        let ratio = ratio.clamp(MIN_STRETCH_RATIO, MAX_STRETCH_RATIO);
        if (ratio - 1.0).abs() < 1e-3 {
            return input.to_vec();
        }
        let input_frames = input.len() / channels;
        if input_frames < self.window_size * 2 {
            return input.to_vec();
        }
        let output_frames = ((input_frames as f64) / ratio)
            .round()
            .max(1.0) as usize;
        let mut output = vec![0.0; output_frames * channels];
        let mono_input = mono_from_interleaved(input, channels);
        let mut mono_output = vec![0.0; output_frames];

        let initial_frames = self.window_size.min(output_frames);
        for i in 0..initial_frames {
            let window = self.window[i];
            for ch in 0..channels {
                output[i * channels + ch] = input[i * channels + ch] * window;
            }
            mono_output[i] = mono_input[i] * window;
        }

        let mut analysis_pos = self.hop_s as f64 * ratio;
        let mut synthesis_pos = self.hop_s;
        let max_analysis_start = input_frames.saturating_sub(self.window_size);

        while synthesis_pos + self.window_size <= output_frames && max_analysis_start > 0 {
            let expected = analysis_pos.round() as isize;
            let expected_clamped =
                expected.clamp(0, max_analysis_start as isize) as usize;
            let search_start = expected
                .saturating_sub(self.search_radius as isize)
                .max(0) as usize;
            let search_end = (expected + self.search_radius as isize)
                .min(max_analysis_start as isize)
                .max(0) as usize;

            let prev_start = synthesis_pos.saturating_sub(self.hop_s);
            let prev_tail = &mono_output[prev_start..synthesis_pos];
            let prev_energy = prev_tail.iter().map(|v| v * v).sum::<f32>();

            let mut best_pos = expected_clamped;
            let mut best_score = f32::NEG_INFINITY;
            if prev_energy > SILENCE_ENERGY {
                for candidate in search_start..=search_end {
                    let mut sum_xy = 0.0f32;
                    let mut sum_y2 = 0.0f32;
                    for i in 0..self.hop_s {
                        let prev = prev_tail[i];
                        let next = mono_input[candidate + i];
                        sum_xy += prev * next;
                        sum_y2 += next * next;
                    }
                    if sum_y2 <= SILENCE_ENERGY {
                        continue;
                    }
                    let score = sum_xy / (prev_energy * sum_y2).sqrt();
                    if score > best_score {
                        best_score = score;
                        best_pos = candidate;
                    }
                }
            }

            let chosen = if best_score < SIMILARITY_THRESHOLD {
                expected_clamped
            } else {
                best_pos
            };

            for i in 0..self.window_size {
                let src_frame = chosen + i;
                let dst_frame = synthesis_pos + i;
                let window = self.window[i];
                for ch in 0..channels {
                    output[dst_frame * channels + ch] +=
                        input[src_frame * channels + ch] * window;
                }
                mono_output[dst_frame] += mono_input[src_frame] * window;
            }

            analysis_pos += self.hop_s as f64 * ratio;
            synthesis_pos += self.hop_s;
        }

        output
    }
}

fn hann_window(size: usize) -> Vec<f32> {
    if size <= 1 {
        return vec![1.0; size];
    }
    let denom = (size - 1) as f64;
    (0..size)
        .map(|i| {
            let phase = 2.0 * PI * (i as f64) / denom;
            (0.5 - 0.5 * phase.cos()) as f32
        })
        .collect()
}

fn mono_from_interleaved(samples: &[f32], channels: usize) -> Vec<f32> {
    let frames = samples.len() / channels.max(1);
    let mut mono = vec![0.0; frames];
    for frame in 0..frames {
        let mut sum = 0.0f32;
        let base = frame * channels;
        for ch in 0..channels {
            sum += samples[base + ch];
        }
        mono[frame] = sum / channels as f32;
    }
    mono
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsola_keeps_length_near_ratio() {
        let sample_rate = 48_000;
        let input_frames = 4_800;
        let mut input = vec![0.0f32; input_frames];
        for (idx, sample) in input.iter_mut().enumerate() {
            *sample = (idx as f32) / input_frames as f32;
        }
        let wsola = Wsola::new(sample_rate);
        let ratio = 2.0;
        let output = wsola.stretch(&input, 1, ratio);
        let expected_frames = ((input_frames as f64) / ratio).round() as isize;
        let actual_frames = output.len() as isize;
        let tolerance = wsola.window_size as isize;
        assert!((actual_frames - expected_frames).abs() <= tolerance);
    }

    #[test]
    fn wsola_preserves_silence() {
        let sample_rate = 44_100;
        let input = vec![0.0f32; 2_000];
        let wsola = Wsola::new(sample_rate);
        let output = wsola.stretch(&input, 1, 1.5);
        let max = output
            .iter()
            .fold(0.0f32, |acc, sample| acc.max(sample.abs()));
        assert!(max <= 1e-6);
    }
}
