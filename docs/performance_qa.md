---
layout: default
title: Performance QA
permalink: /performance_qa
description: Checklist for keeping huge sample libraries responsive in Sempal.
---

# Large-list Performance QA

## Goals
- Confirm 50k+ wav entries remain responsive in egui (scrolling, selection, drag/drop).
- Ensure background loading and status timings are reasonable and non-blocking.

## Setup
- Point a sample source at a folder with ~50k wav files (or duplicate a smaller corpus until the count is reached).
- Start with a clean run so cached labels and entries rebuild once.

## Checklist
- Launch app and select the large source.
- Status should read `Loaded/Cached <count> wav files in <ms>`; note the timing.
- Scroll the triage columns quickly; verify no stutter and no duplicate-ID warnings.
- Select random rows; ensure waveform updates and status remains responsive.
- Drag a few rows to collections; confirm hover highlight and drop works without lag.
- Switch between sources and back; large source should reuse cached labels and load instantly.
- Trigger manual scan and confirm UI stays responsive and reload status updates with new timing.

## Metrics to capture
- Initial load time (ms) from status.
- Frame responsiveness during fast scroll (subjective) and selection latency.
- Post-scan reload time (ms).

## Follow-ups
- Adjust row height/window size if scroll perf regresses.
- If load time exceeds comfort, consider chunked DB reads or async batching.
