Goal
	Design a high-performance tagging and database subsystem so sample tagging and related database updates feel instantaneous while keeping the current features intact.

Proposed solutions
- Profile the current tagging path (UI → DropHandler → SQLite) to identify the main latency sources, especially sync writes and DB opens.
- Introduce a long-lived SQLite handle with prepared statements and batched transactions for tag writes to remove per-event setup costs.
- Add an in-memory tag cache that is kept in sync with the DB (write-through or queued) so UI updates stay instant while background writes commit.
- Restructure scan/update flows to use chunked inserts/updates and avoid redundant file stat calls, improving overall DB/IO throughput.
- Provide lightweight benchmarks/tests around tag toggling and scan performance to guard against regressions.

Step-by-step plan
1. [x] Profile and trace the existing tagging path (UI to DB) to measure latency, DB open frequency, and WAL/pragma effects.
2. [x] Design the new persistence flow: shared DB handle per source, prepared statements for tag updates/reads, and a small in-memory tag cache strategy.
3. [x] Implement the storage layer improvements (connection lifecycle, prepared/batched tag writes, cache maintenance) without changing current features.
4. [x] Optimize scan/update routines to batch DB writes and reduce filesystem calls while keeping correctness.
5. [~] Add targeted tests/benchmarks for tag toggling and scan throughput; document operational guidance for achieving “lightning fast” interactions. (In progress; bugfix for alt-F4 crash landed.)
