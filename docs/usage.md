# Sempal usage guide

## Interface tour
- **Sources (left):** Folders you added; click to load their samples.
- **Waveform viewer (center, top):** Displays the current `.wav`, supports seeking, looping, and range selection.
- **Samples list (center, bottom):** Single-column triage view with filter chips for All/Keep/Trash/Untagged.
- **Collections (right):** Optional sidebar for grouping samples and exporting copies.
- **Status bar (bottom):** Status badge/text and a master volume slider.

## Working with sources
- Click the **+** button in the Sources panel to pick a folder. Sempal opens/creates `.sempal_samples.db` inside that folder and loads its recorded `.wav` entries.
- The first available sample auto-loads; selecting any row updates the waveform and, by default, starts playback.
- New files added outside Sempal must be synced into the source database (the UI does not yet auto-scan the filesystem).

## Browsing, playback, and selections
- Click anywhere on the waveform to seek and play from that point.
- Toggle **Loop on/off** in the waveform header to loop either the current selection (when present) or the full file.
- Create a selection with **Shift + drag** across the waveform. **Shift + click** clears it.
- Drag the selection handle (at the bottom of the shaded region) to resize or to start a drag-and-drop. Dropping the selection onto the Samples list or a Collection saves a trimmed clip (`<name>_sel.wav`, `<name>_sel_2.wav`, …) alongside the source file and optionally tags/adds it to the drop target.

## Triage samples
- Use the filter chips (All/Keep/Trash/Untagged) to control which rows appear. Colors mark keep/trash states; neutral rows remain uncolored.
- Tagging:
  - **Right Arrow:** Tag selected row Keep (Trash → Neutral, others → Keep).
  - **Left Arrow:** Tag selected row Trash (Keep → Neutral, others → Trash).
  - Drag a sample row onto the Samples list to retag it to the active filter’s column (All/Untagged → Neutral).
- Selection movement:
  - **Up/Down Arrows:** Move selection through the visible list; playback follows the selection when enabled.
  - **Ctrl/Cmd + Right/Left:** Cycle the active filter chip.

## Trash management
- Open the **Options** menu in the status bar to choose a trash folder. The choice is saved and reused.
- **Move trashed samples to folder:** After confirmation, moves every sample tagged Trash from all sources into the trash folder (keeping relative names) and removes them from source lists/collections.
- **Take out trash:** After confirmation, permanently deletes everything inside the trash folder.
- **Open trash folder:** Opens the configured trash folder in your OS file explorer.

## Collections
- Click the **+** button to create a collection (enabled by default via feature flags). You’ll be prompted to choose an export folder; if set, Sempal copies members into `<export>/<collection-name>/`.
- Add items:
  - Drag a sample row onto a collection in the sidebar, or onto the Collection items area when a collection is selected.
  - Drag a waveform selection; dropping onto a collection both saves the clip and adds it.
- Managing collections (right-click/long-press a collection row):
  - Set or clear the export folder, refresh exports to reconcile disk vs. list, open the export folder, or rename the collection.
- Selecting a collection item loads and (by default) plays it; the item list shows the source label and relative path.

## Keyboard and mouse shortcuts
- `Space`: Play/pause (respects current selection when available).
- `Up/Down`: Move through visible samples (or collection items when that list has focus).
- `Right Arrow`: Keep (or move Trash → Neutral).
- `Left Arrow`: Trash (or Keep → Neutral).
- `Ctrl/Cmd + Right/Left`: Cycle triage filter chips.
- `Shift + drag` on waveform: Create/adjust a selection; `Shift + click` clears.
- Drag sample rows: Drop into a collection.
- Drag selection handle: Export a trimmed clip to the Samples list or a collection.
