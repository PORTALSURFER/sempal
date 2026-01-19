## Transient detection audit (local)

Note: This audit is based on the current codebase only. No external web sources
were accessed.

### Current pipeline (high level)
- Uses multi-band spectral flux with log compression, band-weighting, and a
  rolling per-band median for whitening. Peak picking uses adaptive thresholds
  (median/MAD) and a global floor.
- Long files now use a decimated mono analysis buffer and run the same ODF
  pipeline with larger hops rather than switching to an envelope-only path.
- A single strict → relaxed pass runs on the same detector when the initial
  thresholds yield no peaks.
- Streaming baselines use rolling medians with MAD for robust thresholds on
  long files.

### Observed weaknesses
- Thresholding uses per-frame median/MAD with a fixed window, but the window is
  not tuned per hop rate or tempo; long decimation factors can reduce time
  resolution and may require re-tuning sensitivity.

### Key improvement areas (status)
1. Unify novelty generation into a single “Ableton-like” pipeline
   (multi-band spectral flux + log compression + whitening) for both normal and
   long files. For long files, downsample *audio* or compute STFT on a decimated
   signal rather than switching to the peaks envelope path. (implemented)
2. Replace local per-frame median/MAD with a robust, streaming baseline:
   maintain a rolling median/MAD (or quantile/IQR) with an efficient structure,
   then use a fixed “median + k * MAD” threshold for peak picking. This makes
   sensitivity more stable and improves long-file performance. (implemented)
3. Add a hysteresis state machine for peak picking:
   require novelty to cross a high threshold to trigger and fall below a lower
   threshold to re-arm, plus a refractory window. This reduces double-triggers.
   (implemented)
4. Normalize band energies with a short-term running median per band (or
   median-filtered band energy) before flux computation to suppress false
   positives from level shifts and noise floors. (implemented)
5. Calibrate sensitivity to a small set of parameters only (k, floor quantile,
   min-gap), and remove the cascade of fallback thresholds. Instead, a single
   “strict → relaxed” pass in the same detector should handle misses without
   changing detection modes. (implemented)
