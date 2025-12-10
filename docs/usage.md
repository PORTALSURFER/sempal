# Sempal usage guide

## Layout at a glance
- **Sources (left):** Add sample folders, rescan, remap, or remove them; missing sources are flagged.
- **Center:** Waveform viewer (seek, loop, selection editing) above the Sample browser triage list (All/Keep/Trash/Untagged with numbered rows and keep/trash markers).
- **Collections (right):** Manage collections, export folders, and per-collection items; missing export paths or files are highlighted.
- **Status bar (bottom):** Status badge/text, Options menu for trash actions, and a persistent volume slider.

## Configuration & storage
- App settings live in `~/.config/.sempal/config.toml` (respects the platform config directory); sources and collections are stored in `library.db` inside the same folder, and legacy `config.json` files are migrated automatically.
- Per-source sample metadata stays beside each source as `.sempal_samples.db`, while logs remain under `.sempal/logs` in the user config directory to avoid temporary paths.

## Add and manage sources
- Click **+** in the Sources panel or drop a folder onto it. Sempal creates/uses `.sempal_samples.db` inside that folder and loads its `.wav` entries; the first available row auto-loads.
- Right-click a source row for **Quick sync**, **Hard sync (full rescan)**, **Remap source...**, or **Remove source**. New files added outside Sempal require a sync.
- Selecting any row loads the waveform and, by default, starts playback. Missing sources are prefixed with `!`.

## Browse and triage samples
- Use filter chips (All/Keep/Trash/Untagged) to change the visible list. Rows show number columns and right-edge markers for Keep/Trash; missing files are prefixed with `!`.
- Type in the browser search box to fuzzy-match sample names within the current filter; clear the query to restore the full list.
- Click focuses a row and clears any existing selection; **Shift + click** extends the selection; **Ctrl/Cmd + click** toggles multi-select while keeping the focused row in the set. **Up/Down** moves the focus; **Shift + Up/Down** extends the selection.
- Tagging: **Right Arrow** -> Keep (Trash -> Neutral, others -> Keep). **Left Arrow** -> Trash (Keep -> Neutral, others -> Trash). **Ctrl/Cmd + Right/Left** moves the selection into the next/previous triage column.
- Context menu on a sample row: Tag Keep/Neutral/Trash, **Normalize (overwrite)**, **Rename**, or **Delete file**. Actions apply to the focused row or the current multi-selection.
- **Ctrl/Cmd + C** copies the focused/selected samples to the system clipboard as file drops (for DAWs/file managers).
- Dragging a sample row into the browser area retags it to the active filter's column (All/Untagged -> Neutral).

## Playback and waveform editing
- **Space** toggles play/pause. **Escape** stops playback and clears browser selection. Click the waveform to seek; clicking while a selection exists clears it.
- **Loop on/off** in the waveform header loops the current selection when present, otherwise the full file. A loop bar shows the active region; the playhead is drawn over the waveform.
- Drag across the waveform to create a selection; drag the edge brackets to resize. The handle at the bottom of the selection supports drag-and-drop.
- Right-click the selection for destructive edits (overwrite the source file): **Crop to selection**, **Trim selection out**, **Fade to null** (left->right or right->left), **Mute selection**, or **Normalize selection** (adds 5 ms edge fades).
- Destructive edits prompt for confirmation before overwriting audio; enable **Yolo mode** in **Options** to apply them without prompting.
- Drag the selection handle:
  - Onto the Sample browser to save a trimmed clip next to the source (`<name>_sel.wav`, `<name>_sel_2.wav`, ...) using the current filter as the tag.
  - Onto a Collection to save the clip and add it there (exports into the collection's folder when configured).
  - On Windows, dragging outside the window exports the clip and starts an external drag so you can drop into a DAW/file manager.

## Collections and exports
- Click **+** (feature-flag enabled by default) to create a collection. Set an export folder to copy members into `<export>/<collection-name>/`; rows show `!` when missing or when no export folder is set.
- Add items by dragging sample rows or waveform selections onto a collection or its items area. Duplicate targets are highlighted while dragging.
- Selecting a collection item loads it in the waveform; triage markers appear beside items.
- Collection row menu: Set/Clear export folder, **Refresh export**, **Open export folder**, **Rename**, **Delete collection**.
- Collection item menu: Tag Keep/Neutral/Trash, **Normalize (overwrite)**, **Rename**, **Delete from collection**.

## Trash and cleanup
- Open **Options** in the status bar to set or open the trash folder.
- **Move trashed samples to folder:** Moves all samples tagged Trash from every source into the trash folder (keeps relative paths) and removes them from lists/collections.
- **Take out trash:** Permanently deletes everything inside the trash folder.

## Drag, drop, and clipboard tips
- Drop folders onto the Sources panel to add them.
- Drag sample rows to collections or back into the browser (for retagging) without using menus.
- Drag selections or samples outside the window on Windows to start an external drag-out. Use **Ctrl/Cmd + C** to copy selections or rows as file drops.

## Hotkeys (focus-aware)
- Global: `Space` play/pause; `Esc` stop/clear selection; `L` toggle loop; `Ctrl/Cmd + /` toggle hotkey overlay; `F11` toggle maximized window.
- Sample browser focus: `Up/Down` move; `Shift + Up/Down` extend selection; `Right Arrow` Keep; `Left Arrow` Trash; `Ctrl/Cmd + Right/Left` move selection across triage columns; `X` toggle selection; `N` normalize (overwrite); `D` delete; `C` add focused sample to the selected collection.
- Collection item focus: `Up/Down` move; `N` normalize (overwrite collection copy); `D` delete from collection.
