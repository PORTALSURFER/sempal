# egui layout design

- Use `eframe` panels to mirror the Slint structure: a top bar (`TopBottomPanel::top`), left sources sidebar (`SidePanel::left`), and a central content area (`CentralPanel`) that stacks waveform and triage lists.
- Maintain dark theme values similar to the current UI (charcoal backgrounds with teal/blue accents) via an app-wide `Visuals` override and shared color constants.
- Top bar: app title, spacer, and a close/quit button aligned to the right. Close should trigger the same shutdown path used for file/system workers.
- Left sources panel (fixed width ~220px):
  - Header row with “Sources” label and a `+` icon button to add a source.
  - Scrollable list of sources; rows highlight on hover/selection and expose a context menu (right-click or small menu button) with “Rescan / Find changes” and “Remove source”.
  - Ensure programmatic scroll-to-index support by tracking row rectangles and requesting `Context::scroll_to_rect`.
- Central panel:
  - Waveform card at the top: texture-backed image fitted to the available width, with overlays for playhead, selection range (draggable handles), and hover line. Loop toggle pill aligned in the header.
  - Triage lists underneath in a 3-column layout (Trash | Samples | Keep). Each column is a scrollable area with compact rows showing an indicator bar, filename, optional tag pill, and selection/loaded highlights. Rows support click-to-select and drag start for folder moves; drag uses a floating `Area` preview following the cursor.
  - Status bar anchored at the bottom of the central stack showing badge + status text.
- Drag-and-drop:
  - Track drag state globally so triage rows can render hover/preview feedback.
  - Drop detection uses pointer position within column/folder rectangles; on drop, invoke tagging/file move handlers without moving the file between triage columns.
- Status & keyboard:
  - Preserve existing shortcuts (Space for play/loop toggle, Ctrl+Space for loop stop/start, arrows for selection navigation/tag stepping, Shift+drag for selection create/clear).
  - Status badge/text rendered in a compact footer with color-coded badge circle.
