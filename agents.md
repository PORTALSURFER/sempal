# Sempal agent overview

## Goal
Sempal is a Rust + egui desktop app for triaging large audio sample libraries and building curated collections. It focuses on fast auditioning, waveform editing, tagging (keep/trash/neutral), similarity search, and exporting trimmed clips or collections.

## Key workflows
- Add one or more source folders, index `.wav` files, and keep a per-source `.sempal_samples.db`.
- Audition samples, tag them, and use fast keyboard navigation for triage.
- Edit waveforms (crop/trim/fade/normalize/etc.) with destructive edits or export clips from selections.
- Prepare similarity embeddings (PANNs) and explore related samples or the similarity map.
- Build collections and export to a configured folder structure.

## High-level architecture
- `src/main.rs` bootstraps logging, the waveform renderer, and the egui app.
- `src/egui_app/` implements the UI and app logic (controller/state/ui/view_model).
- `src/sample_sources/` manages source folders, scanning, per-source DBs, and library data.
- `src/audio/` handles playback and audio device integration.
- `src/waveform/` decodes audio and renders waveforms.
- `src/analysis/` and similarity prep pipeline handle embeddings and clustering.
- `src/updater/` and `src/issue_gateway/` handle update checks and issue reporting.

## Data and configuration
- App data lives in `.sempal` inside the OS config directory.
- Main settings: `config.toml`; library DB: `library.db`.
- Each source folder keeps a local `.sempal_samples.db`.
- Logs are written under `.sempal/logs` with per-launch files.

## Build and setup notes
- Build/run: `cargo run --release`.
- Windows ASIO builds require `CPAL_ASIO_DIR` to point at the Steinberg ASIO SDK.
- Similarity prep needs the PANNs model; see `README.md` and `scripts/setup_panns.*` for setup.

## Code style expectations
- Keep modules focused; avoid deep nesting and shared mutable state.
- Document public-facing items with clear doc comments.
- Add tests for non-trivial logic; prefer unit tests when possible.
