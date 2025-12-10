---
layout: default
title: Sempal
permalink: /
description: Audio sample triage and collection builder for fast waveform review and destructive edits.
---

# Sempal

Sempal is an audio sample triage and collection builder with fast waveform preview, destructive selection edits, and export workflows tuned for DAW users.

## Quick links
- [Usage guide](/usage)
- [Latest downloads](https://github.com/portalsurfm/sempal/releases)
- [Source on GitHub](https://github.com/portalsurfm/sempal)

## Highlights
- Lightning-fast triage with keep/trash tagging, fuzzy search, and keyboard-first navigation.
- Waveform selection tools for crop, trim, fades, normalize, mute, and looping.
- Drag-and-drop exports for clips and collection items, with per-collection folders.
- Cross-platform builds using Rust and egui.

## Build from source
```bash
cargo run --release
# or
cargo build --release && target/release/sempal
```

## Configuration
- App settings live in `~/.config/.sempal/config.toml` (per platform config dir).
- Sources and collections are stored in `library.db` in the same folder.
- Each source keeps `.sempal_samples.db` beside the audio; logs stay under `.sempal/logs`.
