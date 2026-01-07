

3. Redundant Path Normalization in Memory Lookup Table
Severity: Medium
Why it matters: The WavEntriesState lookup table stores three versions of every path (original, forward-slash, and back-slash) to handle cross-platform path inconsistencies. For large libraries, this triples the memory footprint of the path cache, which could easily exceed 100MB for moderately sized collections.
Evidence:
cache.rs:L194-208
: insert_lookup manually inserts normalized variants.
Recommended change: Standardize all paths to a single format (e.g., using path.to_string_lossy().replace('\\', \"/\")) at the boundary (database egress) and perform lookups only against the normalized version. Eliminate the redundant variant storage.
Risk/Tradeoffs: Must ensure all entry points (drag/drop, file scanner) use the same normalization routine.
Quick win?: Yes
Suggested test/verification: Memory profile the app before/after with a library of 100,000 samples.

4. Non-Atomic "Move to Trash" Operations
Severity: Medium
Why it matters: When moving files to trash, the application performs a file-system move followed by a database deletion. If the app crashes or the database write fails, the database becomes out of sync with the filesystem (referencing files that no longer exist in their original location), leading to "missing file" errors that require a full rescan.
Evidence:
trash_move.rs:L140-147
: File move and DB removal are two distinct, non-atomic steps.
Recommended change: Use a SQLite transaction to mark files as "deleted" or "pending move" before the filesystem operation, or ensure robust rollback/retry logic. At minimum, the UI should handle the discrepancy gracefully during the next database load.
Risk/Tradeoffs: Filesystem operations cannot be truly atomic with SQLite; however, ordering matters (DB first, or DB update to 'missing').
Quick win?: No
Suggested test/verification: Simulate a crash (abort process) immediately after move_to_trash returns Ok(()) but before db.remove_file.

5. Inconsistent SHA-256 Verification for PANNs Model Data
Severity: Low
Why it matters: The application downloads a PANNs ONNX model and an optional .data file (weights). While the .onnx file is strictly verified against a SHA-256 hash, the .data file is downloaded without any integrity check. This creates a small security gap where weights could be tampered with or corrupted during download.
Evidence:
model_setup.rs:L73
: download_optional for the data URL lacks a hash parameter.
model_setup.rs:L240
: Implementation of download_optional does not perform hashing.
Recommended change: Update ensure_panns_burnpack and download_optional to accept and verify a SHA-256 hash for the .data weight file, following the same pattern as the .onnx file.
Risk/Tradeoffs: Requires maintaining a second hash in the environment variables or build config.
Quick win?: Yes
Suggested test/verification: Verify that providing a mismatched hash for the .data file prevents the model from being used.