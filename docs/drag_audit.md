# Drag/Drop Call Site Audit

## `ui/collections_panel.rs`
- **Collection list rows**: hover sets `DragTarget::CollectionsRow(id)` so dropping onto a row is explicit.
- **Collection samples list drag**: while dragging, sends `DragTarget::None` updates just to keep pointer position.
- **Drop zone frame hover**: when pointer is within the drop zone, sets `DragTarget::CollectionsDropZone { collection_id: current_selection }`.

## `ui/sample_browser_panel.rs`
- **Sample row drag / frame hover**: sets `DragTarget::BrowserTriage(column)` whenever the pointer sits over a triage column target.

## `ui/sources_panel.rs`
- **Folder rows**: hovering a row sets `DragTarget::FolderPanel { folder: Some(path) }`.
- **Panel hover without row**: when pointer is inside the panel but not over a row, emits `DragTarget::FolderPanel { folder: None }`.
- **Pointer exits panel**: clears the folders source by sending `DragTarget::None`.

## `ui/waveform_view.rs`
- **Selection handle drag**: uses `DragTarget::None` so drag state retains pointer position while interacting with waveform handles.

## External drag-outs
- When `maybe_launch_external_drag` succeeds (Windows only), the controller records `DragTarget::External` so the resolver knows internal targets are suspended during the OS/DAW drag.
