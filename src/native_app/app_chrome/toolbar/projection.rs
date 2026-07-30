use crate::native_app::app_chrome::toolbar::icons::ToolbarIcon;
use crate::native_app::app_chrome::toolbar::identity::{
    TOOLBAR_BEAT_GUIDE_COUNT_ID, TOOLBAR_BEAT_GUIDE_COUNT_KEY, TOOLBAR_BEAT_GUIDES_ID,
    TOOLBAR_BPM_SNAP_ID, TOOLBAR_LOOP_ID, TOOLBAR_METRONOME_ID, TOOLBAR_PLAY_ID,
};
use crate::native_app::app_chrome::toolbar::{
    TOOLBAR_APPLY_EDIT_MARK_EDITS_ID, TOOLBAR_FOCUS_LOADED_ID, TOOLBAR_RANDOM_ID,
    TOOLBAR_SIMILAR_SECTIONS_ID, TOOLBAR_STOP_ID, TOOLBAR_ZERO_CROSSING_SNAP_ID,
};
use crate::native_app::app_chrome::view_models::toolbar::MainToolbarViewModel;

const FOCUS_LOADED_TOOLTIP: &str = "Focus the loaded sample in the browser.";
const LOOP_TOOLTIP: &str = "Loop";
const RANDOM_TOOLTIP: &str = "Play random section\nClick: play a random section now.\nShift-click: pick a random listed sample first.\nCommand-click: make Space use random sections.";
const SIMILAR_SECTIONS_TOOLTIP: &str = "Mark sections similar to the playmark selection.\nSet a playmark first, then toggle this to scan the loaded sample.";
const ZERO_CROSSING_SNAP_TOOLTIP: &str = "Snap play and edit mark edges to nearby zero crossings.";
const BPM_SNAP_TOOLTIP: &str =
    "Snap resized play and edit marks to whole BPM values using the beat guide count.";
const BEAT_GUIDES_TOOLTIP: &str = "Show beat guide lines inside the play selection.";
const METRONOME_TOOLTIP: &str = "Play a metronome from the beat guide divisions.";
const BEAT_GUIDE_COUNT_TOOLTIP: &str = "Beat guide divisions.";
const APPLY_EDIT_MARK_EDITS_TOOLTIP: &str = "Apply edit mark gain and fade edits.";
const PLAY_TOOLTIP: &str = "Play";
const STOP_TOOLTIP: &str = "Stop preview playback.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct ToolbarProjection {
    pub(in crate::native_app) help_tooltips_enabled: bool,
    pub(in crate::native_app) controls: Vec<ToolbarControlProjection>,
}

impl ToolbarProjection {
    pub(in crate::native_app) fn from_model(model: MainToolbarViewModel) -> Self {
        let mut controls = vec![
            ToolbarIconButtonProjection::new(
                TOOLBAR_FOCUS_LOADED_ID,
                ToolbarIcon::FocusLoaded,
                true,
                false,
                FOCUS_LOADED_TOOLTIP,
            )
            .into(),
            ToolbarIconButtonProjection::new(
                TOOLBAR_SIMILAR_SECTIONS_ID,
                ToolbarIcon::SimilarSections,
                true,
                model.similar_sections_enabled,
                SIMILAR_SECTIONS_TOOLTIP,
            )
            .with_icon_enabled(model.similar_sections_available || model.similar_sections_enabled)
            .into(),
            ToolbarIconButtonProjection::new(
                TOOLBAR_ZERO_CROSSING_SNAP_ID,
                ToolbarIcon::ZeroCrossingSnap,
                true,
                model.zero_crossing_snap_enabled,
                ZERO_CROSSING_SNAP_TOOLTIP,
            )
            .into(),
            ToolbarIconButtonProjection::new(
                TOOLBAR_BPM_SNAP_ID,
                ToolbarIcon::BpmSnap,
                true,
                model.bpm_snap_enabled,
                BPM_SNAP_TOOLTIP,
            )
            .into(),
            ToolbarIconButtonProjection::new(
                TOOLBAR_BEAT_GUIDES_ID,
                ToolbarIcon::BeatGuides,
                true,
                model.beat_guides_enabled,
                BEAT_GUIDES_TOOLTIP,
            )
            .into(),
            ToolbarControlProjection::BeatGuideCountField {
                count: model.beat_guide_count,
                id: TOOLBAR_BEAT_GUIDE_COUNT_ID,
                key: TOOLBAR_BEAT_GUIDE_COUNT_KEY,
                tooltip: BEAT_GUIDE_COUNT_TOOLTIP,
            },
            ToolbarIconButtonProjection::new(
                TOOLBAR_METRONOME_ID,
                ToolbarIcon::Metronome,
                true,
                model.metronome_enabled,
                METRONOME_TOOLTIP,
            )
            .into(),
        ];

        if model.pending_edit_mark_edits {
            controls.push(ToolbarControlProjection::ApplyEditMarkEdits {
                id: TOOLBAR_APPLY_EDIT_MARK_EDITS_ID,
                tooltip: APPLY_EDIT_MARK_EDITS_TOOLTIP,
            });
        }

        controls.push(
            ToolbarIconButtonProjection::new(
                TOOLBAR_STOP_ID,
                ToolbarIcon::Stop,
                true,
                false,
                STOP_TOOLTIP,
            )
            .into(),
        );
        controls.push(
            ToolbarIconButtonProjection::new(
                TOOLBAR_PLAY_ID,
                ToolbarIcon::Play,
                true,
                model.playing,
                PLAY_TOOLTIP,
            )
            .into(),
        );
        controls.push(
            ToolbarIconButtonProjection::new(
                TOOLBAR_RANDOM_ID,
                ToolbarIcon::Random,
                model.random_available,
                model.sticky_random_sample_range_playback,
                RANDOM_TOOLTIP,
            )
            .into(),
        );
        controls.push(
            ToolbarIconButtonProjection::new(
                TOOLBAR_LOOP_ID,
                ToolbarIcon::Loop,
                true,
                model.loop_playback,
                LOOP_TOOLTIP,
            )
            .into(),
        );

        Self {
            help_tooltips_enabled: model.help_tooltips_enabled,
            controls,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum ToolbarControlProjection {
    Icon(ToolbarIconButtonProjection),
    BeatGuideCountField {
        count: u8,
        id: u64,
        key: &'static str,
        tooltip: &'static str,
    },
    ApplyEditMarkEdits {
        id: u64,
        tooltip: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct ToolbarIconButtonProjection {
    pub(in crate::native_app) id: u64,
    pub(in crate::native_app) icon: ToolbarIcon,
    pub(in crate::native_app) enabled: bool,
    pub(in crate::native_app) icon_enabled: bool,
    pub(in crate::native_app) active: bool,
    pub(in crate::native_app) tooltip: &'static str,
}

impl ToolbarIconButtonProjection {
    fn new(id: u64, icon: ToolbarIcon, enabled: bool, active: bool, tooltip: &'static str) -> Self {
        Self {
            id,
            icon,
            enabled,
            icon_enabled: enabled,
            active,
            tooltip,
        }
    }

    fn with_icon_enabled(mut self, icon_enabled: bool) -> Self {
        self.icon_enabled = icon_enabled;
        self
    }
}

impl From<ToolbarIconButtonProjection> for ToolbarControlProjection {
    fn from(button: ToolbarIconButtonProjection) -> Self {
        Self::Icon(button)
    }
}
