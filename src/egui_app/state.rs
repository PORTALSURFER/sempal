#![allow(dead_code)]
//! Shared state types for the egui UI.
// Temporary while the egui UI is being wired; types will be exercised by the renderer next.

use crate::audio::{AudioOutputConfig, ResolvedOutput};
use crate::egui_app::ui::style;
use crate::sample_sources::{CollectionId, SampleTag, SourceId};
use crate::selection::SelectionRange;
use crate::waveform::WaveformChannelView;
use egui::{Color32, Pos2};
use std::path::PathBuf;

/// Top-level UI model consumed by the egui renderer.
#[derive(Clone, Debug)]
pub struct UiState {
    pub status: StatusBarState,
    pub sources: SourcePanelState,
    pub browser: SampleBrowserState,
    pub waveform: WaveformState,
    pub drag: DragState,
    pub collections: CollectionsState,
    /// Tracks which UI region currently owns keyboard focus.
    pub focus: UiFocusState,
    /// UI state for contextual hotkey affordances.
    pub hotkeys: HotkeyUiState,
    /// Audio device/options UI state.
    pub audio: AudioOptionsState,
    /// Interaction and navigation tuning options.
    pub controls: InteractionOptionsState,
    /// Master output volume (0.0-1.0).
    pub volume: f32,
    pub loaded_wav: Option<PathBuf>,
    /// Optional trash folder path configured by the user.
    pub trash_folder: Option<PathBuf>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            status: StatusBarState::idle(),
            sources: SourcePanelState::default(),
            browser: SampleBrowserState::default(),
            waveform: WaveformState::default(),
            drag: DragState::default(),
            collections: CollectionsState::default(),
            focus: UiFocusState::default(),
            hotkeys: HotkeyUiState::default(),
            audio: AudioOptionsState::default(),
            controls: InteractionOptionsState::default(),
            volume: 1.0,
            loaded_wav: None,
            trash_folder: None,
        }
    }
}

/// Status badge + text shown in the footer.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusBarState {
    pub text: String,
    pub badge_label: String,
    pub badge_color: Color32,
}

impl StatusBarState {
    pub fn idle() -> Self {
        Self {
            text: "Add a sample source to get started".into(),
            badge_label: "Idle".into(),
            badge_color: style::status_badge_color(style::StatusTone::Idle),
        }
    }
}

/// Sidebar list of sample sources.
#[derive(Clone, Debug, Default)]
pub struct SourcePanelState {
    pub rows: Vec<SourceRowView>,
    pub selected: Option<usize>,
    pub menu_row: Option<usize>,
    pub scroll_to: Option<usize>,
    pub folders: FolderBrowserUiState,
}

/// Display data for a single source row.
#[derive(Clone, Debug)]
pub struct SourceRowView {
    pub id: SourceId,
    pub name: String,
    pub path: String,
    pub missing: bool,
}

/// UI state for browsing folders within the active source.
#[derive(Clone, Debug, Default)]
pub struct FolderBrowserUiState {
    pub rows: Vec<FolderRowView>,
    pub focused: Option<usize>,
    pub scroll_to: Option<usize>,
    pub search_query: String,
    pub search_focus_requested: bool,
    pub rename_focus_requested: bool,
    pub pending_action: Option<FolderActionPrompt>,
}

/// Render-friendly folder row.
#[derive(Clone, Debug)]
pub struct FolderRowView {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
    pub selected: bool,
}

/// Pending inline action for the folder browser.
#[derive(Clone, Debug)]
pub enum FolderActionPrompt {
    Create { parent: PathBuf, name: String },
    Rename { target: PathBuf, name: String },
}

/// Cached waveform image and playback overlays.
#[derive(Clone, Debug)]
pub struct WaveformState {
    pub image: Option<WaveformImage>,
    pub playhead: PlayheadState,
    /// Last play start position chosen by the user (normalized 0-1).
    pub last_start_marker: Option<f32>,
    pub selection: Option<SelectionRange>,
    pub selection_duration: Option<String>,
    pub hover_time_label: Option<String>,
    pub channel_view: WaveformChannelView,
    /// Current visible viewport within the waveform (0.0-1.0 normalized).
    pub view: WaveformView,
    pub loop_enabled: bool,
    pub notice: Option<String>,
    /// Optional path for the sample currently loading to drive UI affordances.
    pub loading: Option<PathBuf>,
    /// Pending confirmation dialog for destructive edits.
    pub pending_destructive: Option<DestructiveEditPrompt>,
}

impl Default for WaveformState {
    fn default() -> Self {
        Self {
            image: None,
            playhead: PlayheadState::default(),
            last_start_marker: None,
            selection: None,
            selection_duration: None,
            hover_time_label: None,
            channel_view: WaveformChannelView::Mono,
            view: WaveformView::default(),
            loop_enabled: false,
            notice: None,
            loading: None,
            pending_destructive: None,
        }
    }
}

/// Raw pixels ready to upload to an egui texture.
#[derive(Clone, Debug)]
pub struct WaveformImage {
    pub image: egui::ColorImage,
    pub view_start: f32,
    pub view_end: f32,
}

/// Normalized bounds describing the visible region of the waveform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaveformView {
    pub start: f32,
    pub end: f32,
}

impl WaveformView {
    /// Clamp the view to a valid range while keeping the width positive.
    pub fn clamp(mut self) -> Self {
        let width = (self.end - self.start).clamp(0.001, 1.0);
        let start = self.start.clamp(0.0, 1.0 - width);
        self.start = start;
        self.end = (start + width).min(1.0);
        self
    }

    /// Width of the viewport.
    pub fn width(&self) -> f32 {
        (self.end - self.start).max(0.001)
    }
}

impl Default for WaveformView {
    fn default() -> Self {
        Self {
            start: 0.0,
            end: 1.0,
        }
    }
}

/// Logical focus buckets used to drive contextual keyboard shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusContext {
    /// No UI surface currently owns focus.
    None,
    /// The waveform viewer handles navigation/shortcuts.
    Waveform,
    /// The sample browser rows handle navigation/shortcuts.
    SampleBrowser,
    /// The source folder browser handles navigation/shortcuts.
    SourceFolders,
    /// The collections sample list handles navigation/shortcuts.
    CollectionSample,
    /// The sources list handles navigation/shortcuts.
    SourcesList,
    /// The collections list handles navigation/shortcuts.
    CollectionsList,
}

/// Focus metadata shared between the controller and egui renderer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiFocusState {
    pub context: FocusContext,
}

impl UiFocusState {
    /// Update the active focus context.
    pub fn set_context(&mut self, context: FocusContext) {
        self.context = context;
    }
}

/// Presentation state for the contextual hotkey overlay.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HotkeyUiState {
    pub overlay_visible: bool,
}

/// UI state for audio host/device selection.
#[derive(Clone, Debug, Default)]
pub struct AudioOptionsState {
    pub hosts: Vec<AudioHostView>,
    pub devices: Vec<AudioDeviceView>,
    pub sample_rates: Vec<u32>,
    pub selected: AudioOutputConfig,
    pub applied: Option<ActiveAudioOutput>,
    pub warning: Option<String>,
    pub panel_open: bool,
}

/// Render-friendly audio host descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioHostView {
    pub id: String,
    pub label: String,
    pub is_default: bool,
}

/// Render-friendly audio device descriptor scoped to a host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDeviceView {
    pub host_id: String,
    pub name: String,
    pub is_default: bool,
}

/// Active audio output the player is currently using.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveAudioOutput {
    pub host_id: String,
    pub device_name: String,
    pub sample_rate: u32,
    pub buffer_size_frames: Option<u32>,
    pub channel_count: u16,
}

/// Interaction tuning surfaced in the UI.
#[derive(Clone, Debug)]
pub struct InteractionOptionsState {
    pub invert_waveform_scroll: bool,
    pub waveform_scroll_speed: f32,
    pub wheel_zoom_factor: f32,
    pub keyboard_zoom_factor: f32,
    pub destructive_yolo_mode: bool,
    pub waveform_channel_view: WaveformChannelView,
}

impl Default for InteractionOptionsState {
    fn default() -> Self {
        Self {
            invert_waveform_scroll: true,
            waveform_scroll_speed: 1.2,
            wheel_zoom_factor: 0.96,
            keyboard_zoom_factor: 0.9,
            destructive_yolo_mode: false,
            waveform_channel_view: WaveformChannelView::Mono,
        }
    }
}

/// Destructive selection edits that overwrite audio on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DestructiveSelectionEdit {
    CropSelection,
    TrimSelection,
    FadeLeftToRight,
    FadeRightToLeft,
    MuteSelection,
    NormalizeSelection,
}

/// Confirmation prompt content for destructive edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DestructiveEditPrompt {
    pub edit: DestructiveSelectionEdit,
    pub title: String,
    pub message: String,
}

impl From<&ResolvedOutput> for ActiveAudioOutput {
    fn from(output: &ResolvedOutput) -> Self {
        Self {
            host_id: output.host_id.clone(),
            device_name: output.device_name.clone(),
            sample_rate: output.sample_rate,
            buffer_size_frames: output.buffer_size_frames,
            channel_count: output.channel_count,
        }
    }
}

impl Default for UiFocusState {
    fn default() -> Self {
        Self {
            context: FocusContext::None,
        }
    }
}

/// Current playhead position/visibility.
#[derive(Clone, Debug)]
pub struct PlayheadState {
    pub position: f32,
    pub visible: bool,
}

impl Default for PlayheadState {
    fn default() -> Self {
        Self {
            position: 0.0,
            visible: false,
        }
    }
}

/// Sample browser state for wav entries with filterable rows.
#[derive(Clone, Debug)]
pub struct SampleBrowserState {
    /// Absolute indices per tag for keyboard navigation and tagging.
    pub trash: Vec<usize>,
    pub neutral: Vec<usize>,
    pub keep: Vec<usize>,
    /// Visible rows after applying the active filter.
    pub visible: Vec<usize>,
    /// Focused row used for playback/navigation (mirrors previously “selected”).
    pub selected: Option<SampleBrowserIndex>,
    pub loaded: Option<SampleBrowserIndex>,
    /// Visible row indices for selection/autoscroll (filtered list).
    pub selected_visible: Option<usize>,
    pub loaded_visible: Option<usize>,
    /// Visible row anchor used for range selection (shift + click/arrow).
    pub selection_anchor_visible: Option<usize>,
    /// Paths currently included in the multi-selection set.
    pub selected_paths: Vec<PathBuf>,
    pub autoscroll: bool,
    pub filter: TriageFlagFilter,
    /// Text query applied to visible rows via fuzzy search.
    pub search_query: String,
}

impl Default for SampleBrowserState {
    fn default() -> Self {
        Self {
            trash: Vec::new(),
            neutral: Vec::new(),
            keep: Vec::new(),
            visible: Vec::new(),
            selected: None,
            loaded: None,
            selected_visible: None,
            loaded_visible: None,
            selection_anchor_visible: None,
            selected_paths: Vec::new(),
            autoscroll: false,
            filter: TriageFlagFilter::All,
            search_query: String::new(),
        }
    }
}

/// Identifies a row inside one of the triage flag columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleBrowserIndex {
    pub column: TriageFlagColumn,
    pub row: usize,
}

/// Wav triage flag columns: trash, neutral, keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriageFlagColumn {
    Trash,
    Neutral,
    Keep,
}

/// Filter options for the single-column sample browser view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriageFlagFilter {
    All,
    Keep,
    Trash,
    Untagged,
}

/// Active drag payload carried across UI panels.
#[derive(Clone, Debug, PartialEq)]
pub enum DragPayload {
    Sample {
        source_id: SourceId,
        relative_path: PathBuf,
    },
    Selection {
        source_id: SourceId,
        relative_path: PathBuf,
        bounds: SelectionRange,
    },
}

/// Drag/hover state shared between the sample browser and collections.
#[derive(Clone, Debug, Default)]
pub struct DragState {
    pub payload: Option<DragPayload>,
    pub label: String,
    pub position: Option<Pos2>,
    pub hovering_collection: Option<CollectionId>,
    pub hovering_drop_zone: bool,
    pub hovering_browser: Option<TriageFlagColumn>,
    pub external_started: bool,
}

/// Collections sidebar and sample list state.
#[derive(Clone, Debug)]
pub struct CollectionsState {
    pub enabled: bool,
    pub rows: Vec<CollectionRowView>,
    pub selected: Option<usize>,
    pub samples: Vec<CollectionSampleView>,
    pub drop_ready: bool,
    pub drop_active: bool,
    pub selected_sample: Option<usize>,
}

impl Default for CollectionsState {
    fn default() -> Self {
        Self {
            enabled: true,
            rows: Vec::new(),
            selected: None,
            samples: Vec::new(),
            drop_ready: false,
            drop_active: false,
            selected_sample: None,
        }
    }
}

/// Display data for a collection row.
#[derive(Clone, Debug)]
pub struct CollectionRowView {
    pub id: CollectionId,
    pub name: String,
    pub selected: bool,
    pub count: usize,
    pub export_path: Option<PathBuf>,
    pub missing: bool,
}

/// Display data for a sample inside a collection.
#[derive(Clone, Debug)]
pub struct CollectionSampleView {
    pub source_id: SourceId,
    pub source: String,
    pub path: PathBuf,
    pub label: String,
    pub tag: SampleTag,
    pub missing: bool,
}
