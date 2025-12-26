---
layout: default
title: Sempal
permalink: /
description: Audio sample triage and collection builder for fast waveform review and destructive edits.
---

# Sempal

Sempal is an audio sample triage and collection builder with fast waveform preview, destructive selection edits, and export workflows tuned for DAW users.

<div class="download-hero">
  <div class="download-copy">Download the latest build bundle for Windows.</div>
  <a class="download-link" href="https://github.com/portalsurfer/sempal/releases/latest/download/sempal-installer-bundle.zip">
    Download latest
  </a>
</div>

<div class="support-callout">
  <div class="support-copy">
    It costs me a lot of time and effort to build this thing. You can use this link to show some appreciation if you enjoy the app.
      </div>
  <a class="support-link" href="https://buymeacoffee.com/portalsurfm">
    <span class="support-icon" aria-hidden="true">
      <svg viewBox="0 0 24 24" role="img" aria-hidden="true">
        <path d="M6 3h11a3 3 0 0 1 3 3v4a4 4 0 0 1-4 4h-1.3A7 7 0 0 1 8 19H7a5 5 0 0 1-5-5V9a6 6 0 0 1 6-6Zm1.3 4A4 4 0 0 0 4 11v3a3 3 0 0 0 3 3h1a5 5 0 0 0 5-5V7H7.3Zm7.7 0v4a7 7 0 0 1-1 3h2a2 2 0 0 0 2-2V6a1 1 0 0 0-1-1h-3Z" />
        <path d="M6 21h12v2H6z" />
      </svg>
    </span>
    Buy me a coffee
  </a>
</div>

<img src="{{ '/assets/screenshot.png' | relative_url }}" alt="Sempal screenshot" style="max-width: 100%; height: auto; margin: 1.5rem 0;" />

## Quick links
- [Usage guide](/sempal/usage)
- [Changelog](https://github.com/portalsurfer/sempal/blob/main/CHANGELOG.md)
- [Latest downloads](https://github.com/portalsurfer/sempal/releases)
- [Source on GitHub](https://github.com/portalsurfer/sempal)

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
