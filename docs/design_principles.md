# sempal — design principles & system goals

## purpose

sempal is a realtime-oriented sample management system whose primary objective is to support the transformation of raw audio recordings into coherent, reusable, and musically compelling samples.

A central use case is the handling of long, unstructured recordings—such as improvisational jams captured from synthesizers, drum machines, or modular systems. sempal is designed to facilitate sustained listening, rapid navigation, and fine-grained exploration of such material, enabling users to identify salient moments and formalize them as discrete samples suitable for continued creative use.

In parallel, sempal is conceived as a **creative sample manager**, oriented around perceptual and sonic qualities rather than taxonomic classification alone. The system prioritizes how samples sound, feel, and relate to one another over rigid organizational schemas.

* exploration, discovery, and critical listening are first-class activities
* sonically compelling material should be easy to surface, revisit, and foreground
* uninspiring, redundant, or low-value material should be easy to identify and remove

Through these mechanisms, sempal encourages the gradual curation of a personal sample library shaped primarily by auditory judgment: retaining material that resonates, discarding what does not, and allowing a distinct sonic character to emerge over time.

Across all use cases, the software must remain immediate, predictable, and non-interruptive, even under sustained or computationally intensive workloads.

---

## core design principles

### 1. realtime primacy

sempal must operate with the behavioral expectations of a realtime system.

* user interactions should exhibit immediate perceptual response
* visual and auditory feedback should occur within a single frame whenever feasible
* operations that cannot complete instantaneously must degrade gracefully rather than blocking interaction

**non-negotiable rule:**
the user interface must never lock, stall, or become unresponsive

---

### 2. non-blocking execution

Blocking operations are explicitly prohibited on the ui thread.

* file i/o, analysis, indexing, rendering, and database access must be asynchronous
* long-running processes must be interruptible or cancelable
* progress and system state must be observable when operations exceed perceptual latency thresholds

When execution time increases, the system must adapt internally rather than imposing cognitive or temporal cost on the user.

---

### 3. integrated mouse and keyboard interaction

sempal is designed around **intentional integration of mouse and keyboard input**, leveraging the complementary strengths of each.

* mouse interaction is required for spatial tasks such as navigation, selection, and exploratory listening
* keyboard input is deeply integrated to support speed, modifiers, and sustained interaction flow
* most meaningful interactions emerge from the coordinated use of both modalities: mouse for intent, keyboard for control

Hotkeys are **contextual and focus-dependent**.

* the active focus determines which actions hotkeys invoke
* identical keys may perform different functions across distinct interaction contexts
* contextual scope must remain explicit, predictable, and inspectable

**Esc functions as a universal escape mechanism.**

* Esc semantically denotes "stop", "cancel", or "step back"
* invoking Esc cancels or exits the most immediate active operation based on current ui state and context
* Esc must never initiate destructive or state-advancing actions
* repeated Esc invocation should progressively unwind nested states (e.g. cancel operation → exit modal → clear selection)

Keyboard shortcuts are not auxiliary conveniences; they materially shape interaction semantics and modify mouse-driven behavior.

This interaction model explicitly rejects a mouse-only or keyboard-only paradigm in favor of a mutually reinforcing system.

---

### 4. flow preservation

sempal should minimize unnecessary disruption to creative flow.

* modal dialogs are acceptable when they serve a clear purpose and align naturally with the task at hand
* modals should be lightweight, scoped, and efficient to enter and exit
* forced confirmations should be avoided for actions that are reversible
* blocking alerts and intrusive notifications should be avoided

Modal interactions should support decision-making and clarity rather than function as impediments.

Errors and edge cases must be communicated clearly and non-intrusively, without halting ongoing work.

---

### 5. immediate auditioning

Audio is the primary medium of interaction.

* playback should initiate without perceptible delay
* scrubbing, looping, and region-based playback should feel continuous and stable
* visual representations exist to support listening, not to supplant it

Any feature that compromises audition speed or continuity should be treated with skepticism.

---

### 6. progressive disclosure

The interface should remain legible, calm, and minimally intrusive by default.

* advanced functionality should be discoverable without being imposed
* contextual information should appear only when relevant
* system complexity should be revealed incrementally in response to user intent

Expressive power should emerge through sustained use rather than visual or functional clutter.

---

### 7. predictability over cleverness

sempal prioritizes consistency, legibility, and user comprehension over surprising or opaque behavior.

* analogous actions should behave consistently across contexts
* keyboard mappings should conform to stable and learnable mental models
* state transitions should be immediately observable and reversible

When an action occurs, the user should be able to infer its cause without ambiguity.

---

### 7a. universal undo / redo

All meaningful actions within sempal must be reversible via a deeply integrated undo/redo system.

* undo and redo constitute first-class interaction primitives, not auxiliary safeguards
* the system should actively encourage experimentation by guaranteeing reversibility
* undo/redo semantics must apply uniformly across editing operations, metadata changes, and curation workflows

This principle obviates the need for most confirmation dialogs: reversibility is the primary mechanism of safety and trust.

---

### 8. data integrity and trust

User data must be treated as durable, inspectable, and trustworthy.

* silent data loss is unacceptable
* destructive actions must have a clear recovery path
* metadata mutations must be reliable, explicit, and traceable

Users should feel secure in exploring and modifying their libraries without fear of irrecoverable loss.

---

### 9. performance as a first-class feature

Performance is not a post hoc optimization concern; it is a core feature of the system.

* large libraries must remain navigable and responsive
* background computation must not impair interactive performance
* system behavior must scale gracefully with increasing data volume

Perceived slowness undermines user trust and is therefore considered a correctness issue.

---

### 10. tool, not platform

sempal is intentionally scoped as a focused creative tool rather than a platform or ecosystem.

* no dark patterns or manipulative interaction strategies
* no artificial friction or retention-driven design
* no features whose primary purpose is attention capture

The software exists to support creative work, not to compete for user attention.

---

## system-level requirements

### responsiveness

* the ui must remain responsive under all supported workloads
* frame hitches, input latency, and perceptible stalls are treated as defects
* visual feedback must never be contingent on blocking computation

---

### asynchronous architecture

* all computationally intensive operations must execute off the main/ui thread
* background tasks must expose progress and state
* cancellation semantics should be supported wherever meaningful

---

### deterministic interaction

* keyboard input must be deterministic and free of ambiguous conflicts
* contextual action scopes must be explicit and inspectable
* hidden or implicit state should be minimized

---

### scalable audio handling

* auditioning must remain reliable across large and heterogeneous libraries
* waveform generation and analysis must not compromise interactivity
* cached results should be reused aggressively to avoid redundant computation

---

### clear state model

* selection, playback, and editing states must be clearly defined
* state transitions should be observable and debuggable
* ui state must never become ambiguous or contradictory

---

### failure is acceptable — freezing is not

* errors may occur
* features may fail gracefully
* the ui must not freeze, lock, or crash as a consequence

---

## non-goals

sempal explicitly does not aim to be:

* a digital audio workstation replacement
* a cloud-centric content or distribution platform
* a social or collaborative network
* a visually ornamental application at the expense of responsiveness

---

## guiding question

Does this change make sempal faster, calmer, and more trustworthy to use—
or does it interrupt creative flow?

If the answer is unclear, the change should be reconsidered.
