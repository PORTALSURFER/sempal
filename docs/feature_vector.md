# Feature Vector (v1)

Sempal stores per-sample analysis output in `library.db` table `features` as a compact little-endian `f32` blob.

## Versioning

The stored blob is versioned by `features.feat_version`.

For `feat_version = 1`, the vector is a fixed-length array of `183` `f32` values, encoded as `vec_blob` where each float is written via `f32::to_le_bytes()`.

- `version`: integer, currently `1`
- `time_domain`: `TimeDomainFeatures`
- `frequency_domain`: `FrequencyDomainFeatures`

## Embedding Contract (PANNs)

Sempal stores PANNs embeddings in the `embeddings` table. The following contract is
authoritative for all embedding inference in the app and dataset tooling.

### Input preprocessing

- Input is mono `f32` samples in `[-1.0, 1.0]`.
- Mono mixdown is required for multi-channel audio.
- Preprocess using `preprocess_mono_for_embedding`:
  - Trim silence with hysteresis.
  - Normalize peak to `1.0` (if non-zero).
  - Sanitize non-finite values (clamp and zero subnormals).

### Model input

- Target sample rate: `32_000Hz` (resample if needed).
- Window length: `10s` (`320_000` samples).
- If shorter than 10s, repeat-pad to length; if longer, truncate to 10s.
- Compute log-mel features:
  - STFT: `n_fft=1024`, `hop=320`, Hann window.
  - Mel: `64` bands, `fmin=50Hz`, `fmax=14_000Hz`.
  - Log scale: `log10(mel + 1e-6)`.
- Model input tensor layout: `[batch, 1, mel, frames]` where `frames=1000`.

### Output embedding

- Model ID: `panns_cnn14__sr32k__nfft1024__hop320__mel64__log10__chunk10__repeatpad_v1`
- Dimension: `2048` `f32` values.
- L2-normalized with `||v|| ~= 1.0` (tolerance `1e-3`).
- Stored with `embeddings.model_id`, `dim`, `dtype`, and `l2_normed = true`.

## Layout (v1)

In order:

1. Time-domain (9)
   - duration_seconds, peak, rms, crest_factor, zero_crossing_rate, attack_seconds, decay_20db_seconds, decay_40db_seconds, onset_count
2. Spectral stats (24)
   - centroid/rolloff/flatness/bandwidth for: global, early, late; each as (mean, std)
3. Band energy ratios (30)
   - sub/low/mid/high/air for: global, early, late; each as (mean, std)
4. MFCC(20) stats (120)
   - global mean[20], global std[20], early mean[20], early std[20], late mean[20], late std[20]

## Frequency-domain configuration (v1)

All frequency-domain features are computed from the analysis-normalized mono signal:

- Sample rate: `22_050Hz` (`sr_used`)
- STFT: Hann window
- Frame size: `1024` samples
- Hop size: `512` samples
- Spectrum: power spectrum over `N/2 + 1` bins

### Per-frame metrics

Computed per STFT frame:

- `spectral centroid` (Hz)
- `spectral rolloff` (Hz, 85% energy)
- `spectral flatness`
- `spectral bandwidth` (Hz)
- band energy ratios:
  - sub: 20–80 Hz
  - low: 80–200 Hz
  - mid: 200–2k Hz
  - high: 2k–8k Hz
  - air: 8k–16k Hz

### MFCC

- Mel bands: 40
- MFCC: 20 coefficients (DCT-II of log mel energies)

### Aggregation

For spectral metrics, band ratios, and MFCC:

- `mean` and `std` over all frames
- `mean_early` / `std_early` over the first 25% of frames
- `mean_late` / `std_late` over the last 25% of frames
