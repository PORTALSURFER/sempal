---
layout: default
title: Sempal
permalink: /
description: Sample manager with fast waveform review and destructive edits.
---

# Sempal

Sempal is a sample manager with fast waveform preview, destructive edits. Tuned for music producers.

[![Build release assets](https://github.com/PORTALSURFER/sempal/actions/workflows/release-build.yml/badge.svg)](https://github.com/PORTALSURFER/sempal/actions/workflows/release-build.yml)

<div class="download-hero">
  <div class="download-copy">Download the latest portable bundle.</div>
  <a class="download-link" href="https://github.com/portalsurfer/sempal/releases/latest" id="release-link">
    View latest releases
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

<script>
  (function () {
    var link = document.getElementById("release-link");
    if (!link) {
      return;
    }
    fetch("https://api.github.com/repos/PORTALSURFER/sempal/releases?per_page=10")
      .then(function (response) {
        if (!response.ok) {
          throw new Error("release fetch failed");
        }
        return response.json();
      })
      .then(function (releases) {
        if (!Array.isArray(releases)) {
          return;
        }
        var match = null;
        for (var i = 0; i < releases.length; i += 1) {
          var release = releases[i];
          if (!release || release.draft) {
            continue;
          }
          if (Array.isArray(release.assets) && release.assets.length > 0) {
            match = release;
            break;
          }
        }
        if (match && match.html_url) {
          link.href = match.html_url;
        }
      })
      .catch(function () {});
  })();
</script>
