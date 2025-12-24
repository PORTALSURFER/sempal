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

  2. FFT optimizations: consider SIMD/vectorized complex multiply/add and bit‑reverse
     (or use a tuned FFT crate if acceptable). Your new SIMD normalizer won’t touch
     this.
  3. DCT/mel: precompute cosine tables once, reuse buffers, and avoid per-frame heap
     allocs.
  4. Avoid per-sample WAV writing in benches: write interleaved buffers rather than
     calling write_sample in tight loops.
  5. Reduce decode overhead: for prep with duration cap, avoid full decode (you
     already planned this).
  6. Eliminate extra memmoves: reuse scratch buffers and avoid realloc/copy.

