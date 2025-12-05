# Local Testing Guide

- Prefer running checks locally (your machine is faster). From the repo root:
  - `SKIP_VERSION_BUMP=1 cargo test` — runs all tests while skipping the build-script version bump.
  - `SKIP_VERSION_BUMP=1 cargo run` — launches the app without bumping the version (useful for quick manual checks).
  - If you explicitly want the build script to bump the version, unset `SKIP_VERSION_BUMP` (or set `FORCE_VERSION_BUMP=1` to bypass the 5-minute spacing guard).
- When verifying loop/selection changes:
  - Load a wav, toggle looping on/off, and confirm playback doesn’t auto-start when enabling loop.
  - While looping, adjust the selection; playback should immediately follow the new loop region without pressing play again.
  - Alt+drag creates a selection, alt+click clears it; thin edge handles can resize the range.
- If you hit build/test issues, share the command and log snippet so I can adapt quickly.***
