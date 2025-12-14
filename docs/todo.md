DRY / Architecture Improvements (highest leverage)
  - Split EguiController into cohesive sub-structs (e.g. LibraryModel, BrowserModel,
    WaveformModel, Jobs, Caches) and move logic with the data; keep EguiController
    as orchestrator.
    
  - Unify background work plumbing (wav loader, audio loader, scans, trash moves)
    behind a single “job manager” + typed message enum to cut repeated channel/state
    handling.
    
  - Replace broad Result<_, String> error flows in core-ish code with typed thiserror
    enums (keep UI-facing strings at the boundary) to improve context, reduce ad-hoc
    formatting, and make testing easier.

Low-risk Cleanups Found by Clippy

  - Address straightforward refactors: src/egui_app/controller/browser_controller/
    helpers.rs:80 (?), src/egui_app/controller/collections_controller/actions.rs:105
    (collapsible if), and “items after test module” in src/egui_app/controller/
    selection_export.rs:234 + src/egui_app/ui/waveform_view.rs:797 (move helper fns
    above mod tests).


- [ ] Refactor src/egui_app/controller/wavs.rs (~1467 LOC): split into focused
    submodules (browser list/filter/search, row actions, selection/triage ops,
    loading/autoplay) and keep EguiController methods as delegators.
  - [ ] Refactor src/egui_app/controller/source_folders.rs (~1119 LOC): extract
    folder tree model/search, filesystem ops (create/rename/delete), selection/
    navigation, and sync orchestration.
  - [ ] Refactor src/egui_app/ui/waveform_view.rs (~844 LOC): separate rendering,
    interactions (pointer/scroll/zoom), context menus/destructive actions, and
    selection-handle drag logic.
  - [ ] Refactor src/egui_app/controller/playback.rs (~802 LOC): split transport/
    playback, random navigation/history, tagging/undo helpers, and keep public
    behavior stable.

- Some wav files fail loading in our app, but with in others, I see Invalid wav:
  wav: malformed fmt_pcm chunk

- zoom sensitivity settings are a bit weird, its now super high, almost full, ander higher will make zooming slower
lowering the value and zoom starts going really fast. lets make this more intuitive
low values should be slow zoom, high values fast zoom.

- renaming items in the sample browser does not work yet. it will also draw the input text on top of the existing item right now, making it very hard to read what the user is writing
please align it much more with how folder renaming works. an for dryness, lets merge/reuse what we can.

- if I replay samples quickly so we restart while its still playing, I hear clicks, lets fix that so we fade out very quickly right before we restart



##
lets audit the codebase, focus on improving maintainability, improve dryness, more
effecient architectures, etc
