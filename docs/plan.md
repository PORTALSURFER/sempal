## Goal
- Add a right-hand sidebar for collections where users can create collections, view the samples within a selected collection, and drag/drop samples to tag them into collections without moving them.

## Proposed solutions
- Introduce a collections state model (ids, names, member sample paths) stored alongside existing sources/tags, with lightweight persistence so collection membership survives reloads.
- Extend the Slint UI with a right sidebar component: collections list with add action, and a collection detail list showing members of the selected collection, keeping existing sample triage panels unchanged.
- Implement drag/drop integration from existing sample rows to the collections sidebar; dropping adds membership (tagging) without altering the sample’s current category, gated behind a feature flag.

## Step-by-step plan
1. [x] Review current wav list rendering, selection, and tag/persistence flow to identify integration points for collections and drag events.
2. [x] Define collection data structures and storage/persistence strategy, including schema for member references and feature flag wiring.
3. [x] Extend application state and handlers to manage collections (create/select/list members) and expose data to the UI while keeping existing workflows stable.
4. [x] Update Slint layouts to add the right sidebar with collections list + add control and a collection member view that syncs with selection.
5. [x] Implement drag/drop from sample rows to collections targets to add membership without moving samples; handle feedback, deduping, and flag-guarded activation.
6. [~] Add tests for collection state/membership logic and perform manual QA of the new UI/drag-drop flow.
