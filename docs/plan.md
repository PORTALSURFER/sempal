## Goal
Hide folder names and file extensions (including `.wav`) from both the collection and regular sample lists so only the clean sample names appear.

## Proposed Solutions
- Strip extensions and path components from item labels before rendering.
- Adjust list components to display a preprocessed display name alongside original data.
- Add regression tests to ensure future list items stay extension-free.

## Step-by-Step Plan
1. [x] Audit the collection and sample list rendering code to find where labels are generated.
2. [x] Introduce helper logic that derives a clean sample name from a path or filename.
3. [x] Update UI components to use the helper for both collection and sample lists.
4. [x] Add or update tests (or snapshots) verifying that `.wav` and folder names no longer appear.
5. [-] Manually verify the UI to ensure both lists now show only sample names.
ghj