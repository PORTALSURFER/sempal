3. Inefficient Search Loop with Repeated Allocations
Severity: Medium (Performance)
Why it matters: The fuzzy search loop calls 
sample_display_label
 for every entry on every keystroke. This function performs multiple String and PathBuf allocations, creating significant pressure on the allocator during search.
Evidence: 
browser_search_worker.rs:L111-114
, 
view_model.rs:L112-123
Recommended change: Pre-calculate and cache the display label strings within the 
SearchWorkerCache
 during the initial load or revision bump.
Risk/Tradeoffs: Slight increase in cache memory usage.
Quick win?: Yes
Suggested test/verification: Use a profiler like flamegraph to confirm reduction in to_string and PathBuf allocations during active search.

4. Race Condition in Application Relaunch during Update
Severity: Medium (Reliability)
Why it matters: The updater spawns the new app process immediately without ensuring the current (old) instance has fully yielded shared resources (like SQLite WAL locks or audio devices), often leading to startup conflicts on Windows.
Evidence: 
apply.rs:L154-158
, 
L262-264
Recommended change: Implement a cross-process lock or signaling mechanism to coordinate the handoff between instances.
Risk/Tradeoffs: Adds complexity to the update flow; may require a small user-facing delay.
Quick win?: No
Suggested test/verification: Run the update cycle on Windows and monitor for overlapping processes in Task Manager.

5. Redundant Path Normalization in Database Operations
Severity: Low (Maintainability)
Why it matters: 
normalize_relative_path
 (which replaces slashes and cleans paths) is called repeatedly at every database interaction. This leads to redundant work and risks inconsistency if logic diverges.
Evidence: 
read.rs:L138
, 
write.rs:L17
Recommended change: Introduce a NormalizedPath wrapper type that ensures paths are processed once at the system boundary and used consistently throughout the database layer.
Risk/Tradeoffs: Requires a widespread API refactor in the db module.
Quick win?: No
Suggested test/verification: Verify path consistency via existing unit tests in 
src/sample_sources/db/mod.rs
.