# egui layout design

- Use `eframe` panels to mirror the Slint structure: a top bar (`TopBottomPanel::top`), left sources sidebar (`SidePanel::left`), right collections sidebar (`SidePanel::right`), and a central content area (`CentralPanel`) that stacks waveform and triage lists.
- Maintain dark theme values similar to the current UI (charcoal backgrounds with teal/blue accents) via an app-wide `Visuals` override and shared color constants.
- Top bar: app title, spacer, and a close/quit button aligned to the right. Close should trigger the same shutdown path used for file/system workers.
- Left sources panel (fixed width ~220px):
  - Header row with “Sources” label and a `+` icon button to add a source.
  - Scrollable list of sources; rows highlight on hover/selection and expose a context menu (right-click or small menu button) with “Rescan / Find changes” and “Remove source”.
  - Ensure programmatic scroll-to-index support by tracking row rectangles and requesting `Context::scroll_to_rect`.
- Central panel:
  - Waveform card at the top: texture-backed image fitted to the available width, with overlays for playhead, selection range (draggable handles), and hover line. Loop toggle pill aligned in the header.
  - Triage lists underneath in a 3-column layout (Trash | Samples | Keep). Each column is a scrollable area with compact rows showing an indicator bar, filename, optional tag pill, and selection/loaded highlights. Rows support click-to-select and drag start for collection drops; drag uses a floating `Area` preview following the cursor.
  - Status bar anchored at the bottom of the central stack showing badge + status text.
- Right collections panel (fixed width ~260px):
  - Header with “Collections” label and `+` button (disabled when feature flag off).
  - Scrollable list of collections showing name and count; clicking selects, and hovering while dragging shows drop affordances. Supports dropping onto a row or the dedicated drop zone below.
  - Drop zone card: changes accent color when ready/hovered; dropping adds the dragged sample to the selected collection.
  - Collection members list: scrollable table showing source label and relative path.
- Drag-and-drop:
  - Track drag state globally so both triage rows and collections can render hover/preview feedback.
  - Drop detection uses pointer position within column/collection rectangles; on drop, invoke tagging/collection handlers without moving the file between triage columns.
- Status & keyboard:
  - Preserve existing shortcuts (Space for play/loop toggle, Ctrl+Space for loop stop/start, arrows for selection navigation/tag stepping, Shift+drag for selection create/clear).
  - Status badge/text rendered in a compact footer with color-coded badge circle.

# Anchor label UX flow (training-free)

## Entry points
- Browser row context menu:
  - "Create label from sample" opens label creation dialog with the sample pre-attached as the first anchor.
  - "Add as anchor to..." lists existing labels and attaches the focused sample.
  - "Manage TF labels" opens the label editor panel.
- Sample browser filter bar:
  - "TF labels" opens the label editor panel.
- Map point (future):
  - Right-click point: "Add as anchor to..." and "Create label from selection" (if lasso exists).

## Flow: create label
1. User selects a sample and picks "Create label from sample."
2. Dialog collects label name, threshold, gap, topK; seed with model defaults.
3. On confirm:
   - Create label row.
   - Add anchor (the selected sample).
4. UI shows success toast + updates label list.

## Flow: add anchors
1. User selects one or more samples.
2. Context menu -> "Add as anchor to..." -> choose label.
3. Anchors added/updated (weight default 1.0).
4. UI refreshes anchor list for that label.

## Flow: review matches
1. User opens "Training-free labels" panel.
2. Select a label -> "Find matches" (ANN-backed).
3. Display ranked matches with scores, bucket coloring, and anchor count.
4. Allow refresh to re-run scoring (for updated anchors).

## Flow: auto-tag (optional)
1. After review, user clicks "Auto-tag high confidence."
2. System assigns label to samples with:
   - score >= threshold AND gap >= gap
   - bucket == High
3. Show summary: tagged count, skipped count, conflicts.

## UI states
- Empty:
  - No labels: show "No training-free labels yet" call-to-action.
  - Label has no anchors: show "Add anchors to enable matching."
- Loading:
  - Show inline spinner and "Searching matches..." when ANN query runs.
- Results:
  - Top matches list with score and bucket; highlight anchors.
- Error:
  - Non-blocking status toast with error details.
