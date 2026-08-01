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
- Keep the model/normalizer and backend-neutral admission/lifecycle library
  slices independently testable before live/replay adapters, then normalize all
  backends into the same work scopes and retain a narrow adapter to the existing
  workflow and persistence owners.
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

### Raw observation provenance is not watcher continuity proof

Every envelope carries a `RawObservationProvenance` value describing where the
observation came from: the associated source/root identity, backend and stream
metadata when available, watcher generation, capture time, event/cursor
metadata, and the lossless root-relative path bytes. This is origin metadata,
not publication authority. A source/root label, event ID, cursor, generation,
or backend callback by itself does not prove physical root identity, current
entry type, complete coverage, or permission to publish a targeted result.

The envelope carries a separate proof state:

- `Proof::Unproven` is the default. Live notify, memory-only evidence, lost or
  evicted evidence, cancellation, rebind, stale-generation rejection, and any
  invalid or incomplete replay remain unproven.
- Only the replay adapter may attach a valid `WatcherContinuityProof`. The
  proof must contain the same source and root identity, backend stream identity,
  watcher generation, a durable prior acknowledgement that the backend can
  replay, and contiguous replay coverage from that acknowledgement through the
  current batch boundary. The pure normalizer and live adapter may carry this
  proof state but may not create it or promote `Proof::Unproven`.

The proof is necessary for watcher-derived targeted work but is not, by itself,
a source commit or projection acknowledgement. The source writer must still
validate the proof against the durable source/checkpoint boundary before a
targeted checkpoint can advance.

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
   `RawObservationProvenance`, and sets `Proof::Unproven` before handing it to
   admission.
2. Admission rejects a wrong root or stale generation, or records the rejection
   as retained uncertainty; it applies the per-source/root and global budgets.
3. The pure normalizer preserves the raw evidence and widens incomplete
   evidence into `ExactEntry`, `Subtree`, or `SourceAudit` work scopes.
4. Until a later adapter PR changes the seam, live notify follows merged PR
   #1024: it routes through `WatcherAuthorityUnproven`, retains the last-good
   projection, and requests an authoritative affected-region/source audit. It
   has no targeted watcher authority and cannot advance a targeted watcher
   checkpoint.
5. Only the committed audit result may publish an authoritative replacement;
   projection and any audit-owned checkpoint advance only after the existing
   committed boundary is satisfied.

The live path does not perform a recursive walk, wait on the database, or use a
best-effort callback-time filesystem read to manufacture proof. A live callback
never becomes a continuity proof merely because its paths look complete.

## macOS FSEvents replay path

The target replay adapter feeds historical FSEvents batches into the same raw
envelope. It marks replay provenance, preserves the backend-delivered order of
relative paths at the raw boundary, and binds every batch to one source, root,
backend stream, and watcher generation. It is the only component allowed to
attach `WatcherContinuityProof`, and does so only after validating the durable
prior acknowledgement and contiguous replay interval.

An invalid, unavailable, or gapped replay cursor produces `Overflow` or `Error`
evidence with `Proof::Unproven` and a conservative recovery scope; it does not
advance a checkpoint or pretend that the interval was clean. FSEvents
coalescing is evidence of possible change, not proof that only the listed path
changed. A restart or rebind without a replayable cursor follows the same
unproven/audit path. The adapter boundary is specified here so live and replay
paths converge; implementing a new backend or cursor is a later slice.

## Conservative normalization (ExactEntry/Subtree/SourceAudit, uncertainty widens, rename/copy/delete/missing-parent/symlink/unsupported-only/empty-folder rules)

Normalization is lossless and syntactic only. It validates root-relative syntax,
retains native path bytes and raw order, and produces immutable work scopes with
their source/root/generation provenance and unchanged proof state. It performs
no Unicode normalization, case folding, lossy conversion such as
`to_string_lossy`, filesystem I/O, database I/O, or current entry-type proof.
Event kind, path spelling, Finder metadata, and replay metadata do not prove
that an entry currently exists, is a leaf, is supported, or is absent. The
scopes mean:

- `ExactEntry`: inspect only the named path; this is a bounded inspection scope,
  not proof of the entry's current type or existence;
- `Subtree`: inspect the named directory and descendants; or
- `SourceAudit`: audit the complete source/root boundary.

Uncertainty widens, never narrows: `ExactEntry -> Subtree -> SourceAudit`.
Queue pressure, path existence guesses, and a convenient filesystem read cannot
reduce a scope.

- A syntactically valid create/modify path may map to `ExactEntry`; the worker
  must classify its current entry without following links and widen if that
  classification is uncertain.
- A syntactically valid delete path may map to `ExactEntry` for no-follow
  verification. An absent, directory, or otherwise uncertain result widens to
  its nearest syntactically valid parent `Subtree`; deleting or losing the root
  maps to `SourceAudit`.
- A rename covers both old and new endpoints as inspection scopes. A directory
  rename, an unknown endpoint, or incomplete rename metadata widens the
  affected parent scope; the event does not prove either endpoint's current
  type.
- A copy covers the destination and, when supplied, the source as inspection
  scopes. Unknown or directory classification widens to the relevant
  `Subtree` scopes.
- A missing-parent indication widens to the nearest syntactically valid parent;
  if no parent/root boundary is established, it becomes `SourceAudit`.
- A symlink or reparse point is classified as an entry and is never followed to
  infer descendants. Unsupported target semantics widen the scope rather than
  escaping the root.
- A batch containing only unsupported or unknown event shapes is not a no-op;
  it becomes `SourceAudit`. Supported records in a mixed batch remain, with any
  uncertainty carried alongside them.
- An empty-folder event is evidence about a directory namespace, not proof that
  it has no children and not a synthetic sample entry. It maps to the directory
  `Subtree`; its deletion follows the missing-parent/delete rules.

Invalid or root-escaping paths produce uncertainty and cannot be dispatched as
arbitrary paths.

### Physical verification and commit gate

The normalizer cannot establish physical truth. Before traversal, the worker
must reacquire a live no-follow `SourceRootCapability` and revalidate the
physical root identity and lifecycle generation immediately before using it.
The source-writer owner must also reacquire a live no-follow
`SourceRootCapability` and revalidate the same physical root/lifecycle boundary
immediately before database mutation. Entry classification uses no-follow
operations; symlinks and reparse points remain entries and are never traversed.

A capability mismatch, root replacement, lifecycle mismatch, unavailable
capability, no-follow classification failure, or any other uncertainty forbids
the traversal/mutation and retains the last-good result while widening to the
affected `Subtree` or `SourceAudit`. It cannot be interpreted as absence,
successful deletion, a clean reconciliation, or checkpoint authority.

## Proof/root/generation boundaries

Every raw envelope carries `RawObservationProvenance`; every normalized scope
retains that provenance and its separate `Proof` state, plus event sequence and
replay cursor boundaries when available. A relative path is valid only under the
root that admitted it; absolute paths, `..` escapes, and unvalidated backend
paths are not proof of any other root. `WatcherContinuityProof` is valid only
when the replay adapter has established all of its identity, generation,
durable-acknowledgement, and contiguous-coverage fields.

Event IDs, cookies, and cursors support ordering and duplicate detection but do
not override source/root or generation checks. A scope cannot combine evidence
from different roots or generations. Checkpoint advancement requires a matching
committed boundary and valid continuity proof for watcher-derived targeted work,
so stale, dropped, overflowed, or uncertain evidence leaves the source
recoverable rather than falsely complete.

## Workflow outcome mapping

| Evidence at the boundary | Workflow outcome | Publication/checkpoint rule |
| --- | --- | --- |
| `RawObservationProvenance` with `Proof::Unproven` (including live notify, cancellation, rebind, stale-generation rejection, or queue eviction) | `WatcherAuthorityUnproven`; retain last-good state and request an affected-region/source audit | No targeted watcher authority or checkpoint; only a committed authoritative audit may publish and clear the uncertainty |
| Valid replay `WatcherContinuityProof` | Normalize to `ExactEntry`/`Subtree` and admit targeted workflow work | Source-writer commit and projection handoff are still required; a matching proof may then authorize the targeted watcher checkpoint |
| Invalid, missing, non-replayable, or gapped proof | Treat as `Proof::Unproven` and use the same `WatcherAuthorityUnproven` audit path | Never infer deletion or advance a watcher checkpoint |
| Explicit source/root audit committed | Authoritative reconciliation result | May replace the last-good projection and acknowledge the retained uncertainty through the audit's committed boundary |

## Queue and budget behavior

Admission is bounded per source/root and globally by event count and encoded
bytes, with a reserved capacity for an `Overflow`/`Error` marker. Ordering is
preserved within a generation. A noisy source cannot consume all capacity or
starve other sources; callback code never waits for a filesystem walk or a
database transaction.

When a budget is exhausted, the implementation may discard superseded raw
detail only while retaining an explicit `Proof::Unproven` uncertainty marker
with the relevant source/root, generation, time, and sequence boundaries.
Repeated pressure may coalesce markers, but it must widen the resulting scope
and remain observable. Queue eviction is not successful delivery, and a queue
drain is not an uncertainty acknowledgement. Durable retention of every raw
record is deliberately not specified here; the bounded uncertainty marker is
still retained by the owning recovery path.

## Compatibility seam to GuiMessage/workflow/source writer/projection/checkpoint

A later, separately reviewed adapter PR translates normalized scopes into the
existing `GuiMessage`/workflow entry point, carrying source/root identity,
generation, scope kind, proof state, paths, and uncertainty provenance. Existing
workflow and source-writer code remains the owner of source mutation and its
current transaction boundaries.

The raw envelope and pure normalizer retain backend order in their own lossless
representations. This document makes no claim that the current `BTreeSet`
coalescing or current `GuiMessage` path payload preserves that raw order. The
adapter PR must define any order-preservation or diagnostic handoff explicitly;
the current compatibility seam is not evidence of that property.

Projection continues to publish only what the source writer accepts. The
checkpoint continues to represent the last committed boundary, not the last
received notification. No new persistent `CommittedSourceDelta` schema is
introduced by this design; a future schema slice must preserve the same
proof-boundary rules.

## Lifecycle/cancellation/restart/crash recovery

Starting a watcher allocates a generation; stopping it cancels admission and
prevents post-stop delivery. Restarting or rebinding increments the generation
before new capture begins. In-flight records from an older generation are
rejected from delivery only after a bounded uncertainty record is retained; they
cannot mutate the new generation.

Every cancellation, rebind, stale-generation rejection, and queue eviction must
retain uncertainty for its affected source/root and sequence interval. That
uncertainty remains actionable until either the source writer returns a committed
reconciliation acknowledgement covering it or an explicit authoritative
source/root audit completes. Cancellation completion, rebinding, dropping a
stale record, draining a queue, receiving a projection message, or receiving a
checkpoint request does not clear it. A restart with no replayable cursor is
`Proof::Unproven`, routes through `WatcherAuthorityUnproven`, retains the
last-good projection, and requests an authoritative audit.

After a crash, recovery resumes from the last committed checkpoint and requests
replay or a conservative `SourceAudit` for the uncovered interval. Missing
raw records are treated as missing evidence, not as proof of no change. Recovery
must be idempotent at the workflow/source-writer boundary and must not advance a
checkpoint until the corresponding work is committed and projected according to
the existing contract.

## PR sequence with bounded library slices

1. Pure model/normalizer library PR: define the bounded raw envelope,
   `RawObservationProvenance`, separate `Proof`/`WatcherContinuityProof` types,
   and a pure conservative normalizer with deterministic tests. This PR has no
   admission supervisor, watcher callback, live/replay adapter, `GuiMessage`,
   `BTreeSet` coalescing, source-writer/database mutation, filesystem I/O, or
   persistent schema.
2. Separate backend-neutral library-only admission/lifecycle PR: add the
   bounded admission supervisor, lifecycle generation, queue budgets, fairness,
   cancellation, and acknowledgement boundaries. Use synthetic envelopes and
   acknowledgements for deterministic tests covering root mismatch,
   stale/duplicate delivery, budgets/fairness, cancellation/rebind/restart,
   eviction/overflow/error, and retained uncertainty. This is a required gate
   before any live/replay adapter or downstream workflow, projection,
   checkpoint, crash-recovery, or native integration.
3. Live/replay adapter PR: connect the validated library boundary to live
   notify and the replay adapter. Live notify must emit `Proof::Unproven` and
   preserve the merged #1024 `WatcherAuthorityUnproven` audit/last-good
   behavior; only the replay adapter may attach a validated continuity proof.
   The adapter PR must define the compatibility handoff without claiming that
   current `BTreeSet`/`GuiMessage` preserves raw order.
4. Downstream workflow/projection/checkpoint/crash/native integration: integrate
   committed acknowledgements with the existing workflow, source-writer,
   projection, and checkpoint boundaries, then validate crash recovery and
   native macOS behavior.
5. Handle directory truth, `schema/CommittedSourceDelta`, recursive hydration,
   a new backend/cursor, durable raw retention, and PR #980 only as separately
   scoped later work.

## Validation matrix

| Area | Target validation | Required evidence |
| --- | --- | --- |
| Contract | Check provenance/proof separation, field names, scope meanings, workflow mapping, and compatibility seam | Review of this design and `git diff --check` |
| Model/normalizer library (first PR) | Unit-test bounded raw envelopes, lossless/syntactic normalization, no I/O/type proof, proof non-escalation, and widening | Deterministic scope/provenance/proof assertions with no admission/lifecycle, adapter, or downstream dependency |
| Admission/lifecycle library (second PR; required gate before adapters) | With synthetic envelopes/acknowledgements, deterministically test root mismatch, stale/duplicate delivery, per-source/global budgets and fairness, cancellation/rebind/restart, eviction/overflow/error, and retained uncertainty | No live/replay adapter proceeds until this gate passes; no cross-root/generation delivery or false checkpoint; every uncertainty survives until committed reconciliation acknowledgement or explicit audit |
| Live notify | In the third adapter PR, drive representative macOS live notifications through the existing seam | `Proof::Unproven`, `WatcherAuthorityUnproven`, last-good retention, no targeted authority/checkpoint |
| Replay | In the third adapter PR, replay ordered/coalesced FSEvents batches, including an unavailable or gapped cursor | Only valid contiguous replay attaches proof; gaps use audit and last-good behavior |
| Commit/recovery | In the fourth downstream integration PR, interrupt before and after writer/projection/checkpoint boundaries, then restart | Idempotent recovery from the last committed boundary |
| Native acceptance | In the fourth downstream integration PR, use the normal writable macOS development profile with real sources | User-visible refresh/reconciliation behavior and no stale-generation effects |

## Explicit non-goals and risks

Non-goals are directory truth, recursive hydration, a new persistent
`CommittedSourceDelta` schema, a new backend or cursor, durable raw-event
retention, changes to existing source-writer ownership, current entry-type proof
at normalization time, and PR #980. This edit also does not change current
runtime behavior.

The principal risks are conservative widening causing extra audit work, backend
coalescing or cursor gaps producing frequent uncertainty, ambiguous rename/copy
semantics, symlink and root replacement edge cases, queue pressure during source
bursts, lossless path representation, and compatibility mistakes at the existing
message/checkpoint seam. Each risk is bounded by explicit provenance/proof
separation, generation rejection, retained uncertainty, reserved overflow/error
capacity, last-good audit fallback, and validation before a later slice is
merged.
