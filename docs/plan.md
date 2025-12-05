Goal
Implement a SQLite-backed sample source workflow that replaces the current file browser with a left sidebar list of user-configured source directories, supports adding sources via folder picker, scans folders in the background for .wav files into per-source databases, and persists sources across app relaunches.

Proposed solutions
- Add a sample source data layer using SQLite (e.g., rusqlite) with a per-source DB file inside each chosen directory to store discovered wav files.
- Create an app-level config file (JSON/TOML) to persist the list of sources and their metadata, loading on startup and saving on changes.
- Build a background scanner that recursively walks a source directory, detects new/removed .wav files, and updates the source DB without blocking the UI.
- Replace the current file browser UI with a left sidebar showing sources and a main list view showing wav entries for the selected source; include a “+” control that opens a folder picker to register a new source.
- Integrate selection, scanning, and playback so choosing a source refreshes the list from its DB and selecting a wav loads it into the existing waveform/player pipeline.

Step-by-step plan
1. [x] Add data models and helpers for sample sources and per-source SQLite schema (sources table metadata + wav entries table), including creating the DB file inside a chosen directory.
2. [x] Implement a recursive scanner that runs off the UI thread, discovers .wav files, and upserts/removes rows in the source database; surface progress/state to the app layer.
3. [-] Introduce an app-level config file for persisting the list of source directories and any cached metadata; load at startup and save on modifications.
4. [-] Replace the file browser UI with a left sidebar listing sources (with “+” add control) and a main wav list view bound to the selected source’s database entries.
5. [-] Wire Slint callbacks and app logic to manage adding/selecting sources, kicking off scans, and feeding selected wav files into the existing waveform/audio player flow.
6. [-] Add unit tests for the scanning/database logic and config persistence (temp directories), and adjust any integration hooks to stay under size/function limits.
7. [-] Perform manual QA passes: add source, rescan after adding/removing wavs, reload app to confirm persistence, and play a selected wav from the list.
