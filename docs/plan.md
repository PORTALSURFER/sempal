Goal
- Diagnose why selecting samples feels slow and outline changes that make selection/playback nearly instant while flagging any feature areas that need redesign.

Proposed solutions
- Profile the selection path (handle_wav_clicked → update_wav_view → load_from_source → waveform decode/render → playback) to pinpoint time spent and UI-thread blockers.
- Cut wav list churn by keeping models stable, updating selection/loaded flags in place, and eliminating duplicate full rebuilds on each click.
- Offload waveform decoding/rendering and audio preparation to background workers with caching/downsampling so per-click work stays small.
- Trim playback startup costs (avoid blocking fades and full buffered decodes) and reuse prepared state where possible.
- Revisit sample list UX for large libraries (virtualization/single filtered list) if current three-column layout remains heavy.

Step-by-step plan
1. [~] Capture baseline timings for the sample selection path on a representative library (trace UI thread vs. worker costs).
2. [x] Confirm and rank bottlenecks (list rebuilds, synchronous waveform decode/render, playback setup, DB fetches) with evidence.
3. [x] Redesign wav list updates to avoid full model rebuilds and duplicate refreshes; validate behavior with large lists.
4. [x] Implement async/cached media loading (waveform + audio) with downsampled previews and an LRU cache to keep UI responsive.
5. [x] Optimize playback start/looping to remove blocking fades and unnecessary buffering; measure the responsiveness gain.
6. [-] Re-profile after changes, add performance-focused tests/benchmarks, and document any feature-level redesign needs for the sample list/preview pipeline.
