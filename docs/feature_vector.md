# Feature Vector (v1)

Sempal stores per-sample analysis output in `library.db` table `analysis_features` as a JSON blob.

## Versioning

The top-level JSON object is `AnalysisFeaturesV1` and includes:

- `version`: integer, currently `1`
- `time_domain`: `TimeDomainFeatures`
- `frequency_domain`: `FrequencyDomainFeatures`

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

