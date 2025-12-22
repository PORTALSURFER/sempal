use std::path::PathBuf;

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
    /// Flag to request focus for the search field in the UI.
    pub search_focus_requested: bool,
    /// When enabled, Up/Down jump through random samples instead of list order.
    pub random_navigation_mode: bool,
    /// Optional predicted category filter (top class) sourced from the latest model.
    pub category_filter: Option<String>,
    /// Minimum prediction confidence required when category filtering is active.
    pub confidence_threshold: f32,
    /// When false, exclude rows predicted as `UNKNOWN` unless explicitly filtering to it.
    pub include_unknowns: bool,
    /// When enabled, show only low-confidence samples for fast review.
    pub review_mode: bool,
    /// Maximum confidence to include during review mode.
    pub review_max_confidence: f32,
    /// When true, include samples that have no prediction row yet.
    pub review_include_unpredicted: bool,
    /// Optional similar-sounds filter scoped to the current source.
    pub similar_query: Option<SimilarQuery>,
    /// Pending inline action for the sample browser rows.
    pub pending_action: Option<SampleBrowserActionPrompt>,
    /// Flag to request focus on the active inline rename editor.
    pub rename_focus_requested: bool,
    /// Active tab in the sample browser area.
    pub active_tab: SampleBrowserTab,
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
            search_focus_requested: false,
            random_navigation_mode: false,
            category_filter: None,
            confidence_threshold: 0.0,
            include_unknowns: true,
            review_mode: false,
            review_max_confidence: 0.55,
            review_include_unpredicted: true,
            similar_query: None,
            pending_action: None,
            rename_focus_requested: false,
            active_tab: SampleBrowserTab::List,
        }
    }
}

/// Holds the current similar-sounds query context.
#[derive(Clone, Debug)]
pub struct SimilarQuery {
    pub sample_id: String,
    pub label: String,
    pub indices: Vec<usize>,
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

/// Pending inline action for the sample browser.
#[derive(Clone, Debug)]
pub enum SampleBrowserActionPrompt {
    Rename { target: PathBuf, name: String },
}

/// Tabs for the sample browser area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleBrowserTab {
    List,
    Map,
}
