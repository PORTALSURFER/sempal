- lets design a smarter context changing ux system which understands the layout, so that alt+arrow key movement will correctly move around the 2d plane based on direction, without hardcoding all this.
the idea to to have contexts chromes, like the sample browser or waveform etc, to navigate these, the user can use alt+arrows.
navigation inside these contexts, like for example, navigating the browser list, the user can use plain arrow keys.

- speed up analysis

- handle adding of new files, and manipulation of files, ensure embeddings are updated

--
  
  - Add size‑aware audio cache eviction. AudioCache evicts by entry count, but cached
    blobs can be large. Track total bytes and evict by size in src/egui_app/
    controller/audio_cache.rs to avoid memory pressure during fast browsing.

  - Prefetch next/previous audio/waveform when selection changes. You already cache
    audio; prefetching one or two neighbors in src/egui_app/controller/wavs/
    audio_loading.rs would make fast navigation feel snappier without increasing
    complexity too much.

  Prepare similarity search analysis

  - Reduce per‑sample SQL lookups during backfill. enqueue_jobs_for_source_backfill
    and enqueue_jobs_for_embedding_backfill query the library DB once per sample. For
    large libraries that’s the biggest cost. Consider staging sample_ids in a
    temporary table and doing set‑based joins against features, samples, and
    analysis_jobs in src/egui_app/controller/analysis_jobs/enqueue/enqueue_samples.rs
    and src/egui_app/controller/analysis_jobs/enqueue/enqueue_embeddings.rs.

  - Avoid redundant embedding work. run_analysis_job already generates embeddings
    when missing; embedding backfill should target only “features present + embedding
    missing” samples. Use a query that identifies those directly rather than scanning
    all entries, and skip backfill if the query yields zero rows.

  - Improve progress stage clarity. Similarity prep goes from scan → analysis →
    finalize, but the progress UI only shows “Preparing similarity search.” Update
    the detail/status as stages change in src/egui_app/controller/similarity_prep.rs
    and src/egui_app/controller/progress_messages.rs (e.g., “Scanning source…”,
    “Analyzing…”, “Embedding backfill…”, “Finalizing…”).

  - Consider a max analysis duration toggle for similarity prep. The job runner
    already supports a duration cap; exposing this in prep settings could avoid long
    analyses on large files during “prepare similarity search” (src/egui_app/
    controller/analysis_jobs/pool/job_runner.rs).
