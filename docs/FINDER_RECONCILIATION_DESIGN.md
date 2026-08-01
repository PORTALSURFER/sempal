## Status and authority

This is a target design, not an implementation change. PR #1024 is merged at
`55f6dc0d3` and is the current baseline for observed behavior. The merged code,
not this document, remains the authority for what the application does today;
this document is the contract for later, separately reviewed slices.

Directory truth, the `schema/CommittedSourceDelta`, recursive hydration, a new
backend/cursor, a durable raw-retention policy, and PR #980 are later and out
of scope here.

## Current evidence/gap

Current source maintenance already has watcher/replay evidence and downstream
`GuiMessage`, workflow, source-writer, projection, and checkpoint consumers.
That path is the compatibility surface, but the evidence reaching it is not
yet one explicit, bounded, backend-neutral contract with durable proof of its
source, root, and watcher generation.

The target gap is therefore not another filesystem walk. It is a small evidence
boundary that can say what was observed, where and when it was observed, which
generation observed it, and when observation was incomplete. Current behavior
must be described as current behavior; the scopes and transitions below are
target behavior until implemented and validated.

## Design goals

- Preserve evidence order and provenance without treating an event as directory
  truth.
- Make overflow, backend errors, missing identity, and stale generations
  explicit and conservatively recoverable.
- Keep capture bounded and non-blocking, with fairness between sources.
- Normalize all backends into the same work scopes and retain a narrow adapter to
  the existing workflow and persistence owners.
- Make root and generation boundaries impossible to cross accidentally.
- Allow cancellation, restart, crash recovery, and later replay without
  claiming completion for work that was not committed.

## Bounded raw evidence model (backend-neutral event kind, ordered relative paths, flags/cookies/event IDs, capture time, source/root identity, watcher generation, overflow/error)

The target raw record is an observation envelope, not a database row and not a
filesystem snapshot. It contains:

- one backend-neutral event kind: `Create`, `Modify`, `Delete`, `Rename`,
  `Copy`, `RootChanged`, `Overflow`, `Error`, or `Unsupported`;
- an ordered list of relative paths, retained in backend delivery order rather
  than converted to an unordered set;
- optional backend flags, rename cookies, event IDs, and cursor information;
- capture time from the observing process;
- opaque source identity and root identity, with the root-relative path base;
- the watcher generation that admitted the observation; and
- overflow/error detail, including whether ordering, path coverage, or backend
  continuity is uncertain.

The envelope is bounded by event count, encoded path/metadata bytes, and queue
budget. Paths are validated as root-relative values before admission. An
envelope may be coalesced only when the resulting uncertainty is retained; it
must never silently become an empty notification.

## Ownership and state transitions

The watcher/backend owns capture only. An admission supervisor owns source/root
identity checks, generation checks, queue budgets, cancellation, and delivery
ordering. A pure normalizer owns the conversion from raw evidence to work
scopes. The existing workflow and source writer own filesystem/database effects;
projection owns publication of accepted results; checkpointing owns the
committed boundary.

The target lifecycle is:

`Stopped -> Starting(g) -> Capturing(g) -> Queued(g) -> Normalizing(g) -> Dispatched(g) -> Applied -> Checkpointed`

`Overflow`, `Error`, `Unsupported`, cancellation, or a stale delivery enters an
explicit uncertain/recovery path. A restart creates a new generation; records
from an older generation cannot advance the new one. No watcher callback or
normalizer writes the filesystem or database.

## Live notify path

1. The backend callback captures one bounded raw envelope quickly, including the
   source/root identity and current generation, and hands it to admission.
2. Admission rejects a wrong root or stale generation, or records the rejection
   as uncertainty; it applies the per-source/root and global budgets.
3. The normalizer preserves the admitted order and widens incomplete evidence
   into `ExactEntry`, `Subtree`, or `SourceAudit` scopes.
4. The compatibility adapter sends those scopes and their provenance through
   the existing workflow/`GuiMessage` seam.
5. The source writer applies the accepted work. Projection and checkpointing
   advance only after their existing committed boundary is satisfied.

The live path does not perform a recursive walk, wait on the database, or use a
best-effort callback-time filesystem read to manufacture proof.

## macOS FSEvents replay path

The target replay adapter feeds historical FSEvents batches into the same raw
envelope. It marks replay provenance, preserves the backend-delivered order of
relative paths, carries event IDs/cursor data when available, and binds every
batch to one source, root, and watcher generation.

An invalid, unavailable, or gapped replay cursor produces `Overflow` or `Error`
evidence and a conservative recovery scope; it does not advance a checkpoint or
pretend that the interval was clean. FSEvents coalescing is evidence of possible
change, not proof that only the listed path changed. The adapter boundary is
specified here so live and replay paths converge; implementing a new backend or
cursor is a later slice.

## Conservative normalization (ExactEntry/Subtree/SourceAudit, uncertainty widens, rename/copy/delete/missing-parent/symlink/unsupported-only/empty-folder rules)

Normalization produces immutable work scopes with their source/root/generation
proof. The scopes mean:

- `ExactEntry`: inspect only the proven named entry;
- `Subtree`: inspect the named directory and descendants; or
- `SourceAudit`: audit the complete source/root boundary.

Uncertainty widens, never narrows: `ExactEntry -> Subtree -> SourceAudit`.
Queue pressure, path existence guesses, and a convenient filesystem read cannot
reduce a scope.

- A proven supported leaf create/modify maps to `ExactEntry`.
- A delete maps to `ExactEntry` only with explicit leaf proof. A deleted or
  unknown directory maps to its nearest proven parent `Subtree`; deleting the
  root maps to `SourceAudit`.
- A rename covers both old and new endpoints. A directory rename, an unknown
  endpoint, or incomplete rename metadata widens the affected parent scope.
- A copy covers the destination and, when supplied, the source. Unknown or
  directory copies widen to the relevant `Subtree` scopes.
- A missing parent widens to the nearest proven parent; if no parent/root
  boundary is proven, it becomes `SourceAudit`.
- A symlink is treated as an entry and is never followed to infer descendants.
  Unsupported target semantics widen the scope rather than escaping the root.
- A batch containing only unsupported or unknown event shapes is not a no-op;
  it becomes `SourceAudit`. Supported records in a mixed batch remain, with any
  uncertainty carried alongside them.
- An empty-folder event is evidence about a directory namespace, not proof that
  it has no children and not a synthetic sample entry. It maps to the directory
  `Subtree`; its deletion follows the missing-parent/delete rules.

Invalid or root-escaping paths produce uncertainty and cannot be dispatched as
arbitrary paths.

## Proof/root/generation boundaries

Every raw envelope and normalized scope carries the tuple `(source identity,
root identity, watcher generation)`, plus its event sequence and replay cursor
boundaries when available. A relative path is valid only under the root that
admitted it; absolute paths, `..` escapes, and unvalidated backend paths are not
proof of any other root.

Event IDs, cookies, and cursors support ordering and duplicate detection but do
not override source/root or generation checks. A scope cannot combine evidence
from different roots or generations. Checkpoint advancement requires a matching
committed boundary, so stale, dropped, overflowed, or uncertain evidence leaves
the source recoverable rather than falsely complete.

## Queue and budget behavior

Admission is bounded per source/root and globally by event count and encoded
bytes, with a reserved capacity for an `Overflow`/`Error` marker. Ordering is
preserved within a generation. A noisy source cannot consume all capacity or
starve other sources; callback code never waits for a filesystem walk or a
database transaction.

When a budget is exhausted, the implementation may discard superseded raw
detail only while retaining an explicit uncertainty marker with the relevant
time and sequence boundaries. Repeated pressure may coalesce markers, but it
must widen the resulting scope and remain observable. Cancellation drains or
abandons work by generation; it does not turn abandoned work into a successful
checkpoint. Durable raw retention is deliberately not specified here.

## Compatibility seam to GuiMessage/workflow/source writer/projection/checkpoint

The first adapter translates normalized scopes into the existing `GuiMessage`/
workflow entry point, carrying source/root identity, generation, scope kind,
paths, and uncertainty provenance. Existing workflow and source-writer code
remains the owner of source mutation and its current transaction boundaries.

Projection continues to publish only what the source writer accepts. The
checkpoint continues to represent the last committed boundary, not the last
received notification. No new persistent `CommittedSourceDelta` schema is
introduced by this design; a future schema slice must preserve the same
proof-boundary rules.

## Lifecycle/cancellation/restart/crash recovery

Starting a watcher allocates a generation; stopping it cancels admission and
prevents post-stop delivery. Restarting or rebinding increments the generation
before new capture begins. In-flight records from an older generation are
discarded or converted into explicit recovery evidence and cannot mutate the
new generation.

After a crash, recovery resumes from the last committed checkpoint and requests
replay or a conservative `SourceAudit` for the uncovered interval. Missing
raw records are treated as missing evidence, not as proof of no change. Recovery
must be idempotent at the workflow/source-writer boundary and must not advance a
checkpoint until the corresponding work is committed and projected according to
the existing contract.

## PR sequence with first code slice

1. First code slice: define the bounded raw envelope, source/root/generation
   proof types, and a pure conservative normalizer with tests for every rule
   above. Adapt it to the existing live-notify seam without changing downstream
   ownership or adding a new persistent schema.
2. Add admission queues, reserved overflow/error capacity, fairness, and
   cancellation/restart generation tests.
3. Add the FSEvents replay adapter at the same envelope boundary, including
   cursor-gap and root/generation rejection tests; do not introduce a new
   backend/cursor in this sequence.
4. Integrate committed workflow, projection, and checkpoint acknowledgements,
   then validate crash recovery and native macOS behavior.
5. Handle directory truth, `schema/CommittedSourceDelta`, recursive hydration,
   a new backend/cursor, durable raw retention, and PR #980 only as separately
   scoped later work.

## Validation matrix

| Area | Target validation | Required evidence |
| --- | --- | --- |
| Contract | Check headings, field names, scope meanings, and compatibility seam | Review of this design and `git diff --check` |
| Normalization | Unit-test exact, subtree, audit, widening, rename/copy/delete, missing-parent, symlink, unsupported-only, and empty-folder cases | Deterministic scope and provenance assertions |
| Boundaries | Test invalid paths, root mismatch, stale generation, duplicate IDs, cursor gaps, overflow, and errors | No cross-root/generation delivery and no false checkpoint |
| Queue/lifecycle | Exercise per-source/global budgets, fairness, cancellation, restart, and repeated overflow | Bounded memory, explicit uncertainty, recoverable state |
| Live notify | Drive representative macOS live notifications through the existing adapter | Correct `GuiMessage`/workflow handoff without callback blocking |
| Replay | Replay ordered/coalesced FSEvents batches, including an unavailable or gapped cursor | Same normalized scopes as live evidence, with replay provenance |
| Commit/recovery | Interrupt before and after writer/projection/checkpoint boundaries, then restart | Idempotent recovery from the last committed boundary |
| Native acceptance | Use the normal writable macOS development profile with real sources | User-visible refresh/reconciliation behavior and no stale-generation effects |

## Explicit non-goals and risks

Non-goals are directory truth, recursive hydration, a new persistent
`CommittedSourceDelta` schema, a new backend or cursor, durable raw-event
retention, changes to existing source-writer ownership, and PR #980. This edit
also does not change current runtime behavior.

The principal risks are conservative widening causing extra audit work, backend
coalescing or cursor gaps producing frequent uncertainty, ambiguous rename/copy
semantics, symlink and root replacement edge cases, queue pressure during source
bursts, and compatibility mistakes at the existing message/checkpoint seam.
Each risk is bounded by explicit provenance, generation rejection, reserved
overflow/error capacity, and validation before a later slice is merged.
