## Goal
- Show `[` or `]` icons at the bottom of a waveform selection’s start and end edges when hovered so users know they can grab and resize each edge.

## Proposed solutions
- Draw per-edge hover zones aligned to the selection bounds and render bracket glyphs anchored at the bottom when the pointer is near either edge.
- Reuse the existing selection state and drag plumbing to start edge-specific drags while keeping the current click-and-drag selection creation intact.
- Preserve the current loop/playback and selection drag-export flow, ensuring the new visuals do not interfere with existing interactions.

## Step-by-step plan
1. [x] Audit current waveform selection rendering and drag handling (`render_waveform`, `selection_handle_rect`, `SelectionState`) to confirm available hooks for edge-specific UI.
2. [x] Add hover detection around the selection start/end bounds and draw `[`/`]` markers at the bottom of each edge when hovered.
3. [x] Wire the edge hover/press into edge drag initiation (start or end) while keeping existing selection creation and handle dragging behaviour unchanged.
4. [x] Validate interactions manually (hover, edge drag, selection creation, drag-export) and add/update tests around selection edge updates if practical.

## Code Style & Architecture Rules Reminder
- File and module structure: Keep files under 400 lines; split when necessary. When functions require more than 5 arguments, group related values into a struct. Each module must have one clear responsibility; split when responsibilities mix. Do not use generic buckets like `misc.rs` or `util.rs`. Name modules by domain or purpose. Name folders by feature first, not layer first.
- Functions: Keep functions under 30 lines; extract helpers as needed. Each function must have a single clear responsibility. Prefer many small structs over large ones.
- Documentation: All public objects, functions, structs, traits, and modules must be documented.
- Testing: All code should be well tested whenever feasible. “Feasible” should be interpreted broadly: tests are expected in almost all cases. Prefer small, focused unit tests that validate behaviour clearly. Do not allow untested logic unless explicitly approved by the user.
