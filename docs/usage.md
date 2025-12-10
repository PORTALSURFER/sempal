---
layout: default
title: Usage
permalink: /usage
description: How to set up Sempal, triage samples, edit waveforms, and manage collections.
---

# Sempal Usage Guide

* TOC
{:toc}

## Quick start
- Add a source folder with **+** or by dropping it onto the Sources panel; the first `.wav` row auto-loads and starts playback.
- Drag on the waveform to create a selection, then right-click it for destructive edits (crop, trim, fades, mute, normalize).
- Drag the selection handle onto the browser or a collection to export a trimmed clip next to the source or into the collection.
- Use filter chips (All/Keep/Trash/Untagged) and arrow-key tagging to triage quickly; `Space` toggles play/pause, `Esc` clears selections.

## Layout at a glance
- **Sources (left):** Add, rescan, remap, or remove sample folders; missing sources show `!`.
- **Center:** Waveform viewer (seek, loop, selection editing) above the Sample browser triage list (All/Keep/Trash/Untagged with numbered rows and keep/trash markers).
- **Collections (right):** Manage collections, export folders, and per-collection items; missing export paths or files are highlighted.
- **Status bar (bottom):** Status badge/text, Options menu for trash actions, and a persistent volume slider.

## Configuration and storage
- App settings live in `~/.config/.sempal/config.toml` (platform config dir); sources and collections are stored in `library.db` in the same folder. Legacy `config.json` files migrate automatically.
- Each source keeps `.sempal_samples.db` beside the audio. Logs live under `.sempal/logs` in the config directory (not temp).

## Manage sources
- Click **+** or drop a folder to add. Sempal creates/uses `.sempal_samples.db` and loads `.wav` entries.
- Right-click a source row: **Quick sync**, **Hard sync (full rescan)**, **Remap source...**, **Remove source**. Add new files outside Sempal? Run a sync.
- Selecting any row loads the waveform and (by default) starts playback. Missing sources are prefixed with `!`.

## Browse and triage
- Filter chips (All/Keep/Trash/Untagged) change the visible list. Rows show number columns and right-edge keep/trash markers; missing files show `!`.
- Search box performs fuzzy matching within the current filter; clear to restore the full list.
- Selection basics: click to focus; **Shift + click** extends; **Ctrl/Cmd + click** toggles multi-select while keeping focus. **Up/Down** moves focus; **Shift + Up/Down** extends. Toggle **Alt + R** to lock random navigation so **Up/Down** jump to random samples (Up replays the last pick).
- Tagging: **Right Arrow** → Keep (Trash → Neutral, others → Keep). **Left Arrow** → Trash (Keep → Neutral, others → Trash). **Ctrl/Cmd + Right/Left** moves the selection across triage columns.
- Row context menu: Tag Keep/Neutral/Trash, **Normalize (overwrite)**, **Rename**, **Delete file**. Applies to the focused row or multi-select.
- **Ctrl/Cmd + C** copies focused/selected samples to the clipboard as file drops (for DAWs/file managers). Dragging a row into the browser retags it to the active filter (All/Untagged → Neutral).

## Playback and waveform editing
- **Space** toggles play/pause. **Esc** stops playback and clears browser + waveform selection. Click waveform to seek; clicking while a selection exists clears it.
- **Loop on/off** uses the selection when present, otherwise the full file; a loop bar shows the active region and the playhead.
- Drag to create a selection; drag edge brackets to resize. The bottom handle supports drag-and-drop.
- Right-click a selection for destructive edits (overwrites source): **Crop to selection**, **Trim selection out**, **Fade to null** (L→R or R→L), **Mute selection**, **Normalize selection** (adds 5 ms edge fades).
- Destructive edits prompt for confirmation; enable **Yolo mode** in **Options** to apply without prompting.
- Drag the selection handle:
  - Onto the Sample browser to save a trimmed clip beside the source (`<name>_sel.wav`, `<name>_sel_2.wav`, ...), tagged by the current filter.
  - Onto a Collection to save and add the clip there (exports into the collection folder when configured).
  - On Windows, dragging outside the window exports and starts an external drag for DAWs/file managers.

## Collections and exports
- Click **+** (feature-flag on by default) to create a collection. Set an export folder to copy members into `<export>/<collection-name>/`; rows show `!` when missing or when no export folder is set.
- Add items by dragging sample rows or waveform selections onto a collection or its items area; duplicates highlight while dragging.
- Selecting a collection item loads it in the waveform; triage markers appear beside items.
- Collection row menu: Set/Clear export folder, **Refresh export**, **Open export folder**, **Rename**, **Delete collection**.
- Collection item menu: Tag Keep/Neutral/Trash, **Normalize (overwrite)**, **Rename**, **Delete from collection**.

## Trash and cleanup
- Open **Options** in the status bar to set or open the trash folder.
- **Move trashed samples to folder:** Moves all Trash-tagged samples from every source into the trash folder (keeps relative paths) and removes them from lists/collections.
- **Take out trash:** Permanently deletes everything inside the trash folder.

## Drag, drop, and clipboard tips
- Drop folders onto the Sources panel to add them.
- Drag sample rows to collections or back into the browser (for retagging) without menus.
- Drag selections or samples outside the window on Windows to start an external drag-out. Use **Ctrl/Cmd + C** to copy selections or rows as file drops.

## Hotkeys (focus-aware)
- **Global:** `Space` play/pause; `Esc` stop/clear selections (including waveform); `L` toggle loop; `Shift + R` play a random visible sample and auto-play; `Alt + R` toggle sticky random navigation; `Ctrl/Cmd + Shift + R` step backward through random history; `Ctrl/Cmd + /` toggle hotkey overlay; `F11` toggle maximized window.
- **Sample browser focus:** `Up/Down` move (or jump randomly when sticky mode is on); `Shift + Up/Down` extend; `Right Arrow` Keep; `Left Arrow` Trash; `Ctrl/Cmd + Right/Left` move across triage columns; `X` toggle selection; `N` normalize (overwrite); `D` delete; `C` add focused sample to the selected collection.
- **Collection item focus:** `Up/Down` move; `N` normalize (overwrite collection copy); `D` delete from collection.
