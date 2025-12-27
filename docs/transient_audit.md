## Transient detection audit (local)

Note: This audit is based on the current codebase only. No external web sources
were accessed.

### Current pipeline (high level)
- Uses multi-band spectral flux with log compression, band-weighting, and an EMA
  mean for whitening. Peak picking uses adaptive thresholds (median/MAD) and a
  global floor.
- Long files fall back to a "peaks envelope" path based on waveform min/max
  buckets. This path uses a simplified novelty and peak picking to avoid
  freezes.
- Several fallback paths attempt to produce some peaks (raw flux, loose picking,
  energy novelty) when the primary detector yields none.

### Observed weaknesses
- Multiple heterogeneous fallbacks can inflate false positives and make
  sensitivity feel inconsistent.
- Peak picking lacks a true hysteresis/arming model; local max + min-gap alone
  is easier to double-trigger.
- Thresholding uses per-frame median/MAD with a fixed window, but the window is
  not tuned per hop rate or tempo and uses an O(n * window) median per sample in
  windows; this is costly and makes parameter tuning brittle.
- The “peaks envelope” path loses spectral detail (no frequency information),
  which causes misses on complex material and can over-trigger on sustained
  loudness changes.
- Sensitivity affects too many knobs (floors, k, caps, fallback thresholds),
  which makes the UI slider hard to predict.

### Key improvement areas (TODOs)
1. Unify novelty generation into a single “Ableton-like” pipeline
   (multi-band spectral flux + log compression + whitening) for both normal and
   long files. For long files, downsample *audio* or compute STFT on a decimated
   signal rather than switching to the peaks envelope path.
2. Replace local per-frame median/MAD with a robust, streaming baseline:
   maintain a rolling median/MAD (or quantile/IQR) with an efficient structure,
   then use a fixed “median + k * MAD” threshold for peak picking. This makes
   sensitivity more stable and improves long-file performance.
3. Add a hysteresis state machine for peak picking:
   require novelty to cross a high threshold to trigger and fall below a lower
   threshold to re-arm, plus a refractory window. This reduces double-triggers.
4. Normalize band energies with a short-term running median per band (or
   median-filtered band energy) before flux computation to suppress false
   positives from level shifts and noise floors.
5. Calibrate sensitivity to a small set of parameters only (k, floor quantile,
   min-gap), and remove the cascade of fallback thresholds. Instead, a single
   “strict → relaxed” pass in the same detector should handle misses without
   changing detection modes.
