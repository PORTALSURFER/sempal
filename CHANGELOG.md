## [unreleased]

### 🐛 Bug Fixes

- *(ui)* Clarify update available message

### ⚙️ Miscellaneous Tasks

- *(changelog)* V0.357.0 (#54)
## [0.355.0] - 2025-12-14

### ⚙️ Miscellaneous Tasks

- *(release)* V0.355.0 (#51)
## [nightly] - 2025-12-14

### 🐛 Bug Fixes

- *(zoom)* Make wheel zoom sensitivity intuitive (#49)
## [0.347.0] - 2025-12-14

### 🚀 Features

- *(hotkeys)* Add M hotkey to mute selection
- *(hotkeys)* Add quote hotkey to tag neutral
- *(hotkeys)* Apply triage tags in collection focus
- *(drag-drop)* Save selection clips in focused folder
- *(drag-drop)* Allow selection drops onto folders
- *(ui)* Highlight folder drops for selection drags

### 🐛 Bug Fixes

- *(windows)* Reveal sample selects correct file
- *(drag-drop)* Cancel selection drop without target
- *(windows)* Avoid losing internal drag on brief pointer leave
- *(windows)* Restore external drag-out while keeping internal drags
- *(collections)* Allow tagging clip-root members
- *(wav)* Tolerate ill-formed headers via rodio fallback
- *(wav)* Sanitize nonstandard fmt chunk sizes
- *(windows)* Restore external drag-out by detecting pointer leave correctly
- *(windows)* Trigger external drag on PointerGone event
- *(windows)* Keep in-app drag active when cursor briefly leaves window
- *(windows)* Preserve drag when leaving and reentering window
- *(windows)* Treat interact_pos as inside during in-window drags
- *(windows)* Detect drag-out using OS cursor position
- *(windows)* Handle Win32 API Result return types
- *(windows)* Clear leave latch when OS cursor re-enters window
- *(windows)* Increase external drag arm delay to avoid accidental launch
- *(windows)* Hide internal drag preview after leaving window
- *(windows)* Cancel in-app drag updates after leaving window
- *(ui)* Start drags even when window unfocused
- *(windows)* Reset in-app drag after external drag ends
- *(windows)* Allow drag-start after external drag
- *(windows)* Correct GetAsyncKeyState bitmask
- *(windows)* Use OS cursor position for drag-start recovery

### 💼 Other

- *(windows)* Enable Win32_UI_WindowsAndMessaging feature

### 🚜 Refactor

- *(tests)* Move controller tests into dedicated modules
- *(controller/wavs)* Extract browser search+label cache helpers into submodule
- *(controller)* Group async jobs and pending state
- *(controller)* Extract tagging service
- *(controller)* Centralize source cache invalidation
- *(controller)* Centralize wav list UI sync
- *(controller/wavs)* Centralize browser sync after entry mutations
- *(controller)* Rely on clear_waveform_view for loaded resets
- *(controller)* Reuse helpers when folder ops mutate entries
- *(controller)* Reuse browser sync helper for selection exports
- *(controller)* Invalidate caches when removing sources
- *(controller)* Reuse clear_waveform_view for collection sample selection
- *(controller/collections)* Extract helpers for sample selection
- *(controller)* Centralize clearing loaded waveform visuals
- *(controller)* Centralize waveform reload for active sample
- *(controller/wavs)* Extract browser actions module
- *(controller/source_folders)* Extract folder actions module
- *(controller/source_folders)* Extract selection and navigation module
- *(controller/source_folders)* Extract folder tree/search module
- *(controller/source_folders)* Split folder selection module
- *(ui/waveform_view)* Extract destructive edit prompt
- *(ui/waveform_view)* Extract selection geometry helpers
- *(ui/waveform_view)* Extract selection context menu
- *(ui/waveform_view)* Extract selection overlay interactions
- *(ui/waveform_view)* Extract hover cursor overlay
- *(ui/waveform_view)* Extract marker/loop/playhead overlays
- *(ui/waveform_view)* Extract base interactions
- *(ui/waveform_view)* Extract base rendering and texture upload
- *(ui/waveform_view)* Extract controls row
- *(controller/playback)* Extract transport and selection ops
- *(controller/playback)* Extract random navigation module
- *(controller/playback)* Extract player and playhead logic
- *(controller/playback)* Extract browser navigation helpers
- *(controller/playback)* Extract tagging and triage helpers
- *(controller/playback)* Extract formatting helpers
- *(controller/wavs)* Extract audio loading module

### 📚 Documentation

- Refresh usage guide for config paths, exports, and hotkeys
- Add refactor strategy for small PR module splits

### ⚙️ Miscellaneous Tasks

- *(release)* V0.347.0 (#40)
## [0.342.0] - 2025-12-13

### ⚙️ Miscellaneous Tasks

- *(release)* V0.342.0 (#30)
## [0.341.0] - 2025-12-13

### ⚙️ Miscellaneous Tasks

- *(release)* V0.341.0 (#28)
## [0.340.0] - 2025-12-13

### 🚀 Features

- *(hotkeys)* Bind brackets to trash/keep samples
- *(ui)* Show app version in status bar
- *(ui)* Unify sample and collection item lists via flat items list component and align selection/focus styling
- *(ui)* Allow toggling selection on collection list rows
- *(ui)* Add soft overlay highlight for multi-selected browser rows
- *(ui)* Move trashed samples in background
- *(hotkeys)* Add 't' to trim waveform selection
- *(waveform)* Add / and \ fade hotkeys; soften fade curve
- *(waveform)* Add 'n' normalize selection or whole sample
- *(waveform)* Add crop hotkeys and non-destructive crop-as-new-sample
- *(collections)* Sync entries from export root folders
- *(undo)* Add 20-step undo/redo with hotkeys
- *(ui)* Make left/right sidebars resizable panels
- *(ui)* Add loop toggle button to waveform view
- *(waveform)* Alt-drag selection handle slides selection
- *(waveform)* Keep source focused on shift-drag selection export
- Add -log to show console in release

### 🐛 Bug Fixes

- *(scanner)* Warn on read_dir entry errors instead of silently flattening
- *(ui)* Left-align numbering in sample browser and collections lists
- *(tests)* Seed loaded_audio so loop-toggle test exercises real path
- *(dragdrop)* Prevent selection-to-collection drop from adding clip to current sample source
- *(dragdrop)* Store selection-to-collection clips in app folder, not source
- *(ui)* Restore browsable source on browser select after collection preview
- *(test)* Avoid deadlocking ConfigBaseGuard during global test config init
- *(windows-clipboard)* Remove bogus GlobalUnlock on lock failure and add RAII for HGLOBAL/locks
- *(windows-clipboard)* Use correct GlobalFree import and release HGLOBAL only after SetClipboardData
- *(audio)* Compute loop progress/remaining with Duration math
- *(waveform)* Correct duration frame math for multichannel wavs
- *(scanner)* Skip symlink dirs and tolerate read errors
- Restore folder drop move for samples
- Finalize drags after UI target update
- *(test)* Isolate config in tests and skip version bump in non-release
- *(tests)* Prevent Instant overflow and stabilize trash-move cancellation
- *(ui)* Stop collection selection from locking scroll
- *(windows)* Suppress hotkey beep by consuming backslash/t text events
- *(hotkeys)* Prevent Windows beep on held keys
- *(collections)* Store selection-drop clips in export dir
- *(windows)* Consume handled hotkeys to prevent system beep
- *(ui)* Color hotkey overlay headings and dedupe actions
- *(windows)* Hide console window in release builds
- Reveal file in Windows Explorer
- Let keyboard cursor override idle mouse hover
- Enable windows console and filesystem APIs
- Enable Win32_Storage for windows crate
- Enable Win32_Security for CreateFileW
- *(waveform)* Avoid stale zoom-cache after edits
- *(ui)* Focus new clip after selection drop
- *(dragdrop)* Update shift behavior during selection drag
- *(ui)* Keep browser selection visible when waveform focused

### 🚜 Refactor

- *(tests)* Split controller tests into focused modules
- *(controller)* Extract browser/waveform/drag-drop/hotkeys/collections sub-controllers behind clear interfaces
- *(controller)* Extract browser/waveform/drag-drop/hotkeys/collections sub-controllers behind clear interfaces
- Drop module-level dead_code allows
- Replace render_list_row args struct
- Cfg-gate windows drag-out paths

### 📚 Documentation

- Add Windows ASIO build note
- Add missing rustdoc on public API
- Update usage guide
- Refresh usage guide

### ⚡ Performance

- *(waveform)* Cache sampled columns per zoom and remove oversampling
- *(decode)* Decimate long wavs into peaks instead of full samples
- *(browser)* Cache fuzzy search scores across rebuilds
- Speed up collection switching
- Avoid source reload when selecting collection items

### 🧪 Testing

- *(app_dirs)* Isolate config home to temp dir during tests
- *(waveform)* Add 24-bit int WAV decode scaling coverage
- *(controller)* Move browser selection integration tests to tests/

### ⚙️ Miscellaneous Tasks

- Add rustfmt and clippy checks with local workflow docs
- *(controller)* Replace guarded unwraps with safer option handling
- Resolve compile warnings
- *(release)* V0.340.0 (#24)
## [0.287.0] - 2025-12-11

### 🚀 Features

- Improve folder browser selection markers and range clicks
- Show selected folders summary below browser
- Support shift+arrow range selection in folder browser
- Add folder browser shortcuts for folder actions and search
- Inline folder rename editing with enter/escape controls
- Clear waveform selection on escape
- Add sticky random navigation mode
- Show visible sample count next to sample browser search bar
- Allow dragging samples into folders
- Refocus browser after tagging filtered samples
- Add ctrl+space playback and idle cursor fadeout
- Add sample browser rename hotkey
- Focus sample browser search with hotkey f
- Start spacebar playback from waveform cursor and clear it on escape
- Keep waveform cursor visible when focused and refine space shortcut
- Add trashed move hotkey
- Focus random sample after filtered tagging in random mode
- Preserve wav extensions during sample rename
- Clear folder selection
- Add folder browser context menu
- Add sticky root entry to folder browser
- Inline folder creation workflow
- Inline folder creation workflow
- Warn when dropping samples without active collection
- Redesign drag/drop targeting
- Right-align sample browser item count label

### 🐛 Bug Fixes

- Stop gs focus hotkey from auto-playing samples
- Keep folder focus on esc and move selection marker left
- Let folder browser fill remaining sidebar space
- Confine selected folders list within sidebar space
- Confine folder sidebar content and slim status bar
- Preserve selection when stopping playback
- Clear last playstart marker when switching samples
- Extend directional fades to sample edges
- Persist folder hover and log folder drag drops
- Clear folder focus when context changes
- Cancel inline renames when focus is lost
- Keep browser and folder focus moving after deletes
- Rerender waveform when audio content changes after edits
- Hide waveform playhead when playback finishes

### 🚜 Refactor

- Split ui, state, and waveform into focused modules

### 📚 Documentation

- Refresh styleguide colors to match app palette
- Convert usage guide for GitHub Pages

### 🧪 Testing

- Fix stuck test

### ⚙️ Miscellaneous Tasks

- *(ui)* Simplify section borders to avoid doubles
- *(ui)* Reduce list row strokes to avoid double borders
- Clean up clippy findings and add todo tracker
- Clear controller and ui clippy warnings
- Remove unused controller methods and tidy plan
- Complete plan for black box migration
- *(review)* Add comprehensive codebase review TODOs
- *(release)* V0.287.0 (#16)
## [0.239.0] - 2025-12-10

### 🚀 Features

- Add fuzzy search to sample browser
- Add shift+space replay from last start marker
- Add random playback history and back hotkey

### 🐛 Bug Fixes

- Consume hotkey events to silence windows beeps
- Cleanup resize handlers
- Harden waveform sampling and render stability

### ⚙️ Miscellaneous Tasks

- *(release)* V0.239.0 (#15)
## [0.230.0] - 2025-12-10

### 🚀 Features

- Color sample labels using triage flags
- Display waveform selection duration label
- Make waveform selection edge drags respond immediately
- Stabilize immediate waveform edge drags
- Migrate config to toml and move collections into sqlite
- Add audio selection support with ASIO
- Add chorded hotkeys and waveform navigation
- Add chorded hotkeys and waveform navigation
- Add focused outline to active panels
- Display key feedback and request initial window focus
- Improved zoom rendering
- Decouple navigation from blocking audio loads
- Add loading animation
- Add audio caching with history and invalidation

### 🐛 Bug Fixes

- Allow selection drops to use active collection fallback without duplicating entries
- Anchor waveform selection start to initial press
- Audio menu dropdown were not working
- Asio was not pickable
- Keep selection edge drags aligned with zoomed viewport
- Derive mouse zoom focus from hover position instead of playhead

### 💼 Other

- Fix zoom
- Improve zoom detail
- Fix: retarget selection hotkeys to consistent edges

### ⚙️ Miscellaneous Tasks

- Unify sempal dirs and add config menu entry
- Outline plan for audio output settings and ASIO support
- *(release)* V0.230.0 (#14)
## [0.189.0] - 2025-12-09

### 🚀 Features

- Add selection normalization with edge fades

### 🐛 Bug Fixes

- Correct edge fade timing and duration math

### ⚙️ Miscellaneous Tasks

- *(release)* V0.189.0 (#11)
## [0.180.0] - 2025-12-08

### 🚀 Features

- Add tracing-based logging with rotation

### 📚 Documentation

- Add animated preview to readme

### ⚙️ Miscellaneous Tasks

- Add collection delete option to context menu
## [0.174.0] - 2025-12-08

### 🚀 Features

- Hide extensions in sample and collection labels
- Highlight missing assets and safeguard waveform/playback
- Bad file read now marked as missing
- Add contextual hotkeys
- Add waveform selection edit menu with crop/trim/fade/mute
- Add source context menu sync and remap actions
- Improve loop playback controls

### 🐛 Bug Fixes

- Embed Windows icon resource and add decoding tests
- Improve waveform rendering accuracy and selection edit tests
- Stop playback when escape is pressed
- Clear selection on waveform click instead of playing when one exists

### 💼 Other

- Feat: accept external folder drops for sample sources

### ⚙️ Miscellaneous Tasks

- *(release)* V0.174.0 (#9)
## [0.153.0] - 2025-12-08

### ⚙️ Miscellaneous Tasks

- *(release)* V0.153.0 (#8)
## [0.151.0] - 2025-12-07

### 🚀 Features

- Drag drop to daw
- Allow copying selected samples to clipboard as file drops

### 🐛 Bug Fixes

- Can drag our of window now
- Restore external drag paths and auto-scan new sources

### ⚙️ Miscellaneous Tasks

- *(release)* V0.151.0 (#7)
## [0.140.0] - 2025-12-07

### 🚀 Features

- Add collection sample context menu actions
- Add triage sample context menu actions and tests
- Draw selection edge brackets with lines instead of glyphs
- Set default fullscreen and add F11 toggle
- Add numbering columns to sample and collection lists
- Add trash management options menu
- Add triage tagging to collection list rows
- Add sample browser multi-selection and focus handling
- Apply browser context actions to multi-selection sets
- Batch triage hotkeys respect multi-selection
- Add selection marker indicator in sample browser
- Add esc hotkey to clear sample browser selection
- Render triage flags as right-edge markers
- Add Windows external drag-out flow for samples and selections

### 🐛 Bug Fixes

- Force waveform reload and list refresh after normalization
- Keep waveform selection drag active when cursor leaves frame
- Enforce fullscreen coverage at startup and smooth F11 toggle
- Force Vulkan backend for eframe startup
- Make F11 toggle window maximization instead of fullscreen
- Draw waveform hover and playhead using line segments
- Enable seekable decoder for audio playback
- Avoid autoplay when tagging samples
- Initialize OLE drag source with default cursors and better cancellation handling

### 🚜 Refactor

- Rename triage UI to sample browser and refresh flags

### 📚 Documentation

- Move setup info to readme

### 🎨 Styling

- Apply rectilinear brutalist theming across egui ui
- Retheme palette to dark hud aesthetic
- Warm desaturated palette with amber-focused accents
- Improve colors

### ⚙️ Miscellaneous Tasks

- Add early-alpha warning to README
- Add emoji to alpha warning in README
- Refine readme
- Rebuild triage list after normalization for browser refresh
- Upgrade dependencies and update egui/rodio integrations
- *(release)* V0.140.0 (#6)
## [0.104.0] - 2025-12-07

### 🚀 Features

- Add collection export workflow and refresh controls
- Add persistent status bar volume slider
- Collapse triage into single filtered list with hue cues
- Improve waveform rendering fidelity

### 🐛 Bug Fixes

- Prune missing sources during config/load to avoid broken database links
- Ensure list autoscroll adds padding so selected rows stay visible
- Expand status bar and keep lists clear
- Clamp triage area height to avoid status bar overlap
- Ensure selection drops target collections reliably

### 🚜 Refactor

- Modularize egui controller/ui and patch playback/drag UX bugs

### 📚 Documentation

- Add README with BuyMeACoffee link
- Add usage guide and sync plan

### ⚙️ Miscellaneous Tasks

- Remove unused top bar
- Adopt CC0 public-domain dedication for licensing
- Set custom app icon
- *(release)* V0.104.0 (#5)
## [0.62.0] - 2025-12-05

### ⚙️ Miscellaneous Tasks

- *(release)* V0.62.0 (#4)
## [0.44.0] - 2025-12-05

### ⚙️ Miscellaneous Tasks

- *(release)* V0.44.0
