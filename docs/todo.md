- lets design a smarter context changing ux system which understands the layout, so that alt+arrow key movement will correctly move around the 2d plane based on direction, without hardcoding all this.
the idea to to have contexts chromes, like the sample browser or waveform etc, to navigate these, the user can use alt+arrows.
navigation inside these contexts, like for example, navigating the browser list, the user can use plain arrow keys.

...

- analysis_jobs::pool::job_runner::run_analysis_jobs_with_decoded_batch (hot) +
  analysis::embedding::infer_embedding_with_model + ort::session::Session::run_inner: inference +
  preprocessing is still the main wall time. Improve by increasing batch size further (32–64) and
  reducing per‑item allocations in infer_embeddings_with_model (reuse resample buffers instead of
  to_vec per input).

- analysis::audio::decode::decode_for_analysis_with_rate_limit: decode cost is non‑zero; add
  early‑exit decode and reuse scratch buffers (already on our list).

- analysis::audio::silence::trim_silence_with_hysteresis: shows up; consider a fast path or SIMD for
  the trim loop.

- analysis_jobs::pool::job_claim::spawn_compute_worker closure: this is essentially the compute loop;
  optimize inner steps (preprocess + inference + DB writes) rather than thread scaffolding.

- analysis_jobs::pool::job_runner::finalize_analysis_job: DB + ANN writes; increase batch size and
  reduce per‑sample writes.
