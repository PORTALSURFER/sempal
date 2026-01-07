use crate::analysis::fft::{Complex32, FftPlan, fft_radix2_inplace_with_plan, hann_window};

use super::mel::PannsMelBank;
use super::stft::{fill_windowed, power_spectrum_into};
use super::{PANNS_MEL_BANDS, PANNS_STFT_HOP, PANNS_STFT_N_FFT};

pub(crate) struct PannsPreprocessor {
    sample_rate: u32,
    n_fft: usize,
    hop: usize,
    window: Vec<f32>,
    plan: FftPlan,
    mel_bank: PannsMelBank,
    fft_buf: Vec<Complex32>,
    power_buf: Vec<f32>,
    mel_buf: Vec<f32>,
}

impl PannsPreprocessor {
    pub(crate) fn new(sample_rate: u32, n_fft: usize, hop: usize) -> Result<Self, String> {
        let n_fft = n_fft.max(1);
        let hop = hop.max(1);
        let window = hann_window(n_fft);
        let plan = FftPlan::new(n_fft)?;
        let mel_bank = PannsMelBank::new(sample_rate, n_fft);
        Ok(Self {
            sample_rate,
            n_fft,
            hop,
            window,
            plan,
            mel_bank,
            fft_buf: vec![Complex32::default(); n_fft],
            power_buf: vec![0.0_f32; n_fft / 2 + 1],
            mel_buf: vec![0.0_f32; PANNS_MEL_BANDS],
        })
    }

    pub(crate) fn set_config(
        &mut self,
        sample_rate: u32,
        n_fft: usize,
        hop: usize,
    ) -> Result<(), String> {
        let n_fft = n_fft.max(1);
        let hop = hop.max(1);
        if self.sample_rate == sample_rate && self.n_fft == n_fft && self.hop == hop {
            return Ok(());
        }
        self.sample_rate = sample_rate;
        self.n_fft = n_fft;
        self.hop = hop;
        self.window = hann_window(n_fft);
        self.plan = FftPlan::new(n_fft)?;
        self.mel_bank = PannsMelBank::new(sample_rate, n_fft);
        self.fft_buf.resize(n_fft, Complex32::default());
        self.power_buf.resize(n_fft / 2 + 1, 0.0);
        if self.mel_buf.len() != PANNS_MEL_BANDS {
            self.mel_buf.resize(PANNS_MEL_BANDS, 0.0);
        }
        Ok(())
    }

    pub(crate) fn log_mel_frames_into_flat(
        &mut self,
        samples: &[f32],
        out: &mut [f32],
        max_frames: usize,
    ) -> Result<usize, String> {
        if max_frames == 0 {
            return Ok(0);
        }
        let frame_len = PANNS_MEL_BANDS;
        let needed = max_frames.saturating_mul(frame_len);
        if out.len() < needed {
            return Err(format!(
                "log-mel output buffer too small: need {needed}, got {}",
                out.len()
            ));
        }
        if samples.is_empty() {
            self.log_mel_silence_into(&mut out[..frame_len]);
            return Ok(1);
        }
        let mut start = 0usize;
        let mut frame_idx = 0usize;
        while start < samples.len() && frame_idx < max_frames {
            let offset = frame_idx * frame_len;
            self.log_mel_frame_into(samples, start, &mut out[offset..offset + frame_len])?;
            start = start.saturating_add(self.hop);
            frame_idx += 1;
        }
        Ok(frame_idx)
    }

    fn log_mel_frame_into(
        &mut self,
        samples: &[f32],
        start: usize,
        out_frame: &mut [f32],
    ) -> Result<(), String> {
        fill_windowed(&mut self.fft_buf, samples, start, &self.window);
        fft_radix2_inplace_with_plan(&mut self.fft_buf, &self.plan)?;
        power_spectrum_into(&self.fft_buf, &mut self.power_buf);
        self.mel_bank
            .mel_from_power_into(&self.power_buf, &mut self.mel_buf);
        write_log_mel(&self.mel_buf, out_frame);
        Ok(())
    }

    fn log_mel_silence_into(&mut self, out_frame: &mut [f32]) {
        self.power_buf.fill(0.0);
        self.mel_bank
            .mel_from_power_into(&self.power_buf, &mut self.mel_buf);
        write_log_mel(&self.mel_buf, out_frame);
    }
}

/// Compute log-mel frames using PANNs defaults (log10 with epsilon).
pub(crate) fn log_mel_frames(samples: &[f32], sample_rate: u32) -> Result<Vec<Vec<f32>>, String> {
    let mut preprocessor = PannsPreprocessor::new(sample_rate, PANNS_STFT_N_FFT, PANNS_STFT_HOP)?;
    let frames_len = if samples.is_empty() {
        1
    } else {
        (samples.len().saturating_sub(1) / PANNS_STFT_HOP.max(1)).saturating_add(1)
    };
    let mut flat = vec![0.0_f32; frames_len * PANNS_MEL_BANDS];
    let written = preprocessor.log_mel_frames_into_flat(samples, &mut flat, frames_len)?;
    let mut out = Vec::with_capacity(written);
    for frame in flat[..written * PANNS_MEL_BANDS].chunks(PANNS_MEL_BANDS) {
        out.push(frame.to_vec());
    }
    Ok(out)
}

fn write_log_mel(input: &[f32], out: &mut [f32]) {
    for (src, dst) in input.iter().zip(out.iter_mut()) {
        *dst = log_mel(*src);
    }
}

fn log_mel(value: f32) -> f32 {
    const EPS: f32 = 1e-10;
    let v = value.max(EPS);
    let out = 10.0 * v.log10();
    if out.is_finite() { out } else { 0.0 }
}
