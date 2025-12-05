**Goal**
- Ensure the item lists (sample sources and wav list) keep the selected item visible by auto-scrolling to it when selection changes.

**Proposed solutions**
- Track selection changes in Rust controller and ask the Slint `ListView` to scroll the current item into view (e.g., via `ensure_item_visible` / `viewport-y` bindings).
- Expose helper callbacks or properties in the UI to request scrolling when `selected_source` or `selected_wav` updates.
- Add small unit tests around selection navigation logic (where feasible) to avoid regressions when moving with arrow keys.

**Step-by-step plan**
1. [x] Inspect current list view components and available Slint APIs to trigger scrolling for selected rows.
2. [x] Wire selection change events (mouse click, keyboard navigation, load) to request keeping the selected row visible in both source and wav lists.
3. [x] Validate behaviour manually (describe steps for the user) and outline any code paths needing tests or follow-up.
