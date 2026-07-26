
# Wavecrate Project Target: Fast Sample Extraction, Curation, and Library Usage

## Working Product Name

The current working product name is **Wavecrate**.

Documentation should use **Wavecrate** when referring to the application and **sample file**, **audio file**, or **WAV file** when referring to files in the library.

## Vision

Wavecrate should become a focused desktop application for browsing, auditioning, extracting, editing, organizing, and using large audio sample libraries without breaking the user’s listening flow.

The application should help users turn messy audio material into a clean, structured, well-managed sample library. A user should be able to record long jams or experiments in tools such as Ableton Live, Bitwig, hardware recorders, or other audio software, save them as ordinary audio files, then use Wavecrate to find the useful parts, cut those parts into usable sample files, clean them up, name them consistently, tag them, rate them, organize them, and use them in music production.

Wavecrate is not a generic GUI library, a DAW, a plugin host, or a file-manager skin. It is a sample-focused workstation built around immediate auditioning, precise waveform interaction, destructive sample editing with safe warnings and undo, fast metadata workflows, similarity-based discovery, safe library hygiene, and reliable file management.

The target is a dense, responsive, predictable, and trustworthy tool for repeated professional use on large local sample collections.

Wavecrate should have a low-noise interface. The app should assume users can read documentation and will quickly become fluent, expert users through repeated use. It should keep the main interface clean, direct, and easy to operate, with minimal explanatory copy, modal interruption, and implementation-detail disclosure. Detailed behavior, storage details, and advanced constraints should live in documentation, diagnostics, or deliberate advanced surfaces unless showing them inline prevents likely data loss or an unsafe action.

## Product Identity

Wavecrate is three things working together:

1. A sample manager for browsing, tagging, filtering, rating, moving, renaming, and organizing sample files.
2. An auditioning tool for quickly moving through sounds, hearing them immediately, comparing related material, and deciding what is worth keeping.
3. A lightweight destructive sample editor for turning long or short audio files into clean, named, reusable samples without leaving the browsing flow.

The product is built around three primary workflows.

### 1. Sample Extraction Loop

This loop turns long audio files into usable smaller sample files.

There is no special “source recording” file type. A long jam recording is just an audio file in the library. It may be longer than other sample files, but Wavecrate should treat it as a normal sample file with the same browsing, auditioning, editing, tagging, rating, and file-management capabilities.

This workflow should support:

1. Open or scan long audio files such as hardware jams, synth experiments, resampling sessions, modular patches, field recordings, rehearsal takes, or other recordings.
2. Navigate quickly through large ordinary WAV files, with AIFF/AIF support planned for a later phase.
3. Audition from any position without waiting.
4. Inspect the waveform at useful zoom levels.
5. Use analysis aids such as manually entered or region-derived BPM, tempo grids, transient cues, silence detection, waveform overviews, and energy/section cues where practical.
6. Create play selections for auditioning sections.
7. Loop selected regions for closer listening.
8. Slide or move loop selections through the waveform to audition different parts quickly.
9. Mark interesting sections.
10. Adjust region boundaries accurately, including zero-crossing, grid-aware, or transient-aware adjustment where practical.
11. Extract selected regions into new audio files that sound exactly like what was auditioned.
12. Show immediate extraction success feedback for the extracted range.
13. Continue searching the original audio file without losing flow.

Extraction is non-destructive because it creates a new sample file from a selected region. The original audio file remains in place unless the user performs an explicit destructive edit on it.

### 2. Sample Library Curation Loop

This loop turns many existing or newly extracted sample files into a clean, structured, well-managed library.

The user should be able to work through large lists of sample files, audition them quickly, decide what they are, decide whether they are useful, clean them up, name them consistently, tag them, rate them, and place them in the right location.

This workflow should support:

1. Browse, search, filter, or explore sample files by folder, tag, rating, metadata, age, review state, or similarity.
2. Select a sample file and hear it immediately.
3. Inspect and edit the waveform.
4. Apply micro-edits such as trimming, fades, silence removal, gain adjustment, normalization, and timing/BPM metadata correction where practical.
5. Add or remove tags quickly.
6. Rate or triage the sample file through the keep/trash rating system.
7. Use generated database display names based on tags, labels, prefixes, BPM, tuning/scale, and other user-authored metadata.
8. Deliberately apply generated names to disk filenames when desired.
9. Move, copy, collect, export, or route sample files into the right folders.
10. Continue through the library without losing flow.

The purpose of this loop is to turn a pile of audio files into a clearly named, rated, tagged, searchable, and organized library.

### 3. Sample Library Usage Loop

This loop uses Wavecrate as a fast sample library while making music.

The user should be able to open Wavecrate during production, quickly find sounds that fit the track, audition related material, and move the chosen sample file into a DAW or other creative tool with minimal friction.

This workflow should support:

1. Browse, search, filter, or explore the library by folder, tag, rating, metadata, age, and list-based similarity first, with 2D map position added in the later map phase.
2. Audition samples immediately without disrupting the creative flow.
3. Set and lock a target audition BPM where useful.
4. Audition BPM-tagged samples warped to the target BPM where practical.
5. Compare groups of related sounds through list-based similarity first, and later through the starmap.
6. Quickly narrow results by sound type, character tags, BPM, rating, age, source, or similarity to a reference sample.
7. Preview enough waveform and metadata context to choose the right sound quickly.
8. Copy, drag, export, reveal, or otherwise hand off the selected sample file to a DAW or external tool.
9. Return to browsing and auditioning immediately.

The purpose of this loop is to make Wavecrate useful as an active production companion, similar in spirit to Sononym-style sample discovery, where finding and using the right sound is fast, fluid, and musically focused.

Everything in the application should support one or more of these workflows.

## Scope and Interpretation

This document is a durable product and architecture target for Wavecrate. It is not a one-shot implementation task.

Use it when reviewing features, refactors, UI changes, audio-engine work, database work, background processing, similarity systems, editing workflows, rating and triage behavior, naming systems, logging, diagnostics, and the Wavecrate/Radiant boundary.

Prefer incremental changes that move Wavecrate closer to this target while preserving working validation lanes. A change does not need to solve the whole product direction at once, but it should not make the intended direction harder.

The current codebase has one Wavecrate desktop UI surface and several supporting
runtime/test surfaces:

- `src/native_app.rs` is the Wavecrate desktop UI entrypoint and should be treated as the place for new Wavecrate desktop UI behavior.
- `src/readiness_execution/**` owns feature and embedding readiness execution used by native source processing. It is part of the default library graph and must remain independent of `src/app/**`.
- `src/app_core/**` contains host-facing projections, action catalog state, and controller integration used by tests and companion runtime surfaces.
- `src/app/controller/**` contains older controller workflow logic that may still be the best evidence for mature behavior such as rating, filtering, trash configuration, similarity, recording experiments, and recovery flows, but it should not be used to re-expand the legacy UI path by default.

The retained `src/app/**`, `src/app_core/**`, and `src/gui_test/**` compatibility
surfaces are opt-in through the `legacy-controller` Cargo feature for tools that
still require them. They must not be compiled by the default Wavecrate library
or native application path.

Current implementation details are evidence, not automatic product requirements.
If current code uses a temporary cap, simplified file operation, developer
fixture, permanent delete, or old runtime-only affordance that conflicts with
this target, treat that as an implementation gap unless this document explicitly
adopts the behavior.

The default GUI already demonstrates several target-aligned behaviors that should be preserved while the fuller architecture is built: adding source folders, incremental folder scanning with progress, a compact source/folder/sample layout, file-list columns for name, extension, size, and modified time, immediate audition on sample selection, stale-safe latest-sample loading, separate play and edit waveform selections, primary-button play selection, secondary-button edit selection, waveform pan/zoom, edit fade preview handles, play-selection extraction to a sibling WAV, extraction success feedback, external drag of selected browser files, and output-device/volume controls.

## Core Product Goals

Wavecrate should provide:

1. Fast source and folder browsing for large sample libraries.
2. Correct handling of both short sample files and long audio files.
3. Full support for ordinary WAV files as the first target audio format, with AIFF/AIF planned for a later phase.
4. A sample extraction workflow for cutting useful regions out of long audio files into new sample files.
5. A sample library curation workflow for editing, tagging, rating, naming, moving, and cleaning up sample files.
6. A sample library usage workflow for finding, auditioning, and handing off sounds to a DAW or external creative tool.
7. Immediate sample auditioning from keyboard, mouse, selection changes, region selections, and similarity workflows.
8. A robust audio engine for playback, decoding, seeking, looping, range auditioning, edit auditioning, BPM-aware auditioning, and future extraction into a standalone audio-engine library.
9. Clear waveform visualization with precise playhead, cursor, play selection, edit selection, range, marker, loop, fade, grid, metronome, transient, extraction-success, and edit-state feedback.
10. Lightweight destructive sample editing for both macro extraction support and micro cleanup, including trimming, cutting, splitting, muting, fading, gain adjustment, normalization, silence removal, timing metadata correction, and range export.
11. Audio-analysis tools that help users find useful material, such as deliberate BPM/grid workflows, transient detection, silence detection, waveform overviews, aging/listen-history indicators, and similarity analysis where practical.
12. Reliable file, folder, export, copy, drag, trash, and DAW handoff workflows with clear recovery behavior where relevant.
13. Fast tagging, rating, filtering, metadata, indexing, display naming, and persistence workflows for large libraries.
14. A tag-category and database-display-name system that helps enforce consistent sample naming.
15. Library triage tools based on keep/trash ratings, aging, listen history, untagged filters, and temporary color collections.
16. Similarity analysis for discovering related sounds through browser filters and a visual starmap.
17. Dense, low-friction UI optimized for repeated scanning, auditioning, cutting, editing, rating, naming, tagging, organizing, finding, and using sounds.
18. Background scanning, decoding, waveform preparation, analysis, edit rendering, similarity indexing, and metadata work that never blocks the GUI thread.
19. Predictable status and progress feedback for long-running work.
20. Detailed logging and diagnostics for tracing failures, stalls, background jobs, and unexpected state changes.
21. A session-local undo/redo system for destructive edits, file operations, metadata operations, rating operations, and meaningful UI interaction state.
22. Clean separation between Wavecrate product logic, Radiant GUI-library logic, reusable audio-engine logic, and persistence/indexing logic.
23. Tests, diagnostics, and validation workflows that protect real user behavior, performance, and recovery guarantees.
24. A cohesive editorial-terminal visual language: near-black charcoal surfaces, cool neutral dividers, warm off-white typography, and a restrained coral accent that keeps dense controls and waveform states legible without visual noise. Spectral waveforms keep low energy in a deep blue-purple lane, mids in coral, and highs in bright cool ivory so frequency content remains distinguishable at a glance.

## Product Principles

Wavecrate should optimize for:

- audition-first workflows
- realtime-feeling navigation and playback
- fast extraction from long audio files
- fast destructive sample preparation and cleanup
- dense, structural layouts over decorative surfaces
- clear warnings for destructive actions until the user opts into advanced destructive workflow mode
- session-local undoable operations wherever practical
- explicit status when work is running
- correctness under large libraries and slow disks
- recoverability for destructive edits and file operations through session undo and safe trash movement
- predictable keyboard and pointer behavior
- similarity-driven discovery without hiding the underlying file/library structure
- fast sample handoff to DAWs and external creative tools
- fast tagging, rating, naming, organizing, and filtering with minimal friction
- gentle library-hygiene workflows that encourage users to keep reviewing, rating, and cleaning their libraries
- traceable behavior when something goes wrong

The GUI thread is for UI work: input handling, selection state, lightweight view-model updates, rendering coordination, and applying completed results. It should remain non-blocking at all times. Any loading, decoding, file I/O, cache hydration, waveform preparation, audio analysis, edit rendering, database/index work, metadata writing, cleanup, logging flush, or other operation that can take noticeable time must be offloaded to background work with clear state handoff back to the UI. Blocking the GUI thread is acceptable only for operations that are proven trivial, bounded, and not practically offloadable. File-operation commands may derive lightweight intent from current UI state, but destination collision probing, file creation/copy/delete/metadata reads, and source database writes should happen behind a background or typed file-operation boundary.

Audio host/device/sample-rate discovery and output-stream reconfiguration follow the same rule: UI handlers record the requested configuration and show pending state, while cancellable background work performs driver discovery and stream construction. Only the latest typed completion may update options, activate output, restart playback, or persist the confirmed configuration.

Perceived stalls are product bugs. If a source scan, decode, rename, edit render, waveform update, BPM/grid metadata calculation, transient analysis, similarity analysis job, database/index update, logging flush, or metadata update can take noticeable time, it belongs off the GUI thread with clear state handoff back to the UI.

Playback command submission and runtime event delivery are bounded,
non-blocking handoffs. The configured playback queue capacity applies to both
queues; repeated polls, retargets, play intents, volume, and gain updates
coalesce, while stop, cancel, and shutdown admission takes precedence over
disposable work. Runtime progress is lossy under pressure; terminal playback
events evict progress and lower-priority cancellation events, and terminal
capacity exhaustion remains non-blocking and bounded. Wavecrate applies a
fixed per-frame event budget so recovery cannot turn one stalled-runtime
backlog into an unbounded UI frame.

Opportunistic metadata updates such as listen-history writes should never delay sample selection, validation, cache loading, or playback. They should use low-priority background work, short database busy timeouts, and skip/retry behavior when source databases are locked by higher-value work.

Supported developer validation is also a bounded automation surface. On macOS,
parent-workspace validation must not depend on an indefinitely growing shared
Cargo artifact directory. It must distinguish quiet active compilation from a
genuinely idle owned process tree, capture actionable command/toolchain/process
samples before termination, and clean up only the process group created for the
current run on failure, cancellation, or confirmed no progress. Focused
standalone Radiant validation remains useful evidence but never replaces the
parent-workspace gate.

## Release Identity

Wavecrate uses pre-1.0 semantic versions until the product is explicitly declared ready for a 1.0 release. The current line is `0.19.1`, with release coordination on `release/0.19` and artifact identities such as `v0.19.1-rc.N` and `v0.19.1`. New builds must not continue the legacy `19.x` numbering.

Previously published `19.x` artifacts remain immutable historical releases. During the one-time renumbering, update checks must allow an installed legacy `19.1.x` build to discover `0.19.1` or a later pre-1.0 release even though ordinary SemVer ordering would treat the new major-zero version as lower. Conversely, a pre-1.0 installation must ignore historical `19.1.x` catalog entries. This exception is scoped to the colliding `19.1.x` line so that a future ordinary upgrade to a real `19.x` release retains normal SemVer ordering. PortalSurfer must display catalog artifact versions truthfully; it may identify the current development line separately, but it must never relabel an existing signed `19.x` artifact as `0.19.1`.

## Non-Goals

Wavecrate should not become:

- a general-purpose GUI framework
- a DAW or multitrack arrangement tool
- a plugin host
- a VST/AU/CLAP effects host
- a modular effects processing environment
- a managed central sample-library system that hides the real filesystem
- a database administration tool
- a collection of one-off UI experiments
- a machine-learning-based audio intelligence platform in the current target
- a general-purpose audio converter for arbitrary file formats
- a standalone resampling/conversion utility
- a broadcast-audio metadata editor or RF64/BWF compatibility tool
- a cloud sync service
- an account-based library service
- a proprietary sample-pack manager
- a hidden collection/export system that replaces real folders

Supported audio formats for the current target are ordinary WAV first, then AIFF/AIF in a later phase. MP3, FLAC, and other audio file formats are non-goals unless explicitly added later.

The WAV target means ordinary RIFF/WAVE files used in typical music-production and sample-library workflows. RF64, MBWF, Broadcast Wave Format-specific behavior, and deep broadcast metadata support are non-goals for the current target.

Wavecrate may contain DAW-like primitives such as waveforms, ranges, meters, fades, markers, loops, transport controls, tempo grids, transient markers, warp auditioning, and timeline overlays. These exist only to support sample exploration, extraction, preparation, and handoff.

Wavecrate may support copy, drag-and-drop, export, reveal-in-explorer, or file handoff into DAWs and other creative tools. This does not make Wavecrate a DAW integration layer or plugin host.

Wavecrate may include simple built-in sample operations such as fades, cuts, mutes, gain changes, normalization, trimming, splitting, silence removal, BPM metadata correction, audition-time warping, and exports. These are editing and auditioning tools, not an open-ended effects system.

Runtime model inference and ML-based similarity are not part of the current target.

Wavecrate should be local-first. It should not require a user account, cloud service, internet connection, online model service, telemetry backend, cloud sync feature, or proprietary pack format for normal browsing, editing, extraction, analysis, organization, collection, and handoff workflows.

## Platform Target

Wavecrate should be developed Windows-first.

The initial primary development and testing target is:

- Windows

Future platform targets may include:

- macOS
- Linux

Cross-platform support does not need to be implemented immediately, but the architecture should avoid unnecessary Windows-only assumptions in core product logic, audio-engine logic, persistence logic, and reusable GUI integration.

Platform-specific code should be isolated behind clear boundaries. File watching, drag-and-drop, DAW handoff, audio-device handling, paths, temporary recovery folders, trash-folder behavior, shell integration, and windowing behavior should be designed so future macOS and Linux support can be added without rewriting core systems.

## Privacy, Network, and Local Data Target

Wavecrate should treat sample libraries as private local creative material.

The product target is fully offline operation. Wavecrate should not upload audio files, filenames, metadata, analysis descriptors, logs, crash reports, or library statistics to external services.

Cloud sync, account-backed libraries, and Wavecrate-specific sample-pack storage are not part of the target. If users want a durable collection of files, Wavecrate should help them create, move, copy, rename, or export ordinary files into ordinary folders.

Local files created by Wavecrate should be understandable and user-recoverable:

- source databases
- waveform caches
- analysis caches
- similarity indexes
- temporary handoff files
- recovery files
- logs and diagnostic bundles

Diagnostic bundles should avoid exposing full audio content by default. If a diagnostic bundle includes paths, filenames, metadata, or audio excerpts, Wavecrate should make that clear before the user shares it.

Diagnostic bundles should exclude source-local `.wavecrate.db` files because they may contain user library metadata. Defining a workflow for including source metadata in diagnostic bundles is a non-goal for the current target.

## Storage Locations

Wavecrate should split storage between source-local state and global application state.

Source-specific information should live in the folder of that source. Each indexed source folder should contain a special Wavecrate source database file named `.wavecrate.db`. This keeps source metadata close to the real files and makes a source folder more portable between machines or Wavecrate installations while avoiding confusion with the global `.wavecrate` configuration folder.

Source onboarding should avoid noisy implementation details in the main add-source flow. The facts that Wavecrate creates or updates `.wavecrate.db` in the selected folder and may write embedded Sample ID metadata to supported files should be documented, but the add-source UI does not need to mention those implementation details by default. The source-add flow does not need a separate destructive-nature warning because non-YOLO destructive safety prompts cover destructive operations when the user actually triggers them; adding a source is not a separate read-only mode.

The `.wavecrate.db` file is internal source metadata and should be hidden from the normal sample browser, folder tree, and `all files` visibility mode. Wavecrate should mark the file as operating-system hidden where the platform supports that behavior. Diagnostics, source repair flows, logs, and support UI may mention the file when relevant, but reveal actions should reveal the source folder rather than selecting or opening `.wavecrate.db` directly.

The `.wavecrate.db` file should be excluded from ordinary copy, move, trash, delete, rename, batch, drag, clipboard, export, and handoff operations that target visible sample/folder items. It should only be manipulated by explicit source maintenance, repair, migration, backup, or diagnostics workflows.

If a user copies, moves, backs up, syncs, or transfers an entire source folder outside Wavecrate, the `.wavecrate.db` file should travel with that folder as source-local metadata. Wavecrate should be able to relink or reopen the moved/copied source folder and use the carried `.wavecrate.db` to preserve file metadata where file identity and paths can be reconciled safely.

Wavecrate should not provide a separate user-facing backup command for `.wavecrate.db` or source metadata in the current target. Normal external whole-folder backup, sync, or transfer is the intended backup path, and diagnostics may explain that source metadata travels with the source folder through `.wavecrate.db`.

When Wavecrate itself copies a folder inside a source, it should exclude `.wavecrate.db` from the copy. In-app folder copy should create ordinary copied files and folders, then let Wavecrate register/reconcile copied audio as new independent file identities instead of carrying source-local database state into the copied folder. This applies only to folders below a configured source root; copying the configured source root itself should not be available as an ordinary in-app folder action.

If a copied source folder is added while the original source is still indexed, Wavecrate should treat the copied folder as an independent source rather than rejecting it as a duplicate source. Any duplicated embedded Sample IDs or duplicate audio content inside the copied source should be handled by the normal file-level duplicate-ID conflict and exact-audio duplicate systems, not by blocking source addition.

Wavecrate should support explicit source roles:

- Normal Source: the default writable source behavior.
- Protected Source: readable, playable, and curatable, but Wavecrate must not mutate existing files inside it. Adding new files to a protected source is allowed when the user explicitly chooses that destination; overwriting or destructively editing existing protected files is not allowed.
- Primary Source: the default writable destination for “make this editable” and harvest output flows. A protected source cannot also be primary; primary sources must be writable.

Protected sources are the intended workflow for DAW project folders, archive drives, downloaded packs, field recordings, and other source piles where the user wants to harvest useful material without damaging originals. Protected-source curation may still write metadata to external/app-level storage. It should not silently create hidden source-local metadata inside folders such as Ableton project directories unless the user explicitly allows that storage mode.

Documentation should cover implementation details such as source database updates, external metadata roots, and embedded Sample ID metadata writes without requiring the main add-source UI to enumerate them by default.

If the operating system, permissions, file locks, or external tools prevent Wavecrate from writing to a source, Wavecrate should report that as a source/file write limitation rather than treating it as an intentional read-only library mode.

Source database schema changes must be migratable through the explicit source-write or maintenance open path. Additive tables and columns should be repaired even when a database already carries the current schema stamp, because development and prerelease builds may leave current-stamped files with incomplete structure. Every runtime caller must select a role-specific open for UI reads, background reads/jobs, source writes, user metadata writes, playback history, or maintenance; general `open` aliases are not part of the supported API. Read-only source database opens must not mutate the database and must tolerate older schema shapes through safe default projections or feature-specific query avoidance.

Browser metadata hydration should read ratings, locks, collection memberships, and playback/curation timestamps from one coherent typed snapshot with a statement count that is independent of source size. Capability discovery happens once per snapshot so legacy read-only compatibility cannot regress into per-file schema inspection. A failed hydration preserves the last good browser metadata projection and reports the failure; opportunistic mutations such as rating decay run later through bounded background maintenance and explicitly refresh affected metadata.

The source database should store information that belongs to that source and its files, such as:

- stable Wavecrate Sample IDs for files in that source
- source-relative paths and file fingerprints
- source scan state and diagnostics
- per-file metadata, ratings, global tag assignments, labels, generated-name inputs, listen history, and analysis state where those records belong to files in that source
- reconciliation state for missing, moved, renamed, changed, unsupported, or failed files

Pending rename metadata is retained by completed authoritative source generations, not by a
wall-clock TTL. A targeted watcher batch may stage or consume candidates but never ages them.
Quick-scan candidates remain eligible through two later successfully completed full-source
enumerations; a partial traversal, cancellation, unavailable root, failed audit verification, or
database failure does not advance that generation. A complete hard scan may remove an unmatched
candidate immediately after current destinations have been reconciled. Pruning and authoritative
generation publication are one source-database transaction, and unresolved deferred-hash
destinations defer pruning. Rename reconciliation queries current destination candidates through
indexed unique lookups capped at two source rows, so memory and lookup work do not scale with
historical deletions. Aggregate diagnostics expose the retained count, authoritative and oldest
candidate generations, oldest diagnostic staging time and age, destinations considered, and
candidates pruned without per-file logging.

Global application state should live in a user-level `.wavecrate` folder. On Windows, the initial target should be a conventional user config/home location such as `%USERPROFILE%\.wavecrate`, similar in spirit to `.cargo`, `.codex`, and other developer/user configuration folders. Future macOS and Linux support should map this same logical root to the platform's appropriate user config/home location.

The global `.wavecrate` folder should contain app-wide state such as:

- application settings and preferences
- configured source list and source database references
- global tag category definitions and user-extensible tag dictionaries where they are shared across sources
- temporary color collection definitions and cross-source marks
- global cache indexes
- waveform, analysis, similarity, and map caches
- handoff staging temp files
- session undo/recovery files
- logs, diagnostics, and diagnostic bundle staging

Caches, logs, handoff staging, and recovery files should not be scattered across arbitrary sample-library folders. They should live under the global `.wavecrate` root unless there is a specific source-local reason to store a compact reference or source-owned database record.

Cache payloads should live under the global `.wavecrate` folder in the current target. Source-local `.wavecrate.db` files may store cache references, status, fingerprints, and invalidation state for files in that source, but waveform, analysis, similarity, map, handoff, and other cache payloads should not be written next to the source database.

Settings should report the current on-disk footprint of the global `library.db` (including active SQLite sidecars) and rebuildable cache, with the filesystem traversal performed outside the GUI thread. This read-only usage insight excludes source-local databases, logs, configuration, and handoff staging so the displayed scope stays consistent with the cache-maintenance surface.

Browser row cache indicators describe compact waveform or browser projections only. They must not imply that decoded playback is persisted or reuse source-readiness markers; supported WAV auditionability comes from the current source file and file-backed playback runtime.

### Durable Source Readiness and Convergence

Every enabled, available source should converge automatically in background work after application startup, a committed source scan, a committed file-watch reconciliation, an internal file mutation, an artifact-contract version change, or a due retry. Normal correctness must not depend on selecting a source or folder, opening similarity, pressing Process/Refresh, or visiting a row. Explicit Process, Refresh, rebuild, and retry actions may remain as repair or priority controls, but they are not the source of truth for whether work is required.

The source-local database is the authoritative durable coordination boundary. It should store desired versioned readiness targets, exact artifact completion generations, source availability, and readiness-owned work metadata. Readiness work must reuse the existing source-local persistent analysis/job storage rather than introducing an unrelated queue format. App-global cache payloads remain rebuildable performance aids outside this readiness contract: compact waveform or browser caches may be missing, stale, or evicted without making an otherwise complete source unready.

Playback and waveform cache work remains automatic, bounded, attributable to exact source-file identity, invalidatable after content changes, and subordinate to foreground audition. Its separate cache lifecycle owns stale-completion rejection, path move/remap, delete, pruning, and source-retirement cleanup. It must not publish a `playback_summary` desired target or completion artifact, and legacy rows from builds that did so must be ignored by read-only reconciliation and retired on the next writable source-processing pass.

Waveform-cache publication and cache-limit maintenance share the single background store worker so pruning cannot race an in-process cache publish. The worker performs a full on-disk reconciliation when it starts, then batches later reconciliations until 64 successful writes, 16 MiB of conservatively counted written payload, or 250 ms of writer idleness. The 2 GiB limit may therefore be exceeded transiently by no more than the payload written before the first threshold is crossed, including the final entry that crosses it; each reconciliation remains authoritative over current directory contents so crashes, failed accounting probes, and external cache-directory changes recover without durable bookkeeping.

Configured source lifecycles also carry a runtime epoch distinct from the durable source identity. Removing or replacing a descriptor cancels that exact epoch, immediately closes new admission, purges its queued UI/readiness work, and lets the control-plane transition return without waiting for scans, hashing, finalization, or cleanup. Late work may not publish after its epoch is retired. A replacement that shares the same source database waits for the old epoch to drain before new processing admission; destructive retirement is suppressed when that database has already been re-added.

The source-processing supervisor publishes progress, readiness advancement, manifest-audit handoff, retry/block state, durable per-source health, and completion through one typed backend-neutral event sink. Every source-scoped event carries its configured lifecycle epoch and is rejected before delivery when that epoch is no longer current. Health is derived from the durable readiness snapshot plus runtime ownership, coalesced to one changed observation per source lifecycle, and retained after global progress becomes idle. The supervisor owns ordering and bounded progress throttling; native-app adapters own `GuiMessage` construction and user-facing stage/detail wording. Telemetry and non-GUI observers consume the semantic event model without depending on native presentation types.

Source-readiness reconciliation progress is phase-specific and must not expose cancellation
checkpoints or cumulative database row visits as completed product work. Manifest inspection is
indeterminate until the existing pass has a reliable file denominator; target preparation,
readiness comparison, and unfinished-work queueing then report monotonic completed/total counts
using files or readiness targets. Internal checkpoints remain frequent diagnostic and
cancellation telemetry, while lifecycle fencing prevents retired-source progress from replacing
the current source presentation.

The native source-processing implementation has one public supervisor facade and one
coordinator thread. Internally, the facade owns lifecycle and admission calls, while the
coordinator delegates through focused discovery/reconciliation, work-execution,
retirement/cleanup, progress-event, and telemetry services. Shared coordinator state,
runtime task models, source-registry identity, and cache-ownership queries are explicit
lower-level contracts. Dependencies point from the facade to the coordinator, from the
coordinator to services, and from services to those contracts; a service must not import
the facade or coordinator, reach through another service, or start a second coordinator
or persistence queue. The existing joined retirement cleanup worker remains an
implementation detail of the retirement service.

The coordinator owns one joined, bounded execution pool for file-scoped feature and
embedding work. Pool workers orchestrate explicit typed claim/read, pure preparation, and
publication phases under the supervisor's lifecycle guard. Production preparation runs
in killable child processes that receive bounded owned inputs, perform no SQLite access,
and return bounded prepared payloads; the surrounding pool worker acquires the single
writer gate only for readiness claims, lease renewal, and fenced publication. The same
gate covers foreground scan database open/migration and bounded manifest or deferred-hash
publication, while scan traversal, file preparation, and content hashing run outside it.
The one-at-a-time foreground scan lane and lifecycle permit remain held across the complete
scan. Publication revalidates exact manifest, content-generation, lifecycle, and
cancellation fences after preparation. The initial rollout admits at most
one execution job per source and two jobs globally, allowing a secondary source to use
spare capacity without changing the visible active-source owner. Requests and results are
bounded, pool workers and child processes are joined on shutdown, and telemetry reports
queue depth, worker concurrency, and writer wait/hold timing by database phase.
Manifest audits, indexed hashing, similarity finalization, and retirement keep their
existing serial compatibility paths until each can preserve its current atomic contract
across the same typed phase boundary.

Watcher recovery barriers advance only after an authoritative manifest traversal and
reconciliation completes. A cancelled, stale, or unreadable traversal retains the prior
barrier even when it publishes a committed partial delta for retry. A traversal that does
complete may separately pause bounded content verification at its durable checkpoint without
withholding the manifest-coverage completion.

Source retirement keeps the audio folder, authoritative source database, source manifest, reusable analysis/similarity artifacts, and safe content-addressed analysis caches. It releases source-owned in-memory playback/preview/browser state and asynchronously removes readiness-managed jobs and unshared managed cache payloads. The source descriptor registry retains role, metadata-storage policy, and primary-import settings so protected/AppData sources rehydrate safely; corrupt present descriptor values fail closed. Cleanup is idempotent, retryable after unavailable/read-only storage, recovered from retained inactive descriptors at startup, and bounded so removal never waits for database maintenance or cache garbage collection.

For every current eligible file, the readiness stages are:

1. the committed source index contains the current file identity and content generation;
2. the required analysis feature version is current;
3. the required embedding model and aspect-descriptor version are current; and
4. the source-level similarity membership, ANN/index, layout, and cluster generation covers exactly the current eligible identities.

This is exactly three file-scoped targets per current eligible file plus one source-scoped similarity target. Playback-cache residency is not a fifth readiness stage.

Stable filesystem identity is an ownership key, not a uniqueness assertion over visible manifest
paths. If two supported live paths are hard links to the same filesystem object, the current
readiness executor must not collapse one path into a silently incomplete browser record. Until
path-alias-aware analysis and browser projections are implemented, discovery records a bounded,
actionable `duplicate_manifest_identity` terminal diagnostic containing the affected relative
paths, parks that unchanged identity-membership revision without a five-second retry, and rechecks
only after a supported live identity changes or an explicit manifest repair is requested. It remains
parked if the duplicate condition persists. Distinct identities are never merged based on size,
timestamps, or content hash.

Each target and completion must carry the committed source generation, a non-empty file content generation or source-membership generation, and the stage's artifact-contract version. Desired-state publications also carry a per-source monotonic readiness revision; a writer whose revision is not newer than the persisted revision is stale even when its source generation is equal, so delayed lifecycle writers cannot reactivate an offline or disabled source. The source generation is a publication and diagnostic fence, not a blanket invalidation token for file-scoped artifacts: an unchanged stable file identity remains current across unrelated manifest changes when its stage version and stage-relevant content/path generation still match. Source-scoped similarity work must additionally match the current source generation and membership generation. A late completion may publish only when the values relevant to its scope still match the current desired target. Timestamps, total row counts, source-prep markers, or the existence of some artifacts are diagnostic inputs only; they must never make a source look ready when a current eligible identity lacks an exact artifact or when stale/deleted identities are still the only covered rows.

The readiness classifier must distinguish current, pending, running with a lease, retryable failure with a retry deadline, permanent failure, unsupported terminal state, offline, disabled, deleted, and stale-by-generation. Unsupported, deleted, and permanent-failure targets remain observable but do not spin. Offline and disabled sources retain desired state without being scheduled until they become active and available. Expired leases and due retryable failures become actionable deficits after restart or worker interruption.

The source-processing execution boundary carries a typed failure class and stable machine code separately from its display/log context. Worker, decoder, filesystem, SQLite, process, timeout, and invariant owners choose that outcome before crossing into the supervisor; presentation text must never determine retry policy. Durable readiness work persists the code separately from `last_error`, while a bounded compatibility migration may inspect legacy rows that predate typed failures. Unknown current failures fail closed as terminal and remain observable with their code and diagnostics.

One revision-aware reconciler owns source completeness. Routine safety probes compare the durable manifest generation, accepted readiness generation, source availability, retry/lease deadlines, and an independently versioned artifact contract before reading file targets. A fully converged unchanged source must stop at that bounded checkpoint. Authoritative committed manifest changes carry created, changed, moved, and deleted identity deltas into readiness persistence; one generation step may update only those identities plus the source similarity target, while a missing checkpoint, generation gap, duplicate identity, or contract change falls back to a complete target publication. Similarity membership uses a durable order-independent cryptographic accumulator and member count so one identity can be added, removed, or replaced without hashing the complete manifest, while full publication remains the correctness fallback.

Reconciliation compares desired targets with persisted artifacts and readiness-owned work, deduplicates deficits by source, scope identity, stage, version, and generation, and exposes per-source/per-stage counts, the accepted readiness revision, current work, retry/failure state, stable failure codes, and diagnostic generations. Fully ready, processing, retry-waiting, prerequisite-blocked, offline, disabled, terminally degraded, and reconciliation-failed sources remain distinguishable without keeping global progress active. It must not report idle while an actionable deficit exists. Source completeness is computed from the exact current eligible identity set plus the current source-level target, never from row counts or timestamps alone. Telemetry distinguishes cheap no-op safety probes, committed target-delta reconciliation, and explicit full manifest audits.

Detection and execution stay off UI, render, and latency-sensitive read paths. UI and read paths may observe a side-effect-free readiness snapshot and request priority, repair, or retry, but they must not discover a missing artifact by directly enqueueing hidden work. Lifecycle writers publish committed desired generations and wake the background coordinator only after their authoritative transaction succeeds.

Native source-preparation callers declare orthogonal intents for readiness wake or reanalysis, source priority, persisted-metadata projection, waveform-cache projection, cache warming, and user feedback. Source selection, folder activation, verified startup, scan completion, committed filesystem mutation, watcher reconciliation, and explicit `Process Source` each request only their required effects; descriptive telemetry reasons never select policy. The source-processing supervisor remains the sole owner of readiness convergence.

The readiness contract must be protected by a deterministic native-runtime
liveness oracle over real temporary sources. The oracle observes committed
manifest/readiness generations and the supervisor's durable/runtime state
directly rather than treating UI progress, queue length alone, or fixed sleeps
as proof. While an active source has an actionable deficit, the source must be
dirty or scheduled, queued, in flight, resource-paused, waiting for a retry
deadline, or waiting for an exact prerequisite; a stable state outside those
categories is silently idle and must fail with generation, stage, job,
lease/retry, watcher-health, active-work, and resource diagnostics. Restart,
external and internal churn, watcher overflow/loss, playback, source
disappearance/reappearance, removal/re-add, stale completion, terminal inputs,
exact removal of obsolete similarity membership, and legacy playback-readiness
retirement belong in this
proof. Large-library throughput and browser/frame responsiveness remain an
explicit calibrated lane rather than an unbounded normal-CI soak.

If a source database is missing, corrupt, locked, or unreadable, Wavecrate should keep the user's audio files untouched, report the source database problem clearly, and offer repair/rebuild/reindex options where safe.

File-operation journal recovery must keep staged publication and cleanup bound to the
descriptor that was verified under the no-follow target-root capability. Windows recovery
uses a handle-bound, atomic no-replace rename and handle-bound disposition; it must not
re-resolve the staged pathname or fall back to pathname move, hard-link, or unlink helpers.
Platforms without an equivalent descriptor-bound primitive fail closed before staged
publication or cleanup and retain both staged data and the journal row. Move recovery must
also open the source database through a self-validated namespace bound to the verified
source-root capability, keeping that binding alive through SQLite use and close so its WAL
and SHM sidecars remain in the same namespace. Root replacement, namespace mismatch, or
binding unavailability must leave source data and recovery state retained for retry.

## Audio Format and Channel Target

Wavecrate should support the following audio formats:

- ordinary WAV as the first fully supported format
- AIFF/AIF as a later-phase supported format

MP3, FLAC, and other audio formats are out of scope for the current target.

WAV support should target ordinary RIFF/WAVE files. Wavecrate should not initially attempt to support RF64, MBWF, or Broadcast Wave Format-specific behavior. If such files are discovered, Wavecrate should classify them as unsupported or playback-only unless the audio engine can safely decode them without risking destructive edits.

This is a deliberate simplification. Wavecrate should prioritize reliable destructive editing, extraction, metadata writing, waveform analysis, and DAW-compatible file behavior over broad format support.

Recognizing a file as an audio-looking file is not the same as supporting it. Non-goal formats such as MP3 or FLAC should be classified and reported as unsupported, but they should not appear in the normal sample browser by default. The browser should provide an explicit `all files` visibility flag that reveals unsupported audio files, unsupported non-audio files, and folders that are otherwise hidden by the supported-audio view. If an unsupported audio file can be decoded safely, Wavecrate may allow playback-only auditioning in `all files` or diagnostic views, but it should still block editing, metadata writes, waveform previews, waveform-derived workflows, analysis/cache generation, tagging/rating as sample metadata, and handoff.

Wavecrate should align its practical audio-file compatibility with common DAW workflows, especially Ableton Live-style sample workflows. It should support common bit depths, sample rates, and channel layouts used in music production.

The target includes:

- common PCM bit depths such as 16-bit and 24-bit where supported by the format
- higher precision PCM or floating-point formats where they are part of normal DAW sample workflows
- common music-production sample rates such as 44.1 kHz, 48 kHz, 88.2 kHz, 96 kHz, and higher rates where practical
- mono files
- stereo files

## Audio Write Format and Engine Configuration

Wavecrate should provide an audio engine/output configuration menu similar in spirit to Ableton Live's audio preferences and export/write settings.

The audio engine/output configuration UI should open as its own movable application window, not only as an inline panel or dropdown inside the main browser. The user should be able to move it independently while keeping the main Wavecrate window visible, so device, backend, sample-rate, and write-format settings can be adjusted while inspecting the current sample context.

The user should be able to choose the active Windows audio engine/backend where available, such as WASAPI or ASIO, then choose the output device exposed by that backend. Wavecrate should also expose the write-format settings used when it creates or rewrites audio files.

Write-format settings should include at minimum:

- output sample rate
- PCM bit depth or floating-point sample format
- output channel behavior for normal mono/stereo writes
- dithering behavior where relevant for reducing bit depth

For WAV files, "bitrate" should usually be treated as PCM bit depth/sample format rather than compressed-audio bitrate. The UI may use user-friendly wording, but the implementation should store precise settings such as 16-bit PCM, 24-bit PCM, or 32-bit float.

Wavecrate should use the configured write format for ordinary new-file audio writes, including:

- extracted regions
- duplicated/processed files
- copied waveform selections that require a staged audio file
- drag-and-drop region handoff files
- explicit exports
- batch edit outputs
- downmix/conversion outputs

Destructive in-place edits should preserve the source file's existing sample rate, bit depth or sample format, channel layout, and container wherever practical. A simple same-file edit such as fade, trim, mute, gain, or normalize should not quietly convert the file to the configured Wavecrate write format just because the global write settings differ from the source audio properties.

If the user explicitly chooses an edit or rewrite workflow that also converts the current file to the configured Wavecrate write format, the command should make that conversion clear before it is committed. Resampling or sample-format conversion should use practical high-quality conversion and should be logged as part of the write operation.

Resampling should be an implicit write-stage behavior, not a separate general-purpose conversion workflow in the current target. When Wavecrate extracts, stages, exports, duplicates, converts, or otherwise creates a rendered audio file, it should render to the configured output sample rate as part of that operation. Same-file destructive edits should preserve the source sample rate by default. The user should not need to run a separate resample command for ordinary Wavecrate-created files.

Wavecrate should not add a standalone "resample this file" command unless the product target is explicitly expanded later. If users want another sample rate, they should change the configured write sample rate before performing the edit, extraction, export, or other write operation.

The configured write format should be persistent, visible in settings, and easy to confirm before creating new rendered files. Commands that create new files should use the same configured write-format policy unless a command-specific export dialog explicitly overrides it. Same-file destructive edits should use source-format preservation by default unless the user deliberately chooses a conversion/rewrite path.

Wavecrate should preserve source channel layout during normal destructive edits. Editing a stereo file in the mono-style editor must not collapse it to mono. In the mono-first workflow, edits should affect both stereo channels equally.

Mono and stereo files are fully supported in the current target.

Files with more than two channels should remain visible in the browser and should be playable where the audio engine can safely decode and route them. They are outside the normal editing target. Wavecrate should block ordinary destructive edits, extraction, normalization, gain, fades, reverse, silence trim, and other editing commands on multichannel files unless explicit support for that operation exists.

For files with more than two channels, Wavecrate should show a clear warning that multichannel editing is not currently supported. The user should be able to convert the file to stereo or mono through an explicit downmix/conversion command if they want to make it editable.

Unsupported, partially supported, playback-only, too-long, and multichannel-limited files should be visually distinct when they are visible in the sample list or eligible similarity surfaces. Unsupported files should remain hidden from the normal supported-audio browser by default, and playback-only unsupported audio files should not appear in similarity or map views because Wavecrate does not create analysis or map projection data for them. When unsupported files are shown through `all files` or diagnostics, rows should show warning/error styling and a tooltip or status explanation describing why normal editing or extraction is unavailable.

Downmixing a multichannel file to stereo or mono is a destructive audio conversion unless the user chooses an export/duplicate workflow. It should follow the normal destructive-edit safety, YOLO mode, recovery, logging, and session undo behavior. The warning should explain that channel information may be lost or combined and that the original multichannel layout will not be preserved in the converted file.

Initial downmix behavior should use simple presets, not a complex channel-mapping UI.

- Convert to mono should combine all channels into one mono signal with safe gain handling to avoid clipping where practical.
- Convert to stereo should preserve obvious left/right channel pairs when available; otherwise it should distribute channels into left and right outputs with safe gain handling to avoid clipping where practical.
- Advanced per-channel routing, surround mapping, ambisonic decoding, and custom mix matrices are non-goals for the current target.

The waveform view should default to a mono-style overview because it is compact and fast for browsing. A later stereo split-view mode should show channels separately and allow channel-specific selection, editing, and extraction.

Implementation should be phased:

1. First, implement a complete mono-style editor that works correctly for mono and stereo files, with stereo edits applied equally to both channels.
2. Later, add stereo split-view editing where users can independently view, select, mark, edit, and extract left/right channels where useful.

## Practical File Size and Duration Limits

Wavecrate should support long audio files intended for jam extraction, resampling sessions, hardware recordings, field recordings, and other extended source material.

The target practical maximum duration for a single audio file is a few hours. An initial target limit should be **4 hours per audio file**.

This limit is not meant to restrict normal sample-library usage. It exists to keep decoding, waveform generation, editing, extraction, undo recovery, and analysis behavior predictable.

Files longer than the practical limit should not crash the application. Wavecrate should detect them, mark them as unsupported or partially supported, and show a clear user-facing message explaining that the file exceeds the current practical duration target.

Wavecrate should not impose artificial limits on total library size beyond available disk space, operating-system constraints, database scalability, and practical background-processing throughput.

## Core Domain Model

Wavecrate should keep the domain model simple and file-based.

### Audio File / Sample File

An audio file is the core object. Long recordings, short one-shots, loops, extracted slices, and edited sounds are all just audio files in the library.

There should be no functional distinction between a “source recording” and a “sample.” A long jam recording is simply a longer audio file. An extracted region is simply a new audio file. An edited file remains the same audio file unless the user duplicates or exports it.

### Region

A region is a marked time range inside an audio file.

Regions may be used for auditioning, looping, extraction, editing, markers, or visual history. Regions should be visually clear in the waveform view and should support precise boundary adjustment.

While a sample remains loaded, the waveform should accumulate the portions that
were actually auditioned and show them as a subtle thick rail along the bottom
edge. This played-range history is session-local to that loaded waveform, resets
when another sample is loaded, and is not persisted as sample metadata. The rail
should advance on paint-only playback frames, use a subdued neutral grey, and
begin at the sample's actual playback start. Supported WAV navigation should
stream the original file from frame zero as one continuous runtime source, so
the waveform can attach to that source without replay, estimated seek offsets,
or an audition-to-full-source splice. Rapid navigation should replace sources
through a minimal three-millisecond crossfade rather than clearing the outgoing
source at an arbitrary sample, preserving sharp transients without hard cuts.

### Play Selection

A play selection is a waveform range used for auditioning and looping.

The play selection should be created with the primary pointer interaction: left mouse button drag in the waveform. It should be easy to move or slide the play selection through the waveform so users can loop and audition different parts of a long file quickly.

### Edit Selection

An edit selection is a waveform range used specifically for destructive editing.

The edit selection should be distinct from the play selection from day one. It should be created with the secondary pointer interaction: right mouse button drag in the waveform. This allows the user to audition one area while editing another area, or to create a more precise edit range without disturbing the audition loop.

Edit commands should follow this priority:

1. If an edit selection exists, edit commands apply to the edit selection.
2. If no edit selection exists but a play selection exists, edit commands apply to the play selection.
3. If neither selection exists, edit commands apply according to the command’s explicit behavior, such as whole-file normalization or current-cursor operations.

The play selection and edit selection should not interfere with each other visually or behaviorally. They are two separate core functions, not one selection with a later mode layered on top.

Beat guides may derive a tempo from the selected duration and configured guide count. An explicit BPM Snap mode should quantize play-selection and edit-selection resize gestures so that derived tempo is a whole-number BPM, while leaving selection creation and whole-selection movement continuous. The live resize preview and committed selection should use the same snapped boundary, and the non-dragged edge should remain anchored.

Previewed fade handles on an edit selection are audition and preview state until explicitly applied. Pressing Enter while an edit selection has active fades should route through the destructive-edit system and bake those fades into the current audio file. After a successful apply, the edit-marked area should flash as confirmation.

Applied edit feedback should be consistent. When any destructive edit succeeds, including mute, normalize, trim/crop, gain, reverse, silence trim, paste, fade apply, envelope apply, or downmix conversion, the affected edit region or whole-file region should briefly pulse or flash as a small visual confirmation. This should be similar in spirit to the export/extraction confirmation on the play region.

### Harvest Derivation History

When a user extracts a region from a longer audio file into a new sample file, the extracted file should become an independent audio file and independent Wavecrate sample identity.

Wavecrate should also store durable harvest derivation history. A source file used as origin material and a file created from it remain independent editable audio files, but Wavecrate should remember the parent/child relationship so the user can see what was touched, what was created, and what still needs review.

Derivation edges should be global/app-level state because they can cross sources. Each edge should record parent identity, child identity, operation type, optional source range or output duration, source and destination source IDs, destination folder, inherited metadata snapshot, timestamp, and Wavecrate/tool version. A file can be both a derived child and a later origin for more derivatives.

Harvest state should be tracked per origin file as New, Seen, Touched, Done, or Ignored. “Has Derivatives” should be computed from the derivation graph rather than stored as the only state. Automatic transitions may move New to Seen or Touched, but must not override user Done or Ignored states. Manual actions may reset or override state.

Protected-source extraction should create the derived file in the harvest destination by default, usually `Primary Source/_Harvests/<Source Name>/`, while keeping focus and the loaded waveform on the protected source file so the user's browsing and audition context stays intact. The derived file should still be registered and visible when the user navigates to the Harvest destination. Normal writable sources may keep current in-place behavior while Harvest Mode still tracks state and graph edges. The waveform should show immediate extraction success feedback for the selected range, such as a short pulse or flash.

### Wavecrate Sample ID

Every indexed audio file should have a stable internal Wavecrate Sample ID.

The filename is not the stable identity. Disk filenames may change, display names may change, and generated names may change. The internal Sample ID should remain the stable identifier for Wavecrate metadata, rating history, analysis cache, similarity data, aging/listen history, and other persisted state.

Wavecrate should store this ID in the database and should also attempt to embed it directly into supported audio files.

During scan and indexing, Wavecrate should automatically assign a database Sample ID to supported ordinary WAV files that do not already have one. The scan should then queue embedded-ID writing as low-priority background metadata work instead of making source discovery, indexing, folder browsing, auditioning, editing, extraction, tagging, rating, or handoff wait for file metadata rewrites.

If a newly scanned or externally added file already contains an embedded Wavecrate Sample ID that is also used by another indexed file identity, Wavecrate should mark the file with a duplicate embedded-ID conflict rather than silently treating both files as the same file or silently overwriting the ID. The file should remain visible enough for browsing, filtering, inspection, reveal, and conflict resolution, but the browser and file details should show a clear warning.

Duplicate embedded-ID conflicts should block identity-mutating actions on the affected files until the conflict is resolved. Blocked operations should include tag edits, rating edits, label edits, metadata edits, temporary color collection changes, destructive audio edits, metadata embedding writes, apply-to-disk rename, manual in-app disk rename, move, copy, duplicate, extraction from the conflicted file, trashing, handoff, and any operation that would use, create, or update durable file identity state. Playback, audition, waveform loading, inspection, reveal, source reconciliation, and explicit conflict-resolution actions should remain available so users can compare conflicted files and decide which identity to preserve. Wavecrate should show a clear message that identity-mutating actions require resolving the duplicate ID first.

Duplicate embedded-ID conflict indicators should be visually distinct from ordinary exact-audio duplicate badges. They do not need to appear more severe by default, but users must be able to tell whether a row has an advisory audio duplicate warning or a blocking identity conflict.

Wavecrate should provide an explicit user action to resolve duplicate embedded-ID conflicts for the duplicate-ID groups represented by the selected conflicted files. Selecting one or more conflicted files chooses which conflict group or groups to resolve; resolution itself applies to each whole duplicate-ID group, including unselected files in that group.

The duplicate embedded-ID resolve action should be available from the conflicted sample row's context menu and/or the conflict warning badge context menu. Global command/menu access may also exist, but the row-level context menu is the primary surface.

When resolving a duplicate embedded-ID group, Wavecrate should preserve one file's existing Sample ID and assign new unique Sample IDs to the other conflicted files in that group. The preserved file should be chosen predictably, such as the focused selected row when it belongs to the group, otherwise the first selected row in the group, otherwise the first indexed available row. The UI should show which file kept the original ID when reporting the result. Resolution should preserve user-authored metadata for each file identity, log the change, refresh affected caches/indexes, and report any files whose embedded ID could not be rewritten.

Resolving duplicate embedded-ID conflicts may run immediately when the user triggers the resolve action. It does not need a confirmation prompt. The action should be visibly reported, logged, and treated as forward-only identity repair rather than a normal global undo transaction.

Resolving duplicate embedded-ID conflicts should not try to roll embedded Sample IDs backward through normal undo. If the user chooses the wrong preserved identity or later wants a different identity assignment, Wavecrate should provide an explicit regenerate/reassign Sample ID action for the affected file or conflict group. That follow-up action should be deliberate, visibly reported, logged, refresh affected caches/indexes, and report any files whose embedded ID could not be rewritten.

Automatic embedded-ID writing is an intentional metadata mutation, but it must not rewrite or reinterpret the audio payload. It should use the safest practical WAV metadata write path, preserve existing audio data and unknown chunks where practical, and use atomic or recovery-safe file replacement where needed.

Automatic embedding should expose visible embedded-ID state such as pending, writing, written, failed, unsafe, or duplicate-conflict where useful. If the file is read-only, locked, unsupported for safe metadata writing, permission-denied, changed during the write, or fails validation, Wavecrate should keep the database Sample ID, mark the embedded-ID state as pending, failed, or unsafe, and show/log a clear status instead of treating the whole file as unusable.

When Wavecrate later rewrites, extracts, duplicates, exports, or otherwise creates a normal audio file through its audio-write pipeline, it should write the embedded Sample ID metadata where safe as part of that same operation.

Copying or duplicating a file should create a unique Wavecrate file identity. The copied audio file should receive a new Wavecrate Sample ID and should not preserve the original file's embedded Sample ID as its own identity. If the raw copied bytes contain an embedded ID from the source file, Wavecrate should replace it with the new copied-file ID where safe, or mark embedded-ID state as pending/failed/unsafe while keeping the database ID authoritative.

Copied and duplicated files should inherit the original file's Wavecrate metadata by default, including assigned tags, label, prefix, BPM, Tuning/Scale, rating state, temporary color collection marks, generated display-name inputs, and other user-authored metadata where relevant. The inherited metadata is a one-time copy and belongs to the copied file's new Sample ID after the copy succeeds. The copied file's label is independent after creation and can be edited without changing the original file.

Copied and duplicated files should inherit generated display-name input fields, but their generated display-name uniqueness suffix should be recomputed in the destination folder after the copy succeeds. This allows copied files to keep the same naming intent while avoiding folder-local display-name collisions.

Copied and duplicated files should not inherit aging or listen history. They should start as newly created, unlistened files for aging/listen-history purposes.

Exact audio duplicates may reuse analysis, waveform, and similarity cache outputs from the original file when the copied file's audio fingerprint and relevant analysis settings match exactly. The copied file should still have its own Sample ID and cache records, but those records may reference or clone the existing cache payload instead of recomputing it.

Partial copies, waveform-selection copies, extracted regions, rendered edits, warped exports, downmixes, resampled writes, or any copy operation that changes the audio content should be treated as new audio for cache purposes. They should get fresh cache/analysis/similarity state and should not inherit the original file's analysis results unless a later fingerprint check proves the rendered audio is identical.

Wavecrate should expose exact duplicate warnings in the UI when multiple indexed files that currently exist on disk share the same audio-content fingerprint. Duplicate grouping should be global across all indexed sources, not limited to the current source or folder. Missing, unavailable, disconnected, or unresolved stale file records should not participate in duplicate warnings until the file is available on disk again. Duplicate warnings are advisory: they should help users find redundant files, but they should not block editing, tagging, moving, copying, trashing, or handoff workflows.

Files with identical bytes should count as duplicates even if their Wavecrate database metadata, tags, rating, display name, or folder location differ. Metadata differences must not hide exact duplicate status.

Files with the same decoded audio content should also count as duplicates even if their WAV metadata chunks, embedded Sample ID chunks, timestamps, or other non-audio bytes differ. Byte-identical duplicates are the simplest case, but decoded-audio identity is the stronger duplicate signal for ordinary WAV files.

Near-duplicates should not be shown as exact duplicate warnings. Files that are the same musical material but differ because of normalization, gain changes, fades, trimming, resampling, downmixing, encoding differences, noise, or other audio processing should be discoverable through similarity search rather than exact duplicate grouping.

Duplicate fingerprinting should run automatically during initial scan/indexing so exact duplicate warnings appear without requiring a separate first action. Wavecrate should also provide a manual duplicate analysis command to re-run duplicate fingerprinting and grouping for selected files, folders, sources, or the whole indexed library.

For WAV files, Wavecrate should use a custom RIFF chunk for the primary embedded ID. The chunk should be application-specific, versioned, and small. Unknown RIFF chunks should be preserved where practical when Wavecrate rewrites the file.

Wavecrate should not attempt to interpret or edit RF64, MBWF, or Broadcast Wave Format-specific metadata in the current target. If an ordinary WAV file contains unknown chunks, Wavecrate may preserve them where safe, but preservation is best-effort and must not take priority over audio-file safety.

For AIFF/AIF files, Wavecrate should use an application-specific metadata chunk where practical, following the same principle: small, versioned, and safe to ignore by other applications.

The embedded metadata should contain at minimum:

- Wavecrate Sample ID
- metadata schema version
- optional creation/update timestamp
- optional checksum or file fingerprint reference where useful

The database remains the authoritative fallback. If embedded metadata is missing, pending, stripped, duplicated, failed, unsafe, or conflicting, Wavecrate should reconcile using the database record, file path, file fingerprint, audio properties, and user-visible recovery behavior.

Wavecrate must validate embedded IDs by round-tripping files through common DAW workflows, especially Ableton Live and Bitwig. Validation should confirm that audio data, timing, channel layout, readability, and metadata recovery remain intact.

If embedded ID writing proves unsafe for WAV or later AIFF/AIF support, Wavecrate should disable embedding for that format and treat the target as needing adjustment rather than risking file damage.

### Audio Properties

Wavecrate should persist and display the audio properties needed for reliable editing and handoff:

- file format
- encoding subtype where known
- sample rate
- channel count and channel layout
- bit depth or floating-point format
- duration in samples and time
- byte size
- whether the file is editable, playback-only, unsupported, too long, missing, or failed

Audio operations should use sample/frame positions internally rather than only millisecond timestamps. UI labels may show time, bars/beats, or samples, but edits, selections, extraction, and history should retain enough precision to avoid cumulative timing drift.

### Disk Filename

The disk filename is the actual file name on disk.

Wavecrate should be able to show and manipulate real disk filenames, but the disk filename is not the Wavecrate label.

Manual disk filename edits should follow normal operating-system filename rules. Users may manually rename files with casing, spaces, and other OS-allowed characters.

Manual disk rename and external disk rename should not automatically change the label. Disk filename and label are separate concepts: the disk filename is the real OS filename, while label is independent Wavecrate metadata used for generated display names.

### Display Name / Database Name

The display name is the clean name shown by Wavecrate based on metadata such as tags, tag categories, label, prefix, BPM, tuning/scale, and uniqueness suffix.

Wavecrate should support view modes that show either the real disk filename or the generated database/display name. Applying a generated display name to the disk filename should be a deliberate file operation, not an invisible background behavior.

The generated display/database name should update automatically when its input metadata changes. If the user changes tags, label, prefix, BPM, tuning/scale, or another naming-template input, Wavecrate should recompute the display name and update the browser view without requiring a separate "regenerate name" command.

Generated display/database names should be derived from the current naming template, metadata inputs, and folder-local uniqueness rules. They should not be treated as independent durable user-authored metadata. Wavecrate should persist a generated-name cache in the database or index for sorting, filtering, rendering, and restart performance, but the cache must be rebuildable from the underlying metadata and current naming rules.

Changing the naming template should not trigger an immediate full-library generated-name rebuild. Wavecrate should mark affected generated-name cache projections stale and recompute them lazily for visible rows, active filters/sorts, explicit apply-to-disk operations, and other places where the generated name is actually needed. Opportunistic background refresh is acceptable only when it does not block browsing, playback, editing, scanning, or handoff work.

Automatic display-name regeneration must not rename the file on disk. Disk filename changes still require an explicit apply-to-disk rename command.

Generated display-name uniqueness suffixes may change automatically when metadata changes or when the collision set changes. They do not need to be stable per file because the stable identity is the Wavecrate Sample ID, not the generated display name. Display-name collision checks should be scoped to the file's containing folder. Disk filename numbering remains governed by the explicit rename/collision policy when the user applies a generated name to disk.

### Label

A label is a single-value, user-editable free-text metadata field that can act as a human-readable naming component inside Wavecrate. It is not the disk filename.

Labels should also be normalized to lowercase. User input casing should not be preserved in labels because labels participate in generated display names and generated filenames.

Labels should be single-token values with no spaces or multi-word entries. If the user types spaces or other whitespace, Wavecrate should normalize that whitespace to `-`. Label characters should be limited to lowercase letters, numbers, hyphen, and underscore.

Newly scanned or imported files should start with an empty label unless a specific creation workflow assigns or inherits one. Manual disk renames, generated apply-to-disk renames, and external renames should not automatically update the label. Editing the label should not rename the file on disk. The label can be used in generated display names.

Audio files that appear through external additions detected by file watching or rescan should use the same default initialization as newly scanned files: empty label, unrated rating state, no temporary color collection marks, no inherited metadata, and fresh aging/listen-history state unless a later explicit Wavecrate command changes them.

Externally added supported audio files should enter the same indexing and embedded Sample ID workflow as normal scanned files. Wavecrate should create or reconcile a database Sample ID, embed that ID automatically where safe, and show pending/failed/unsafe/duplicate-conflict embedded-ID state using the same rules as initial scan indexing.

### Prefix

A prefix is a single-value structured metadata field that can identify an artist, creator, project, pack, session, or other user-selected namespace.

The prefix can be used in generated display names and can help users distinguish their own sounds from other material.

### Tag Categories

Tags should be organized into predefined, hard-locked categories.

The categories themselves are fixed by Wavecrate because they are used for filtering, display-name generation, filename generation, naming order, and consistent sample-library structure.

Some structured metadata, such as BPM, participates in filtering, naming, and display order but is not technically a tag category. BPM should be stored and edited as scalar numeric metadata.

Users should be able to create, edit, rename, and remove tags inside user-extensible categories, but they should not be able to create new top-level tag categories unless the product target is explicitly expanded later.

User-created tags inside user-extensible categories should be global by default. If a user creates a custom Sound Type tag such as `zap` while working in one source, that tag should become available as a suggestion, filter, structured-search token, display-name component, and generated-filename component for all sources.

User-extensible value dictionaries should be global regardless of whether the category is single-value per file or multi-value per file. Prefix, Sound Type, Character, and Tuning/Scale values should all be shared across sources through the global `.wavecrate` dictionary.

The global `.wavecrate` state should own tag category definitions and the user-extensible tag dictionary. Each source database should store which global tags are assigned to files in that source, plus source-local file metadata. Source databases should not silently fork tag definitions into incompatible per-source dictionaries.

Creating a new value in the global dictionary should be an undoable dictionary transaction. If the user undoes creation after assigning that value to files, the file assignments should remain intact as selection-local/local-only values that are no longer normal global dictionary suggestions.

When the user types or selects a brand-new value in a picker for a user-extensible category, Wavecrate should immediately add that value to the global dictionary and assign it as part of the same user action. The transaction should be undoable. Undo should remove the assignment according to normal assignment undo and, when appropriate, also undo the dictionary creation while preserving any remaining file assignments as local-only values.

All user-extensible dictionary values should be normalized to lowercase, including Prefix, Sound Type, Character, and Tuning/Scale values. Labels should also be normalized to lowercase even though they are free-text metadata rather than dictionary values. Values such as `Kick`, `kick`, and `KICK` should all be stored, displayed, suggested, filtered, and used in generated names as `kick`. User input casing should not create separate dictionary entries or custom display casing.

User-extensible dictionary values and labels should be single-token values. Spaces and multi-word values should not be stored. If users type spaces or other whitespace, Wavecrate should normalize that whitespace to `-`. Stored values should be limited to lowercase letters, numbers, hyphen, and underscore, for example `drum-loop`, `metal-floor`, or `19edo`.

Punctuation, symbols, accented characters, non-ASCII characters, and other unsupported characters outside lowercase letters, numbers, hyphen, and underscore should not be stored in dictionary values or labels. The UI should normalize these unsupported characters to `-` where practical and show the normalized result before committing.

Normalization should collapse repeated separators and trim separators at the start or end of the value. For example, `metal---floor`, `metal   floor`, and `--metal-floor--` should normalize to `metal-floor` where practical.

If normalization produces an empty value, such as when the user enters only spaces, punctuation, symbols, or unsupported characters, Wavecrate should reject the value and show a clear inline message rather than creating an empty dictionary value or label.

Editing the global user-extensible tag dictionary should not silently mutate existing files or source databases. Removing a tag from the optional/suggested tag dictionary should stop offering it as a normal suggestion for new tagging, but it should not remove that tag from files that already have it, rewrite generated names on disk, delete audio files, or otherwise cause collateral changes.

Deleting a value from the global dictionary should be allowed even while files still use that value. Existing file assignments should remain intact and should continue to display, filter, and participate in generated display names where assigned. When one or more selected files use a value that is no longer in the global dictionary, Wavecrate should show that value in the relevant picker for the current selection so the user can keep, remove, or apply it deliberately. Showing it for the selected files should not automatically re-add it to the global dictionary.

Deleting a value from the global dictionary should be an undoable transaction. Undoing that transaction should restore the value to the global dictionary and make it available again as a normal suggestion, filter value, and naming value without changing existing file assignments.

Renaming a value in the global dictionary should be an undoable dictionary-only change. It should affect future suggestions and future assignments of that value, but it should not update, migrate, rename, rewrite, or otherwise mutate existing file assignments that used the old value. Undoing the dictionary rename should restore the previous dictionary value for future suggestions without changing existing file assignments. If users want to retag existing files from the old value to the new value, that should be a separate explicit retag/migration command with preview, confirmation, undo, and affected-file count.

Selection-local values that are assigned to files but no longer exist in the global dictionary should have a distinct visual treatment, such as a different chip color, outline, icon, or muted/alternate style. The distinction should make it clear that the value is still assigned to the selected file or files but is not currently a normal global dictionary suggestion.

Wavecrate should provide an explicit action to add a selection-local value back into the global dictionary. This command should be deliberate, undoable, and scoped to the value/category the user chooses. It should not retag files; it only makes that value available again as a normal global suggestion and filter/naming value.

Existing files should remain intact unless the user explicitly deletes files, explicitly retags files, or explicitly runs a generated-name/auto-rename workflow. If a dictionary edit would affect existing assignments, Wavecrate should treat that as a separate explicit migration or retag command with preview, confirmation, undo, and a clear affected-file count.

Wavecrate should not automatically assign tags by parsing filenames, folder names, source paths, or pack names during scan. Tags should be assigned through manual tag entry, explicit batch tag commands, extraction inheritance, or other deliberate user actions.

Initial tag categories should include:

- **Playback Type**: a fixed single-value category with exactly two allowed values: `one-shot` and `loop`. Users cannot add custom tags to this category, and no other playback-type values such as phrase or drum-loop are part of the current target.
- **Sound Type**: a single-value, user-extensible category for sound identity, with values such as kick, snare, clap, hat, bass, stab, texture, vocal, percussion, ambience, effect, loop, drum loop, synth loop, and similar identities. A file should have at most one Sound Type value at a time.
- **Character**: a multi-value, user-extensible descriptive category for tags such as warm, harsh, clean, noisy, distorted, punchy, soft, metallic, dark, bright, wide, dry, wet, raw, polished, and similar qualities. A file may have multiple Character tags at the same time.
- **BPM**: scalar numeric tempo metadata, not a tag category. BPM is used for filtering, naming, tempo-grid display, BPM-aware auditioning, and loop workflows.
- **Prefix**: a single-value structured metadata category for artist name, creator name, project name, pack name, session name, or other namespace-like identifiers. A file should have at most one Prefix value at a time.
- **Tuning/Scale**: a single-value, user-extensible name-based category for musical tuning, scale, mode, pitch system, or temperament where useful, including microtonal scale names. This category is manually assigned by the user rather than automatically detected as conventional key/pitch metadata. A file should have at most one Tuning/Scale value at a time.

The display-name and generated-filename systems should use these categories in a predictable order. The naming template may be customized through a config file, but the category structure should remain stable enough that generated names are consistent across the library.

When extracting a new file, Wavecrate should set Playback Type from the active playback state rather than blindly inheriting it from the source file. If loop playback is active for the extracted selection, the extracted file should be tagged as `loop`. If loop playback is not active, the extracted file should be tagged as `one-shot`.

This decision should be based on the explicit loop playback state only. BPM metadata, grid visibility, grid alignment, or region length may help the user choose and audition a region, but they should not silently change Playback Type during extraction.

### Rating State

Rating state is based on the keep/trash rating system. It is separate from tags.

Rating state should be fast to apply, visually obvious, persistent, and useful for sorting, filtering, cleanup, and library hygiene.

Extracted files should not inherit the source file's rating state. A newly extracted sample should start with a single keep rating by default, because the act of extracting it indicates the user intentionally selected it as useful material. The source file's rating should remain unchanged.

### Aging / Listen History

Aging and listen history track when a sample file was last auditioned, whether it has never been listened to, and how often it is used or auditioned.

This should help users find new files, neglected files, recently used files, and frequently used files.

### Temporary Color Collections

Temporary color collections are lightweight shelves inspired by Ableton-style color collections.

They are not the primary file organization system and do not replace folders. They should help users temporarily collect files for a project, task, comparison pass, cleanup pass, or sound selection workflow.

Temporary color collections are global virtual database-backed marks, not physical folders. The target is a small fixed set of color-coded slots, such as 10 slots, that users can apply and clear quickly across Wavecrate.

Temporary color collections should persist across app restarts until the user explicitly clears the collection or removes individual marks. "Temporary" describes their workflow purpose, not their storage lifetime. They are intended for short-lived triage and staging, but Wavecrate should not drop them automatically at shutdown.

Extracted files should not inherit temporary color collection marks from the source file. If the user wants an extracted file in a collection, they should assign it directly after extraction or through a batch collection command.

Temporary color collections are allowed to be hidden managed metadata because they are intentionally temporary workflow marks. They are not durable library containers, not a replacement for making a real folder of files, and not a Wavecrate-specific pack/export model. When a user wants a collection that should exist outside the current temporary workflow, Wavecrate should support moving or copying those marked files into an ordinary folder.

Because temporary color collections are global, they may reference files from different source folders. The collection UI should show missing or unavailable files with a warning rather than silently dropping the marks. A source does not need to be actively selected for a marked file to remain in a color collection, but if the file path no longer exists, Wavecrate should show that the marked file is missing and allow the user to remove the stale mark.

Missing marked files should have a clear visual indicator in collection views, browser rows, and any map/list projection where they appear. The indicator should be distinct from unsupported, playback-only, and analysis-failed states.

## File Ownership and Source of Truth

Wavecrate should remain grounded in the real filesystem.

The filesystem is the source of truth for file existence, folder structure, and physical file location. Wavecrate indexes existing folders and operates on real files in those folders.

Wavecrate should not require a central managed library folder. Users should be able to point Wavecrate at existing folders and work with the actual files there.

Wavecrate should not create hidden managed durable collections, opaque pack files, or cloud-backed libraries for normal organization. Durable organization means real files in real folders. Temporary color collections are the deliberate exception: they are short-lived virtual marks stored as metadata for fast collection and triage.

Wavecrate may create, move, rename, duplicate, export, collect, copy, or trash files and folders, but those operations should physically happen on disk. If a file or folder is moved from one folder to another in Wavecrate, it should actually move on disk.

Folder names created or renamed inside Wavecrate should follow normal operating-system filesystem rules, not Wavecrate metadata-token normalization rules. Real folder names may preserve spaces, casing, and other characters allowed by the OS, subject to ordinary filesystem validation and collision handling.

The database is the source of truth for Wavecrate-specific metadata, including tags, ratings, labels, prefixes, naming-template inputs, analysis results, similarity descriptors, waveform cache references, aging/listen history, and session undo/history state where appropriate. Generated display/database names are derived from that metadata and may be cached, but they are not independent durable metadata.

Wavecrate should detect and handle:

- missing files
- moved files
- renamed files
- changed files
- duplicate files
- stale metadata
- stale analysis cache
- stale waveform cache
- database records pointing to unavailable files
- files without embedded Wavecrate Sample IDs
- files with embedded IDs that conflict with database state

When filesystem and database state diverge, Wavecrate should provide clear reconciliation behavior instead of silently losing user metadata or pretending missing files still exist.

Wavecrate does not use a project-export model for normal audio editing.

When the user saves an edited audio file, Wavecrate saves the actual audio file currently being edited.

When the user extracts a region, Wavecrate creates a new audio file.

When the user copies, drags, or hands off a whole file, Wavecrate hands off the real audio file.

When the user copies, drags, or hands off a selected region, Wavecrate creates a normal audio file for that region and hands off that file.

Wavecrate should not create opaque project files, hidden managed media, durable managed collection files, or proprietary sample packs unless needed for temporary recovery, undo, waveform cache, analysis cache, database metadata, or temporary color collection metadata.

## Destructive Editing, Duplication, and Session Undo Policy

Wavecrate is intentionally a fast destructive sample editor.

When the user performs an edit such as mute, cut, delete, fade, normalize, gain change, trim, or silence removal, the edit should modify the current audio file in place unless the command is explicitly an extraction, export, duplicate, or copy operation.

This is a deliberate product direction. Wavecrate should not automatically create endless duplicate versions for every edit. Users who want to preserve an original file should duplicate it before editing.

Wavecrate should therefore provide a clear duplicate-file command that makes it easy to create a backup or alternate version before destructive editing.

Destructive editing should have two user-facing safety modes.

### Default Safety Mode

YOLO mode should be off by default for all users. In default non-YOLO mode, destructive edits should always warn the user clearly before modifying the file in place. These warnings should not be dismissed permanently per command type or hidden after repeated use while YOLO mode remains off.

The warning should name the destructive command and explain that the edit will modify the audio file on disk and that the change can be undone during the current session. For example, it may say that Normalize, Trim, or Apply Fade will modify the file on disk. The primary choices should be command-specific confirm or cancel, such as "Apply Normalize" rather than a generic "Apply". Every destructive audio edit warning may show a deliberate secondary action for users who understand the behavior; this should not require another advanced preference. The secondary action should also include the command name, such as "Enable YOLO and Apply Normalize" rather than generic "Enable YOLO and Apply". Choosing that secondary action should enable YOLO mode and immediately proceed with the current destructive command without a second confirmation. It should not be the default button or easiest accidental path.

For batch destructive audio edits while YOLO mode is off, Wavecrate should show one warning for the whole batch rather than one warning per file. The warning should include a concise affected-file count so the user understands the scope before confirming, but it should not list every selected file. Detailed per-file information belongs in progress and result summaries. If the batch warning offers a command-specific YOLO action, it should follow the same rule as single-file destructive warnings: enable YOLO mode and immediately proceed with the current batch without a second confirmation.

Development-only shortcuts may exist outside the product target, but product builds should remain warning-protected by default unless YOLO mode is explicitly enabled.

### Advanced Destructive Workflow / YOLO Mode

Advanced users should be able to enable a persistent destructive workflow mode, informally called YOLO mode.

When this mode is enabled, Wavecrate should stop showing repetitive destructive-edit warnings and should allow fast in-place editing. Users in this mode are expected to duplicate files themselves when they want backups.

YOLO mode applies only to destructive audio editing warnings. It should not suppress file-operation safety rules or prompts for trashing, moving, copying, deleting/removing folders with hidden contents, changing trash configuration, source management, or other non-audio-edit workflows.

YOLO mode should not suppress progress or result visibility for batch destructive audio edits. Even when preflight warnings are skipped, batch operations should still show concise progress where useful and report succeeded, skipped, failed, and rolled-back counts when the operation completes.

YOLO mode should be explicit and persistent. It should not be enabled accidentally, but it does not need a persistent main-interface indicator because it is a set-and-forget advanced setting. The current state should remain visible in settings and in destructive-warning flows where it matters.

Changing YOLO mode from Settings should be asymmetric. Enabling YOLO mode removes destructive warnings and should require confirmation before the setting is committed. Disabling YOLO mode restores the default safety warnings and may apply immediately without confirmation.

Changing YOLO mode from Settings should affect future destructive actions only. If a destructive warning dialog is already open, that dialog should keep its current confirm, cancel, or secondary action choices, and the settings change should not automatically apply or dismiss that pending destructive command.

### Session-Local Undo and Redo

Even though edits are destructive, Wavecrate should have a deeply integrated undo/redo system.

Undo/redo is session-local. It only needs to work while the application is running. Undo history does not need to persist across application restarts.

Every meaningful user action that changes audio, files, metadata, organization state, play/edit selection state, edit-preview state, or workflow marks should be modeled as an undoable transaction unless the command is explicitly documented as non-undoable. Ordinary browsing, row focus, source/folder navigation, scroll, zoom, cursor movement, and search/filter changes should not enter the global undo stack by default.

Undo/redo should cover:

- destructive audio edits
- play selection changes
- edit selection changes
- explicit workflow-context changes where undo is clearly useful, such as committed edit-preview state, active edit mode, or view state tied to an undoable edit workflow
- marker changes
- region changes
- extraction actions
- duplicate actions
- rename actions
- generated-name application actions
- tag changes
- rating changes
- temporary color collection changes
- metadata changes
- audition and workflow flag changes where they affect the working context, such as loop mode, normalized-audition mode, target-BPM lock, warp audition mode, grid visibility, and edit-preview toggles
- source add/remove and source reference changes
- move/copy/export/trash operations
- folder operations where practical

Transport play/stop/restart, momentary playback position, ordinary seek state, browser focus, row selection, source/folder navigation, browser position, waveform scroll/zoom position, cursor movement, ordinary view switching, Open/Reveal in Finder/Explorer/File Manager actions, and Copy Path actions should not be undoable by default. Opening or revealing a file, folder, or source in the platform file manager and copying a path string to the clipboard are external non-mutating utility actions, not Wavecrate state. Undo should restore metadata, file, rating, collection, edit, workflow-flag, and play/edit selection context, but it should not behave like a transport, browsing history, external-app launch, or clipboard utility history.

Undo/redo should be transaction-based. A user action should either complete as a coherent operation or fail in a recoverable way. Partial failures should be logged and reported clearly.

Undo should revert the latest undoable action taken by the user. Wavecrate should use a normal linear undo stack: `Ctrl+Z` undoes the most recent undoable transaction, then the one before it, and so on. It should not offer arbitrary out-of-order transaction undo that would overwrite newer work on the same file or metadata record.

Redo should reapply the latest undone action when the undo stack has not been invalidated by a new action. If the user performs a new undoable action after undoing, Wavecrate should clear redo entries according to normal linear undo/redo behavior.

Undo and redo should be global Wavecrate commands. `Ctrl+Z`, `Ctrl+Y`, and `Ctrl+Shift+Z` should route to the global undo/redo stack even when focus is inside a tag editor, rename field, label field, search field, numeric field, or other text/value editor. Text and value edits should therefore commit meaningful changes into the global transaction history rather than maintaining a separate local text-field undo stack.

Text and value editors that change durable user state should create undo transactions at meaningful commit points rather than on every keystroke. Typical commit points include Enter, blur/focus loss, selecting an autocomplete/tag suggestion, clicking Apply, confirming a rename, or completing a numeric edit. Search/filter fields are the deliberate exception: committed query changes should update browser query history rather than the global undo stack. Cancel/Escape should discard the uncommitted field edit where practical rather than creating an undo transaction.

Settings and preference changes should not enter the global undo stack by default. Normal settings changes should commit immediately because changing a setting is treated as intentional user action. Risky settings such as enabling YOLO mode, changing trash folder location, cache cleanup policy, audio backend/device, or write format should require confirmation before the setting change is committed.

Restoring a previous audio backend or device setting through a settings-local restore action should restore the previous backend/device only if it is still available. If the previous device has disappeared, is disconnected, or cannot be opened, Wavecrate should keep or choose a safe available fallback and show a clear warning.

Changing the configured trash folder should not invalidate later trash-move undo records. Each trash action should store the trash destination it actually used in its own undo transaction, so settings-local restore of the current trash-folder preference does not affect the recovery path for already committed trash moves.

Source removal should be the explicit reversal path for an added source rather than relying on `Ctrl+Z` to undo source addition. Removing a source should remove Wavecrate's configured reference, stop file watching, stop or cancel in-progress scan/indexing/background jobs for that source, and ignore stale completions from those jobs. It must not delete the real folder, source database, audio files, reusable analysis/similarity data, or safe content-addressed caches. It should release volatile in-memory state and asynchronously retire readiness-managed jobs and unshared managed cache payloads. Source removal may happen immediately without confirmation and without waiting for cancellation or cleanup. Re-adding or relinking a source should restore the retained source descriptor and database path, reconcile retained artifact ownership/version state, then restart scanning, file watching, and readiness work after any old runtime epoch has drained.

Play/edit selection and workflow-flag undo should be useful without becoming noisy. Wavecrate should coalesce rapid repeated selection adjustments and repeated flag toggles into meaningful undo steps where practical, so undo can return the user to a previous editing context without filling the history with every tiny pointer or key movement.

When undo restores an edit, metadata, file, rating, collection, or workflow transaction, it may also restore the supporting browser context that belonged to that transaction where practical, such as active source, folder selection, browser view mode, browser selection, focused row, visible position, or waveform view. That context restoration is attached to the real undoable transaction; ordinary navigation should use local browser history instead of the global undo stack. Search/filter query changes should be restored through browser/query history controls rather than global undo. If a previously selected file is missing, moved, hidden by unavailable source state, or no longer part of the restored result set, Wavecrate should restore the closest sensible focus and report the mismatch only when useful.

The undo history should be deep enough to be useful while remaining bounded to avoid excessive memory or disk usage. A reasonable initial target is 50 meaningful user transactions, with the architecture allowing a configurable range such as 50 to 100 where disk space and recovery-file size make that practical.

The undo system should enforce a disk budget for recovery files. When the budget would be exceeded, Wavecrate should expire the oldest undo entries that are safe to drop, clean their recovery files in the background, and make the remaining undo depth clear in diagnostics where useful.

Undo transactions should have user-readable names such as "Normalize selection", "Move 12 files", "Apply fade", "Rename folder", or "Assign red collection". Undo and redo status should make clear what action will be undone or redone.

Actions that cannot reasonably be undone should be rare. They should require explicit confirmation, explain why undo is unavailable, and be logged.

### Temporary Recovery Files

For destructive audio edits and file operations that need recovery support, Wavecrate should use temporary recovery files.

Wavecrate should create a standard application temp/recovery folder appropriate for the operating system. On Windows, this should live in a conventional user/application data or temp location, not inside arbitrary sample-library folders.

Temporary recovery files should support undo for the current session only. Files from previous sessions should be considered stale and eligible for cleanup.

Wavecrate should not slow down shutdown by doing heavy temp cleanup on exit. Instead, a background worker should clean stale recovery files while the application is running, such as on startup or during idle/background maintenance. The cleanup worker should remove old files that do not belong to the current session and are no longer valid for undo.

Recovery-file creation, cleanup, and failures should be logged.

## Extraction Policy

Extraction is not the same as destructive editing.

When a user selects a region of an audio file and extracts it, Wavecrate should create a new sample file containing that region. The original file should remain unchanged unless the user separately performs a destructive edit on it.

Extraction output should be failure-atomic and collision-safe. Wavecrate should write and finalize an exclusively owned staging file in the destination folder, then publish it without replacing any path that appeared concurrently. Failed or cancelled work should remove only its own staging artifact and must never expose a partial WAV as a completed extraction.

If audition-time processing is active, such as target-BPM warping, extraction should create what the user hears. If a user auditions a warped loop and extracts it, the created sample file should bake the auditioned warp result into the new file.

Extraction should support:

- extracting the current play selection
- extracting the current edit selection where appropriate
- drag-out extraction where practical
- enter/confirm extraction where practical
- naming or labeling extracted files
- applying tags and metadata to extracted files
- assigning a new Wavecrate Sample ID to extracted files
- writing embedded Sample ID metadata where practical
- avoiding filename collisions through predictable numbering

A selected waveform range should expose an extraction drag handle, such as a small handle in the corner of the selected range.

Dragging this handle inside Wavecrate into the sample browser list should perform the same logical operation as pressing the extract shortcut: it should create a new audio file from the selected range and add/index that file in the current browser context.

The keyboard shortcut for extraction should be `E` by default.

Extraction through keyboard, internal drag, clipboard, or external drag should all use the same underlying extraction pipeline so naming, collision handling, metadata assignment, embedded Sample ID writing, waveform cache creation, logging, and undo behavior remain consistent.

Extracted audio files should be physically written to the currently active/open folder in Wavecrate unless the user explicitly chooses a different destination through a dedicated command.

When a user drags an extracted region into a DAW, Wavecrate should first create the extracted file in the active Wavecrate folder, then hand that file path to the DAW.

When a user drags an extracted region into Explorer, the new file should be created in the Explorer drop target directory.

In all cases, the resulting file should be an ordinary WAV file in the first supported format target, and later an ordinary AIFF/AIF file where that format is explicitly supported. It should not require Wavecrate to be open for the DAW or Explorer to use it afterward.

### Region Handoff and Temporary File Semantics

Wavecrate should distinguish between durable extraction and transient handoff staging.

A durable extraction creates a new indexed sample file. It should:

- write the extracted audio into the current active/open Wavecrate folder unless a command specifies another destination
- assign a new Wavecrate Sample ID
- register the file in the source database
- write embedded Sample ID metadata where safe
- copy or derive relevant metadata according to extraction policy
- inherit all assigned tags by default from the inherited tag categories, including Sound Type, all Character tags, Prefix, and Tuning/Scale tags where present
- derive Playback Type at extraction time from explicit loop playback state only: if loop playback is active for the extracted selection, mark the new file as `loop`; otherwise mark it as `one-shot`
- inherit Prefix metadata exactly by default rather than adding an extraction-specific prefix
- inherit the source file's label by default when one exists, as a one-time metadata copy
- inherit the source file's BPM metadata by default when the source has BPM metadata
- not inherit temporary color collection marks from the source file
- start with a single keep rating by default because extraction is an intentional selection action
- start as newly created and unlistened for aging/listen-history purposes
- create waveform, analysis, and similarity work as needed
- participate in session undo/redo
- appear in the sample browser after successful registration

An extracted file should be treated as a new independent sample after creation. Wavecrate should not store a durable source-to-extracted-file relationship, source Sample ID link, original range record, or extracted-region history in the current target. Inherited metadata is copied once at creation time and then belongs to the extracted file independently. Wavecrate should not encode the original source filename, source folder, source name, or source identity into the extracted file's label, Prefix, generated display name, generated disk filename, tags, or other user-facing naming metadata unless the user explicitly adds that metadata themselves.

Keyboard extraction, internal extraction-drag into the browser, explicit "extract to file", and extraction from a saved region should use durable extraction by default.

Transient handoff staging exists only to satisfy operating-system clipboard or drag-and-drop APIs that require a file path before the receiving application accepts the drop or paste. Transient staged files should be ordinary WAV files in the first supported format target, and later ordinary AIFF/AIF files where that format is explicitly supported. They should live in a Wavecrate-managed handoff temp folder under the global `.wavecrate` root.

Transient staged files should not be deleted immediately at the moment a paste/drop appears to succeed, because the receiving DAW or operating-system shell may still need to read the file path. Wavecrate should keep them for a short internally defined grace period and then clean them up automatically when they are no longer needed. The grace period does not need to be user-configurable in the current target. If the user quits Wavecrate after copying or dragging a staged handoff file, the staged file should remain available through the normal grace period so clipboard or drop consumers can still read it after the app exits. Cleanup should remove stale staged files from previous sessions on the next launch or during normal cache/staging maintenance, and cleanup failures should be logged and visible in diagnostics.

Transient staged handoff files should use predictable, user-readable filenames where practical because DAWs and file managers may display the imported filename after handoff. Names should be based on the current generated/display name or selected-region naming context, sanitized through the normal filename rules, and made unique through the shared collision-numbering policy. They should not use opaque random temp names unless a readable name cannot be produced safely.

Transient staged handoff files should contain the rendered audio and only the minimal required or safely useful WAV metadata. They should not inherit durable Wavecrate metadata such as tags, rating, label, Prefix, BPM, Tuning/Scale, temporary color collections, or embedded Wavecrate Sample ID by default. Durable extraction owns metadata inheritance; transient handoff staging should avoid creating metadata side effects.

Transient staged handoff files should not be indexed as library samples. They should live outside indexed source folders under the global `.wavecrate` handoff staging area. If a path edge case or user configuration would otherwise place staging near an indexed source, Wavecrate should still treat the staged file as temporary handoff data, not as a new source sample. Users who want an indexed file should use durable extraction, export, or copy/import workflows.

Wavecrate may reuse an existing transient staged handoff file for repeated copy or drag operations when it is still valid. Reuse is allowed only when the source file identity and fingerprint, selection range, audition/render settings, write format, cache/staging version, and resulting audio payload still match. If any relevant input changes, Wavecrate should render a fresh staged file.

Transient handoff staging should be allowed only when the user intent is clearly temporary and no durable Wavecrate destination is implied. Waveform-selection clipboard copy is not transient in the current target: it should first create a durable extracted file in the active Wavecrate folder, then hand that file path to the operating system clipboard.

Region handoff behavior should be:

- Copying selected browser files places the existing real files on the clipboard.
- Dragging selected browser files hands off the existing real files.
- Copying a waveform selection creates a durable extracted file in the active Wavecrate folder, then places that ordinary audio file on the clipboard.
- Dragging a waveform selection into a DAW creates a durable extracted file in the active Wavecrate folder, then hands that file path to the DAW.
- Dragging a waveform selection into Explorer creates the new file directly in the Explorer drop target directory.
- Dragging a waveform selection inside Wavecrate creates a durable extracted file through the shared extraction pipeline.
- Explicit export commands write to the user-chosen destination and do not silently register the result unless the destination is inside an indexed source and registration succeeds.

When an explicit export writes a new audio file inside an indexed source folder, Wavecrate should register and index the exported file if registration succeeds. When an export writes outside indexed sources, the result should remain an external file and should not become Wavecrate library state unless the user later adds/imports that location as a source.

If an explicit export successfully writes a file but registration or indexing fails, Wavecrate should leave the exported file on disk and show a clear recoverable warning. The warning should explain that the file was created but is not yet indexed, and should offer retry registration, rescan/reconcile, reveal in the platform file manager, or diagnostics where practical.

Explicit exports outside indexed sources should not be undoable in Wavecrate because they create user-directed external files outside Wavecrate-managed library state. Explicit exports that are successfully registered into an indexed source should be undoable as library file-creation transactions, using the same recovery and file-removal safety expectations as other Wavecrate-created source files.

Undoing a registered export should remove the exported file through the configured Wavecrate trash workflow where possible, not hard-delete it. If no trash folder is configured, Wavecrate should demand trash-folder configuration just like normal trashing and then continue the undo if the user configures one. If trash configuration is cancelled or the trash move fails, Wavecrate should leave the file in place, mark the undo as failed or partially applied, and show a clear recovery message.

Redoing a registered export after undo should restore the exported file from the trash folder if that file is still available and matches the undo record. Wavecrate should not re-render the export as redo fallback in the current target. If the trashed file is unavailable or no longer matches the undo record, redo should fail with a clear recovery message.

Whole-file browser copy and drag handoff should use the existing real audio file path. Wavecrate should not create a staged copy for ordinary whole-file handoff. Staging is for waveform selections and selected-region handoff where the handed-off audio differs from the real source file. A separate explicit processed-copy handoff workflow is a non-goal for the current target.

If a whole-file handoff is requested while the file has unapplied preview edits, Wavecrate should still hand off the existing real file only. The UI should clearly indicate that unapplied preview edits are not included in ordinary whole-file handoff. Users who want edited audio in the handoff should apply the edit, export, or extract a region.

If a staged handoff file cannot be created, copied, or dropped, Wavecrate should keep the source audio unchanged and report the failure.

If durable extraction writes the audio file but database registration fails, Wavecrate should clean up the orphan file where safe or leave a clearly reported recoverable file with diagnostics. The UI should not pretend the extraction is indexed until both file write and registration are consistent.

## Trash and Cleanup Policy

Wavecrate should use a configured trash folder for cleanup.

Trashing a file means moving it from its current source folder into the configured trash folder. It does not mean permanent deletion.

Trashing a folder inside a source means moving that real folder and its contents to the configured trash folder. It should preserve the folder name where possible and use the shared destination-folder collision-numbering policy if a folder with that name already exists in the trash folder. It should be undoable through the same transaction/recovery model as file trashing and does not need a confirmation prompt. If no trash folder is configured, Wavecrate should demand trash-folder configuration before moving the folder.

Wavecrate should not permanently delete files automatically. Permanent deletion is outside the normal cleanup workflow.

The trash folder must be configured before Wavecrate can move files there. Wavecrate should not silently create a default trash folder.

If no trash folder is configured and the user triggers a trash action, Wavecrate should demand configuration before moving files. The prompt should explain that trashing in Wavecrate means moving files into a user-chosen folder, not permanent deletion and not the operating-system recycle bin unless the user explicitly chooses such a destination where supported.

Until a trash folder is configured, Wavecrate may still allow trash ratings, but it must not move files. Rejected files should remain in place and show that trash movement is pending configuration.

Trash workflow should support:

- explicit trash actions
- explicit folder trash actions
- automatic trash movement when a file reaches the rejected threshold
- undo for recent trash moves during the current session
- clear logging of trash moves
- clear UI indication that a file was moved out of the active library
- predictable behavior if a destination file already exists in the trash folder

No special long-term untrash system is required beyond current-session undo and the fact that files remain physically present in the trash folder. Users can move files back manually if needed.

Current-session undo for a trash move should restore the file or folder from the configured trash folder back to its original source folder/path where practical and restore the previous Wavecrate state for affected file identities, including rating, tags, label, prefix, BPM, Tuning/Scale, temporary color collection marks, display-name state, and other relevant database metadata. If the original restore path now collides with a file or folder that was created after the trash action, Wavecrate should use the shared collision-numbering policy to choose a safe restore path and report the changed destination clearly. This should be true even if the trash folder is also indexed as a Wavecrate source. Trashing is just another undoable action, so the undo transaction should reconcile both source views and metadata ownership so restored items are removed from the trash-source view and restored to the original source view when the undo succeeds.

The trash folder is an ordinary folder on disk. Wavecrate should not show files moved there in the original source after the move succeeds. If the user explicitly adds the trash folder as a Wavecrate source, Wavecrate should index and show those files as ordinary files belonging to that trash source.

## Keep/Trash Rating System

Wavecrate should use a fast keep/trash rating scale rather than a generic star-rating system.

All files start unrated.

The user should be able to quickly apply a keep mark or a trash mark while auditioning. These marks should be visually clear in the sample browser.

The rating state should move in the direction of the latest rating action. It should not use simple cancellation back to unrated once a file has been rated.

The target rating behavior is:

- Unrated plus keep becomes keep level 1.
- Keep level 1 plus keep becomes keep level 2.
- Keep level 2 plus keep becomes keep level 3.
- Keep level 3 plus keep becomes accepted/favorite/locked.
- Unrated plus trash becomes trash level 1.
- Trash level 1 plus trash becomes trash level 2.
- Trash level 2 plus trash becomes trash level 3.
- Trash level 3 plus trash becomes rejected and eligible for trash-folder movement.
- Applying trash to a keep-rated file should move it to trash level 1, not back to unrated.
- Applying keep to a trash-rated file should move it to keep level 1, not back to unrated.

Accepted/favorite files should be visually distinct and locked from further rating changes. The lock applies only to the rating state. To change the rating of an accepted file, the user must manually unlock it first.

Rejected files should be moved to the configured trash folder automatically when cleanup policy allows it.

Rejected files do not have a rating lock. Once a rejected file is successfully moved to the configured trash folder, it should disappear from normal source browsing because it no longer physically exists in that source folder. If the user explicitly adds the trash folder as a Wavecrate source, those files may appear there like ordinary files in that source.

The UI should represent ratings visually rather than primarily with text labels. Each sample-list row should include a compact rating section on the same line as the sample item.

Partial ratings should be shown as small rating squares:

- keep level 1 shows one green square
- keep level 2 shows two green squares
- keep level 3 shows three green squares
- trash level 1 shows one red square
- trash level 2 shows two red squares
- trash level 3 shows three red squares

The fourth keep step creates the accepted/favorite/locked state. Accepted files should show four green rating squares and should turn the whole sample-list item into a special accepted visual state so accepted files are obvious while scanning.

The fourth trash step creates the rejected state and should trigger trash-folder movement when configured. There should not normally be a visible fourth red square in the browser because the file should become rejected and move to the configured trash folder. If trash movement is pending because no trash folder is configured or movement failed, the row should show a rejected/pending-trash warning state rather than a normal fourth trash mark.

Text labels may still appear in tooltips, accessibility names, filter menus, logs, diagnostics, and status messages, but the dense browser-row representation should be rating squares and row state, not words like "Keep 2" or "Trash 3" printed beside every sample.

Rating controls should be keyboard-friendly and should not interrupt auditioning.

After a rating action is applied from the browser, Wavecrate should automatically advance selection to the next sample in the active browser traversal order so the user can rate many files quickly. The next item should immediately audition when immediate audition is enabled.

Rating advance must respect the current browser state. If filters, folder selection, search, similarity sorting, temporary collection filtering, or random mode are active, the "next" item should come from that active result/order rather than from the unfiltered folder order.

Random mode should still be deterministic enough for undo/redo and user trust. Applying a rating in random mode should advance to the next random candidate from the active filtered set without repeating recently visited files where practical.

Rating filters and sorting should be explicit:

- Unrated, keep levels, accepted/favorite/locked, trash levels, rejected, and trashed states should be separately filterable.
- Default browser sorting should not hide rejected files that are still pending trash movement unless a filter or cleanup mode says to hide them. Successfully trashed files usually disappear from the original source because they have been physically moved to the configured trash folder.
- Accepted/favorite/locked status only locks the rating from accidental rating changes. It should not create special restrictions for ordinary file, metadata, tag, rename, move, copy, edit, trash, cleanup, extraction, handoff, or batch operations.
- Operations on accepted/favorite/locked files should follow the same confirmation, destructive-edit safety, recovery, undo, configured write-format, trash-folder, collision, and logging rules as the same operation on any other rated file. YOLO mode affects only destructive audio-edit warnings and does not create special file-operation behavior for accepted/favorite/locked files.
- Moving, copying, renaming, editing, tagging, trashing, or otherwise operating on an accepted/favorite/locked file should preserve the accepted/favorite/locked rating state unless the user explicitly unlocks and changes that rating state.
- Rejected files should remain visible enough for review only while trash movement is pending or failed. After successful trash movement, they should no longer appear in the original source unless the configured trash folder itself is indexed as a source.
- Moving, renaming, editing, or tagging a rated file should preserve the rating state unless the user explicitly changes it.
- Batch operations should report how many locked, rejected, missing, unsupported, or failed files were skipped.

## Aging and Listen-History System

Wavecrate should track listen history and use it to support library hygiene.

The system should track information such as:

- never auditioned
- last auditioned recently
- last auditioned days ago
- last auditioned weeks ago
- last auditioned months ago
- audition count where useful

The sample browser should visually communicate aging state. Files that have not been listened to recently may be visually faded, greyed, or otherwise marked so users can quickly identify neglected material.

Aging should support sorting and filtering. Users should be able to find files that are never listened to, recently listened to, not listened to for weeks/months, or heavily auditioned.

Aging state should combine with tags, ratings, similarity, folders, and text filters.

## Temporary Color Collections

Wavecrate should support temporary color collections similar in spirit to Ableton-style collections.

The target is a global small fixed set of color-coded virtual collections, such as 10 slots. Users should be able to name these color collections and quickly assign sample files to them across sources.

Temporary color collection names should follow the same lowercase single-token normalization rules as labels and user-extensible dictionary values. Spaces and unsupported characters should normalize to `-`, repeated separators should collapse, leading/trailing separators should be trimmed, and empty normalized names should be rejected.

The 10 collection colors should be carefully chosen as a coherent Wavecrate palette. They should be visually distinct from each other, bright enough to scan quickly in dense lists and map points, and still fit the application theme rather than feeling like arbitrary system-default colors.

Color indicators should work in compact UI surfaces such as browser rows, collection filters, map points, toolbar buttons, and status chips. They should remain legible in selected, focused, disabled, missing, unsupported, and warning row states.

Color should not be the only way to identify a collection. Tooltips, labels, keyboard numbers, accessible names, and optional collection names should make the collection identifiable for users who cannot rely on color alone.

Color collections should behave like temporary shelves or marks, not as the main organization model. They are stored as Wavecrate metadata in the database and should be fast to assign, remove, clear, filter, and inspect.

Color collection marks should survive app restarts and source switching until the user clears them. They are temporary because they are meant for working sets, triage, and staging, not because they are session-only.

The 10 color collections replace the older single mark / marked-filter concept. Wavecrate should not maintain a separate one-off mark flag alongside color collections. Marking files for quick review, staging, filtering, moving, copying, rating, or cleanup should happen by assigning one or more temporary color collections and using collection filters.

Possible uses include:

- collecting samples for a current track
- collecting kick candidates
- collecting sounds to clean up later
- collecting samples to move into a folder
- comparing similar sounds
- staging files before export or DAW handoff
- quickly marking files before moving, copying, deleting, tagging, or rating them

Color collections should be fast to assign, clear in the UI, sortable/filterable, and easy to clear when no longer needed.

Physical folders remain the main durable organization system. Color collections are a lightweight metadata layer for temporary workflow support.

If a user wants to make a durable collection from a temporary color collection, Wavecrate should provide a command to copy or move those files into a normal folder. The result should be ordinary audio files on disk, not a Wavecrate-specific bundle.

Clearing a temporary color collection should only remove the virtual marks. It should not delete, move, rename, or otherwise mutate the marked audio files.

Temporary color collection views should keep stale marks visible enough to repair. If a marked file is missing, unavailable, or points to a source folder that is not currently loaded, Wavecrate should show a warning state and provide actions such as reveal if available, rescan/reconcile, remove stale mark, or clear missing entries from the collection.

Wavecrate should provide an easy cleanup action for temporary color collections, such as "Remove missing from collection". This action should remove only stale virtual marks for missing files. It must not delete audio files, remove marks for files that still exist, or clear the whole collection unless the user explicitly chooses that broader action.

## Source and Scan Lifecycle

Wavecrate should let users add one or more source folders. A source folder is a real filesystem folder that Wavecrate indexes and presents through the folder tree and sample browser according to the active visibility mode.

Adding a source should:

- avoid noisy implementation details while making clear that adding a source lets Wavecrate manage supported files according to normal commands and safety settings
- validate that the path exists and is readable
- reject exact duplicate source roots that point to the same resolved filesystem location already configured
- reject nested source roots when the new source is inside an existing source or contains an existing source
- create or open source database state
- scan supported audio files incrementally
- report scan progress without blocking browsing or auditioning
- preserve existing metadata when a source is reopened

When a source-add request is rejected because it is nested with an existing source, Wavecrate should explain the conflict and offer a deliberate source swap where safe. For example, if the user tries to add a parent folder that contains an existing child source, Wavecrate may offer to remove the child source reference and add the parent source instead. If the user tries to add a child folder inside an existing parent source, Wavecrate may offer to replace the parent source with the child source. A swap should be an explicit source-reference operation, should not delete audio files or source database files, and should follow the undo, relink, and source-scan rules for source changes.

Removing a source from Wavecrate should remove it from the indexed source list but should not delete audio files or the authoritative source database from disk. Source removal should be available from the source's context menu as `Remove Source` and should act as the explicit cancel/remove path for a source that was just added. It should stop file watching, retire the exact active runtime epoch, stop or safely abandon source-owned scan/indexing/background work, clear source-owned browser/playback memory, and ignore stale completions. Path-owned readiness and unshared managed cache cleanup runs asynchronously and must preserve reusable analysis/similarity data for fast re-add. Source removal can happen immediately without confirmation and without waiting for cleanup. The result status should make that boundary clear, such as by saying the source was removed from Wavecrate and files and reusable analysis were kept. If the user has no configured sources, the source list should be empty; Wavecrate should not show a bundled, hardcoded, or otherwise non-removable placeholder source in the normal source list.

Configured sources can be reordered directly in the source list with a primary-pointer drag. The active gesture should show a source-row ghost that follows the pointer and a precise insertion line at the pending destination. The drop commits only the source-list order: stable source identity, role, selected source, loaded tree state, processing state, and source contents remain unchanged. The new order must persist across restart, and source-row reordering must remain distinct from dropping samples or folders onto a source.

Source items should have a context menu with a platform-native file-manager action. The visible label should be "Open in Explorer" on Windows, "Open in Finder" on macOS, and "Open in File Manager" on fallback platforms. This should open the source root folder in the platform file manager so users can inspect files, manually back up `.wavecrate.db`, or manage the folder outside Wavecrate.

Sample rows should have a context menu with a platform-native reveal action. The visible label should be "Reveal in Explorer" on Windows, "Reveal in Finder" on macOS, and "Reveal in File Manager" on fallback platforms. This should open the containing folder and select the specific audio file where the platform supports file selection. If the file is missing or unavailable, Wavecrate should show the missing-file state and offer source reconciliation or cleanup actions instead of opening an invalid path.

Folder rows in the folder tree should also have the same platform-native open action as source rows. This should open the actual folder path in the platform file manager. If the folder is missing or unavailable, Wavecrate should show the missing-folder/source state and offer reconciliation, relink, rescan, or cleanup actions instead of opening an invalid path.

The context-menu labels should be standardized by platform: sources and folders use the platform's open label; sample files use the platform's reveal label. When multiple rows are selected, these file-manager actions should operate on only one item: the focused row if it is part of the relevant selection, otherwise the first selected row in visible order. They should not open multiple file-manager windows in the current target.

Source items, folder rows, and sample rows should also have a shared "Copy Path" context-menu action for path copying. This should copy the real absolute filesystem path as text to the normal text clipboard only. Copied paths should use forward slashes as separators, including on Windows. If the copied path contains spaces, Wavecrate should wrap the copied path in standard double quotes, for example `"C:/sample folder/kick.wav"`. Paths without spaces should be copied unquoted. For sample rows it should copy the selected file path; for folder rows it should copy the folder path; for source rows it should copy the source root path. It should not copy source-relative paths in the current target. This is distinct from clipboard audio/file handoff, which places files on the system clipboard for DAWs or Explorer.

When multiple rows are selected, "Copy Path" should copy only one path: the focused row if it is part of the relevant selection, otherwise the first selected row in visible order. It should not copy multiple newline-separated paths in the current target.

"Copy Path" should be allowed for existing files even when they have a duplicate embedded-ID conflict, because reveal/inspection actions remain allowed for that blocked state. For missing or unavailable file records, Wavecrate should not offer a path-copy action.

If the root folder for a configured source is moved, renamed, deleted, disconnected, or otherwise unavailable outside Wavecrate, Wavecrate should mark the source as broken or missing in the source list and folder tree. The UI should not silently drop the source, delete its metadata, or pretend the source is empty.

Wavecrate should not allow a configured source root to be copied, moved, or renamed through ordinary in-app folder commands. Source-root location and duplication workflows should go through explicit source relink, remove, source-swap, backup, or export/source-management flows so `.wavecrate.db` ownership, file watching, scan state, and source identity stay clear. Ordinary in-app folder copy/move/rename remains available for folders inside a source root.

A broken or missing source should keep its source metadata and source database reference available for repair where possible. Wavecrate should provide clear actions to:

- relink the source to a new folder path
- retry or rescan the original path
- remove the source from Wavecrate's configured source list

Relinking should validate the chosen folder, reopen or reconcile the source database, and preserve metadata where file identity can be matched safely. Removing a broken source from Wavecrate should preserve audio files, the source database, reusable source artifacts, and safe content-addressed caches while asynchronously releasing volatile and path-owned derived state whose ownership can be proven.

Scanning should be recursive by default. Wavecrate should skip hidden or system folders only when configured to do so, and it should avoid traversing dangerous or redundant filesystem structures such as cycles, inaccessible junctions, or repeated symlink targets. Every recursive traversal generation must derive a stable directory identity from its opened no-follow descriptor and visit each identity at most once; the identity includes the Unix device or Windows volume so distinct devices are not collapsed, while bind mounts or repeated mount targets that resolve to an already visited identity are skipped. An identity that cannot be obtained on an unsupported platform or because descriptor metadata failed is a bounded, typed incomplete-scan diagnostic, not a path-string fallback. Repeated and identity-unavailable subtrees remain uncertain for missing-row reconciliation so they cannot cause destructive deletion. The authoritative scanner and native browser projection must classify source entries without following links: directory and file symlinks, junctions, and equivalent reparse-point links stay outside both the manifest and browser tree. Entry-type inspection failures should be logged and skipped without making the source unavailable or exposing paths beyond its configured root.

Every scanner content-hash path, including full scans, periodic content audits, deferred hashing, exact-path hashing, and targeted watcher reconciliation, must open through a source-root capability without following the final component or any ancestor. Filesystem facts, content reads, and post-read revalidation must use the retained descriptor for one object, and the manifest path must still bind to that object immediately before publication. A path that disappears, becomes linked, or is replaced remains retired or retryable without reading the replacement target, and content reads must remain outside source-database writer ownership.

On platforms that expose raw non-Unicode filenames, scanner classification must not let one such entry abort publication for the surrounding source. Supported audio with a non-Unicode relative path remains outside the normal sample manifest and is retained as a typed, lossless index-only diagnostic; other index-only classifications use the same lossless path identity. The encoding must preserve distinct raw names, round-trip across restart, and remain consistent for full scans, targeted sync, deletion, rename, and browser/index projections. Existing Unicode source paths retain their current readable database representation.

Filesystem modification timestamps persisted by the scanner and source-file metadata callers use one shared checked SystemTime-to-Unix-nanoseconds conversion in the existing signed `i64` database units. Exact pre-epoch values, including nanoseconds immediately before the epoch, remain negative and preserve ordering across zero; timestamps outside the representable range saturate deterministically to `i64::MIN` or `i64::MAX`. Full scans, targeted scans, index-only entries, descriptor revalidation, journal recovery, and mutation fences share this conversion contract. The conversion does not change filesystem timestamps, database units, or wall-clock-second semantics, and timestamps remain metadata/fence inputs rather than content identity.

The final scanner should not silently impose arbitrary product limits on folder depth, number of child folders, or number of files per source. Any defensive scan limit used to protect responsiveness should be explicit, configurable where practical, logged, and visible as a partial-scan warning with a clear rescan or settings path.

The scanner should classify discovered files as:

- supported audio file
- unsupported audio file
- unsupported non-audio file
- inaccessible file
- too-long or otherwise practically unsupported audio file
- changed file requiring metadata, waveform, analysis, or similarity invalidation
- missing or deleted file that still has database state

Unsupported files should not crash scanning. They should be classified and reported, but hidden from the normal sample browser by default. They should become visible when the user enables the explicit `all files` visibility flag or opens a diagnostic/unsupported-files view.

Unsupported audio files and unsupported non-audio files should remain scanner/index entries only. Wavecrate may store enough path, type/classification, availability, and diagnostic status to show them in `all files`, diagnostics, and filesystem-management workflows, but it should not create Wavecrate Sample IDs, embedded-ID state, sample metadata records, analysis records, similarity records, rating records, tag assignments, listen-history records, or generated-name state for them.

If a later Wavecrate version adds support for a format that was previously classified as unsupported, rescan or file-watch reconciliation should promote matching index-only entries into normal supported sample records. Promotion should use the same default initialization and embedded Sample ID workflow as newly scanned supported files, starting without inherited Wavecrate metadata because no sample metadata record existed while the file was unsupported.

Initial scan/indexing should queue the fingerprint work needed for exact duplicate grouping where practical. Duplicate fingerprinting should run in background work, stream status like other indexing work, and mark duplicate state incrementally as matching available files are found. It should not block source scanning completion, folder browsing, sample auditioning, editing, extraction, tagging, rating, or handoff. Users should also be able to trigger manual duplicate analysis for selected files, folders, sources, or the whole indexed library.

Initial scan/indexing may queue compact waveform-summary work for supported audio files where practical. Waveform preparation is a rebuildable browsing optimization, not durable source readiness, and its absence or eviction must not keep source processing active. Audition should read supported WAV files through the file-backed playback runtime on demand instead of precomputing decoded per-file playback payloads for the library.

File watching should update source state when files are created, removed, renamed, moved, or modified outside Wavecrate. External changes should become visible in the folder tree, sample list, filters, and later starmap as close to immediately as practical without requiring a manual rescan.

The watcher should cover ordinary source-folder changes, including:

- new audio files added by another application
- files deleted or moved out of the source folder
- files renamed outside Wavecrate
- folders created, deleted, moved, or renamed
- audio files overwritten or modified by another application
- source database files changed, locked, or unavailable

File-watch events are bounded, debounced hints and must be reconciled through the same source database rules as manual scans. Targeted sync, full scans, and deferred hashing publish a monotonic committed source revision plus structured created, changed, moved, and deleted identity deltas. Browser projection and readiness reconciliation may update only after that authoritative commit. The UI may show a short scanning/reconciling state while Wavecrate validates the change, but it should not wait for a full source rescan before showing committed additions, removals, folder changes, or stale/missing states.

Watcher-targeted synchronization must scale with the changed subtree. Debounced paths are normalized once and an ancestor target subsumes its descendants before filesystem traversal, index reconciliation, or manifest reads. Targeted manifest reads use one revision-consistent exact-subtree boundary and retain only scoped rows; committed deltas use touched rows plus explicit rename evidence. Targeted completion and rename recovery use revision-fenced bounded commits and candidate-scoped reads. A revision gap, uncertain traversal, watcher overflow, or explicit authoritative request may choose the existing full-snapshot/full-traversal fallback; a targeted revision gap may instead return a typed stale result for retry. Ordinary one-file or one-directory changes must not load or clone the complete source manifest. Targeted telemetry records canonical scope count, manifest rows materialized, scoped query count, and elapsed synchronization time so regression thresholds can be checked at representative source sizes.

Watcher ingress and per-source path accumulation must remain bounded. Events should coalesce by source and path; overflow, watcher restart, or irreducibly uncertain targeted sync must degrade to an authoritative background source scan while keeping the current browser usable and clearly indicating that reconciliation is in progress. One source-scoped causal plan coordinates targeted watcher paths and full-refresh causes: paths arriving during active work are deduplicated for one ordered follow-up, a pending full traversal subsumes compatible queued paths, and a full fallback waits for the active targeted commit before execution. Pending full refreshes must retain their typed cause, source lifecycle generation, and committed revision when one exists. An accepted scan may suppress a revision-backed refresh only when its authoritative snapshot covers that revision; unconditional watcher-loss, cancellation, and incomplete-sync recovery must remain queued. Convergence telemetry should record the cause, covered revision, lifecycle generation, queue age, suppression/coalescing result, and terminal idle transition. Watch roots must be tracked by platform filesystem identity as well as configured path so a directory or mounted volume replaced at the same path fences the retired watcher, re-registers the current object, and triggers authoritative reconciliation. Identity-read uncertainty must fail toward bounded, backoff-limited full reconciliation without turning Wavecrate-owned database metadata echoes into a scan loop. Watch roots must be re-established with bounded backoff after runtime failure, and missing roots must be detected and reconciled when they disappear or reappear. Backend construction and teardown are coordinator-owned lifecycle work: at most one unresolved initializer per backend and a fixed number of off-coordinator teardown workers may exist. A constructor or backend drop that does not return must fence its generation, retain its accounted slot, log the degraded state, and block additional recovery work of that kind rather than creating detached workers; native-to-polling fallback and full-manifest reconciliation remain available while capacity exists.

Foreground source scans publish one typed, source-scoped lifecycle from queueing through source replacement, scan-capacity and database admission, scanning, persistence, projection application, retry, and terminal outcomes. A successful authoritative scan carries its exact committed identity delta through projection application into bounded readiness reconciliation; an unchanged delta is a no-op, while an incomplete traversal, revision mismatch, missing delta, or stale lifecycle chooses complete reconciliation instead of treating a partial delta as authoritative. Queue age, last meaningful progress age, lifecycle generation, current capacity owner, and retry count use monotonic runtime state and remain observable without flooding the UI with timer events. A healthy wait names its blocker; a nonterminal task with no meaningful change for the calibrated threshold becomes `Taking longer than expected` and exposes retry/cancel actions that still use the normal lifecycle and resource fences. User cancellation, source retirement or replacement, shutdown, panic, and dropped completion must never strand the visible scan owner. Stale progress and completion from a retired task or generation are ignored.

Watcher delivery is not the correctness boundary. While Wavecrate is running, frequent source-processing safety activity should inspect only durable revisions, contracts, availability, and due deadlines when nothing changed. A separate infrequent authoritative audit of each active source manifest repairs closed-app changes and dropped events without high-frequency full-manifest polling or UI-thread filesystem work. Manifest-traversal completion and content-verification coverage are separate durable facts: completing one traversal must not claim that its content slice completed.

Interrupted manifest-audit checkpoints are resumable traversal candidates, not verification evidence. A resumed traversal must reopen every checkpointed path before excluding it from normal reconciliation; it must hash one bounded checkpoint slice at a time so same-size edits with restored modified times cannot retain a stale content identity. Completion remains due until all durable checkpoint candidates have been revalidated. Missing or renamed checkpointed paths remain eligible for ordinary missing and rename reconciliation. Checkpoint persistence, manifest publication, and completion timestamp publication must remain ordered so a crash can leave work due for retry but cannot publish a false complete audit.

Content verification advances through a durable per-source rotation with a revisioned forward cursor and independently due retry entries. Each admitted slice derives an adaptive entry target from the 30-day coverage objective, then remains bounded by elapsed time, bytes read, a hard entry ceiling, source storage class, and current playback/foreground activity. Forward and retry planning use indexed cursor windows with explicit row limits proportional to that entry ceiling; an admitted slice must not load or clone the complete manifest or audit-state map. A skipped, unavailable, unsupported, failed, or concurrently changed entry retains a stable reason and retry deadline without preventing forward progress. Cancellation, shutdown, restart, source replacement, and root unavailability preserve committed entry checkpoints; legacy `source_content_audit_cursor_v1` state is imported as cursor progress but never treated as verification evidence.

Per-source coverage telemetry reports the active rotation and checkpoint revision, verified and remaining entries/bytes, bytes read, retry scope, oldest-unverified age, projected rotation time, and the previous measured rotation time. A content delta may be published only after the hashed filesystem snapshot is revalidated and its authoritative source-database transaction commits. Diagnostic acceleration may raise the normal slice ceilings but must retain the same time, byte, lifecycle, identity, revision, and foreground-resource fences.

When a file disappears, Wavecrate should retain metadata as unavailable state until reconciliation determines whether it was moved, renamed, deleted, or temporarily inaccessible. The UI should distinguish missing, unavailable, unsupported, and failed-to-scan files.

When reconciliation determines that a file was renamed outside Wavecrate, Wavecrate should keep the same Sample ID where file identity matches and update the disk path/filename state without changing the label.

Scan-based rename reconciliation should preserve the complete user-metadata snapshot atomically, including every collection membership and the exact prior curation timestamp. Reconciliation is not a new curation event: an unset curation timestamp should remain unset, and metadata staged for deferred rename recovery should remain durable across restart and schema migration.

Rename ownership must be decided against the complete relevant destination set, not directory visitation or scan-batch order. A unique stable filesystem identity takes precedence over content equality. Content-hash recovery may transfer identity only when exactly one retained source and one eligible live destination remain; multiple same-content destinations stay independent and neutral while the original metadata remains pending.

External filesystem changes detected by file watching or rescan should be reconciled as external state changes, not added to the Wavecrate undo stack. This includes external renames, moves, deletes, additions, and overwrites. Undo should only cover actions initiated inside Wavecrate.

When a file is changed outside Wavecrate, Wavecrate should invalidate stale waveform, audio-analysis, BPM, transient, silence, similarity, embedded-ID, and display-name-derived state as needed. It should not silently reuse stale analysis for changed audio.

Scan progress should expose queued, active, completed, skipped, failed, and cancelled counts. Users should be able to keep browsing while scanning continues in the background.

Waveform-summary progress may be visible globally and per item when it runs. The bottom status bar should include a compact background-job progress bar for scanning, indexing, and cache preparation work. The visible progress bar should represent one current job at a time rather than combining unrelated concurrent job types into a misleading aggregate. If several job types are active, the visible status-bar job should prefer foreground/current-selection work first, active-folder work second, and source-wide background work after that. The indicator should show successive job progress as queued jobs run, so a sequence of small background jobs can appear as repeated fills while broader queue counts live in the details popover.

The status-bar background-job indicator should provide a direct way to pause and resume non-critical background work such as waveform preparation. This may be a small pause/resume button on the indicator, a click target, or a right-click context menu. Pausing background work should not cancel the currently selected foreground file load, prevent selected-file loading from starting, or block urgent user-requested work. The pause is session-local, should not auto-resume because the user is idle, and should reset to enabled on the next application launch.

Playback and foreground source loading may cooperatively cancel the current low-priority waveform-cache batch, but cancellation is a pause boundary rather than queue deletion. Every unprocessed path from planning must return to the retained backlog and resume after foreground activity ends.

Background scan, indexing, and waveform-cache jobs should continue while Wavecrate is minimized unless the user manually pauses them, the operating system suspends the process, or the app is shutting down.

On application shutdown, non-critical background scan, indexing, and waveform-cache jobs should cancel quickly rather than making shutdown wait for long work to finish. Wavecrate should persist enough state to know what completed, what failed, and what remains stale or incomplete, then requeue needed work on the next launch for enabled and available sources. Broken, disabled, or missing sources should be skipped and marked instead of creating retry churn.

Clicking the status-bar job indicator should open a compact job-details popover or equivalent surface. It should show the active job type, current file or folder where useful, queued count, completed count, skipped count, failed count, cancelled count, and pause/resume controls. It should include compact actions for failed background work, such as retry failed jobs and open diagnostics. Retrying failed jobs from the normal job popover should retry failed work in the current source/folder context by default. A broader global retry-all-failed action may live in diagnostics or maintenance UI. The popover should be concise enough for normal use but provide enough context that users can understand why the progress bar is moving or stuck.

Individual sample rows should show subtle readiness indicators for pending, active, ready, stale, failed, or unsupported cache state where practical, without making the list visually noisy.

If a background waveform-summary job fails for a file, Wavecrate may retry a small bounded number of times. After the retry limit, it should mark that file's cache state as failed and stop retrying until the file changes, the cache version/settings change, or the user explicitly triggers rescan, rebuild, or retry. A failed waveform cache job should not make the file disappear, block other background work, or make the source incomplete.

Selecting a file should be treated as an explicit foreground request even if its background waveform-cache state is failed. Wavecrate should attempt one fresh foreground load for the selected file before treating it as unavailable for waveform/playback use. If that foreground load fails, the UI should show a clear selected-file load failure with any available retry, reveal, rescan, or diagnostics actions.

## Selection, Search, and Filter Semantics

Wavecrate should have predictable browser selection behavior for single-file and batch workflows.

The sample browser should support:

- single selection
- range selection with Shift
- additive/removal multi-selection with Ctrl
- select all in the current filtered view
- stable selection when rows are resorted, rescanned, renamed, or updated
- visible distinction between selected, focused, playing, edited, locked, missing, and processing rows

The focused row is the primary keyboard target. The selected set is the batch-operation target. The playing file may differ from the focused row during preview or handoff workflows, but the UI should make that clear.

Marking a sample or folder with the `X` hotkey should briefly flash the marked item in the active browser view. Removing an item from the selected set should not show the positive marking flash.

When multiple sample files are selected, Wavecrate should visually indicate metadata differences across the selection. Shared values should be shown normally, while fields with mixed values should show a clear mixed-state indicator rather than pretending all selected files share the focused file's metadata. This should apply to common metadata such as tags, label, prefix, BPM, Tuning/Scale, rating, temporary color collections, Playback Type, Sound Type, Character tags, and generated-name inputs where relevant.

If the user edits a mixed metadata field while multiple files are selected, the committed value should apply to all selected files for that field. The edit should be one undoable batch metadata transaction with a clear summary of affected, skipped, and failed files.

Single-value fields such as BPM, Prefix, label, Playback Type, Sound Type, Tuning/Scale, rating, and similar scalar metadata should overwrite that field for all selected files when committed from a multi-selection. Label is single-value free text, not a tag list.

For multi-selected tag fields, adding a tag should add that tag to all selected files while preserving each file's existing other tags. This should behave as an additive batch tag transaction rather than replacing whole tag sets.

Batch commands should operate on the selected set. If no selected set exists but a focused row exists, commands may operate on the focused row only when the command label and status make that clear.

Search should be fast, incremental, and combinable with filters. It should match useful text fields such as disk filename, display name, label, prefix, source, tags, BPM text, tuning/scale text, folder path segments, and file extension where relevant.

Search should support both plain text and structured query tokens. Plain text should remain the fastest path for quick search, while tokens should let users express precise filters from the same search box.

Initial structured tokens should include practical fields such as:

- `tag:kick`, `sound:kick`, or category-specific tag tokens where useful
- `char:warm` or similar character-tag filtering
- `bpm:120`, `bpm:120-130`, `bpm:unknown`, and `bpm:known`
- `scale:pythagorean`, `scale:slendro`, `tuning:19edo`, `scale:unknown`, and `scale:known`
- `rating:keep`, `rating:trash`, `rating:accepted`, `rating:unrated`, and level-specific variants where useful
- `collection:1` through `collection:10`, plus collection names where configured
- `format:wav`, `channels:mono`, `channels:stereo`, `state:missing`, `state:unsupported`, `state:editable`, `state:duplicate`, `state:duplicate-id`, and similar availability/support tokens
- `source:name` and `folder:path-fragment` where practical

Structured tokens should support negation for common filters, such as `-tag:kick`, `!collection:3`, or another consistent syntax chosen by the implementation. The syntax should be forgiving enough for fast use, but invalid tokens should be shown as understandable query errors or treated as plain text only when that is clearly safer.

The search parser should produce an inspectable filter model rather than hiding behavior in ad hoc string matching. Active structured tokens should appear as clear filter chips or status indicators so users can remove individual filters without rewriting the whole query.

Structured search should be available through both direct text input and UI controls. Users should be able to type tokens freely in the search box, but common filters should also be exposed as chips, menus, segmented controls, or compact filter panels that create or update the same underlying filter model.

Typical UI-generated filters should include tags, sound type, character, BPM, tuning/scale, rating squares/accepted state, temporary color collections, format/channel/support state, missing/unsupported/exact-duplicate/duplicate-ID-conflict state, source, folder, and analysis readiness. The UI and typed query should stay synchronized so changing a chip updates the active query/filter state and typing a supported token updates the visible chips.

Filters should be composable. The target filter dimensions include:

- source
- folder
- text search
- Playback Type
- Sound Type
- Character
- BPM range or known/unknown BPM
- Tuning/Scale
- Prefix
- configured source folder
- rating state
- accepted/locked state
- rejected/trashed state
- age/listen-history state
- supported/editable/unsupported/missing/duplicate state
- analysis pending/stale/failed/ready state
- similarity-to-reference state
- one or more temporary color collections

The UI should show active filters clearly and provide a fast way to clear one filter or all filters. Empty filtered results should identify that filters are active.

Sorting should be stable and predictable. Sort keys should include at least display name, disk filename, folder, duration, BPM, rating, age/listen state, modified time, created/imported/indexed time where available, and similarity score when a reference is active.

## Sample Browsing, Auditioning, and Usage Target

Browsing, auditioning, and using samples should feel immediate, even on large libraries.

Wavecrate should support:

- fast source and folder navigation
- incremental source scanning
- responsive sample lists for large folders and libraries
- keyboard-first movement through samples
- pointer-based selection and auditioning
- immediate playback on selection by default
- quick restart, stop, seek, loop, and range playback
- target-BPM auditioning where practical
- copy, drag, export, reveal, or handoff of selected sample files into DAWs and external tools
- visible selected, playing, loading, unavailable, failed, edited, unsaved, unreviewed, aged, keep-rated, trash-rated, accepted, rejected, and trashed states
- stable behavior when the user moves quickly through many samples

Fast browsing is more important than decorative UI. The user should be able to scan thousands of files, hear sounds quickly, find sounds that fit a production context, and move chosen samples into their DAW without breaking flow.

Selecting a sample should start playback by default. Users should be able to disable immediate audition in settings, but the product default should favor fast auditioning without requiring a separate play command for each selected file.

## DAW and External Tool Handoff Target

Wavecrate should make it easy to use found sounds in music production.

The initial target is practical Windows-first file-based handoff, not deep DAW integration.

Wavecrate should support clipboard-based audio handoff.

When the user selects one or more supported sample files in the sample browser and presses copy, Wavecrate should place those files on the system clipboard in a format that DAWs and Explorer can consume as ordinary audio files.

On Windows, the compatibility clipboard writer must open the clipboard with the
current Wavecrate UI thread's active window as a non-null owner. It must close
the clipboard on every post-open failure, including an `EmptyClipboard` failure,
before returning the error.

Playback-only unsupported audio files should not be eligible for Wavecrate's DAW/external audio handoff workflows, even when the original file path could technically be copied or dragged. Wavecrate cannot guarantee DAW compatibility for unsupported formats, so clipboard and drag/drop audio handoff should stay limited to supported sample formats. Users may still reveal, rename, move, copy, or remove the unsupported file through filesystem-management actions where those actions are available.

Wavecrate should distinguish between path-copy commands and file-copy commands. "Copy Path" is a context-menu utility that copies one absolute path as text. It must not place the file itself on the clipboard. Copying the actual file is a separate file handoff action, such as the normal copy command on selected browser files.

When the user selects an audio range in the waveform editor and presses copy, Wavecrate should create a durable extracted audio file for that selection in the active Wavecrate folder and place that file on the clipboard as an ordinary audio file.

Waveform-selection clipboard copy should use two distinct visual confirmations. The first pulse acknowledges that Wavecrate accepted the copy command and started extracting the selected range. The second pulse must happen only after the extracted file has been written and the platform clipboard contains that durable extracted file path, so the user can treat the second pulse as the reliable "ready to paste into a DAW" signal.

For unprocessed WAV range handoff or extraction, Wavecrate should seek directly to the selected audio range and write only the selected frames rather than reading or decoding the skipped prefix.

Pasting into a DAW should make the DAW receive normal audio files, not Wavecrate-specific data.

Pasting into Explorer should create one or more audio files in the target directory.

Clipboard handoff should preserve the exact audio the user intended to copy. For a waveform selection, that means the selected range. For browser selections, that means the selected whole files.

Temporary clipboard or DAW handoff staging files, for operations that still genuinely require transient staging, should live under the global `.wavecrate` handoff staging area. Waveform-selection clipboard copy and Explorer drops are different: Wavecrate should create the final ordinary audio file in the active Wavecrate folder or Explorer drop target directory rather than leaving the user with a temporary staged file.

Drag-and-drop should mirror clipboard handoff behavior.

Wavecrate should support dragging:

- one selected sample file
- multiple selected sample files
- a selected audio region from the waveform editor

Dragging files or regions into a DAW should provide ordinary audio files that the DAW can import.

Dragging files into Explorer should copy or create the corresponding audio files in the target folder.

Dragging a waveform selection outside Wavecrate should create a new audio file for that selected region and hand off that file to the drop target.

Wavecrate should also support workflows such as:

- copy a file path
- reveal a sample file in the platform file manager
- export or copy a prepared sample file to a chosen folder
- extract a region and immediately drag/copy/export the new file
- hand off the exact audio the user auditioned, including baked audition-time warp when extraction/export requires it

Initial implementation should prioritize drag-and-drop into DAWs such as Ableton Live and Bitwig where practical, plus reveal-in-platform-file-manager and copy-file-path.

Handoff should be fast and predictable. It should not require users to understand Wavecrate internals.

Future macOS/Linux handoff should be possible through platform-specific adapters without rewriting core product logic.

## Import, Export, and Collision Policy

Wavecrate should not require a formal import step for existing libraries. Adding a source indexes existing supported files in place.

Import-like workflows may still exist for copying external files into an indexed source folder. When Wavecrate copies files into a source, it should:

- preserve the original file where possible
- avoid filename collisions with predictable suffixes
- register the copied file after the filesystem write succeeds
- assign or reconcile Wavecrate Sample IDs
- preserve supported metadata where safe
- report skipped, copied, failed, and unsupported files

Export writes an ordinary audio file to a chosen destination. Export does not imply source registration unless the destination is inside an indexed source and Wavecrate successfully indexes it.

Export, extraction, import-like copy, duplicate, collect, move, generated-rename, new-file creation, new-folder creation, and other derived-file or derived-folder commands should use a shared collision policy:

- never overwrite an existing file or folder silently
- use predictable numbering or suffixes by default when the desired destination path already exists
- scope collision checks to the destination folder, because filesystem names only collide within the same folder
- avoid overwrite prompts for ordinary new-file or new-folder creation by choosing the next available numbered name
- reserve overwrite behavior for commands that are explicitly rewriting the same file being edited
- keep source files unchanged unless the command is explicitly destructive
- report final destination paths clearly

The default numbering style should be stable and easy to predict. For files, use a pattern such as `name.wav`, then `name_001.wav`, `name_002.wav`, and so on when collisions exist. For folders, use an equivalent folder-safe pattern such as `kick`, then `kick 2`, `kick 3`, or a consistent zero-padded variant if the product settles on one. The exact separator and padding can be adjusted later, but all file-creation and folder-creation paths should use shared helpers so extraction, export, duplicate, copy, collect, generated rename, handoff staging, new sample creation, and new folder creation do not drift into different naming rules.

Overwriting is allowed when a destructive edit is writing back to the same source file that is currently being edited. In that case, the overwrite must follow the destructive-edit safety, recovery, undo, logging, and configured write-format policy. Wavecrate should not overwrite another existing file or folder at the destination merely because the user is creating, moving, copying, extracting, exporting, duplicating, or creating a folder where that name already exists.

When exporting or extracting warped, gained, faded, normalized, reversed, or otherwise processed audio, the written file should match the audio the user intended to render, including the current audition-time processing when the command says it will bake that processing.

## Sample Editing and Cleanup Target

Wavecrate should support lightweight, sample-centric editing directly in the waveform view.

Wavecrate editing exists in two related scopes:

1. **Waveform-selection editing**, where an operation applies to the current edit selection or play selection inside the waveform editor.
2. **Item-based editing**, where an operation applies to one or more selected sample files in the sample browser list.

Operations that make sense in both contexts should be available in both contexts. For example, normalize should be available for a selected waveform range and also for one or more selected sample files in the browser.

Waveform-selection edit actions should include:

- trim/crop to selection, removing audio outside the selected range
- cut/delete selected range
- split at cursor or split selected range into separate regions/pieces
- mute/silence selected range
- copy selected audio
- paste copied audio at cursor or insertion point
- duplicate selected audio, appending or inserting a repeated copy
- reverse selected audio
- normalize selected range
- apply gain adjustment to selected range
- add fade-in and fade-out to selected range
- draw or edit a volume envelope/automation curve over the selected range
- remove or reduce silence where practical
- extract selected range into a new audio file

Item-based edit actions should include:

- normalize selected sample files
- trim silence from the start and/or end of selected sample files
- reverse selected sample files
- apply gain adjustment to selected sample files where practical
- batch metadata/tag/rating/label operations where relevant

Item-based operations should behave like batch operations. They should process all selected files through the same operation, report progress, skip or fail individual files safely, and provide a clear summary of successful, skipped, and failed files.

Batch operations should be cancellable where practical. If a user cancels a batch operation after some files have already been changed, Wavecrate should try to roll back as much completed work as possible using the same transaction and recovery records used for undo. Successfully rolled-back files, unchanged pending files, skipped files, failed files, and files that could not be rolled back should all be reported clearly.

Rollback should be best-effort but serious. For audio edits, file moves, generated renames, tag/rating/metadata changes, temporary color collection changes, and trash moves, Wavecrate should capture enough before-state to undo completed items when cancellation happens. If rollback is unsafe or impossible for an item, Wavecrate must inform the user that not everything could be rolled back. The UI and logs should say exactly which items remain changed and why.

Native file moves, copies, and external-file imports must claim their final destination without replacing an existing filesystem entry. A destination that appears after planning is a recoverable collision: moves keep the source, copy and import flows continue the predictable collision-numbering policy, and rollback or failure cleanup mutates only a destination whose filesystem identity still belongs to the transaction. Cross-device move fallback must publish a complete same-folder staged copy with the same no-replace guarantee before removing the source.

Silence trimming should be available both as a waveform-editor action and as an item-based batch action.

In the waveform editor, silence trimming should let the user visually inspect and apply removal of silence at the start and/or end of the current audio file or selected range.

In item-based mode, silence trimming should apply to all selected sample files and remove detected leading and/or trailing silence according to the current silence-detection settings.

Silence trimming should not remove intentional internal silence unless the user explicitly chooses an operation designed for internal silence removal.

Wavecrate should support a visual silence-extension mode in the waveform editor.

When silence-extension mode is active, the waveform should show a dedicated extension handle at the right edge of the audio file or selected region. The user can drag this handle to the right to make the sample longer by appending silence.

The silence-extension handle should only be visible while the extension mode is active. It should not clutter the normal waveform editor.

This operation does not time-stretch, loop, or synthesize new audio. It only extends the file duration by adding silence.

Silence extension should also support manually entering a target length.

The user should be able to specify the desired final sample length in musical or time units where practical, such as seconds, milliseconds, bars/beats when BPM is known, or samples/frames where useful.

When the target length is longer than the current audio length, Wavecrate should append silence until the requested duration is reached. If the requested length is shorter than the current file, Wavecrate should not silently trim the file; it should route the user to the normal trim/crop workflow instead.

Wavecrate should support a volume-envelope editing mode.

In this mode, the user can draw or edit a gain curve directly over the waveform, similar in spirit to a simple DAW automation lane.

The curve should affect playback preview while editing and should only be baked into the audio file when the user explicitly applies the edit. Applying the curve is a destructive edit and should follow the normal destructive-edit safety, YOLO mode, recovery, and undo behavior.

The initial target is simple volume automation only. It is not a general automation system for effects, plugins, modulation, or multitrack mixing.

Core editing workflows should also include:

- correcting or setting BPM metadata where practical
- aligning loop boundaries to grid or transients where practical
- creating named markers or regions
- naming selected regions or extracted slices
- rating selected regions or extracted slices
- exporting selected ranges or slices as new files
- saving destructive edits with clear warning/YOLO behavior

Editing should be designed around fast preparation of individual sample files and long recordings. The user should be able to clean up a tail, remove silence, isolate a hit, split a loop into useful pieces, cut a long jam into named regions, normalize a quiet file, mute an unwanted section, and export prepared versions without leaving the browsing flow.

The first complete editing target is mono-style editing. Stereo files should retain their stereo data, but edits should affect both channels equally until stereo split-view editing is added later.

### Edit Lifecycle and Save Semantics

Wavecrate should make the lifecycle of an edit explicit. Users should be able to tell whether they are previewing a change, have applied a destructive change, or still need to save/apply a pending operation.

Edit lifecycle states should include:

- clean file with no pending edit
- selected range or cursor state only
- previewing a non-destructive edit parameter, such as fade length, gain amount, or volume-envelope curve
- pending destructive edit that has not yet been written
- rendering or writing edit
- edit succeeded
- edit failed and source file was preserved or restored
- edited file unavailable or requiring recovery

Preview state should affect audition playback where practical but should not rewrite the source file until the user explicitly applies the edit or confirms a command that is defined as immediate destructive editing.

Wavecrate should use a mixed apply model:

- Command-style edits such as mute/silence, normalize, trim/crop, cut/delete, reverse, gain with a confirmed value, silence trim, paste, and downmix conversion may apply immediately after the normal destructive warning or immediately in YOLO mode.
- Handle-based, modal, or continuously edited preview operations such as fade handles, amplitude/volume-envelope automation curves, and other drag-adjusted edit previews should remain preview state until the user explicitly applies them, such as by pressing Enter or clicking an Apply control.
- If an edit exposes parameters in a dialog, popover, inspector, or mode-specific interface, the edit should apply only after the user confirms those parameters.
- Escape should cancel preview state where possible without modifying the file.

Applying a destructive edit should:

- check whether the file is supported for editing
- stop or coordinate playback as needed
- create a recovery file before overwriting the source file
- render the edited audio to a safe temporary output path
- preserve channel layout where practical and apply the configured Wavecrate write format for sample rate, bit depth, and sample format
- preserve unknown safe chunks or metadata blocks where practical
- write embedded Wavecrate Sample ID metadata where safe
- atomically replace or otherwise safely overwrite only the same source file that is currently being edited
- update file size, duration, fingerprint, waveform cache, and database state
- invalidate stale BPM, transient, silence, similarity, waveform, and display-name-derived state
- register an undo transaction with enough information to restore the prior file for the current session
- trigger the applied-edit visual pulse for the affected region after the write and state update succeed
- log success, failure, rollback, and timing information

If any step fails before the source file is overwritten, the original file should remain unchanged. If a failure happens after the source file is touched, Wavecrate should restore from the recovery file where possible and report whether recovery succeeded.

Copy and paste inside the waveform editor should not silently overwrite audio without following destructive-edit safety rules. Pasting audio at the cursor or insertion point should create a pending or confirmed destructive edit according to the active safety mode.

Saving an edited audio file means saving the actual file currently being edited. It does not create a Wavecrate project export.

If a handle-based, modal, or continuously edited preview state is active and the user tries to leave the file, select another file, switch sources, close the app, or otherwise discard the active edit context, behavior depends on destructive safety mode:

- In YOLO mode, Wavecrate should automatically discard unapplied preview state and continue the transition without writing the preview to disk.
- Outside YOLO mode, Wavecrate should warn that the user is about to leave a file with unapplied edit state and offer Apply, Discard, or Cancel.

Apply should route through the normal destructive-edit system, including configured write format, recovery, undo transaction creation, logging, and applied-edit visual feedback. Discard should leave the audio file unchanged and clear the preview state. Cancel should keep the user in the current file with the preview state intact.

Undoing a destructive edit should restore the previous audio file from the current-session recovery record and refresh database, waveform, and analysis state. Redoing the edit should reapply or restore the edited version without losing metadata such as tags, rating, lock state, playback state, source identity, or Wavecrate Sample ID.

## BPM, Grid, Warp, and Timing Target

Wavecrate should support BPM-aware auditioning and extraction in a later production-usefulness phase. This is not part of the minimum ordinary-WAV browsing/editing/extraction foundation, but it is part of the fuller Wavecrate target.

BPM metadata is stored per audio file. A long source recording may have its own BPM metadata, and every extracted region that becomes a new audio file may have its own independent BPM metadata after creation. Regions that are only selections inside a source file do not own durable BPM metadata until they are extracted or otherwise written as their own file.

When Wavecrate extracts a region from a source file that has BPM metadata, the extracted file should inherit that BPM by default. The extracted file's BPM is independent after creation and can be manually edited or recalculated through the play-selection BPM workflow without changing the source file.

When Wavecrate extracts a region from a source file that has a label, the extracted file should inherit that label by default as a one-time copied metadata value. The extracted file's label is independent after creation and can be edited without changing the source file.

When Wavecrate extracts a region from a source file with Prefix metadata or Prefix tags, the extracted file should inherit that prefix exactly by default. Wavecrate should not automatically add an extraction-specific prefix such as the source filename, session name, or region marker unless the user changes the naming template later. File and display-name uniqueness should be handled by the normal generated-name and collision-numbering policy.

Extracted files should start as newly created and unlistened for aging/listen-history purposes. They should not inherit the source file's age, last-auditioned time, or audition count.

The system should support:

- manually setting or correcting BPM metadata
- deriving BPM from a deliberate play-selection/grid workflow
- tempo-grid display when BPM is known
- grid-aware selection and loop adjustment
- transient-aware loop boundary adjustment where practical
- setting a target audition BPM
- locking the target audition BPM
- auditioning BPM-tagged loops warped to the target BPM where practical
- extracting or exporting warped audio exactly as heard when the user extracts from a warped audition state

Time-stretching/warping is part of the audition and extraction workflow, not a general DAW-style warping environment.

The initial warp target should be practical and beat-oriented, similar in spirit to a simple beats-style warp mode. Special high-quality, multi-algorithm, formant-preserving, or advanced warping modes are non-goals for the current target.

Wavecrate should not become a full arrangement, clip-warping, or multitrack timing editor. The goal is to audition loops in a production-relevant tempo context and to bake that auditioned result into a new sample file when the user extracts or exports it.

BPM should be set by deliberate user action, not by silent automatic BPM analysis. The user should be able to type a BPM value directly into the BPM field, and Wavecrate should use that value to show beat-grid lines over the waveform when the grid is enabled.

Wavecrate should also support deriving BPM from the play selection only. The user should be able to create a play region over a visible rhythmic phrase, choose or enter how many beats the region represents, and adjust the region boundary against the waveform so it spans those beats. A grid-resize gesture, such as Alt-dragging a play-selection boundary, should scale the assumed beat span against the waveform and calculate the BPM from the resulting region length.

For example, if the user marks a region that represents four beats and adjusts the region to line up with four kicks in the waveform, Wavecrate should calculate the implied BPM and set the file's BPM metadata from that deliberate action.

This BPM derivation workflow should not use the edit selection. Edit selection remains the target for destructive waveform edits, while play selection owns audition looping and BPM/grid derivation.

The beat count used for BPM derivation is transient workflow state. Wavecrate does not need to store "this region was treated as four beats" as durable metadata. If the user wants to revise the BPM, they can create or adjust the play selection and repeat the same BPM derivation workflow. Only the resulting BPM metadata needs to be stored.

The BPM grid should be visible only when enabled or when the user is in a BPM/grid workflow. Grid lines should show every beat at the current BPM and should update as the user changes the BPM field or derives a new BPM from the play selection.

Audio analysis should be a deliberate workflow. Wavecrate should not run BPM analysis automatically as part of ordinary source scanning. Initial background analysis may exist for non-destructive preparation such as waveform/cache generation or explicitly enabled analysis workflows, but metadata-changing analysis should be visible and intentional.

When analysis runs, it should be allowed to update the metadata fields it owns, such as transient state, silence information, similarity descriptors, and analysis-derived readiness flags. Manual edits are normal metadata edits, not permanent protection against future analysis. If the user deliberately runs or enables analysis again, Wavecrate may replace earlier manual or detected values for those analysis-owned fields.

Analysis should not casually rewrite unrelated user-authored fields. Tags, labels, prefixes, ratings, temporary color collections, accepted/rejected states, generated disk filenames, and manual organization choices should only change when the specific command or analysis workflow explicitly owns that output.

Analysis outputs should include confidence, provenance, and freshness state where useful:

- BPM metadata should distinguish manually entered, region-derived, unavailable, stale, and failed states.
- Transient detection should report whether transient markers are ready, pending, stale, unavailable, or failed.
- Silence detection should expose the threshold/settings used so batch silence trimming can be reproduced.
- Warp audition should make it clear when playback is original-speed, target-BPM warped, unavailable, or failed.

Analysis-triggered metadata changes should be undoable or revertible where practical, especially for visible fields. Wavecrate should show enough status that users understand whether a value is manual, detected, stale, low-confidence, or failed.

## Audio Engine Target

Wavecrate should include a robust audio engine for fast, predictable auditioning, destructive editing, extraction, and sample-library usage.

The audio engine should support:

- immediate playback of selected sample files
- low-latency start, stop, seek, loop, and retrigger behavior
- accurate playback of play selections, edit selections, regions, markers, slices, loops, and pending edit operations
- target-BPM auditioning where practical
- reliable decoding of ordinary WAV first, with AIFF/AIF added in a later phase
- editing/write support for ordinary WAV first, with AIFF/AIF added in a later phase
- reusable decoded audio, waveform, and peak data where practical
- background preparation of audio data without blocking the GUI thread
- stale-result-safe handoff when selection or edit state changes while work is running
- clear error reporting for unsupported files, decode failures, device failures, metadata write failures, and playback failures
- diagnostic events that make playback, decode, seek, loop, warp, render, and device bugs traceable

Decoded-sample playback should keep ordinary live span edits on the existing
source graph. Its control handoff must be non-blocking and allocation-free on
the audio callback, and each published update must carry span bounds, explicit
seek intent, and metronome beat/cycle/phase state as one coherent transaction.
Rapid or out-of-order retarget commands must converge on the newest complete
request without exposing torn state or forcing avoidable source fades and
rebuilds.

The audio engine should be designed with future extraction in mind. Wavecrate can drive its immediate requirements, but the core engine should avoid unnecessary dependency on Wavecrate-specific UI, metadata, tag, similarity, naming, or source-management concepts.

The practical target is a clean internal boundary today that allows reusable audio-engine components to become their own standalone library later without rewriting the playback, editing, and extraction stack.

## Playback and Audio Device Target

Wavecrate playback should prioritize immediate auditioning over studio-routing complexity.

The initial playback target should support:

- choosing an audio engine/backend where the platform exposes multiple options, such as WASAPI or ASIO on Windows
- choosing an output device where the platform exposes one
- choosing the active playback sample rate where the selected backend/device exposes one
- falling back to the system default output device
- recovering when an output device disappears
- reporting device-open and device-lost failures clearly
- low-latency start and restart for sample browsing
- clean stop behavior when switching files, sources, or closing the application
- loop playback for play selections
- range playback for edit selections and regions
- waveform-position feedback that remains aligned with audible playback

Wavecrate is not a recorder in the current target. Audio input recording, multichannel routing, external sync, MIDI control, plugin routing, and DAW transport sync are out of scope unless the product target is expanded later.

Existing audio-input or recording code should be treated as migration/development scaffolding unless a future product decision explicitly brings recording into scope. It should not drive default UI priorities ahead of source browsing, playback, extraction, editing, tagging, rating, file operations, handoff, and analysis.

Playback settings should not compromise file safety. Audition-time processing such as target-BPM warp, gain preview, fade preview, envelope preview, or range looping should remain preview state until explicitly baked by extraction/export/edit commands.

## Similarity and Discovery Target

Wavecrate should include a similarity engine for finding related sounds by audio character, not only by filename, folder, or manually assigned tags.

The similarity system should remain local, deterministic, cacheable, and optimized for fast sample exploration.

Similarity analysis should aim for Sononym-style perceptual usefulness: sounds that feel musically or sonically related should be discoverable near each other even when filenames and folders are unrelated.

The default engine should use:

- analysis-normalized audio
- stable versioned DSP feature vectors
- L2-normalized descriptor embeddings
- cosine approximate-nearest-neighbor search
- lightweight reranking that balances broad timbral similarity with practical envelope and loudness cues

Runtime model inference is not part of the current target.

The initial similarity system should use non-ML audio descriptors where practical. It should consider features such as:

- spectral shape / timbre
- brightness and darkness
- transient strength and attack shape
- temporal envelope
- rhythmic density or onset pattern
- duration
- loudness / energy profile
- pitch or tonal center where detectable
- BPM or tempo metadata where available
- noise vs tonal balance
- stereo width where practical

Similarity search should balance quality and speed. Expensive analysis may run in the background, but browsing and auditioning must remain responsive.

Similarity data should be stored in the database/index and invalidated when the source file changes.

Similarity records should include descriptor version, source file fingerprint or change token, analysis status, last analysis time, and failure reason where relevant. If descriptor settings or feature versions change, Wavecrate should treat old embeddings as stale and requeue analysis rather than mixing incompatible vectors silently.

Map projection and clustering are secondary exploration aids built from the same persisted embeddings. They should preserve compatibility while using implementation names that reflect the current projection method.

The similarity system should support:

- background audio analysis for feature extraction
- persistent storage of analysis results
- stale-result-safe updates when files change or analysis jobs complete out of order
- finding sample files similar to the selected sample file
- sorting the browser by similarity to a selected reference file
- visual similarity indicators in the sample browser
- filtering the sample list by similarity to a selected reference sound
- combining similarity filters with tags, ratings, folders, search text, metadata, aging/listen state, and file properties
- browsing a starmap as an alternate view of the current browser result set
- clear status for analysis progress, unavailable analysis, stale analysis, and failed analysis

Playback-only unsupported audio files should be excluded from similarity sorting, similarity filters, and starmap projection. They may be visible in `all files` list browsing, but they should not become reference files, similarity results, or map points because those workflows depend on analysis data Wavecrate deliberately does not generate for unsupported formats.

The normal folder/sample list is the primary way to view sample files. Wavecrate should focus first on making the regular source, folder, and sample-list workflow complete, fast, and reliable.

The starmap is a later-version feature, in the same post-core bucket as AIFF/AIF support. It should come after the core list-based browser, editing, tagging, rating, file-management, handoff, and analysis workflows are working well. It should be available behind a tab or view toggle as a secondary way to view the same selected folder/filter/result set.

The starmap should provide an XO-like dotted exploration surface for samples. Similar-sounding samples should appear near each other so the user can scan clusters, audition nearby files, and discover variations quickly.

The map should not be a separate library, source, or search mode. It should reflect the same sample items that would appear in the list browser for the active source, folder selection, search query, and filters. Switching between list view and map view should preserve selection, focus where practical, active filters, active reference sample, audition state, and visible status.

Selecting a sample point in the starmap should select the same sample file in the list browser. Selecting a sample row in the list browser should select or highlight the same sample point in the map when that point is visible in the current map result set. Multi-selection should remain synchronized where practical. The list and map are two projections of the same browser selection state, not independent selection models.

The sample browser should provide Sononym-like similarity workflows. The user should be able to choose a reference sample and quickly find related kicks, hats, loops, textures, impacts, vocals, or other sound types through list-based filters and sorting.

Similarity should assist discovery without becoming opaque. The UI should make it clear when results come from similarity analysis, when analysis is still pending, and when ordinary file, tag, rating, aging, search, or metadata filters are also active.

Similarity should support both management and production use. It should help users clean up libraries, but it should also help music producers quickly audition clusters of related sounds and choose material to drag, copy, export, or otherwise hand off to a DAW.

## Tagging and Metadata Target

Tagging should be fast, flexible, category-aware, and low-friction.

Wavecrate should support:

- adding and removing tags quickly from one or many sample files or regions
- custom tags created directly by the user inside user-extensible categories
- custom tags added to existing user-extensible tag categories
- a dynamic tag library built from tags already used in the library
- autocomplete suggestions based on existing tags
- clear pill visuals for assigned tags
- keyboard-friendly tag entry and removal
- typo-resistant tag workflows where practical
- tag-based filtering in the sample browser
- combination filters across tags, ratings, age, triage states, similarity, folders, search text, and metadata

Tag assignment should be manual and explicit. Scan/indexing should not infer tags from folder names, filenames, or common sample-pack naming conventions. Suggestions may come from the user's existing global tag dictionary while typing, but Wavecrate should not silently apply those suggestions.

Values assigned to the current selection should be visible and pickable in the editor even if they were deleted from the global dictionary. These selection-local values should be clearly distinguishable from normal global suggestions through visual styling such as a different color, outline, icon, or muted/alternate chip style, and using them should not silently recreate them in the global dictionary.

The editor should expose an explicit command to add a selection-local value back to the global dictionary. This should be available from the relevant picker, chip, or context menu where practical.

When multiple files are selected, adding a multi-value tag should add it to every selected file without removing each file's existing tags. Removing a tag from a multi-selection should remove that tag from every selected file that has it and should leave other tags untouched. Prefix, Sound Type, and Tuning/Scale are exceptions because they are single-value: setting one on a multi-selection should replace the previous value for every selected file. Whole-category or whole-tag-set replacement for multi-value categories should happen only through an explicit replace-tags command that makes the destructive metadata scope clear.

The tag library should use the fixed top-level categories described in the domain model. Users should be able to add their own tags to user-extensible categories, but the top-level category structure should remain stable.

For example, if the user starts typing `ki`, the tag input should suggest an existing `kick` tag. Selecting the suggestion should apply the existing tag instead of creating a duplicate spelling variant. If the intended value does not exist, the user should be able to create it immediately; creating it should add it to the global dictionary and apply it in the same undoable user action.

Tagging UI should favor compact, readable pill components. Tags should be visible enough to be useful during scanning, but not so visually heavy that they dominate the sample list or waveform/editor area.

## Display Naming and Disk Rename Target

Wavecrate should make it easy to create a consistently named and organized sample library while avoiding accidental breakage of external file references.

Wavecrate should distinguish between:

- the actual disk filename
- the generated display/database name
- the user-editable label
- the stable internal Wavecrate Sample ID

The generated display name should be based on structured metadata such as:

- Prefix
- Playback Type
- Sound Type
- Character tags
- BPM where known
- Tuning/Scale where known
- user label
- uniqueness suffix

Generated display names should update automatically whenever the structured metadata used by the naming template changes. This includes tag changes, label changes, prefix changes, BPM changes, tuning/scale changes, and template-relevant metadata updates. The browser should reflect the new display name immediately after the metadata transaction succeeds.

Generated display names are derived values, not manually authored names. The durable source of truth is the metadata, naming template, Sample ID, real disk filename, folder location, and any explicit apply-to-disk rename transaction. Wavecrate should persist generated-name cache/index projections so the speed gain is retained after restart, but those projections remain rebuildable derived state rather than user-authored metadata.

The naming template should be configured only through a user-editable config file in the current target. Wavecrate should not provide an in-app naming-template editor, in-app template builder, or normal settings control for editing the template. Wavecrate should load the naming template at launch from the global `.wavecrate` configuration area, falling back to the built-in default only when the config file is missing. Editing the config file while Wavecrate is running does not need to hot-reload the template.

The config-file template syntax should be a string template with named tokens, such as `{prefix}_{playback-type}_{sound-type}_{character}_{bpm}_{label}_{number}`. Supported initial tokens should match the structured metadata fields used by the default naming order. Literal text should be allowed in templates, for example `{prefix}_drum_{sound-type}_{number}`. Unknown tokens should make the template invalid rather than being silently ignored. Wavecrate should own token normalization, separator cleanup, empty-token omission, uniqueness numbering, and validation errors so a user-edited template cannot create malformed generated names.

Literal text in naming templates should not be normalized with tag/label metadata-token rules. It may preserve casing, spaces, and other ordinary filename characters. Template literals must still satisfy normal operating-system filename rules, and Wavecrate should reject invalid template literals at startup even if the user only uses generated display names and never applies them to disk.

Naming templates should produce single filenames only. They must not create paths or subfolders. Path separators such as `/` and `\`, drive prefixes, parent-directory segments, absolute paths, and other path-like template output should make the config invalid at startup.

The `{number}` token should be optional. Users may leave it out of the naming template. If `{number}` is absent and a generated display name or apply-to-disk filename collides within the relevant folder scope, Wavecrate should append or otherwise add a predictable numeric suffix through the normal uniqueness policy. If `{number}` is present, it indicates the preferred location for that suffix.

Naming-template changes are not undoable through the global undo stack because they are config-file changes loaded at launch rather than in-app commands. If the config file exists but contains invalid naming-template syntax, unknown tokens, unsupported structure, or otherwise invalid configuration, Wavecrate should fail startup with a clear user-facing message that identifies the config file path, the invalid setting, and the correction needed where practical. Invalid config should not be silently ignored.

Naming-template changes should update the active view and user-requested operations through lazy recomputation rather than forcing an immediate all-source rebuild. Stale generated-name cache entries are acceptable internally as long as the UI never presents a stale name as current once that row, filter, sort, or apply-to-disk command is evaluated.

Automatic display-name updates are database/UI updates only. They must not change the disk filename unless the user explicitly applies the generated name to disk.

The uniqueness suffix for generated display names may be recomputed automatically. If metadata changes cause a display-name collision inside the same folder, Wavecrate may add or change suffixes. If a same-folder collision disappears, Wavecrate may remove or renumber suffixes. Files in different folders may have the same generated display name without needing suffixes. This is acceptable because generated display names are not stable identifiers.

The default initial naming order is:

```text
[prefix]_[playback-type]_[sound-type]_[character-tags]_[bpm]_[label]_[number]
```

The equivalent config-file string template would be:

```text
{prefix}_{playback-type}_{sound-type}_{character}_{bpm}_{label}_{number}
```

Examples:

```text
wanja_loop_kick_distorted_raw_140_metal-floor_001.wav
wanja_oneshot_hat_bright_noisy_short_017.wav
modular_texture_noise_dark_wide_006.wav
```

Missing metadata should simply be omitted. Empty labels should be treated as missing metadata for generated names, so they should not create placeholder text, doubled separators, or empty name segments. The naming system should not invent misleading metadata.

The generated name should be available as a Wavecrate display mode in the browser. Users should be able to switch between viewing the real disk filename and the generated display/database name.

Generated display names should not include the file extension. They are display stems. File extension should remain available as a separate browser field/column and should only be attached when Wavecrate creates or applies an actual disk filename.

When Wavecrate creates or applies a real disk filename, the extension should match the actual audio file type being written or renamed. Ordinary WAV files created or renamed by Wavecrate should use lowercase `.wav`. Later AIFF/AIF support should use the configured or source-appropriate AIFF/AIF extension. Wavecrate should not preserve a stale or misleading extension when the underlying file type changes through an edit, extraction, conversion, or future format-aware workflow. Manual user renames remain governed by normal operating-system filename rules.

Existing files with manually chosen extension casing, such as `.WAV`, should be left alone during ordinary scanning, browsing, metadata edits, tagging, rating, filtering, and display-name changes. Extension casing should change only when Wavecrate performs a real file write or apply-to-disk rename that owns the resulting filename.

Applying the generated display name to the actual disk filename should be an explicit action. This action may be triggered from a context menu, command, batch operation, or future workflow mode, but it should be understood as a real filesystem rename.

Manual disk rename inside Wavecrate and apply-to-disk rename should both be normal current-session undoable file operations. Undo should rename the file back where practical, restore the previous path/filename state in the database, refresh browser rows and caches that depend on path/name state, and use the shared collision-numbering policy if the original name is no longer available.

Apply-to-disk rename collisions should be checked against real filenames in the destination file's containing folder. Wavecrate should add or change numbered suffixes only when the desired real disk filename already exists in that folder. Files in different folders may use the same disk filename.

Filenames produced by applying generated display names to disk should combine normalized metadata-token values with any literal text from the naming template. Metadata-token values should follow Wavecrate metadata-token rules: lowercase, single-token components, no spaces, and normalized unsupported characters. Literal template text should follow normal operating-system filename rules and may preserve casing, spaces, and other OS-allowed characters. Manual disk rename remains an OS-rule filename edit.

Manual disk rename, generated apply-to-disk rename, and direct label edits should remain separate operations. Manual disk rename changes the real filename only. Direct label edits change metadata only. Applying a generated display name to disk uses the current metadata, including label, to produce a real filename, but it should not rewrite the label just because the disk filename changed.

Accepted/favorite rating lock should not block manual disk rename, generated apply-to-disk rename, label edits, tag edits, metadata edits, file moves, copies, extraction, destructive audio edits, or other non-rating operations. Locking only protects the rating state.

Renaming should:

* preserve the stable internal Wavecrate Sample ID
* avoid collisions through predictable numbering
* handle invalid filename characters
* be undoable during the current session where practical
* be logged clearly
* warn the user where external DAW/project references may break

A fully automatic disk auto-rename mode is not the default target. The safer target is automatic database/display naming plus explicit apply-to-disk rename.

## Physical Organization Target

Wavecrate should rely primarily on real files and folders.

Users organize durable collections by creating folders and moving/copying sample files into those folders. Wavecrate should expose this clearly rather than hiding it behind a central managed library.

Wavecrate should not have a separate project-export, cloud-sync, or proprietary pack model for normal durable collection storage. A collection that needs to persist outside temporary workflow state should be represented as files in a folder. Temporary color collections remain virtual marks for fast triage and short-lived collection workflows.

Wavecrate should support:

* creating folders
* moving files between folders
* moving folders inside sources
* copying files between folders
* copying folders inside sources
* duplicating files
* renaming files
* applying generated display names to disk filenames
* exporting extracted or edited files to chosen folders
* moving rejected files to the configured trash folder
* revealing files in the platform file manager

Moving or renaming a folder inside a source should happen immediately without confirmation, even when the folder contains files hidden by the current visibility mode, as long as the folder is not the configured source root itself. It should be undoable during the current session, preserve Wavecrate metadata for affected files, update source/folder/browser state, and use the shared destination-folder collision-numbering policy when the destination already contains a folder with the same name.

Copying a file or folder inside a source should usually happen immediately without confirmation, as long as the copied folder is not the configured source root itself. If the user copies a folder that contains files hidden by the current visibility mode, Wavecrate should ask whether to include those hidden files in the copy. The prompt should use a simple yes/no choice, should not enumerate hidden-file categories or counts, and should default to no so the copy includes only the files visible in the current view unless the user deliberately includes hidden contents. Copying should be undoable during the current session, copy the real file or folder contents on disk according to the user's hidden-file choice, create new Wavecrate Sample IDs for copied audio files, inherit Wavecrate metadata and workflow marks for the copied files, update source/folder/browser state, and use the shared destination-folder collision-numbering policy when the destination already contains a file or folder with the same name. Copied files are unique files, not second references to the original file identity.

The folder tree should reflect actual folders on disk according to the active visibility mode. By default, the folder tree should show folders that contain supported audio files and should hide folders that contain no supported audio files, with one exception: purely empty folders should remain visible so users can create a folder and drop files into it without enabling another mode. When the user enables the explicit `all files` visibility flag, the folder tree and browser should show all normal indexed files and folders, including unsupported audio files, unsupported non-audio files, and folders that do not contain supported audio.

The `all files` visibility flag should be a temporary browser override, not a persistent user preference. Wavecrate should reset to the default supported-audio view on app restart so ordinary browsing starts from the safer, sample-focused surface.

The `all files` visibility flag should not automatically expose operating-system hidden or system files and folders. Those entries should remain excluded by default even when `all files` is enabled, unless a later diagnostic or advanced setting explicitly expands the target.

When unsupported audio files or non-audio files are visible through `all files`, Wavecrate may offer basic filesystem-management actions such as reveal, rename, move, copy, and remove where those actions are otherwise safe and allowed. Unsupported audio files may also be auditionable as playback-only files when the audio backend can decode them safely. Playback-only should mean audio audition only: no waveform preview, waveform cache, analysis cache, listen-history update, recent-use update, or waveform-derived workflow should be created for unsupported audio. When a playback-only unsupported file is selected, the waveform view should show a clear unsupported-format message instead of a waveform so the user understands why waveform interaction, editing, extraction, analysis, and handoff are unavailable. Other Wavecrate-specific sample features should remain disabled for unsupported or non-audio files, including playback when decoding is unsupported, waveform analysis, destructive audio editing, extraction, tag/rating/label/prefix metadata, temporary color collections, generated display names, embedded Sample ID writes, duplicate analysis, similarity, aging/listen-history workflows, generated display-name application, and handoff workflows that require a supported sample.

Folder badges, source totals, and normal browser counts should match the active visibility mode. In the default supported-audio view, counts should represent visible supported sample files rather than including hidden unsupported or non-audio files. Hidden files should not be added to those counts or surfaced as separate count badges in the default view. However, if the user tries to trash, remove, or otherwise delete a folder that contains files hidden by the current visibility mode, Wavecrate must show a generic warning that hidden files are included in the folder contents and require explicit confirmation before proceeding. The warning should not enumerate hidden-file categories or counts.

## Database, Persistence, and Indexing Target

Wavecrate should use a fast, robust persistence layer for sources, tags, ratings, age/listen history, metadata, persistent generated-name cache projections, labels, prefixes, embedded Sample ID state, analysis state, similarity data, waveform cache state, edit state where needed, and current-session recovery information.

The database and indexing system should support:

* responsive queries for large sample libraries
* fast tag filtering, rating filtering, age filtering, text filtering, metadata filtering, and similarity lookup
* efficient updates when tags, ratings, age state, metadata, names, analysis results, source contents, embedded IDs, or edit state change
* clear schema ownership and migration behavior
* transactional or recovery-safe updates for user-trust surfaces
* consistency between persisted state and UI projection
* diagnostic information for failed writes, stale records, migration issues, metadata embedding failures, and indexing problems

Schema ownership should include explicit base DDL, idempotent migrations, schema contract tests, read-only compatibility coverage where applicable, and versioned migration tests for non-additive changes. A database schema version stamp is an optimization hint, not the only source of truth for structural compatibility.

Tag, rating, aging, naming, and metadata workflows should remain fast as the library grows. Filtering should feel interactive, and persistence should be treated as part of the product performance surface rather than a passive storage detail.

The persistence model should include these core entities:

- Source: configured root folder, scan settings, source database path, source status, last scan state, and source-level diagnostics.
- Folder: real folder path, parent relationship, availability, scan state, and display expansion state where durable.
- Indexed File: path, source ID, file type/classification, availability, filesystem metadata, and diagnostic status for scanned files that are visible to Wavecrate but are not supported Wavecrate samples.
- Sample File: stable Wavecrate Sample ID, source ID, current path, file fingerprint/change token, duration, sample rate, channel layout, bit depth, format, size, availability, supported/editable state, and timestamps.
- Embedded ID State: whether the Sample ID is present, missing, pending automatic embedding, stale, conflicting, duplicate-conflict, failed to write, unsafe to write, or last written successfully.
- Tag Category: fixed category identity, display name, ordering, whether user-extensible, and whether structured rather than free-tagged.
- Tag: category ID, canonical name, aliases where useful, display name, usage count, and deletion/rename state.
- Sample Tag Assignment: sample ID, tag ID, assignment source, and updated timestamp.
- Metadata: label, prefix, BPM, tuning/scale, custom fields where supported, and manual-versus-detected provenance where relevant.
- Rating State: unrated, keep levels, accepted/favorite/locked, trash levels, rejected, trashed, lock state, and rating timestamps.
- Aging/Listen Event: last auditioned time, audition count, recent-use markers, and optional handoff/use events.
- Temporary Color Collection: collection ID, name, color, slot order, and sample assignments.
- Region: source sample ID, time/frame range, label, role, and current validity against source-file changes.
- Waveform Cache: sample ID, source fingerprint, resolution/level, channel summary, cache path or blob reference, status, and version.
- Analysis Result: sample ID, analysis kind, version, status, confidence, input fingerprint, result payload, failure reason, and timestamps.
- Similarity Embedding: sample ID, descriptor version, embedding vector reference, normalized state, ANN index membership, and stale status.
- Duplicate Group: global audio-content fingerprint, member sample IDs across indexed sources, duplicate status, last computed time, and stale status.
- Harvest File: source ID, relative path, size, modified timestamp, optional content hash, workflow state, discovered/seen/touched/done/ignored timestamps, and optional note.
- Harvest Derivation Edge: parent file identity, child file identity, operation type, source range or output duration where relevant, destination source/folder, inherited metadata snapshot, creation timestamp, and tool/version metadata.
- Map Projection: projection version, embedding set/version, coordinates, cluster labels where available, and stale status.
- Undo Transaction: action ID, command kind, user-visible description, affected sample/source paths, recovery files, before/after metadata snapshots, and redo state.
- Recovery File: transaction ID, path, original path, edited path where relevant, checksum/fingerprint, expiry/cleanup state, and failure diagnostics.
- Background Job: job ID, kind, source/sample ID, queued/running/completed/skipped/failed/cancelled status, progress, cancellation token, stale-result token, and failure reason.
- Log/Event Record: timestamp, operation kind, action/job/sample/source IDs, severity, user-facing message key, and diagnostic details.

Relationships should be stable enough that file paths can change without losing sample identity. Path records are important, but the Wavecrate Sample ID is the durable identity for metadata, analysis, waveform, rating, and age state.

Database updates that touch both filesystem state and metadata state should be transaction-oriented. If the filesystem stage succeeds but database registration fails, Wavecrate should either roll back the filesystem stage or surface a recoverable partial-failure state with enough diagnostics to repair it.

## Cache and Storage Lifecycle

Wavecrate may create local caches for generated display names, compact waveform summaries, analysis outputs, similarity indexes, map projections, thumbnails or visual summaries, handoff staging, and undo/recovery. Cache payloads should be stored under the global `.wavecrate` root. Source-local `.wavecrate.db` files should only store cache references, cache status, fingerprints, and invalidation metadata where needed.

Caches should be treated as rebuildable unless explicitly documented otherwise. Generated display-name caches and compact waveform summaries may persist across restarts for speed, but they are still rebuildable projections of source files, metadata, and naming rules. Decoded playback buffers are bounded process-local state, not a persistent per-file library cache. User-authored metadata, ratings, tags, labels, naming-template inputs, source configuration, Sample IDs, and undo state for the current session are not disposable caches.

Compact waveform caches should make repeated browsing immediate where the source identity and cache version still match. Missing, stale, invalid, or evicted waveform data should rebuild through the normal foreground or bounded background loading path without changing durable source readiness.

Supported WAV audition should start through the file-backed playback runtime without waiting for a persistent decoded playback artifact. Foreground selection and playback have priority over waveform preparation and other background cache work. The runtime may retain a bounded in-memory working set for current and recently used audio, but it must not precompute or persist an unbounded decoded copy of the library.

Cache records should include enough version and input identity to decide whether they are valid:

- cache kind
- sample/source ID
- file-byte fingerprint where exact byte-level duplicate detection is useful
- source file fingerprint or change token
- audio-content fingerprint where cache reuse across exact duplicates is allowed
- algorithm/schema version
- audio format/channel assumptions
- relevant audio backend, playback sample-rate, write-format, and resampling assumptions where they affect the cache payload
- creation/update timestamp
- status and failure reason where relevant

Wavecrate should be able to rebuild stale or missing caches without losing user metadata. Cache cleanup should run in the background and should respect active jobs, active playback, active edits, and current-session recovery files.

Cache cleanup should be automatic and bounded when cache size, age, or stale-entry limits are exceeded, with user-configurable limits exposed where practical. Cleanup may delete rebuildable cache payloads, stale projections, obsolete cache versions, and unused handoff staging files, but it must not delete audio files, source databases, user-authored metadata, Sample IDs, ratings, tags, labels, extracted-region history, or current-session recovery data needed for undo/safety.

Cleanup order should prefer stale, obsolete, failed, incompatible-version, and orphaned cache entries first. If more space is needed, Wavecrate should evict least-recently-used rebuildable cache entries while preserving caches for the current selection, active folder, active playback, active edit, and recently used files where practical.

Cache size limits should be global in the current target. Diagnostics may show per-source cache usage so users can understand where cache space is going, but per-source cache quotas are not required unless the product target expands later.

Cache reuse across copied or duplicated files should be based on exact audio identity, not on shared Wavecrate Sample ID. If two files have the same audio-content fingerprint and compatible cache versions/settings, Wavecrate may reuse cache payloads while keeping separate file identity and metadata records. If the audio differs, the copied/extracted/rendered file should build its own caches.

The user should be able to clear rebuildable caches from settings or diagnostics without deleting audio files or durable metadata. Manual clear-cache actions should warn that Wavecrate will need to rebuild waveform, display-name, analysis, similarity, and other rebuildable cache data later, which may make browsing temporarily slower until background work catches up.

## File Operations and Recovery Target

Wavecrate should treat the filesystem, source database, edit state, tags, ratings, names, similarity index, action history, and logs as user-trust surfaces.

File, folder, metadata, rename, and edit operations should:

* operate on real files and folders
* preserve metadata where possible
* avoid silent data loss
* make destructive operations explicit unless YOLO mode is enabled
* use session-local undoable transactions where practical
* use clear recovery paths for partial failure
* keep UI projection and persisted state aligned
* report failures in user-actionable terms
* avoid blocking the GUI while work is planned or executed
* make derived files, exports, overwritten files, renamed files, trashed files, duplicated files, and moved files understandable to the user

Background workers should be cancellable or stale-result-safe when the user changes selection, sources, folders, filters, tags, ratings, names, edit selections, play selections, or map views before work completes.

Similarity data, BPM data, transient data, aging data, waveform data, embedded Sample ID state, and display-name data should be invalidated or refreshed when the underlying file changes. Tag, rating, naming, age, and metadata updates should be persisted predictably and reflected consistently across browser, waveform/editor, filters, and map views.

## Background Job Priority and Cancellation

Wavecrate should prioritize work that keeps the user in flow.

Suggested job priority from highest to lowest:

1. user-visible playback, stop, seek, and device recovery
2. current-selection decode and waveform preparation
3. explicit user commands such as edit render, extraction, export, rename, move, copy, trash, undo, and redo
4. current-folder scan and metadata refresh
5. visible-row waveform or analysis preparation
6. background analysis for transients, silence, similarity, and map projection, plus user-triggered BPM/grid metadata work
7. cache cleanup and maintenance

Long-running jobs should expose progress where useful and should be cancellable or stale-result-safe. Cancelling a job should not leave partially written audio files, partial database state, or misleading UI state. For mutating jobs, cancellation should trigger best-effort rollback of completed items before reporting the final cancelled state.

If the user changes selection quickly, older decode, waveform, analysis, and UI-projection jobs should not overwrite newer state. Stale completion should be logged at debug/trace level where useful but should not be user-visible unless it explains a real failed command.

## Logging and Diagnostics Target

Wavecrate should have detailed logging throughout the application so bugs, stalls, failed operations, and unexpected state changes can be traced.

Logging should cover:

* application startup and shutdown
* source scanning and file discovery
* database reads, writes, migrations, and indexing work
* embedded Sample ID reads, writes, conflicts, and failures
* audio device setup, decoding, playback, seeking, looping, warping, and render failures
* waveform preparation and cache usage
* duplicate fingerprinting and duplicate-group updates
* BPM/grid metadata changes, grid generation, transient detection, silence detection, and analysis failures
* play selection and edit selection state changes where useful
* extraction actions and independent extracted-file creation
* destructive edit creation, rendering, overwrite, and failure recovery
* session undo/redo transaction creation, success, failure, and rollback
* temp recovery file creation, usage, cleanup, and failure
* tag creation, autocomplete, filtering, display naming, disk rename application, and metadata persistence
* rating, aging/listen history, review-state, and trash-workflow changes
* similarity analysis, indexing, sorting, filtering, and later 2D map generation
* temporary color collection changes
* background job lifecycle, cancellation, stale completions, and handoff back to the UI
* file operations such as rename, duplicate, move, copy, trash, restore via undo, collect, reveal, and export
* DAW/external handoff actions
* user-actionable errors and internal diagnostic details

Logs should be structured enough to support debugging. Important operations should include useful context such as source IDs, Wavecrate Sample IDs, region IDs, file paths where safe, job IDs, selection versions, edit versions, action transaction IDs, cache keys, timing information, and failure reasons.

Logging should help answer practical debugging questions: what happened, in what order, on which thread or worker, for which file or source, how long it took, and why it failed. Logging itself must not cause UI stalls, excessive disk writes, or unreadable noise.

Default `INFO` logging should favor bounded aggregate health and convergence summaries over
routine per-transition, per-phase, or per-source success-path events. Detailed structured
source-processing lifecycle events remain available at `DEBUG`, while warnings, failures,
actionable parked states, and aggregate sweep outcomes stay visible at their appropriate
operational levels. Use explicit severity and deterministic aggregation rather than time-based
rate limiting so incident reconstruction remains reliable.

## UI Target

The Wavecrate UI should be compact, stable, and optimized for scanning, auditioning, extracting, editing, naming, tagging, rating, organizing, and using sample files.

The UI should provide:

* tight margins and predictable panel geometry
* resizable sidebars and durable split positions
* folder tree, sample list, waveform/editor, tags, ratings, age indicators, filters, similarity controls, naming controls, color collections, and status surfaces available without excessive navigation
* clear selected, playing, loading, edited, unsaved, failed, analyzing, unreviewed, aged, keep-rated, trash-rated, accepted, rejected, trashed, and unavailable states
* keyboard navigation for common browse, audition, loop, mark, cut, edit, duplicate, rename, tag, rate, collect, triage, filter, copy, and handoff actions
* pointer interactions that show exact positions, play selections, edit selections, ranges, fades, markers, loops, grids, transients, extraction feedback, and edit handles
* compact tag pills and autocomplete controls
* region, marker, loop, grid, transient, BPM, and target-BPM controls where relevant
* fast keep/trash rating controls that do not interrupt auditioning
* aging/listen-history visuals that make neglected or recently used files clear
* generated-name display mode and disk-filename display mode
* deliberate apply-display-name-to-disk controls
* mono-style waveform view by default
* future stereo split-view mode for independent channel inspection and editing
* copy, drag, export, reveal, and DAW handoff controls that are fast and predictable
* similarity controls that can be used from list workflows first and later from the map tab
* concise status surfaces for long-running work
* no marketing-style hero layout, decorative cards, or ornamental whitespace

The waveform/editor should be a primary work surface, not a decorative preview. It should support precise cursor movement, play selection, edit selection, range selection, playhead display, loop display, edit handles, fades, markers, grids, transient cues, extraction success feedback, and clear feedback for destructive edits.

When a sample is selected and its audio or waveform data is still loading, the waveform view itself should act as the progress indicator. The full waveform surface should fill from left to right, or otherwise show clear proportional progress, across the full loading pipeline needed before the waveform is usable. This includes decoding, waveform preparation, cache lookup or generation, and any other required setup for inspection and interaction. The same loading UI should be used when caches are missing, stale, manually cleared, incompatible, or being rebuilt. This should make loading visible without opening a blocking dialog or moving the user's attention away from the editor.

If cached, partial, or early waveform content is available before the full loading pipeline completes, Wavecrate should show that waveform content immediately and draw loading progress as a transparent overlay on top of it. The overlay should preserve waveform readability while making it clear that loading is still in progress.

While the waveform loading overlay is active, waveform interactions should remain disabled. The user should be able to continue navigating the browser and selecting another file, but waveform-specific actions such as play-region selection, edit-region selection, fades, envelope editing, extraction, trimming, and zoom/pan gestures should wait until the full loading pipeline has completed.

Selection-triggered playback should also wait until the full loading pipeline has completed. Wavecrate should not start immediate audition from a partially loaded state just because decode is ready if the waveform/editor is still gated by loading. Once the selected sample is fully loaded, playback should be able to start immediately according to the normal audition behavior.

If exact loading progress is temporarily unavailable, the waveform surface should show an indeterminate loading state until measurable progress exists. Once loading completes, the progress display should be replaced by the actual waveform. Loading feedback must be cancellation-aware and stale-safe: if the user selects another file before the previous waveform finishes loading, Wavecrate should actively cancel the previous decode/waveform task where possible, and late results from the old selection must not overwrite the current view.

Waveform rendering should preserve the current multiband visual style rather than falling back to a plain single-envelope waveform. The waveform should distinguish useful frequency or energy bands so users can visually scan transients, bass-heavy material, noisy textures, tonal material, and quiet sections faster.

Multiband normalization is a file-level visual contract. Overview and viewport-detail summaries for the same audio identity must reuse the same normalization profile so progressive refinement and zoom-level changes do not recolor the waveform or introduce visible band-color flicker. Cache identity and format versioning must invalidate summaries that do not carry the required profile.

Waveform visual quality should improve where practical through antialiasing, cleaner band blending, stable peak rendering, and other polish that makes the waveform easier to read. These improvements must not compromise interaction latency. Zooming, panning, playhead updates, selection dragging, fade-handle dragging, envelope previews, hover feedback, and edit overlays should remain realtime-feeling.

The hard performance target is interaction first: if a visual improvement causes noticeable latency in zoom, scroll, selection, fade handles, or playback-position feedback, the implementation should use cached levels of detail, GPU primitives, simplified preview rendering, or disable that enhancement at the current zoom level rather than making waveform interaction feel slow.

Status bars should stay concise. Long-running operations should report what is happening without monopolizing the interface.

Wavecrate should provide basic tooltips for important controls, modes, handles, buttons, and destructive actions.

Tooltips should help users understand what an action does without interrupting fast workflows.

Tooltips should be user-configurable and can be disabled globally by experienced users who prefer a cleaner interface.

### Application UI Layout and Screens

The main window should be the working application, not a landing page.

The default layout should include:

- Top command bar: source controls, search/filter entry, transport/audition controls, target BPM controls, view toggles, settings access, and visible background-work status.
- Left sidebar: source list, folder tree, and compact source/scan status.
- Center browser: virtualized sample list with columns or compact row fields for filename/display name, tags, rating, age/listen state, BPM, tuning/scale, duration, format/channel state, analysis status, and availability.
- Browser view tabs or toggles: list view as the primary/default browser and starmap as a secondary alternate view of the same current browser result set.
- Waveform/editor panel: large primary waveform surface with playhead, cursor, play selection, edit selection, fades, markers, regions, grid/transient overlays, extraction success feedback, and mode-specific handles.
- Metadata/editor panel: selected sample details, tag editor, label/prefix fields, source/folder information, rating controls, generated-name preview, disk-rename action, analysis state, file details, and mixed-state indicators for multi-selection metadata differences.
- Bottom status bar: concise current action, playback state, selected count, compact interactive scan/job progress bar, warnings, and last user-action result.

The layout should support dense work on a laptop screen while remaining usable on larger displays. Panels should be resizable, durable across sessions, and keyboard reachable.

The sample browser row should show enough information to make scanning fast:

- selected/focused/playing state
- display name or disk filename according to active view mode
- core file columns or compact row fields for name, extension/format, size, and modified time
- compact waveform or duration cue where practical
- Playback Type, Sound Type, Character tags, BPM, Tuning/Scale, Prefix, and label where space allows
- keep/trash/accepted/rejected state
- age/listen state
- waveform cache state where useful
- analysis pending/stale/failed indicators
- exact duplicate indicator where the same audio-content fingerprint appears in more than one indexed file
- missing, unavailable, unsupported, locked, edited, or unsaved indicators

Unsupported and partially supported files should not look like ordinary editable files when they are visible. Unsupported files are hidden from the normal browser by default and should appear only when the user enables `all files` or opens a diagnostic/unsupported-files view. In those views, list rows and map points should distinguish playback-only, unsupported format, unsupported encoding, unsupported non-audio, multichannel-limited, too-long, missing, and failed-analysis states where practical. Warning indicators should be compact but visible enough that users understand why audio-specific commands, including DAW handoff, are disabled while basic filesystem-management commands may remain available.

Exact duplicates should have a compact advisory badge or indicator in the sample list and later map view. Duplicate grouping should be global across all indexed sources but should include only files that currently exist on disk. Missing or unavailable records should not make an otherwise unique available file look duplicated. Source, folder, search, and filter state should still control which duplicate members are visible in the current browser. The user should be able to filter to duplicate files, inspect which available files share the same audio-content fingerprint across sources, reveal their folders, and decide whether to keep, move, tag, or trash them. Duplicate grouping should not automatically delete or merge files.

Duplicate badges should appear on every available member of a duplicate group, not only on copies after a canonical or first-seen file. Duplicate badges should update incrementally as background fingerprinting discovers groups. The UI should not wait for whole-library duplicate analysis to complete before showing warnings. Rows may gain or lose duplicate badges one by one as duplicate state is computed, invalidated, or reconciled. These updates should be smooth and non-blocking, with enough status to explain that duplicate analysis is still running when relevant.

Duplicate groups should not assign a primary, canonical, original, or preferred file automatically. All available duplicate members should be shown as peers. Any decision about which duplicate to keep, move, tag, or trash belongs to the user.

Duplicate grouping should count byte-identical files as duplicates even when Wavecrate metadata differs. Metadata differences such as tags, rating, label, generated display name, or collection marks should remain visible as differences between duplicate files, but they should not suppress the duplicate warning.

Duplicate grouping should also count files with identical decoded audio content as duplicates even when non-audio metadata chunks differ. For WAV files, two files should still be treated as duplicates if they decode to the same audio samples but differ only in RIFF metadata chunks, embedded Sample ID chunks, timestamps, or other non-audio fields.

Near-duplicates belong to similarity search, not exact duplicate warnings. Normalized, gained, trimmed, faded, resampled, downmixed, or otherwise processed variants should not be treated as exact duplicate groups unless their decoded audio content is still identical.

Context menus should exist for sample rows, folders, waveform selections, tag pills, rating controls, generated names, and background-job/status surfaces. Context-menu actions should use the same command model as keyboard shortcuts and toolbar buttons.

Empty states should be actionable. A fresh install should guide the user to add a source folder. An empty source should say that no supported ordinary WAV files were found in the first format target, and should include AIFF/AIF after that later format phase is implemented. If unsupported audio-looking files or other files hidden by the supported-audio view exist, the empty state should offer the `all files` visibility flag rather than implying the folder is truly empty. A filtered-empty browser should make it clear that filters, not the source itself, are hiding supported files.

Settings should be a normal application screen or dialog, not hidden behind config files. The generated-name naming template is the current deliberate exception because it is a launch-loaded advanced config-file setting and should remain config-file only in the current target.

### Accessibility and Keyboard Usability

Wavecrate should be efficient for keyboard-heavy users and usable with standard accessibility expectations.

The UI should provide:

- visible focus indicators
- keyboard access to major panels, lists, waveform controls, menus, settings, and dialogs
- predictable Tab and Shift+Tab traversal
- Escape behavior that cancels transient modes before leaving the current workflow
- sufficient text contrast and non-color-only status indicators
- readable labels or accessible names for icon-only controls
- tooltip text that can also serve as command help where practical
- scalable UI density without clipping text
- no reliance on hover-only affordances for destructive or essential commands

Waveform-specific interactions may require pointer precision for advanced editing, but core workflows such as browse, audition, select, extract, tag, rate, rename, undo, and handoff should remain keyboard reachable.

## Command Model and Default Shortcuts

Wavecrate should have a centralized command model. Keyboard shortcuts, toolbar buttons, context-menu actions, drag handles, clipboard actions, and pointer gestures should dispatch the same product commands where they perform the same logical operation.

Commands should be routed by focus context:

- Text inputs, tag editors, rename fields, search fields, numeric fields, and other editable controls own ordinary typing, caret movement, selection, copy, paste, and cut while focused.
- Undo and redo are global Wavecrate commands, not local text-editor commands. `Ctrl+Z`, `Ctrl+Y`, and `Ctrl+Shift+Z` should route to the global undo/redo stack even when a text or value editor is focused.
- Text and value editor changes should enter the global undo stack when the edit is committed, not on every keystroke.
- Search/filter query changes are browser/query history changes, not global undo transactions. They may have their own local history or clear/recent-query controls, but `Ctrl+Z` should not walk through ordinary search and filter edits before reaching metadata, file, selection, or edit transactions.
- Other global application shortcuts should not fire while a focused text or value editor expects the same key input.
- Browser-focused commands apply to selected sample rows or folders.
- Waveform-focused commands apply to the active waveform, play selection, edit selection, cursor, or active waveform mode.
- Non-modal utility popovers such as job details may own dismissal shortcuts such as Escape, but must pass unrelated application and navigation shortcuts through.
- Modal dialogs and confirmation prompts own Enter, Escape, arrow keys, Tab, and text input until dismissed.

The command layer should expose enabled, disabled, hidden, pending, and destructive states so the UI can present actions consistently across menus, buttons, tooltips, status text, and keyboard handling.

Default shortcuts should be discoverable in tooltips and command menus. They may be user-configurable later, but the initial defaults should be stable enough for fast workflows.

Core default shortcuts:

| Shortcut | Command | Context |
| --- | --- | --- |
| `Space` | Play/pause or restart audition according to the current playback state | Browser or waveform |
| `Enter` | Confirm the current focused action, apply previewed waveform edit where relevant, or open the selected folder/sample action | Focused control, browser, or waveform |
| `Escape` | Cancel active selection/edit/drag/prompt/mode where safe | Global contextual |
| `Up` / `Down` | Move sample selection and audition according to audition settings | Browser |
| `Left` / `Right` | Seek, nudge cursor, or move selection boundary according to waveform focus and modifiers | Waveform |
| `Page Up` / `Page Down` | Move by larger browser or waveform increments | Browser or waveform |
| `Home` / `End` | Jump to start/end of list or audio file according to focus | Browser or waveform |
| `Ctrl+Z` | Undo the latest undoable action | Global |
| `Ctrl+Y` or `Ctrl+Shift+Z` | Redo the latest undone action | Global |
| `Ctrl+Shift+\` | Toggle the transaction inspector | Global debugging |
| `Ctrl+C` | Copy selected files or selected waveform audio | Browser or waveform |
| `Ctrl+V` | Paste copied audio at waveform insertion point or paste files where supported | Waveform or folder target |
| `Ctrl+X` | Cut selected waveform audio where destructive editing is allowed | Waveform |
| `Ctrl+D` | Duplicate selected file or selected waveform audio according to focus | Browser or waveform |
| `Ctrl+S` | Save/apply pending destructive edit state where the current edit mode requires explicit save | Waveform or app |
| `E` | Extract the current play/edit selection into a new audio file | Waveform |
| `Delete` or `Backspace` | Delete/cut selected waveform range, remove selected tag text, or move selected files toward trash according to focus | Contextual |
| `F2` | Rename selected file, folder, label, or editable row | Browser or focused row |
| `Ctrl+F` | Focus search/filter input | Global |
| `Ctrl+L` | Focus source/folder location or active path control where present | Browser |
| `Ctrl+A` | Select all rows or all text according to focus | Browser or text editor |
| `Ctrl+R` | Refresh/rescan the active source or folder | Browser |
| `Ctrl+O` | Add/open a source folder | Global |
| `Ctrl+,` | Open settings/preferences | Global |

Waveform editing shortcuts:

| Shortcut | Command |
| --- | --- |
| `T` | Trim/crop to active edit/play selection |
| `S` | Split at cursor or split selected range according to current selection |
| `M` | Mute/silence active selection |
| `R` | Reverse active selection |
| `N` | Normalize active selection |
| `G` | Open gain adjustment for active selection |
| `F` | Add or edit fade handles for active selection |
| `V` | Toggle volume-envelope editing mode |
| `X` | Toggle silence-extension mode |
| `L` | Toggle loop playback for the active play selection |
| `B` | Set or edit BPM metadata for the current file |

Rating, triage, and color collection shortcuts:

| Shortcut | Command |
| --- | --- |
| `[` | Step the selected files one level down toward trash/rejected |
| `]` | Step the selected files one level up toward keep/accepted |
| `U` | Unlock an accepted/favorite file or clear lock where allowed |
| `1` through `9` | Toggle or assign the selected files to temporary color collections 1 through 9 |
| `0` | Toggle or assign the selected files to temporary color collection 10 |

Number keys should not be used for keep/trash rating. They are reserved for the 10 temporary color collections.

Rating should use step-based keyboard controls by default. `]` moves the selected files one step up the keep ladder, and `[` moves them one step down toward trash/rejected. Keyboard rating must not interrupt playback or force a modal flow during normal auditioning.

Pointer and gesture defaults:

- Single-clicking a sample row selects it and starts auditioning by default when immediate audition is enabled.
- Double-clicking a sample row should open or focus the waveform/editor for that sample.
- Left mouse drag in the waveform creates or adjusts the play selection.
- Right mouse drag in the waveform creates or adjusts the edit selection.
- Dragging selection edges adjusts boundaries with sample-accurate or frame-accurate feedback where practical.
- Dragging inside a selection moves or slides the selection when that interaction is enabled.
- Dragging the extraction handle creates an extracted file through the shared extraction pipeline.
- Dragging selected browser rows or waveform regions outside Wavecrate performs file handoff according to the handoff policy.

Every command should have a clear no-selection behavior. If a command cannot run, Wavecrate should disable it or report a concise status message rather than failing silently.

## Settings and User Preferences

Wavecrate should persist user preferences that affect workflow, safety, and performance.

Settings should include:

- source folder list and per-source scan options
- immediate audition on selection, enabled by default
- playback restart/continue behavior on selection changes
- audio engine/backend, output device, and playback sample rate
- audio write format for edits, extractions, staged handoff files, exports, and processed duplicates
- default target BPM and whether target BPM lock is enabled
- destructive-edit safety mode and YOLO mode state
- tooltip visibility
- display-name versus disk-filename browser mode
- tag suggestion and autocomplete behavior
- silence-detection threshold and leading/trailing trim defaults
- waveform visual density and overlay visibility
- cache locations and cache size limits where exposed
- handoff temp-folder cleanup policy where exposed
- trash folder location
- logging level and diagnostic bundle options
- keyboard shortcut overrides when custom shortcuts are supported

Settings changes should take effect predictably when committed, but they should not be represented as undoable global transactions by default. Normal settings changes should commit immediately because the user explicitly changed the setting. Risky settings such as enabling YOLO mode, changing trash folder location, cache cleanup, audio backend/device, audio write format, and destructive overwrite behavior should require clear confirmation before the setting change is committed.

The generated-name naming template is not a normal runtime setting in the current target. It is a launch-loaded config-file-only setting and is therefore not represented as an undoable in-app settings transaction.

## Error States and User-Facing Messages

Wavecrate should turn failures into concise user-facing states plus detailed logs.

User-facing errors should explain:

- what failed
- which file, folder, source, or action was affected
- whether the user's original audio is safe
- whether partial output was created
- whether retry, rescan, reveal, configure, undo, or cleanup is available

Important error states should include:

- unsupported file format
- supported format but unsupported encoding or channel layout
- file exceeds practical duration limit
- file missing or moved outside Wavecrate
- permission denied
- source folder unavailable
- scan failed or partially failed
- decode failed
- playback device unavailable
- waveform generation failed
- analysis failed or stale
- edit render failed
- edit overwrite failed and rollback succeeded
- edit overwrite failed and rollback failed
- extraction failed before file write
- extraction wrote file but database registration failed
- clipboard or drag handoff failed
- trash folder not configured
- trash move failed
- database busy, locked, corrupt, or migration failed
- metadata embedding failed but database fallback succeeded

Errors should avoid vague wording such as "operation failed" when Wavecrate can report the failing stage. Detailed stage information belongs in logs, but the UI should still describe the practical consequence.

## Performance Target

Wavecrate should handle large sources without freezing or rebuilding unnecessary UI work.

Important performance rules:

* the GUI thread must not perform loading, decoding, cache hydration, filesystem scanning, database/index work, metadata writes, analysis, rendering/export work, cleanup, or other potentially slow operations
* source scanning must stream discoveries to the UI incrementally
* long audio files must remain navigable without loading unnecessary full-resolution UI data at once
* folder and sample views should be virtualized or windowed for large datasets
* sample selection, folder navigation, filtering, sorting, rating, tagging, and transport commands should update UI state immediately and queue any slow work instead of waiting for it
* sample decode and waveform preparation must run in background work
* duplicate fingerprinting, BPM/grid metadata work, transient detection, silence detection, similarity analysis, and later 2D map generation must run in background work
* destructive edit rendering and file overwrite work must avoid blocking the GUI thread
* normalization scans, extraction renders, warp renders, and derived-file exports must run in background work
* database writes, indexing, migrations, metadata embedding, and maintenance work must not block the GUI thread
* stale background completions must not overwrite newer selection, edit, filter, tag, rating, naming, database, aging, or triage state
* playback should reuse already-loaded bytes where possible
* waveform previews should reuse cached analysis and peak data where practical
* tag, rating, metadata, naming, aging, triage, and similarity filters should remain responsive on large libraries through indexing, caching, or incremental computation
* recovery-file management should not block shutdown or normal UI interaction
* repaint requests should match actual visual changes
* GPU waveform overlays should be composited by Radiant, not faked with extra application surfaces
* logging should provide useful traceability without becoming a performance bottleneck

Performance-sensitive paths should have focused tests, diagnostics, examples, benchmarks, or manual validation notes when practical.

## Architecture Boundaries

Wavecrate owns product behavior. Radiant owns reusable GUI capability. The audio engine should move toward reusable audio capability. The persistence layer should provide fast, reliable storage and indexing without leaking database details into every product workflow.

### Change Routing Rules

Route changes by ownership before editing:

* domain workflows, persistence orchestration, and application state belong in `src/`
* default product-specific GUI behavior belongs in `src/native_app.rs` and `src/app_core/**`
* current default-GUI folder/file drag/drop behavior belongs under `src/native_app/sample_library/**`
* generic UI vocabulary re-exported for Wavecrate code belongs in `src/ui_primitives/**`
* native host launch, lifecycle, automation, and shutdown adaptation belongs in `src/native_runtime/**`
* do not route current product drag/drop fixes to `src/app/controller/ui/drag_drop_controller/**` unless a task explicitly names that deprecated compatibility controller
* reusable UI, runtime, layout, widget, input, focus, invalidation, rendering, and test primitives belong in the standalone `../radiant` sibling repository
* runtime compatibility behavior should stay inside runtime/test surfaces rather than leaking into generic Radiant modules

Large import lists are not formatting problems. In short: large import lists are architecture signals. When a Wavecrate GUI module needs broad imports from unrelated UI, domain, runtime, and helper areas, first split the module by responsibility or move reusable GUI behavior into Radiant. Keep imports explicit, avoid wildcard imports outside tests/preludes, and avoid using facade modules as dumping grounds for state, view construction, side effects, and re-exports at the same time. A facade may wire focused modules together, but it should not become the owner of app state shape, widget construction, side effects, and reusable GUI helpers.

Normal Wavecrate view-construction modules should prefer `use radiant::prelude as ui;` and then call `ui::...` builders/types. Low-level custom widget modules may import explicit Radiant subsystem contracts such as widget input/output, paint primitives, layout output, and theme tokens, but a long cross-subsystem list should trigger a boundary review: keep Wavecrate-specific paint/domain mapping app-side, move reusable interaction, layout, focus, invalidation, or rendering behavior into Radiant, and avoid creating Wavecrate-local GUI preludes to hide the dependency shape.

Normal Radiant `.view(...)` projections and Wavecrate view-model construction must borrow host state immutably. Wavecrate should prepare derived sample-browser windows, starmap projections, and similar host-owned caches before initial launch and after the update messages that invalidate them; view construction must not hide durable state transitions or cache mutation.

Cross-crate public facades must make ownership explicit. Prefer named `pub use`
lists when Wavecrate owns the compatibility surface. Wildcard-style re-exports
are allowed only for audited compatibility shims whose module docs name the
owning crate and explain why the Wavecrate facade exists. New cross-crate
wildcard exports should fail the source-quality guardrail until they are either
narrowed or added as a deliberate documented exception.

### Implementation Ownership Map

`src/app/controller/**` owns UI intent handling, controller orchestration, and library workflows that have not yet moved to newer app-core or GUI seams. It should avoid renderer-specific geometry logic and low-level database primitives.

`src/app_core/**` owns host-facing projections, action catalog state, UI bridge projection/invalidation rules, and controller integration used by tests and companion runtime surfaces. It should avoid direct filesystem mutation policy outside the persistence layer and new coupling back into the removed legacy UI boundary.

`src/native_app.rs` owns the default Wavecrate desktop GUI entrypoint and composition of Radiant application, runtime, widget, and GPU-surface APIs for the sample-workstation UI. Its support modules under `src/native_app/**` own Wavecrate-specific product UI behavior and should avoid owning reusable Radiant behavior or reintroducing dependencies on the deprecated legacy UI path.

The target `src/native_app/**` module map is:

* `src/native_app/app/**` owns native-app state shape, messages, update routing, startup-loaded state, and orchestration between product domains. It may connect app chrome, sample-library behavior, waveform behavior, metadata, audio, shell lifecycle, and workflows, but it should not become a rendering dumping ground or hide broad domain APIs behind convenience re-exports.
  * `app/state/waveform/**` owns the native app's waveform display/preview state, selection transaction and playback-marker snapshots, load progress, and cache/warm state. App chrome and audio code consume these stable grouped contracts through `WaveformAppState` rather than defining parallel waveform DTOs.
* `src/native_app/app_chrome/**` owns Wavecrate window composition and product-specific view rendering: top and bottom chrome, toolbar, status bars, center-panel layout, source/folder sidebar placement, sample-list panel placement, waveform panel placement, tag-library panel placement, modals, popovers, context-menu overlays, drag previews, and other visible shell layers. It may adapt product state into Radiant views, but it should keep inputs narrow and should not own source scanning, file mutation, metadata persistence, playback, waveform editing rules, or sample-library command side effects.
* `src/native_app/sample_library/**` owns source, folder, and sample state; source scanning; source watching; file rows; collections; ratings; folder and file commands; drag/drop domain handling; reveal/copy/move/trash workflows; and sample-library context targets. Behavior belongs in this module unless a change is explicitly splitting view code into `app_chrome`.
  * `folder_browser/starmap/**` owns browser-source layout requests, cached layout and preview-warm indexes, similarity-aware result shaping, and browser selection navigation. It must remain independent from the Starmap rendering composition under `app_chrome`.
  * `harvest_tracking/**` owns Harvest identity and family queries, background persistence handoff, derivation reconciliation, destination routing, relationship navigation, target selection, and state mutation. Database and filesystem work must stay in persistence or background-task paths rather than view construction or synchronous UI/update handlers.
* `src/native_app/waveform/**` owns waveform state, audio-derived waveform data, viewport state, play/edit selection state, waveform interaction rules, waveform widget props, waveform cache helpers, and waveform-domain tests. `app_chrome::waveform_panel` may frame and paint this state, but waveform state and editing/audition rules stay in `waveform`.
  * `src/native_app/waveform_edits/**` owns destructive-edit entrypoints, protected/Harvest copy routing, confirmation prompts, background worker handoff, completion publication, and undo/redo restoration. Radiant input behavior stays outside this workflow boundary.
* `src/native_app/metadata/**` owns tag vocabulary, tag categories, tag completion data, metadata assignment rules, display-category projection, metadata persistence, and metadata tests. Chrome may paint tag panels and tag pills, but metadata rules and persistence stay here.
* `src/native_app/audio/**` owns playback, output-device settings, sample loading, cache warming, normalization actions, and audio progress behavior. Chrome may expose controls and status, but audio state transitions and playback policy stay here.
* `src/native_app/workflows/**` owns cross-domain commands that have side effects or meaningful product semantics, such as context-menu command dispatch. A workflow may call sample-library, metadata, audio, waveform, or shell helpers, but view rendering should remain in app chrome.
* `src/native_app/shell/**` owns native-app launch, lifecycle hooks, message dispatch integration, shortcut resolution, logging, and Radiant runtime setup for the Wavecrate app. It should not own product rendering internals or domain mutation policy.
* `src/native_app/ui/**` owns Wavecrate-local UI constants and identifiers that are not reusable Radiant primitives. It should stay small and should not become a second app-chrome or a generic UI facade.
* `../radiant/**` owns generic GUI and runtime capability. If a Wavecrate view needs a reusable menu, overlay, list, drag/drop, focus, shortcut, layout, invalidation, or rendering primitive, add or improve the generic Radiant API with neutral names and keep Wavecrate as the product adapter.

Library-browser views are allowed to be part of app chrome because they are visible regions of the Wavecrate window: the source/folder sidebar, sample-list panel, tag-library panel, and context-menu overlay are chrome composition concerns. Library-browser behavior must remain outside app chrome because it owns product facts and side effects: which sources exist, how folders scan, how samples are selected, how collections and ratings mutate, how file paths are copied or trashed, and how metadata is persisted. Moving a view helper into `app_chrome` should reduce rendering ambiguity; moving command policy or mutable library state into `app_chrome` is a boundary regression.

Import direction should make that ownership visible:

* Acceptable: `app_chrome` imports narrow view models, immutable domain references, message constructors, and Radiant builders to paint visible regions.
* Acceptable: `app` imports both app-chrome and domain modules to route messages and orchestrate state transitions.
* Acceptable: workflow modules import domain modules to execute commands and report status.
* Acceptable: domain modules import `crate::native_app::ui::ids` for stable widget IDs when the ID is part of product interaction state.
* Unacceptable: sample-library, metadata, audio, waveform, or workflow modules import `crate::native_app::app_chrome::*` rendering helpers to perform product behavior.
* Unacceptable: `app_chrome` directly performs source scans, file writes, metadata persistence, playback state transitions, trash movement, or waveform edit mutations instead of sending or routing product messages.
* Unacceptable: root-level native-app modules named after generic widgets or visible regions, such as a broad `context_menu`, `browser`, `library_browser`, or `widgets`, unless the module truly owns an app-wide abstraction and not one product region.
* Unacceptable: Wavecrate-local generic GUI helpers that should be Radiant primitives, especially when names and behavior are domain-neutral.

`src/ui_primitives/**` owns Wavecrate's thin, backend-agnostic UI primitive vocabulary re-export boundary. It should not own widget construction, product state transitions, layout policy, hit testing, input propagation, or rendering orchestration.

`src/native_runtime/**` owns Wavecrate's native host adaptation around Radiant runtime launch, automation snapshots, timing artifacts, and shutdown reporting. It should not own product UI behavior or reusable Radiant widget/runtime primitives.

`src/app/controller/ui/drag_drop_controller/**` owns controller-level drag/drop behavior still exercised by compatibility tests. It is not the default target for current `src/native_app.rs` product drag/drop bugs.

`src/sample_sources/**` owns database schema, read/write APIs, journal-backed file operations, and crash recovery behavior for file and folder mutations. It should avoid UI policy and rendering behavior.

`src/issue_gateway/**` owns issue-reporting DTOs, token storage, and integration boundaries.

`../radiant/**` owns reusable layout primitives, widgets, runtime/backend integration, and reusable runtime/test primitives used by Wavecrate's GUI. It should avoid Wavecrate-specific controller, audio, persistence, or product policy.

### Wavecrate Owns

* source configuration and sample-library policy
* audio-file discovery and supported media rules
* audition behavior and product-level playback policy
* sample extraction workflows and user-facing analysis behavior
* destructive sample editing semantics
* play selection and edit selection product rules
* keep/trash rating behavior
* aging/listen-history behavior
* temporary color collection behavior
* sample metadata, tags, ratings, age states, filters, and custom tag policy
* similarity workflows and user-facing discovery behavior
* display-name generation policy, disk-rename policy, and folder-routing policy
* file operations such as rename, duplicate, move, copy, trash, reveal, collect, export, overwrite, and recovery workflows
* DAW/external handoff policy
* destructive warning and YOLO mode policy
* Wavecrate-specific status wording and command behavior

### Radiant Owns

* layout panels and split panes
* virtualized trees, lists, and detail tables
* text input, editable rows, keyboard focus, and shortcuts
* pill/tag input primitives, autocomplete popovers, and selection chips
* GPU surface overlays and waveform/timeline rendering primitives
* range selection, cursor, marker, playhead, loop, grid, and timeline interaction primitives when they are domain-neutral
* icon buttons, SVG/image resource caching, progress widgets, and status surfaces
* repaint, invalidation, subscriptions, and background-resource ergonomics
* generic map/canvas interaction primitives where useful for the starmap

### Audio Engine Owns

* reusable playback primitives
* decoding and buffering for ordinary WAV first, with AIFF/AIF added in a later phase
* seeking, looping, retriggering, and range playback primitives
* audition-time warp primitives where reusable
* audio data preparation for waveform and edit auditioning
* render-preparation primitives needed for destructive edits, extraction, export, and baked warp output
* mono-style edit application across mono/stereo data
* future stereo split-channel editing primitives where useful
* audio-analysis primitives where they are reusable beyond Wavecrate
* audio-device and playback diagnostics

### Persistence Layer Owns

* durable storage
* indexing
* schema migrations
* transactional or recovery-safe updates
* query performance for tags, metadata, similarity, rating, naming, aging, color collections, and source state
* embedded Sample ID state
* persistence diagnostics

### Undo/Recovery System Owns

* session-local undo/redo history
* action transactions
* recovery file registration
* recovery file cleanup coordination
* undo/redo diagnostics

If Wavecrate has to build a custom UI primitive only because Radiant lacks the right general API, prefer improving Radiant and then migrating Wavecrate back to the generic primitive.

If Wavecrate has to add playback, decoding, looping, warping, destructive rendering, or reusable audio-analysis code only because the audio engine lacks a reusable primitive, prefer improving the audio-engine boundary instead of embedding one-off audio behavior directly into product code.

## Runtime and Data Contracts

Durable runtime, automation, recovery, and data-format contracts belong in this target document with their owning code boundaries.

### Instant Playback Limits for File-Backed WAVs

Wavecrate should make supported WAV audition feel immediate without requiring a persistent decoded copy of every file. Playback uses the source file through the file-backed runtime, while compact waveform summaries remain an independent rebuildable browsing cache. A process restart still discards decoded working buffers, audio-device state, worker-thread state, and operating-system file-cache warmth that Wavecrate does not control.

Wavecrate may persist compact waveform summary data, GPU-friendly preview levels, source-format metadata, file length, modification timestamp, and content identity needed to reject stale preview data. It must not treat a decoded playback sidecar, a playback-summary row, or cache residency as durable source readiness. Cache-budget eviction therefore cannot make an otherwise complete source incomplete or requeue permanent source-processing work.

The playback runtime may keep a bounded in-memory working set for the current and recently auditioned files, use asynchronous file reads, and prefill enough audio to start safely. Foreground audition must outrank source scanning, analysis, similarity, and waveform-cache preparation. No startup lane should warm an unbounded library or serialize decoded PCM for every supported file.

Latency targets should distinguish three states:

* **memory-ready WAV**: once the required working buffer is resident and the output path is open, transport start should remain perceptually immediate, with low-tens-of-milliseconds behavior as the target
* **file-backed WAV on responsive local storage**: opening, validating, and prefilling the selected file should remain near-instant without waiting for whole-file decoding
* **cold, network, removable, busy, unavailable, or corrupt storage**: readiness may take longer and must be represented as foreground loading or a clear playback failure rather than hidden source-processing work

Fallback behavior should keep the GUI responsive. Valid compact waveform data may remain available while playback opens or prefills the source file, but waveform-cache state must not claim that audio is playable. Unsupported or inaccessible files should fail explicitly without holding the source-processing queue open.

### UI Bridge Projection Cache

`src/app_core/ui_bridge/**` owns the retained UI projection model.

Core rules:

* pulls should reuse retained segments when the relevant projection keys match
* invalidation should stay as targeted as correctness allows
* overlay-only waveform edits should avoid unnecessary waveform-image rebuilds
* local-only pull shortcuts are conservative and must never bypass required derived invalidation

Use the bridge metrics helpers instead of ad-hoc logging when profiling:

* `measure_projection_segment_lookup_counts`
* `measure_projection_segment_probe`
* `measure_projection_rebuild_cause_counts`

Behavioral coverage anchor: `src/app_core/ui_bridge/tests.rs`.

### GUI Test Platform

The GUI test platform has two explicitly separated runtime families:

1. product certification for `live`, `isolated-startup`, the legacy `default`
   alias, and the versioned native `empty`, `small-multi-source`, and
   `large-source` fixtures lowers `NativeAppState` through the same Radiant
   runtime bridge, subscriptions, message reducer, and shutdown hook as the
   desktop executable;
2. older named fixtures such as `browser`, `waveform`, and `map` remain
   retained-controller compatibility coverage.

The product-native path owns startup and persistence certification. Named
controller fixtures must never be treated as proof of production startup or
background-worker composition. Architecture guardrails reject attempts to
route live/startup fixtures back through `new_ui_bridge` or controller startup.

The important contract is semantic stability:

* target controls by stable node IDs and action IDs
* prefer semantic automation over screenshot matching when possible
* emit both native and compatibility snapshots through Radiant's backend-neutral
  automation schema
* record the fixture runtime and observable worker composition in artifacts so
  product certification proves one native watcher/readiness supervisor and no
  legacy analysis pool
* keep automated startup on an isolated temporary config base using the
  `automated-tests` profile; only the explicit `live` tag may resolve the live
  profile
* complete native captures through the real shutdown hook and retain its
  machine-readable artifact

Native source-system fixtures use one versioned generator for shell, PowerShell,
and in-process automation. `empty` has no sources; `small-multi-source` is the
routine source-system and visual-QA baseline with two disposable configured
sources; `large-source` is opt-in for high-cardinality validation only. A clean
reset removes the fixture's non-live profile, source databases, caches, WAL
state, and prior mutations, then regenerates exact audio inputs, stable source
ids, and a machine-readable hash/count manifest. Fixture tooling must reject
the live profile and confine every write-capable source below the resolved
fixture config base.

Current development loops:

* semantic contract lane: `scripts/gui.ps1 contract`
* broader suite: `scripts/gui.ps1 suite`

### Run Artifacts

Run outputs should remain machine-readable and deterministic. Artifacts include run manifest metadata, GUI test artifacts, optional bug-bundle outputs, and perf-guard outputs where relevant. When adding a new run artifact, reuse the existing run metadata model instead of inventing a parallel schema.

### File-Operation Recovery

`src/sample_sources/db/file_ops_journal.rs` owns copy/move crash recovery.

Durability rules:

1. write journal intent before assuming filesystem mutation
2. advance journal stages only after each durable boundary completes
3. reconcile by checking filesystem state again instead of trusting the journal blindly
4. prefer data preservation when observed state is ambiguous

### Analysis and Readiness Triggers

`src/app/controller/library/analysis_backfill.rs` owns explicit controller analysis requests, while the durable source-readiness reconciler owns automatic lifecycle catch-up. Sample addition, destructive audio-content edits, user-requested reanalysis, and explicit similarity repair may still request analysis directly through the controller boundary.

Committed scan completion, watcher/auto-sync reconciliation, internal rename/move/edit completion, deferred hash completion, periodic source audit, startup catch-up, and artifact-version changes must publish or refresh desired readiness and wake the coordinator. Discovery-side wakeups occur only after the source transaction publishes its committed revision and structured identity delta. A wakeup is not permission to recompute every artifact: the reconciler compares exact identity, version, and generation state and emits only real deficits. A path-only rename may require best-effort waveform/cache ownership repair outside readiness while leaving content-derived analysis current.

Readiness work ownership is durable across Wavecrate processes. Startup discovery must not reset running jobs. A running job with a lease later than the claim timestamp remains owned; an expired lease is recoverable, and a compatibility row whose running lease is null is treated as abandoned and recoverable. Reclamation runs only through the immediate transactional claim path, increments the claim generation, and preserves source-generation fencing so the original worker cannot renew, complete, or fail the newly owned generation.

Every Wavecrate-owned file operation must finish through one committed-mutation contract after its filesystem work reaches a durable terminal state. Duplicate, extract/copy/harvest, import/drop, destructive edit, normalize, undo/redo, rename, move, trash, and delete outcomes carry one operation ID, the committed source revision, before/after paths, before/after content identities, and create/content-change/path-only/delete semantics. The contract reconciles the source manifest first, refreshes browser projection and metadata ownership from that committed state, acknowledges the matching watcher echo, and only then wakes readiness. Content changes invalidate file-derived stages, path-only moves retain content-derived readiness, and deletes invalidate source membership; the readiness reconciler still decides the exact deficits. Failed, partially failed, and rolled-back operations publish explicit terminal outcomes without asking the watcher to repair application-owned state. Delayed or duplicate completions are fenced by source revision and operation ID.

Similarity browsing, row rendering, source selection, folder activation, status reads, and other UI/read-path resolution must not enqueue work directly. They may observe readiness and raise priority for already-authoritative deficits. When trigger behavior changes, update this contract and route lifecycle call sites through the coordinator rather than adding a hidden enqueue path.

### Folder-Delete Recovery

`src/app/controller/library/source_folders/delete_recovery/**` owns retained folder-delete recovery.

Contract:

* staged deletes move data into an app-owned trash area
* startup recovery restores incomplete deletes conservatively
* fully committed deletes remain recoverable until explicit restore or purge
* explicit restore merges carefully and keeps both copies when content differs
* recovery journal paths are untrusted and must be validated as relative,
  symlink-free, and contained under the source or delete-staging root before
  restore, restage, rollback, retained restore, or purge filesystem effects

### Updater Policy

Updater behavior is intentionally conservative:

* install paths must not traverse unsafe symlink paths
* Windows release installs are manual by design
* development-only overrides belong behind explicit env vars
* updater failures should preserve the installed app rather than forcing risky writes
* a staged update is not committed until every destination swap succeeds; failures
  before that boundary must roll back and remain fatal
* once every swap succeeds, failure to remove disposable `.old` or `.new` siblings is
  a warning-bearing committed outcome and must not prevent the relaunch attempt
* post-commit cleanup diagnostics retain each affected path and error, while a later
  cleanup attempt may act only on `.old` or `.new` siblings reconstructed from
  manifest destinations under a newly validated install root
* transaction-remnant cleanup removes symlinks rather than following them, and the
  next update run retries safe remnant cleanup before staging the same destination
* relaunch failure remains independent and fatal even when the committed update also
  carries cleanup warnings

See `docs/TROUBLESHOOTING.md` and `docs/ENV_VARS.md` for diagnostics and overrides.

### Release Lifecycle

Wavecrate uses continuous development on `main`. All ordinary development,
including stabilization fixes, continues through the normal PR pipeline.

Publishing the first RC for a target version is the explicit boundary that
starts stabilization. It changes the scope of release-train work toward bugs,
regressions, reliability, performance, packaging, and release blockers; it does
not freeze `main` or automatically authorize a stable release. New features
should wait until stabilization ends unless the release scope is explicitly
changed.

The remote `release/X.Y` branch is a persistent coordination ref for the latest
candidate snapshot. When stabilization changes intended for the release land
on `main`, update that ref and publish another RC. A stable release must promote
the exact commit already published and reviewed as the latest RC, never newer
un-RCed release work from `main`.

Stable publication is a separate explicit product decision. PR approval,
`approved`, sign-off, or RC acceptance can authorize merge and cleanup, but none
of them authorize a stable release unless the user explicitly asks to release
or promote the version as stable.

Stable publication is temporarily disabled while Wavecrate remains in RC
stabilization. The promotion implementation stays preserved, but both the
active GitHub workflow and local dispatch entrypoint must fail closed until a
dedicated reviewed change explicitly re-enables them.

### Data-Format Notes

Keep these formats stable unless there is a coordinated migration:

* feature-vector payloads used by the similarity pipeline
* ANN container artifacts used for search/index assets

When either format changes, document the migration at the owning code boundary, update regression or fixture coverage, and avoid version drift between generated assets and the reader.

### Performance Posture

Performance-sensitive work should preserve these expectations:

* the app stays responsive under large-library workloads
* bridge and waveform hot paths remain measurable and testable
* performance checks use the existing wrapper scripts and reproducible datasets instead of one-off local measurements

Historical execution diaries and phase logs belong in `tmp/`, not durable target sections.

## Code Quality and Maintainability Target

Wavecrate should be simple, focused, and maintainable internally. The codebase should make the product direction easier to execute, not harder.

Clean code in Wavecrate means code that a future AI coding agent or human maintainer can read, modify, test, and validate without reconstructing hidden intent from scattered behavior. It should be obvious where a product behavior lives, which module owns a decision, which data is durable, which work is background work, which action is undoable, and which side effects touch audio files, databases, caches, the clipboard, Explorer, or external DAWs.

The codebase should optimize for clarity under change:

* Prefer direct, readable implementations until real repetition or complexity justifies an abstraction.
* Prefer small cohesive modules over large mixed-purpose files.
* Prefer explicit domain vocabulary such as source, sample, selection, extraction, rating, cache, handoff, recovery, and undo transaction over generic names such as item, data, manager, helper, or service when the domain meaning is known.
* Prefer predictable dependency direction: product workflows may call persistence, audio, file-operation, and UI-command layers through clear interfaces, but low-level utilities should not know about high-level Wavecrate workflows.
* Prefer data structures that make invalid states hard to represent, especially for sample identity, rating state, duplicate-ID conflicts, file availability, edit safety, cache state, background-job state, and undo transactions.
* Prefer typed command/result models for user actions that can fail, be undone, affect files, or update multiple state surfaces.
* Prefer one shared implementation path for equivalent user actions. For example, keyboard extraction, internal drag extraction, clipboard handoff, external drag handoff, and context-menu extraction should reuse the same extraction or handoff pipeline where they perform the same logical operation.
* Prefer local reasoning: reading a function or module should reveal its important inputs, outputs, state changes, failure modes, and invariants without requiring a search through unrelated UI code.
* Prefer stable seams for risky behavior: destructive edits, trashing, renames, writes, source scanning, cache cleanup, embedded-ID repair, and external handoff should have clear transaction or job boundaries.
* Prefer behavior-preserving refactors with focused validation over sweeping rewrites that change architecture and behavior at the same time.

Code should be organized around clear responsibilities:

* sample discovery and source scanning
* audio playback and decoding
* waveform preparation and editing
* large-audio-file analysis and extraction
* similarity analysis and indexing
* tags, ratings, aging, display names, metadata, and persistence
* embedded metadata and Sample ID handling
* file operations and recovery
* undo/redo transactions
* UI state and product commands
* logging and diagnostics
* Radiant integration

Files and modules should be small, focused, and organized by responsibility. Each file should have a clear reason to exist. Each module should expose a clean surface and hide internal implementation details where practical. Module boundaries should follow real architecture boundaries, not arbitrary file splitting.

Wavecrate code should follow these standards:

* Keep functions small and single-purpose.
* Keep structs focused on one responsibility.
* Keep traits minimal, meaningful, and justified.
* Name functions after the domain action they perform, not after vague implementation mechanics.
* Avoid ambiguous suffixes such as `manager`, `processor`, `handler`, or `util` unless the type has a genuinely narrow and obvious role.
* Split complex methods into named helpers.
* Separate large impl blocks where it improves clarity.
* Expose only intentional public API.
* Keep internal types internal.
* Make error handling explicit and understandable.
* Preserve useful error context, including source ID, sample ID, path, command, job ID, transaction ID, cache key, or setting name where relevant.
* Keep control flow readable.
* Encode or document important invariants.
* Keep state mutation clear and predictable.
* Make side effects easy to identify.
* Keep filesystem writes, database writes, cache writes, clipboard writes, OS-shell actions, and audio-device actions visible at API boundaries.
* Keep hot-path code simple and efficient.
* Prefer clear composition over broad inheritance-like trait systems.
* Avoid premature abstraction.
* Avoid cleverness where straightforward code is better.
* Prefer explicit data flow.
* Minimize global mutable state.
* Avoid long-lived mutable state that is shared across scanning, playback, editing, and UI unless the ownership and synchronization model is explicit.
* Remove dead code.
* Remove unused experiments unless intentionally preserved and documented.
* Make every abstraction earn its place.
* Avoid large rewrites unless they clearly reduce complexity, unlock important architecture, improve performance, or remove real blockers.

Comments should explain intent, constraints, invariants, and non-obvious tradeoffs. They should not narrate obvious code. A useful comment explains why an operation must be transactional, why a stale-result token is needed, why a cache key includes an audio setting, why an edit cannot run on a multichannel file, or why an external handoff file needs a grace period.

Duplication should be judged pragmatically. Small local duplication is acceptable when it keeps two workflows simple and independent. Duplication is harmful when it lets product rules drift, especially around extraction, naming, collision numbering, destructive-edit safety, undo/redo, embedded Sample ID handling, cache invalidation, rating transitions, trash movement, or external handoff. Shared product rules should live in shared helpers or domain services with tests.

Public APIs should be designed as contracts, not incidental access points. A public function should have a clear caller, a clear reason to exist, and stable behavior. Avoid exposing internal state just to make a UI update easier. When UI code needs behavior, prefer a product command or view model that preserves domain rules rather than letting the UI mutate persistence, cache, or audio state directly.

Background work should be modeled explicitly. Scans, waveform-cache work, waveform loads, renders, duplicate analysis, similarity analysis, metadata writes, and cleanup jobs should have job IDs, progress state, cancellation/stale-result handling, failure reasons, and clear priority rules. The UI should observe job state instead of owning job execution details.

Error paths are first-class product behavior. Code should be written so rollback, partial failure, retry, diagnostics, and user-facing status are handled deliberately rather than as afterthoughts. A failure in an edit, extraction, trash move, rename, source scan, cache rebuild, or handoff should leave the system in a state that is visible, explainable, and recoverable where practical.

Code smells to avoid:

* god files and god objects
* long functions
* deep nesting
* repeated logic
* ambiguous names
* hidden side effects
* unclear ownership
* overly broad traits
* tight coupling between unrelated modules
* product behavior leaking into Radiant
* Radiant implementation details leaking into Wavecrate workflows
* audio-engine internals leaking into unrelated product code
* persistence details leaking into every workflow
* temporary hacks becoming architecture
* stale removed-UI assumptions shaping the current architecture
* UI code duplicating domain rules
* background jobs without cancellation or stale-result protection
* untyped stringly-typed state for important product concepts
* silent fallbacks that hide data loss, failed writes, failed metadata updates, stale caches, or unsupported files
* tests that pass by asserting implementation trivia while real workflows remain unprotected

Wavecrate should be improved incrementally. Refactors should usually preserve working behavior, keep validation lanes intact, and leave the codebase in a clearer state than before. Do not turn quality work into endless renaming; renaming is useful only when it improves API clarity, architectural understanding, or developer experience.

Tests should support refactoring rather than prevent it. Avoid tests that only lock in names, incidental file layout, or implementation details. Prefer tests that protect real workflows, recovery behavior, stale-result safety, persistence correctness, playback behavior, destructive editing behavior, extraction behavior, undo/redo behavior, similarity behavior, and performance-sensitive paths.

When an AI coding agent changes Wavecrate, it should leave behind code that is easier for the next agent to reason about. It should avoid hiding complexity in generic helpers, adding undocumented special cases, or scattering product rules across UI callbacks. If a change introduces a new concept, failure mode, cache state, command, transaction, or background job, the code should name that concept clearly and connect it to the relevant documentation and validation path.

## MVP End-to-End Acceptance Criteria

The minimum complete Wavecrate application should support one end-to-end loop without relying on hidden developer tools or manual database edits.

The MVP should allow a user to:

1. Add a source folder containing WAV files.
2. See folders and supported files appear incrementally while scanning continues.
3. Select files from the browser and hear immediate playback.
4. View a waveform for the selected file.
5. Create and adjust a play selection.
6. Loop-audition the selected range.
7. Create an edit selection or use the play selection as the edit target according to the selection-priority rules.
8. Extract the selected range into a new WAV file in the active folder with collision-safe naming.
9. See the extracted file appear in the browser.
10. Apply at least trim/crop, mute/silence, fade, normalize, and gain adjustment to a supported file with destructive safety and session undo.
11. Duplicate a file before editing.
12. Add fixed-category tags and a label to a sample.
13. Rate samples through keep/trash states.
14. Filter by folder, text, tag, and rating.
15. Generate a display name from structured metadata and deliberately apply it to disk filename.
16. Copy or drag selected whole files to Explorer or a DAW as ordinary audio files.
17. Copy or drag a selected waveform range as an ordinary audio file according to the handoff rules.
18. Move rejected files to a configured trash folder without permanent deletion.
19. Close and reopen Wavecrate with source, metadata, rating, tags, generated names, and listen history preserved.
20. Recover from a failed scan, unsupported file, missing file, failed decode, failed edit, or failed extraction with clear UI status and useful logs.

The MVP does not need AIFF/AIF support, final stereo split editing, final starmap quality, advanced warp quality, cross-platform support, custom shortcut editing, or every planned analysis feature. It should, however, establish the architecture and data contracts needed for those later phases.

## Milestone Strategy

Wavecrate should move toward the target through clear phases. These phases are not rigid release promises, but they help keep implementation work practical.

### Phase 1: Reliable Browser, Playback, Database, and Diagnostics

Establish the core application foundation:

* Windows-first folder/source scanning
* responsive sample browser
* immediate playback
* ordinary WAV reliable decoding, playback, waveform display, and metadata writing
* mono-style waveform display
* stable Wavecrate Sample IDs
* database metadata foundation
* embedded Sample ID writing where practical
* basic tags
* basic keep/trash ratings
* aging/listen-history tracking
* logging and diagnostics
* Windows-first file operations

### Phase 2: Mono-Style Destructive Editing, Selections, Extraction, and Undo

Build the core waveform workflow:

* play selection
* edit selection
* selection-priority rules
* loop auditioning
* mono-style destructive edit commands that preserve stereo files while applying edits equally to both channels
* default warnings and YOLO mode
* duplicate-file workflow
* extraction from selected regions
* extraction success feedback
* session-local undo/redo transaction system
* temp recovery file system and background cleanup

### Phase 3: Library Hygiene, Naming, and Organization

Make the library manageable over time:

* tag categories
* generated display/database names
* disk filename view versus generated-name view
* apply generated name to disk
* untagged filters
* aging filters and sorting
* four-keep/four-trash rating thresholds
* accepted/favorite lock behavior
* configured trash folder workflow
* temporary color collections
* folder organization workflows

### Phase 4: Analysis, BPM, Warp Auditioning, and DAW Handoff

Improve production usefulness:

* manual BPM metadata editing
* region-derived BPM calculation from play-selection/grid workflows
* tempo-grid display
* target BPM lock
* practical beats-style audition-time warping
* extraction/export of warped audio exactly as heard
* drag/copy/export/reveal workflows
* practical Windows-first DAW handoff

### Phase 5: Similarity Search

Build Sononym-style list discovery after the core browser and library workflows are mature:

* deterministic descriptor pipeline
* versioned DSP feature vectors
* persisted descriptor cache
* cosine ANN search
* browser similarity sorting
* visual similarity indicators
* similarity filters
* list-based similar-sample workflows

### Phase 6: Later-Version Format, Map, Stereo, Polish, and Cross-Platform Readiness

Harden and extend the product after the main ordinary-WAV list-based application is complete. This is effectively a later-version bucket, such as a 0.2 or version-2 style target rather than the first complete product slice:

* AIFF/AIF decoding, playback, waveform display, metadata writing, editing, extraction, and handoff where safe
* starmap projection
* map tab as an alternate view of the current browser result set
* cluster exploration
* map/list audition workflow
* stereo split-view waveform display
* independent channel selection where useful
* independent channel editing/extraction where useful
* large-library stress testing
* long-file waveform performance
* background worker reliability
* stale-result safety
* undo/redo robustness
* file reconciliation workflows
* improved diagnostics
* macOS/Linux architecture review
* documentation and validation passes

## Documentation and Validation

Durable product and architecture contracts belong in `docs/`. Planning and backlog state belongs in Linear.

Meaningful changes should usually include:

* a cleanup pass against the touched area and obvious neighboring code before commit, using this target document and `AGENTS.md` as the quality contract
* focused tests for behavior that can regress
* smoke or focused validation before ordinary commit/push
* agent validation before intentional versioned release commits on `main`
* updated docs when a durable contract changes
* Radiant example updates when a new generic GUI API is introduced
* audio-engine validation when playback, decode, seek, loop, warp, render, channel handling, or device behavior changes
* persistence and migration validation when durable storage contracts change
* undo/redo validation when transactional behavior changes
* embedded metadata validation when Sample ID writing changes
* logging and diagnostic validation for important workflows and failure paths
* manual validation notes for workflows that are hard to automate

Important validation lanes should cover:

* browsing responsiveness on large libraries
* supported format import/audition behavior for ordinary WAV files first, and AIFF/AIF files after that later phase is implemented
* mono and stereo file playback correctness
* stereo file preservation during mono-style edits
* long audio file navigation, auditioning, looping, marking, extraction, and export
* immediate audition behavior during fast selection changes
* play selection and edit selection behavior
* waveform precision for playhead, cursor, range, loop, fade, marker, grid, transient, and extraction interactions
* destructive edit warnings and YOLO mode behavior
* session-local undo/redo for edits, file operations, metadata operations, rating operations, play/edit selection changes, and workflow-flag changes, while excluding ordinary transport, browsing, search/filter, and navigation history
* temporary recovery file creation and background cleanup
* stale-result safety for decode, waveform, analysis, edit-render, database, naming, and similarity jobs
* tag creation, autocomplete, duplicate prevention, persistence, generated names, disk rename application, and filtering
* keep/trash rating scale, accepted lock behavior, aging/listen-history behavior, and trash-folder movement
* embedded Sample ID creation, reading, conflict handling, and fallback behavior
* temporary color collection behavior
* target-BPM auditioning and baked warped extraction/export
* similarity list sorting and filtering first, with 2D map interaction after the later map phase is implemented
* file mutation, duplicate, export, overwrite, rename, trash, move, copy, reveal, and partial-failure recovery
* DAW/external handoff workflows
* logging usefulness for tracing failures and background operations

## Completion Criteria

Wavecrate is moving toward the target when:

* browsing remains responsive on large sample sources
* short sample files and long audio files are both handled accurately and smoothly
* ordinary WAV files are fully supported first, and AIFF/AIF files are supported after that later phase is implemented
* MP3, FLAC, and other unsupported formats are clearly ignored, rejected, or reported as unsupported
* mono and stereo files play correctly
* mono-style editing preserves stereo files while applying edits equally to both channels
* sample selection starts playback quickly and reliably
* waveform marks, playhead, cursor, play selections, edit selections, loops, grids, transients, fades, edits, and markers are precise and stable
* long audio files can be navigated, analyzed, auditioned, looped, marked, cut into useful pieces, named, rated, tagged, and exported without leaving the browsing flow
* extracted regions create new sample files while preserving the original file unless the user performs a destructive edit
* extraction creates independent new sample files without durable source-link history
* destructive edits are fast, warning-protected by default, YOLO-mode capable for advanced users, and undoable during the current session where practical
* users can duplicate files before destructive editing when they want backups
* temp recovery files support session undo without slowing down shutdown
* stale recovery files are cleaned up by a background worker
* existing sample libraries can be quickly auditioned, cleaned up, tagged, rated, display-named, physically renamed when desired, moved, collected, and organized without losing flow
* producers can quickly find, audition, compare, copy, drag, export, reveal, or otherwise hand off samples to a DAW without losing creative flow
* target-BPM auditioning helps users hear loops in production context where practical
* extraction/export of warped audition material creates what the user heard
* the audio engine is robust internally and cleanly separated enough to become a standalone library later
* tag, rating, aging, naming, metadata, database, and indexing workflows remain fast on large libraries
* embedded Wavecrate Sample IDs are written where practical and safely fall back to database tracking where not
* long-running work is visible but non-blocking
* similarity analysis helps users find related sounds from both list and map workflows without runtime ML inference
* tags are quick to add, remove, autocomplete, categorize, visualize, and filter by
* keep/trash ratings, aging/listen-history states, accepted locks, and trash-folder movement help keep the library clean over time without unsafe permanent deletion
* generated display names and deliberate disk-renaming behavior are predictable and safe
* temporary color collections help users stage and compare material without replacing physical folder organization
* file operations preserve trust and recovery behavior
* logs make failures and slow operations traceable
* files, modules, functions, structs, and traits stay small, focused, and maintainable
* code smells are actively reduced instead of normalized
* tests protect real workflows without locking in incidental implementation details
* Wavecrate code owns sample-domain decisions
* Radiant code owns reusable GUI/runtime primitives
* reusable audio-engine code is not unnecessarily coupled to Wavecrate product behavior
* the current GUI preserves core workflows without depending on removed UI paths

## Source Refresh Projection Contract

Native source refreshes build the browser from the authoritative scanner traversal and one
coherent committed database snapshot. A routine refresh must not recursively enumerate the same
source a second time to construct browser rows. Full projections are prepared off the UI thread,
published in bounded batches, and replace the visible tree only after the complete snapshot is
ready. Failed or stale hydration retains the last good projection and never replaces the cache
with an empty or partial tree. Exact watcher deltas may update the projection incrementally only
at the next committed revision; stale deltas are ignored and revision gaps recover through a full
authoritative snapshot. The source scan cache is startup assistance, never source authority.
