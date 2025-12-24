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

  - SIMD for hot loops: time/frequency feature extraction (RMS, windowing), vector
    normalization; only after profiling
  - FFT optimization: use an optimized backend or precomputed plans; avoid
    re‑planning
  - Add sampling‑based analysis for long files: compute features from several short
    windows instead of full duration



