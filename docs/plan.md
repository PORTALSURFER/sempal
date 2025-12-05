Goal
Let users browse and judge samples faster by highlighting the selected/loaded wav, navigating the list with arrow keys, and marking items as “keep” or “trash” via keyboard gestures.

Proposed solutions
- Add explicit selection/loaded state to wav entries and surface it in the Slint list styling so the current/loaded sample is visually distinct.
- Extend the custom winit keyboard handling to move selection up/down through the wav list, reusing existing `DropHandler` state and avoiding conflicts with playback shortcuts.
- Introduce per-sample “keep”/“trash” tagging stored alongside wav metadata (e.g., in the source DB) and render the status as a badge/icon in the list.
- Map left/right arrow keys to toggle keep/trash/neutral states for the current selection, updating persistence and UI cues immediately.
- Provide minimal tests around tag persistence and selection navigation logic, and add a manual QA pass to confirm keyboard flows and highlighting.

Step-by-step plan
1. [-] Review the current wav selection/loading flow in `DropHandler` and the Slint list to define where selection state should live and how it interacts with playback/loading.
2. [-] Add selection/loaded state to wav rows and update the Slint UI to style the active/loaded sample distinctly without regressing existing list rendering.
3. [-] Implement keyboard navigation (up/down) in the custom winit handler to change the current wav selection, clamping bounds and updating UI state.
4. [-] Add keep/trash tagging: persist tags with each wav (via the source DB), expose left/right key bindings to cycle tags, and render badges/icons in the list.
5. [-] Write/extend tests for tag persistence and navigation helpers, then perform manual QA for keyboard browsing, visual highlights, and keep/trash toggles.
