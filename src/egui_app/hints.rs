use rand::prelude::SliceRandom;

#[derive(Clone, Copy, Debug)]
pub struct HintItem {
    pub title: &'static str,
    pub body: &'static str,
}

const HINTS: &[HintItem] = &[
    HintItem {
        title: "Drag-drop folders",
        body: "Drop a folder onto Sources to add it to the library.",
    },
    HintItem {
        title: "Quick zoom",
        body: "Use the mouse wheel to zoom; hold Shift to pan horizontally.",
    },
    HintItem {
        title: "Loop from selection",
        body: "Enable loop to keep playback inside the current selection.",
    },
    HintItem {
        title: "Selection nudge",
        body: "Use arrow keys to nudge a selection and keep timing tight.",
    },
    HintItem {
        title: "BPM snapping",
        body: "Turn on BPM snap to align selections to the beat grid.",
    },
    HintItem {
        title: "Transient snapping",
        body: "Enable transient snap to grab onsets when slicing samples.",
    },
    HintItem {
        title: "Batch triage",
        body: "Multi-select samples to apply keep/trash tags in one action.",
    },
    HintItem {
        title: "Collect from selection",
        body: "Use selection export to save a clip without leaving the waveform view.",
    },
    HintItem {
        title: "Search samples",
        body: "Press F to focus the search box and filter the browser list.",
    },
    HintItem {
        title: "Similarity prep",
        body: "Run similarity prep after adding sources to unlock map navigation.",
    },
    HintItem {
        title: "GPU embeddings",
        body: "Open GPU embedding options to speed up analysis on compatible hardware.",
    },
    HintItem {
        title: "Snap beats then transients",
        body: "When BPM snap is on, it wins over transient snapping.",
    },
    HintItem {
        title: "Loop zoom",
        body: "Zoom in around a selection to fine-tune the edges quickly.",
    },
    HintItem {
        title: "Undo edits",
        body: "Use Undo to revert selection changes or edits you just made.",
    },
    HintItem {
        title: "Tag from keyboard",
        body: "Use the tag hotkeys to triage samples without touching the mouse.",
    },
];

pub fn random_hint() -> &'static HintItem {
    let mut rng = rand::rng();
    HINTS
        .choose(&mut rng)
        .unwrap_or_else(|| &HINTS[0])
}
