I see a .sempal_delete_staging folder appear in my source folder. can you make sure this gets cleared up properly, and make sure the folder is hidden.


----

lets add a lock to bpm snap, the user can enable this to lock the bpm to a specified value, preventing it from changing when a sample with bpm metadata is loaded.
also add a 'stretch' toggle next to it.
if stretch is enabled, stretch the loaded sample to fit the current bpm value, using WSOLA (Waveform Similarity Overlap-Add)

# WSOLA (Waveform Similarity Overlap-Add)
## Implementation Plan & Technical Specification

---

## 1. Scope and Goals

### Goals
- Time-stretch audio without changing pitch
- Support BPM-synced playback for loops and samples
- Operate in real time
- Preserve transients and rhythmic feel

### Non-goals (v1)
- Pitch shifting
- Formant preservation
- Extreme stretch ratios (> 2× or < 0.5×)

---

## 2. Algorithm Overview

WSOLA performs time-stretching by:
1. Advancing a synthesis pointer at a fixed hop size
2. Advancing an analysis pointer according to the stretch ratio
3. Searching around the expected analysis position for the waveform segment that best matches the previous output
4. Overlap-adding the selected segment using a window function

Key property: segment selection is driven by waveform similarity, not fixed spacing.

---

## 3. Core Parameters

| Parameter | Symbol | Recommended Value |
|---------|--------|------------------|
| Sample rate | Fs | Source sample rate |
| Window size | W | 20–40 ms (e.g. 1024 @ 48 kHz) |
| Synthesis hop | Hs | W / 2 |
| Analysis hop | Ha | Hs × stretch_ratio |
| Search radius | R | ±(Hs / 2) |
| Window function | — | Hann |
| Stretch ratio | α | target_bpm / source_bpm |

---

## 4. Data Structures

```rust
struct Wsola {
    window_size: usize,
    hop_s: usize,
    search_radius: usize,

    window: Vec<f32>,

    input: RingBuffer<f32>,
    output: RingBuffer<f32>,

    analysis_pos: f64,
    synthesis_pos: usize,

    stretch_ratio: f64,
}
````

Notes:

* `analysis_pos` is fractional to support arbitrary ratios
* `synthesis_pos` advances in fixed hops
* Ring buffers allow streaming playback

---

## 5. Processing Pipeline

### 5.1 Initialization

* Precompute Hann window
* Fill input buffer
* Set:

  * `analysis_pos = 0.0`
  * `synthesis_pos = 0`

---

### 5.2 First Frame (Bootstrap)

* Copy first window directly:

```text
output[0..W] = input[0..W] * window
```

* Advance pointers:

```text
analysis_pos += Ha
synthesis_pos += Hs
```

---

### 5.3 Main Processing Loop

Repeat until output buffer is filled:

#### 5.3.1 Expected Analysis Position

```text
expected_pos = round(analysis_pos)
```

---

#### 5.3.2 Similarity Search

Search candidate positions:

```text
[expected_pos - R, expected_pos + R]
```

For each candidate `c`, compute similarity against the previous output tail.

Similarity metric (normalized cross-correlation):

```text
score(c) =
    Σ (prev_tail[i] * input[c + i]) /
    sqrt(Σ prev_tail² * Σ input²)
```

* Use overlap region of `Hs` samples
* Select `c_best` with highest score

Performance notes:

* Normalization can be skipped initially
* SIMD optimization optional later

---

#### 5.3.3 Overlap-Add Synthesis

```text
for i in 0..W {
    output[synthesis_pos + i] +=
        input[c_best + i] * window[i];
}
```

Overlapping windows accumulate naturally.

---

#### 5.3.4 Advance Pointers

```text
analysis_pos += Ha
synthesis_pos += Hs
```

---

## 6. Stretch Ratio and BPM Sync

```text
stretch_ratio = target_bpm / source_bpm
```

Recommended clamp:

```text
0.5 ≤ stretch_ratio ≤ 2.0
```

Ratios outside this range significantly increase artifacts.

---

## 7. Loop Handling

### Requirements

* Loop length ≥ 2 × window_size
* Wrap `analysis_pos` modulo loop length
* Preserve previous frame tail across loop boundary

### Optional Enhancements

* Crossfade loop edges
* Reset similarity history on loop restart if needed

---

## 8. Edge Cases and Heuristics

### Transients

* If best similarity score < threshold:

  * Skip search
  * Use `expected_pos` directly
* Reduces transient “flam” artifacts

### Silence

* Detect near-zero energy frames
* Bypass similarity search and copy directly

---

## 9. Performance Characteristics

| Aspect         | Value              |
| -------------- | ------------------ |
| Complexity     | O(W × R) per frame |
| Latency        | ~W                 |
| FFT required   | No                 |
| Real-time safe | Yes                |

---

## 10. Testing & Validation

Test signals:

* Click track (transient alignment)
* Drum loops (groove stability)
* Sustained pads (phasiness detection)
* Bass tones (low-frequency stability)
* Noise (artifact exposure)

Validation criteria:

* Stable RMS
* No rhythmic drift
* Clean transients
* No amplitude modulation

---

## 11. Public API Sketch

```rust
trait TimeStretcher {
    fn set_ratio(&mut self, ratio: f64);
    fn process(&mut self, input: &[f32], output: &mut [f32]);
    fn reset(&mut self);
}
```

WSOLA should be implemented as one backend among others.

---

## 12. Expected Outcome

* High-quality rhythmic time-stretching
* Excellent transient preservation
* Low CPU usage
* Suitable for real-time sample preview and loop playback
* Close perceptual quality to commercial engines for rhythmic material

---
