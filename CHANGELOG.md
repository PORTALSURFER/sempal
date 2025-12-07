## [unreleased]

### 🚀 Features

- Drag drop to daw
- Allow copying selected samples to clipboard as file drops

### 🐛 Bug Fixes

- Can drag our of window now
- Restore external drag paths and auto-scan new sources
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
