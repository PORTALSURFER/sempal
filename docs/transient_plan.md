## Transient detection review and implementation plan

Note: This plan reviews the provided reference text and the current codebase.
No external web sources were accessed directly in this environment.

### Summary of the provided reference (what we should align to)
- Use a spectral-flux ODF as the core detector.
- Improve robustness with log compression, band weighting, and whitening.
- Prefer SuperFlux-style max-over-recent-frames per bin to suppress vibrato.
- Use adaptive thresholds with median/MAD and a refractory period.
- Keep peak picking deterministic; avoid ML.

### Current codebase audit (high-level)
- `src/waveform/transients.rs` implements:
  - Multi-band spectral flux with log compression + rolling median whitening.
  - Adaptive thresholds (median/MAD) and local maxima with min-gap.
  - Long files use a decimated mono analysis buffer with the same ODF pipeline.
  - A strict → relaxed pass handles empty detections without switching modes.
- Peak picking uses hysteresis/arming behavior with min-gap.
- Sensitivity controls focus on k, floor quantile, and min-gap.

### Implementation plan (Ableton-style, fast + accurate)
1. **Unify ODF generation**  
   - Remove the peaks-envelope-only detection path.
   - For long files, downsample the *audio* (not the envelope) and run the same
     spectral pipeline at a lower analysis rate (e.g., resample or increase hop).
   - Keep one canonical novelty curve used for all modes.

2. **Add SuperFlux per-bin max memory**  
   - Maintain a short rolling max per bin (e.g., lag=2 frames, window=3–5
     frames).
   - Replace `mag[n] - mag[n-1]` with `mag[n] - prev_max` in the flux step.
   - Keep log compression and band weighting as today.

3. **Improve whitening / normalization**  
   - Move from EMA mean to a rolling median/quantile per band (or IIR + clamp)
     to better suppress level changes and vibrato.
   - Keep a light log compression prior to whitening.

4. **Peak picking with hysteresis + adaptive baseline**  
   - Use a rolling median/MAD baseline on the novelty curve.
   - Trigger when `novelty > median + k*MAD`.
   - Require re-arm when `novelty < median + k_low*MAD`.
   - Add a refractory window (e.g., 50–80 ms) to prevent double triggers.

5. **Simplify sensitivity mapping**  
   - Map the UI slider to a small set of parameters only:
     - `k` (threshold multiplier)
     - `min_gap` (refractory)
     - `floor_quantile` (global floor)
   - Remove multiple fallback thresholds; use one strict pass and one relaxed
     pass if needed, both with the same ODF.

6. **Cache ODF for realtime UI**  
   - Compute the ODF once per waveform load.
   - Re-run only the peak-picking phase when sensitivity changes for realtime
     updates.

7. **Tests + diagnostics**  
   - Add tests with synthetic onsets (single spike, double hit, soft attack).
   - Add a debug flag to dump ODF statistics (min/median/max) and transient
     counts for tuning.

### Expected outcomes
- Fewer false positives (vibrato/steady passages suppressed via SuperFlux).
- Better recall on soft onsets (adaptive thresholds + band normalization).
- Predictable sensitivity control (single ODF + consistent thresholds).
- Stable behavior on long files (no envelope-only path).
