  ### 1. STFT panics on non power-of-two frame size

  - Severity: High
  - Why it matters: FftPlan::new rejects non power-of-two sizes; compute_frames uses expect, so a misconfigured
    frame size crashes the app instead of returning a recoverable error. This is a correctness and reliability risk.
  - Evidence: src/analysis/frequency_domain/stft.rs:29-39 (compute_frames).
  - Recommended change: Validate frame_size/hop_size before building the plan; return Result<FrameSet, String> or
    round to the next power of two and document the behavior. Propagate errors to callers.
  - Risk/Tradeoffs: Changing to fallible API affects callers; rounding changes feature outputs.
  - Quick win? Yes
  - Suggested test/verification: Add a unit test that passes a non-power-of-two frame_size and asserts a graceful
    error (or deterministic rounding).

  ### 2. WAV header scan can overflow on crafted chunk sizes

  - Severity: High
  - Why it matters: chunk_data + chunk_size is unchecked; a crafted chunk_size can overflow usize, bypass bounds
  - Evidence: src/wav_sanitize.rs:187-218 (sanitize_wav_header).
  - Recommended change: Use checked_add (or saturating_add with explicit overflow handling) for chunk_data +
    chunk_size and offset = chunk_data + chunk_size; bail out or return false on overflow. Consider validating
    chunk_size against total_file_len.
  - Suggested test/verification: Add a regression test with a header that sets chunk_size near usize::MAX and verify
    no panic and a safe return.

  ### 3. Token fallback cache panics on poisoned mutex

  - Severity: Medium
  - Recommended change: Recover from poisoning by logging and clearing the cache, similar to
    WaveformZoomCache::lock_inner, or use poisoned.into_inner() with a reset.
  - Risk/Tradeoffs: May mask underlying concurrency bugs; consider logging at warn level.
  - Quick win? Yes
  - Suggested test/verification: Add a test that poisons the mutex and confirms subsequent get/set calls succeed.

  ### 4. Updater GitHub requests lack retry/backoff

  - Severity: Medium
  - Why it matters: Transient 5xx/429 errors from GitHub can cause update checks to fail. There is a retry helper
    but it’s unused here.
  - Evidence: src/updater/github.rs:58-67 (get_json uses .call() directly), src/http_client.rs (retry_with_backoff
    exists).
  - Recommended change: Wrap get_json in retry_with_backoff, retry on network errors and 429/5xx responses, and
    honor Retry-After when present.
  - Risk/Tradeoffs: Update checks may take longer under failure; must avoid retrying on permanent errors.
  - Quick win? No
  - Suggested test/verification: Add a test using a local test server that returns 500/429 before 200 and assert
    retries occur.

  ### 5. Public read_sanitized_wav_bytes reads entire file into memory

  - Severity: Low
  - Why it matters: The function loads the full WAV into memory; for large files this can spike memory usage and
    cause latency or OOM. It’s public and may be used outside tests.
  - Evidence: src/wav_sanitize.rs:233-238 (read_sanitized_wav_bytes).
  - Recommended change: Make it pub(crate) (if feasible), add a size cap, or document it as test-only. Prefer
    streaming with open_sanitized_wav.
  - Risk/Tradeoffs: API change if external code relies on the function.
  - Quick win? Yes
  - Suggested test/verification: Add a test ensuring large files are rejected (if capped) or document intended use
    in doc comments.
