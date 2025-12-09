## Goal
- Keep the UI and navigation responsive while audio buffers and waveforms load asynchronously, so browsing and interaction are never blocked by IO or decoding.

## Proposed solutions
- Offload waveform decoding and audio buffer preparation to a background loader (thread/task) that reports progress through the existing channel pattern.
- Add lightweight UI state for "loading audio" vs "ready" so selection changes and navigation update immediately with placeholders and queued actions.
- Gate playback so it starts when a ready buffer arrives, while allowing navigation (next/prev/seek) to pre-empt or cancel slower loads.
- Optionally prefetch or prioritize likely-next samples when idle to reduce perceived latency without blocking foreground interactions.

## Step-by-step plan
1. [-] Audit the current selection → load → play pipeline (controller navigation, waveform/audio loaders, hotkeys) to pinpoint where UI waits on IO/decoding.
2. [-] Design a background audio/waveform loader API using the channel/work queue pattern with request IDs, cancellation of stale loads, and progress/error reporting hooks.
3. [-] Refactor selection and navigation handlers to enqueue load requests and immediately update UI focus/selection, showing non-blocking loading state/placeholders.
4. [-] Integrate playback initiation with async loads so playback triggers when buffers are ready; ensure navigation commands stay responsive and handle load failures gracefully.
5. [-] Add UI cues and logging for loading states and fallbacks; make waveform rendering resilient to missing/pending data without blocking.
6. [-] Add automated coverage for async load and stale-result handling, and manually verify startup and navigation remain smooth under slow IO.

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
