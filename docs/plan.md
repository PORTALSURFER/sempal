## Goal
- Reduce long-sample load time and playback latency by adding caching and a short history of loaded samples that stays in sync with edits.

## Proposed solutions
- Profile the current load path (disk read → decode → waveform render → playback) for long files to spot the slowest stages and quick wins.
- Add a cache keyed by source ID + relative path + file metadata that stores decoded waveform data and audio bytes for reuse across selections and previews.
- Maintain a bounded MRU history of recently loaded samples to enable instant replay without reloading or decoding.
- Invalidate or refresh cached entries whenever edits occur (selection edits, trims, fades) or file metadata changes, falling back to fresh loads.
- Consider preloading likely-next samples (e.g., neighbouring browser rows) when idle if it does not regress responsiveness.

## Step-by-step plan
1. [x] Map the existing audio load lifecycle (controller queueing, `audio_loader`, waveform render, playback) and measure long-sample latency to identify bottlenecks.
2. [x] Design cache structures keyed by source/path/metadata plus capacity/eviction rules, and define the history size and what artefacts (bytes/decoded/render meta) are stored.
3. [x] Implement cache lookup/populate paths in the audio/waveform loader flow so selections and previews can short-circuit to cached results, with history-backed instant replay.
4. [x] Wire cache invalidation on edits and metadata changes (selection edits, rescans) to prevent stale playback while keeping UI state consistent.
5. [x] Add targeted tests/QA for cache hits, eviction, invalidation after edits, and long-sample playback to confirm latency improvements and stability.

## Code Style & Architecture Rules Reminder
- Keep files under 400 lines; split when necessary.
- When functions require more than 5 arguments, group related values into a struct.
- Each module must have one clear responsibility; split when responsibilities mix.
- Do not use generic buckets like `misc.rs` or `util.rs`. Name modules by domain or purpose.
- Name folders by feature first, not layer first.
- Keep functions under 30 lines; extract helpers as needed.
- Each function must have a single clear responsibility.
- Prefer many small structs over large ones.
- All public objects, functions, structs, traits, and modules must be documented.
- All code should be well tested whenever feasible.
- “Feasible” should be interpreted broadly: tests are expected in almost all cases.
- Prefer small, focused unit tests that validate behaviour clearly.
- Do not allow untested logic unless explicitly approved by the user.
