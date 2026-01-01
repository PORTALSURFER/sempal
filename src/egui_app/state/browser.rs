use std::path::PathBuf;

/// Sample browser state for wav entries with filterable rows.
#[derive(Clone, Debug)]
pub struct SampleBrowserState {
    /// Absolute indices per tag for keyboard navigation and tagging.
    pub trash: Vec<usize>,
    pub neutral: Vec<usize>,
    pub keep: Vec<usize>,
    /// Visible rows after applying the active filter.
    pub visible: VisibleRows,
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
    /// Last focused browser item to restore focus after context changes.
    pub last_focused_path: Option<PathBuf>,
    pub autoscroll: bool,
    pub filter: TriageFlagFilter,
    /// Text query applied to visible rows via fuzzy search.
    pub search_query: String,
    /// Flag to request focus for the search field in the UI.
    pub search_focus_requested: bool,
    /// When enabled, Up/Down jump through random samples instead of list order.
    pub random_navigation_mode: bool,
    /// Sorting mode for the sample browser list.
    pub sort: SampleBrowserSort,
    /// True when similarity sorting should follow the loaded sample.
    pub similarity_sort_follow_loaded: bool,
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
            visible: VisibleRows::List(Vec::new()),
            selected: None,
            loaded: None,
            selected_visible: None,
            loaded_visible: None,
            selection_anchor_visible: None,
            selected_paths: Vec::new(),
            last_focused_path: None,
            autoscroll: false,
            filter: TriageFlagFilter::All,
            search_query: String::new(),
            search_focus_requested: false,
            random_navigation_mode: false,
            sort: SampleBrowserSort::ListOrder,
            similarity_sort_follow_loaded: false,
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
    /// Similarity scores aligned with `indices` (0.0 = least similar, 1.0 = most similar).
    pub scores: Vec<f32>,
    pub anchor_index: Option<usize>,
}

impl SimilarQuery {
    pub fn score_for_index(&self, entry_index: usize) -> Option<f32> {
        let position = self.indices.iter().position(|idx| *idx == entry_index)?;
        self.scores.get(position).copied()
    }

    pub fn display_strength_for_index(&self, entry_index: usize) -> Option<f32> {
        let position = self.indices.iter().position(|idx| *idx == entry_index)?;
        let score = *self.scores.get(position)?;
        if score < -1.0 {
            return Some(0.0);
        }
        let mut min_score = f32::INFINITY;
        let mut max_score = f32::NEG_INFINITY;
        for &value in &self.scores {
            if !value.is_finite() {
                continue;
            }
            if value < -1.0 {
                continue;
            }
            min_score = min_score.min(value);
            max_score = max_score.max(value);
        }
        let range = max_score - min_score;
        let normalized = if range.is_finite() && range > 1.0e-4 {
            (score - min_score) / range
        } else if self.scores.len() > 1 {
            1.0 - (position as f32 / (self.scores.len() - 1) as f32)
        } else {
            1.0
        };
        Some(normalized.clamp(0.0, 1.0))
    }
}

/// Visible list representation for the sample browser.
#[derive(Clone, Debug)]
pub enum VisibleRows {
    All { total: usize },
    List(Vec<usize>),
}

impl VisibleRows {
    pub fn len(&self) -> usize {
        match self {
            VisibleRows::All { total } => *total,
            VisibleRows::List(rows) => rows.len(),
        }
    }

    pub fn get(&self, row: usize) -> Option<usize> {
        match self {
            VisibleRows::All { total } => (row < *total).then_some(row),
            VisibleRows::List(rows) => rows.get(row).copied(),
        }
    }

    pub fn clear_to_list(&mut self) {
        *self = VisibleRows::List(Vec::new());
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

/// Sort modes for the sample browser list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleBrowserSort {
    ListOrder,
    Similarity,
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
