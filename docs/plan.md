## Goal
- Add an options menu to manage a user-chosen trash folder: let users pick the folder, move all samples tagged as Trash into it after a warning, optionally hard-delete everything in the trash folder after a warning, and open the trash folder in the OS file explorer.

## Proposed solutions
- Introduce a persisted, optional trash folder path in the app config with normalization and backward compatibility for existing configs.
- Add controller helpers to pick a trash folder, open it via the OS, and batch operations that move tagged Trash samples into the folder with status updates and confirmation gating.
- Walk cached wav lists or source databases to find trashed samples, move them safely (preserving names, handling collisions/missing files), and keep databases, caches, and collections consistent after moves.
- Provide a “take out trash” action that wipes the configured trash folder contents with a confirmation prompt and resilient error handling.
- Surface these controls in a new options menu consistent with the existing egui chrome/status patterns, reusing dialogs/tooltips for warnings and status feedback, and update docs/tests to cover the flows.

## Step-by-step plan
1. [-] Decide where the options menu lives in the current egui layout and the confirmation UX for destructive actions, keeping the existing chrome/status style.
2. [-] Extend configuration models and load/save paths with an optional trash folder path, ensuring old configs still load and paths are normalized.
3. [-] Implement controller logic to pick/open the trash folder and to move all Trash-tagged samples into it after confirmation, updating caches, databases, and collections to avoid stale entries.
4. [-] Add a “take out trash” action with confirmation that deletes files inside the configured trash folder and handles missing/invalid folders gracefully.
5. [-] Wire the options menu UI to the new actions (pick folder, trash tagged files, take out trash, open folder) and surface warnings/status messaging.
6. [-] Add tests for config persistence and trash move/delete flows with temp dirs, and refresh user-facing docs to describe the new options.

## Code Style & Architecture Rules Reminder
- Keep files under 400 lines; split when necessary.
- When functions require more than 5 arguments, group related values into a struct.
- Each module must have one clear responsibility; split when responsibilities mix.
- Do not use generic buckets like `misc.rs` or `util.rs`. Name modules by domain or purpose.
- Name folders by feature first, not layer first.
- Keep functions under 30 lines; extract helpers as needed.
- Each function must have a single clear responsibility.
- Prefer many small structs over large ones.
- All public objects, functions, structs, traits, and modules must be documented.
- All code should be well tested whenever feasible.
- “Feasible” should be interpreted broadly: tests are expected in almost all cases.
- Prefer small, focused unit tests that validate behaviour clearly.
- Do not allow untested logic unless explicitly approved by the user.
