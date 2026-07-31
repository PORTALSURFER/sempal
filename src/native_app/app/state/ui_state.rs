use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    path::PathBuf,
    time::{Duration, Instant},
};

use radiant::widgets::PointerModifiers;
use radiant::{gui::panel as panel_ui, prelude as ui};
use wavecrate::sample_sources::SampleSource;
use wavecrate::sample_sources::config::AppSettingsCore;
use wavecrate::selection::SelectionRange;

use crate::native_app::app::{
    AppSettingsTab, AudioSettingsDropdown, GlobalStorageUsageState, NativeFileDropHover,
    SamplePlaybackSession,
};
use crate::native_app::sample_library::context_menu_target::{
    BrowserContextMenu, BrowserContextPointerAnchor,
};
use crate::native_app::waveform::WaveformContextMenu;

pub(in crate::native_app) const DEFAULT_BEAT_GUIDE_COUNT: u8 = 4;
pub(in crate::native_app) const MIN_BEAT_GUIDE_COUNT: u8 = 2;
pub(in crate::native_app) const MAX_BEAT_GUIDE_COUNT: u8 = 32;
pub(in crate::native_app) const DEFAULT_WAVEFORM_PANEL_HEIGHT: f32 = 226.0;
pub(in crate::native_app) const MIN_WAVEFORM_PANEL_HEIGHT: f32 = 94.0;
pub(in crate::native_app) const MAX_WAVEFORM_PANEL_HEIGHT: f32 = 512.0;

pub(in crate::native_app) struct UiAppState {
    pub(in crate::native_app) chrome: ChromeUiState,
    pub(in crate::native_app) status: StatusState,
    pub(in crate::native_app) release_update: ReleaseUpdateState,
    pub(in crate::native_app) settings: SettingsAppState,
    pub(in crate::native_app) browser_interaction: BrowserInteractionState,
    pub(in crate::native_app) startup: StartupState,
}

impl UiAppState {
    pub(in crate::native_app) fn new(
        chrome: ChromeUiState,
        status: StatusState,
        settings: SettingsAppState,
        startup: StartupState,
    ) -> Self {
        Self {
            chrome,
            status,
            release_update: ReleaseUpdateState::default(),
            settings,
            browser_interaction: BrowserInteractionState::default(),
            startup,
        }
    }
}

pub(in crate::native_app) struct ChromeUiState {
    pub(in crate::native_app) folder_panel: panel_ui::PanelResizeState,
    pub(in crate::native_app) waveform_panel: panel_ui::PanelResizeState,
    pub(in crate::native_app) job_details_open: bool,
    pub(in crate::native_app) transaction_list_open: bool,
    pub(in crate::native_app) shortcut_help_open: bool,
    pub(in crate::native_app) help_tooltips_enabled: bool,
    pub(in crate::native_app) sticky_random_sample_range_playback: bool,
    pub(in crate::native_app) sample_browser_display: SampleBrowserDisplayMode,
    pub(in crate::native_app) starmap_viewport: StarmapViewport,
    pub(in crate::native_app) starmap_audition_drag: Option<StarmapAuditionDragState>,
    pub(in crate::native_app) starmap_audition_queue: StarmapAuditionQueueState,
    pub(in crate::native_app) harvest_family_open: bool,
    pub(in crate::native_app) curation_filter_dropdown_open: bool,
    pub(in crate::native_app) harvest_filter_dropdown_open: bool,
    pub(in crate::native_app) bpm_snap_enabled: bool,
    pub(in crate::native_app) beat_guides_enabled: bool,
    pub(in crate::native_app) beat_guide_count: u8,
    pub(in crate::native_app) overflow_fades: OverflowFadeAnimations,
}

/// Lightweight transition state for paint-only clipping indicators.
///
/// It is intentionally kept with the transient UI state: the indicator is a
/// presentation cue, not persisted browser or waveform data.
pub(in crate::native_app) struct OverflowFadeAnimations {
    transitions: BTreeMap<u64, OverflowFadeTransition>,
    last_updated_at: Duration,
    pending_visibility_sample: bool,
}

#[derive(Clone, Copy)]
struct OverflowFadeTransition {
    start_alpha: f32,
    target_alpha: f32,
    started_at: Duration,
}

impl OverflowFadeAnimations {
    const TRANSITION_DURATION: Duration = Duration::from_millis(260);

    /// Arm a one-shot paint request so a newly visible clipping sample gets a
    /// chance to establish its transition before the normal animation cadence
    /// takes over.
    pub(in crate::native_app) fn arm(&mut self) {
        self.pending_visibility_sample = true;
    }

    /// Consume the one-shot visibility sample at the beginning of an overlay
    /// paint. Active transitions continue to request paint-only frames.
    pub(in crate::native_app) fn consume_pending_visibility_sample(&mut self) {
        self.pending_visibility_sample = false;
    }

    /// Return the interpolated opacity and retarget it without a visual jump.
    pub(in crate::native_app) fn opacity(
        &mut self,
        fade_id: u64,
        target_alpha: u8,
        now: Duration,
    ) -> u8 {
        self.opacity_from(fade_id, target_alpha, 0, now)
    }

    /// Return the interpolated opacity, beginning a newly visible indicator
    /// from `entry_alpha` rather than necessarily from transparent.
    pub(in crate::native_app) fn opacity_from(
        &mut self,
        fade_id: u64,
        target_alpha: u8,
        entry_alpha: u8,
        now: Duration,
    ) -> u8 {
        self.last_updated_at = now;
        let target_alpha = f32::from(target_alpha);
        if !self.transitions.contains_key(&fade_id) {
            if target_alpha == 0.0 {
                return 0;
            }
            let entry_alpha = f32::from(entry_alpha).min(target_alpha);
            self.transitions.insert(
                fade_id,
                OverflowFadeTransition {
                    start_alpha: entry_alpha,
                    target_alpha,
                    started_at: now,
                },
            );
            return entry_alpha.round() as u8;
        }
        let (current_alpha, settled_at_zero) = {
            let transition = self
                .transitions
                .get_mut(&fade_id)
                .expect("overflow fade transition must exist");
            let current_alpha = transition.alpha_at(now);
            if (transition.target_alpha - target_alpha).abs() > f32::EPSILON {
                transition.start_alpha = current_alpha;
                transition.target_alpha = target_alpha;
                transition.started_at = now;
            }
            let current_alpha = transition.alpha_at(now);
            (
                current_alpha,
                target_alpha == 0.0 && !transition.is_animating(now),
            )
        };
        if settled_at_zero {
            self.transitions.remove(&fade_id);
            return 0;
        }
        current_alpha.round().clamp(0.0, f32::from(u8::MAX)) as u8
    }

    /// Forget an indicator whose paint anchor has been removed from the view.
    ///
    /// It cannot animate out without bounds to draw into, and retaining its
    /// settled target would make a later remount appear at full opacity.
    pub(in crate::native_app) fn clear(&mut self, fade_id: u64) {
        self.transitions.remove(&fade_id);
    }

    /// Whether this indicator currently owns a transition or settled paint
    /// state. Callers use this to distinguish a newly clipped surface from a
    /// directional change within an already clipped surface.
    pub(in crate::native_app) fn contains(&self, fade_id: u64) -> bool {
        self.transitions.contains_key(&fade_id)
    }

    pub(in crate::native_app) fn frame_needed(&self) -> bool {
        self.pending_visibility_sample || self.is_animating()
    }

    pub(in crate::native_app) fn is_animating(&self) -> bool {
        self.transitions
            .values()
            .any(|transition| transition.is_animating(self.last_updated_at))
    }
}

impl Default for OverflowFadeAnimations {
    fn default() -> Self {
        Self {
            transitions: BTreeMap::new(),
            last_updated_at: Duration::ZERO,
            pending_visibility_sample: true,
        }
    }
}

impl OverflowFadeTransition {
    fn alpha_at(self, now: Duration) -> f32 {
        let elapsed = now.saturating_sub(self.started_at);
        let progress = (elapsed.as_secs_f32()
            / OverflowFadeAnimations::TRANSITION_DURATION.as_secs_f32())
        .clamp(0.0, 1.0);
        let eased = progress * progress * (3.0 - 2.0 * progress);
        self.start_alpha + (self.target_alpha - self.start_alpha) * eased
    }

    fn is_animating(self, now: Duration) -> bool {
        now.saturating_sub(self.started_at) < OverflowFadeAnimations::TRANSITION_DURATION
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum SampleBrowserDisplayMode {
    List,
    Map,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct StarmapAuditionDragState {
    pub(in crate::native_app) last_hit_file_id: Option<String>,
    pub(in crate::native_app) last_position: ui::Point,
    pub(in crate::native_app) modifiers: PointerModifiers,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::native_app) struct StarmapAuditionQueueState {
    pub(in crate::native_app) active_file_id: Option<String>,
    pub(in crate::native_app) last_played_file_id: Option<String>,
    pub(in crate::native_app) last_played_session: Option<SamplePlaybackSession>,
    pub(in crate::native_app) queued_file_ids: VecDeque<String>,
    pub(in crate::native_app) modifiers: PointerModifiers,
    pub(in crate::native_app) gesture_moved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::native_app) struct StarmapViewport {
    pub(in crate::native_app) center_x: f32,
    pub(in crate::native_app) center_y: f32,
    pub(in crate::native_app) zoom: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::native_app) enum StarmapViewportChange {
    Pan { delta: ui::Vector2 },
    Zoom { anchor: ui::Vector2, factor: f32 },
    Center { x: f32, y: f32 },
    Reset,
}

impl Default for StarmapViewport {
    fn default() -> Self {
        Self {
            center_x: 0.5,
            center_y: 0.5,
            zoom: 1.0,
        }
    }
}

impl StarmapViewport {
    const MIN_ZOOM: f32 = 0.65;
    const MAX_ZOOM: f32 = 1024.0;
    const MIN_CENTER: f32 = -1.0;
    const MAX_CENTER: f32 = 2.0;

    pub(in crate::native_app) fn apply_change(&mut self, change: StarmapViewportChange) {
        match change {
            StarmapViewportChange::Pan { delta } => {
                self.center_x -= delta.x / self.zoom;
                self.center_y -= delta.y / self.zoom;
                self.clamp_center();
            }
            StarmapViewportChange::Zoom { anchor, factor } => {
                self.zoom_at(anchor, factor);
            }
            StarmapViewportChange::Center { x, y } => {
                self.center_x = x;
                self.center_y = y;
                self.clamp_center();
            }
            StarmapViewportChange::Reset => *self = Self::default(),
        }
    }

    fn zoom_at(&mut self, anchor: ui::Vector2, factor: f32) {
        let previous_zoom = self.zoom;
        let next_zoom = (self.zoom * factor).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        if (next_zoom - previous_zoom).abs() <= f32::EPSILON {
            return;
        }
        let world_x = self.center_x + (anchor.x - 0.5) / previous_zoom;
        let world_y = self.center_y + (anchor.y - 0.5) / previous_zoom;
        self.zoom = next_zoom;
        self.center_x = world_x - (anchor.x - 0.5) / next_zoom;
        self.center_y = world_y - (anchor.y - 0.5) / next_zoom;
        self.clamp_center();
    }

    fn clamp_center(&mut self) {
        self.center_x = self.center_x.clamp(Self::MIN_CENTER, Self::MAX_CENTER);
        self.center_y = self.center_y.clamp(Self::MIN_CENTER, Self::MAX_CENTER);
    }
}

impl ChromeUiState {
    pub(in crate::native_app) fn new(folder_width: f32) -> Self {
        Self {
            folder_panel: panel_ui::PanelResizeState::new(folder_width),
            waveform_panel: panel_ui::PanelResizeState::new(DEFAULT_WAVEFORM_PANEL_HEIGHT),
            job_details_open: false,
            transaction_list_open: false,
            shortcut_help_open: false,
            help_tooltips_enabled: false,
            sticky_random_sample_range_playback: false,
            sample_browser_display: SampleBrowserDisplayMode::List,
            starmap_viewport: StarmapViewport::default(),
            starmap_audition_drag: None,
            starmap_audition_queue: StarmapAuditionQueueState::default(),
            harvest_family_open: false,
            curation_filter_dropdown_open: false,
            harvest_filter_dropdown_open: false,
            bpm_snap_enabled: false,
            beat_guides_enabled: false,
            beat_guide_count: DEFAULT_BEAT_GUIDE_COUNT,
            overflow_fades: OverflowFadeAnimations::default(),
        }
    }

    pub(in crate::native_app) fn waveform_panel_height(&self) -> f32 {
        self.waveform_panel
            .size()
            .clamp(MIN_WAVEFORM_PANEL_HEIGHT, MAX_WAVEFORM_PANEL_HEIGHT)
    }

    pub(in crate::native_app) fn resize_waveform_panel(
        &mut self,
        message: radiant::widgets::DragHandleMessage,
    ) {
        self.waveform_panel.resize(
            message,
            panel_ui::PanelResizeConstraints::bottom(
                MIN_WAVEFORM_PANEL_HEIGHT,
                MAX_WAVEFORM_PANEL_HEIGHT,
            ),
        );
    }

    pub(in crate::native_app) fn set_beat_guide_count(&mut self, count: u8) {
        self.beat_guide_count = clamp_beat_guide_count(u16::from(count));
    }

    pub(in crate::native_app) fn preview_beat_guide_count_input(&mut self, value: &str) {
        if let Some(count) = parse_in_range_beat_guide_count(value) {
            self.beat_guide_count = count;
        }
    }

    pub(in crate::native_app) fn commit_beat_guide_count_input(&mut self, value: &str) {
        self.beat_guide_count = parse_beat_guide_count(value)
            .map(clamp_beat_guide_count)
            .unwrap_or(MIN_BEAT_GUIDE_COUNT);
    }
}

fn parse_in_range_beat_guide_count(value: &str) -> Option<u8> {
    let count = parse_beat_guide_count(value)?;
    (u16::from(MIN_BEAT_GUIDE_COUNT)..=u16::from(MAX_BEAT_GUIDE_COUNT))
        .contains(&count)
        .then_some(count as u8)
}

fn parse_beat_guide_count(value: &str) -> Option<u16> {
    (!value.is_empty() && value.chars().all(|character| character.is_ascii_digit()))
        .then(|| value.parse::<u16>().ok())
        .flatten()
}

fn clamp_beat_guide_count(value: u16) -> u8 {
    value.clamp(
        u16::from(MIN_BEAT_GUIDE_COUNT),
        u16::from(MAX_BEAT_GUIDE_COUNT),
    ) as u8
}

pub(in crate::native_app) struct SettingsUiState {
    pub(in crate::native_app) audio_settings_open: bool,
    pub(in crate::native_app) app_settings_tab: AppSettingsTab,
    pub(in crate::native_app) audio_settings_dropdown: ui::ExclusiveOpen<AudioSettingsDropdown>,
    pub(in crate::native_app) global_storage_usage: GlobalStorageUsageState,
}

impl Default for SettingsUiState {
    fn default() -> Self {
        Self {
            audio_settings_open: false,
            app_settings_tab: Default::default(),
            audio_settings_dropdown: ui::ExclusiveOpen::new(),
            global_storage_usage: GlobalStorageUsageState::default(),
        }
    }
}

pub(in crate::native_app) struct SettingsAppState {
    pub(in crate::native_app) persisted: AppSettingsCore,
    pub(in crate::native_app) ui: SettingsUiState,
    pub(in crate::native_app) similarity_persist_deadline: Option<Instant>,
    pub(in crate::native_app) similarity_persist_inflight: bool,
}

impl SettingsAppState {
    pub(in crate::native_app) fn new(persisted: AppSettingsCore) -> Self {
        Self {
            persisted,
            ui: SettingsUiState::default(),
            similarity_persist_deadline: None,
            similarity_persist_inflight: false,
        }
    }
}

pub(in crate::native_app) struct StatusState {
    pub(in crate::native_app) sample: String,
}

impl StatusState {
    pub(in crate::native_app) fn new(sample: impl Into<String>) -> Self {
        Self {
            sample: sample.into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::native_app) struct ReleaseUpdateState {
    pub(in crate::native_app) status: ReleaseUpdateStatus,
    pub(in crate::native_app) latest: Option<wavecrate::updater::PublicReleaseInfo>,
    pub(in crate::native_app) last_error: Option<String>,
}

impl ReleaseUpdateState {
    pub(in crate::native_app) fn begin_check(&mut self) {
        self.status = ReleaseUpdateStatus::Checking;
        self.latest = None;
        self.last_error = None;
    }

    pub(in crate::native_app) fn finish(
        &mut self,
        result: Result<Option<wavecrate::updater::PublicReleaseInfo>, String>,
    ) {
        match result {
            Ok(Some(release)) => {
                self.status = ReleaseUpdateStatus::Available;
                self.latest = Some(release);
                self.last_error = None;
            }
            Ok(None) => {
                self.status = ReleaseUpdateStatus::UpToDate;
                self.latest = None;
                self.last_error = None;
            }
            Err(error) => {
                self.status = ReleaseUpdateStatus::Error;
                self.latest = None;
                self.last_error = Some(error);
            }
        }
    }

    pub(in crate::native_app) fn available(&self) -> bool {
        self.status == ReleaseUpdateStatus::Available && self.latest.is_some()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::native_app) enum ReleaseUpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available,
    Error,
}

#[derive(Default)]
pub(in crate::native_app) struct BrowserInteractionState {
    pub(in crate::native_app) context_menu: Option<BrowserContextMenu>,
    pub(in crate::native_app) waveform_context_menu: Option<WaveformContextMenu>,
    pub(in crate::native_app) context_menu_pointer_anchor: Option<BrowserContextPointerAnchor>,
    pub(in crate::native_app) clipboard_handoff_target: ClipboardHandoffTarget,
    pub(in crate::native_app) pending_folder_delete: Option<PendingFolderDelete>,
    pub(in crate::native_app) pending_trash_folder_setup: Option<PendingTrashFolderSetup>,
    pub(in crate::native_app) unsupported_files_dialog: Option<UnsupportedFilesDialogState>,
    pub(in crate::native_app) pending_waveform_destructive_edit:
        Option<PendingWaveformDestructiveEdit>,
    pub(in crate::native_app) pending_protected_extraction_target_source:
        Option<PendingProtectedExtractionTargetSource>,
    pub(in crate::native_app) cut_file_clipboard: Option<CutFileClipboard>,
    pub(in crate::native_app) cut_file_paste_task_id: Option<u64>,
    pub(in crate::native_app) native_file_drop_hover: Option<NativeFileDropHover>,
    pub(in crate::native_app) pending_internal_file_drag_paths: HashSet<PathBuf>,
    pub(in crate::native_app) pending_internal_file_drag_adds_keep_rating: bool,
    pub(in crate::native_app) file_move_conflict_apply_to_remaining: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::native_app) enum ClipboardHandoffTarget {
    #[default]
    BrowserFiles,
    WaveformSelection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct CutFileClipboard {
    pub(in crate::native_app) file_ids: Vec<String>,
}

impl CutFileClipboard {
    pub(in crate::native_app) fn new(file_ids: Vec<String>) -> Self {
        Self { file_ids }
    }

    pub(in crate::native_app) fn len(&self) -> usize {
        self.file_ids.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct PendingFolderDelete {
    pub(in crate::native_app) path: PathBuf,
    pub(in crate::native_app) name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct PendingTrashFolderSetup {
    pub(in crate::native_app) paths: Vec<PathBuf>,
    pub(in crate::native_app) action: &'static str,
    pub(in crate::native_app) started_at: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct UnsupportedFilesDialogState {
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) source_label: String,
    pub(in crate::native_app) loading: bool,
    pub(in crate::native_app) paths: Vec<PathBuf>,
    pub(in crate::native_app) error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum ExtractedFilePlaybackType {
    OneShot,
    Loop,
}

impl ExtractedFilePlaybackType {
    pub(in crate::native_app) fn from_loop_active(loop_active: bool) -> Self {
        if loop_active {
            Self::Loop
        } else {
            Self::OneShot
        }
    }

    pub(in crate::native_app) fn tag(self) -> &'static str {
        match self {
            Self::OneShot => "one-shot",
            Self::Loop => "loop",
        }
    }
}

#[derive(Clone, Debug)]
pub(in crate::native_app) struct PendingWaveformDestructiveEdit {
    pub(in crate::native_app) prompt: WaveformDestructiveEditPrompt,
    pub(in crate::native_app) source: SampleSource,
    pub(in crate::native_app) relative_path: PathBuf,
    pub(in crate::native_app) absolute_path: PathBuf,
    pub(in crate::native_app) selection: SelectionRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct PendingProtectedExtractionTargetSource {
    pub(in crate::native_app) action: PendingProtectedExtractionAction,
    pub(in crate::native_app) title: String,
    pub(in crate::native_app) message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum PendingProtectedExtractionAction {
    WaveformDestructiveEdit {
        kind: WaveformDestructiveEditKind,
        target: WaveformDestructiveEditTarget,
    },
    ExtractPlaymarkedRange,
    ExtractPlaymarkedRangeToHarvestDestination,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum WaveformDestructiveEditKind {
    CropSelection,
    TrimSelection,
    ReverseSelection,
    MuteSelection,
    ExtractAndTrimSelection,
    ApplyEditSelectionEffects,
    SlideSampleAudio { frame_offset: i64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum WaveformDestructiveEditTarget {
    ActiveSelection,
    PlaySelection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct WaveformDestructiveEditPrompt {
    pub(in crate::native_app) edit: WaveformDestructiveEditKind,
    pub(in crate::native_app) title: String,
    pub(in crate::native_app) message: String,
}

pub(in crate::native_app) struct StartupState {
    pub(in crate::native_app) source_scan_pending: bool,
    pub(in crate::native_app) folder_verify_pending: bool,
    pub(in crate::native_app) auto_load_pending: bool,
    pub(in crate::native_app) app_icon_install_pending: bool,
    pub(in crate::native_app) release_update_check_pending: bool,
}

impl StartupState {
    pub(in crate::native_app) fn new(
        source_scan_pending: bool,
        folder_verify_pending: bool,
        auto_load_pending: bool,
    ) -> Self {
        Self {
            source_scan_pending,
            folder_verify_pending,
            auto_load_pending,
            app_icon_install_pending: true,
            release_update_check_pending: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starmap_viewport_zoom_keeps_anchor_world_position_stable() {
        let mut viewport = StarmapViewport::default();
        let anchor = ui::Vector2::new(0.25, 0.75);
        let before = (
            viewport.center_x + (anchor.x - 0.5) / viewport.zoom,
            viewport.center_y + (anchor.y - 0.5) / viewport.zoom,
        );

        viewport.apply_change(StarmapViewportChange::Zoom {
            anchor,
            factor: 2.0,
        });

        let after = (
            viewport.center_x + (anchor.x - 0.5) / viewport.zoom,
            viewport.center_y + (anchor.y - 0.5) / viewport.zoom,
        );
        assert!((before.0 - after.0).abs() < 0.0001);
        assert!((before.1 - after.1).abs() < 0.0001);
        assert_eq!(viewport.zoom, 2.0);
    }

    #[test]
    fn starmap_viewport_zoom_allows_extra_map_margin() {
        let mut viewport = StarmapViewport::default();

        viewport.apply_change(StarmapViewportChange::Zoom {
            anchor: ui::Vector2::new(0.5, 0.5),
            factor: 0.5,
        });

        assert_eq!(viewport.zoom, 0.65);
        assert_eq!(viewport.center_x, 0.5);
        assert_eq!(viewport.center_y, 0.5);
    }

    #[test]
    fn starmap_viewport_zoom_allows_dense_cluster_inspection() {
        let mut viewport = StarmapViewport::default();

        viewport.apply_change(StarmapViewportChange::Zoom {
            anchor: ui::Vector2::new(0.5, 0.5),
            factor: 10_000.0,
        });

        assert_eq!(viewport.zoom, 1024.0);
        assert_eq!(viewport.center_x, 0.5);
        assert_eq!(viewport.center_y, 0.5);
    }

    #[test]
    fn starmap_viewport_pan_is_scaled_by_zoom_and_allows_offscreen_nodes() {
        let mut viewport = StarmapViewport {
            center_x: 0.5,
            center_y: 0.5,
            zoom: 2.0,
        };

        viewport.apply_change(StarmapViewportChange::Pan {
            delta: ui::Vector2::new(0.2, -0.1),
        });

        assert!((viewport.center_x - 0.4).abs() < 0.0001);
        assert!((viewport.center_y - 0.55).abs() < 0.0001);

        viewport.apply_change(StarmapViewportChange::Pan {
            delta: ui::Vector2::new(100.0, 100.0),
        });
        assert_eq!(viewport.center_x, -1.0);
        assert_eq!(viewport.center_y, -1.0);

        viewport.apply_change(StarmapViewportChange::Pan {
            delta: ui::Vector2::new(-100.0, -100.0),
        });
        assert_eq!(viewport.center_x, 2.0);
        assert_eq!(viewport.center_y, 2.0);
    }

    #[test]
    fn starmap_viewport_center_clamps_to_extended_map_bounds() {
        let mut viewport = StarmapViewport {
            center_x: 0.5,
            center_y: 0.5,
            zoom: 4.0,
        };

        viewport.apply_change(StarmapViewportChange::Center { x: -5.0, y: 5.0 });

        assert_eq!(viewport.center_x, -1.0);
        assert_eq!(viewport.center_y, 2.0);
    }
}
