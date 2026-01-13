use std::path::PathBuf;

/// Sample browser state for wav entries with filterable rows.
#[derive(Clone, Debug)]
pub struct SampleBrowserState {
    /// Absolute indices per tag for keyboard navigation and tagging.
    pub trash: Vec<usize>,
    /// Absolute indices for neutral-tagged rows.
    pub neutral: Vec<usize>,
    /// Absolute indices for keep-tagged rows.
    pub keep: Vec<usize>,
    /// Visible rows after applying the active filter.
    pub visible: VisibleRows,
    /// Focused row used for playback/navigation (mirrors previously “selected”).
    pub selected: Option<SampleBrowserIndex>,
    /// Loaded row used for playback.
    pub loaded: Option<SampleBrowserIndex>,
    /// Visible row indices for selection/autoscroll (filtered list).
    pub selected_visible: Option<usize>,
    /// Visible index for the loaded row, if any.
    pub loaded_visible: Option<usize>,
    /// Visible row anchor used for range selection (shift + click/arrow).
    pub selection_anchor_visible: Option<usize>,
    /// Paths currently included in the multi-selection set.
    pub selected_paths: Vec<PathBuf>,
    /// Last focused browser item to restore focus after context changes.
    pub last_focused_path: Option<PathBuf>,
    /// Whether autoscroll is enabled for selection changes.
    pub autoscroll: bool,
    /// Active triage filter.
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
    /// Near-duplicate highlight set for the focused sample.
    pub focused_similarity: Option<FocusedSimilarity>,
    /// Pending inline action for the sample browser rows.
    pub pending_action: Option<SampleBrowserActionPrompt>,
    /// Flag to request focus on the active inline rename editor.
    pub rename_focus_requested: bool,
    /// Active tab in the sample browser area.
    pub active_tab: SampleBrowserTab,
    /// True when a background search/filter job is running.
    pub search_busy: bool,
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
            focused_similarity: None,
            pending_action: None,
            rename_focus_requested: false,
            active_tab: SampleBrowserTab::List,
            search_busy: false,
        }
    }
}

/// Holds the current similar-sounds query context.
#[derive(Clone, Debug)]
pub struct SimilarQuery {
    /// Sample id used as the similarity anchor.
    pub sample_id: String,
    /// Display label for the anchor sample.
    pub label: String,
    /// Entry indices in similarity order.
    pub indices: Vec<usize>,
    /// Similarity scores aligned with `indices` (0.0 = least similar, 1.0 = most similar).
    pub scores: Vec<f32>,
    /// Optional anchor index in the visible list.
    pub anchor_index: Option<usize>,
}

impl SimilarQuery {
    /// Return the raw similarity score for a given entry index.
    pub fn score_for_index(&self, entry_index: usize) -> Option<f32> {
        let position = self.indices.iter().position(|idx| *idx == entry_index)?;
        self.scores.get(position).copied()
    }

    /// Return a normalized similarity strength for UI display.
    pub fn display_strength_for_index(&self, entry_index: usize) -> Option<f32> {
        let position = self.indices.iter().position(|idx| *idx == entry_index)?;
        let score = *self.scores.get(position)?;
        // Use absolute scoring to match FocusedSimilarity and avoid misleading "relative best" highlighting.
        let normalized = ((score.clamp(-1.0, 1.0) + 1.0) * 0.5).clamp(0.0, 1.0);
        Some(normalized.powf(2.0))
    }
}

/// Highlight metadata for near-duplicate rows relative to the focused sample.
#[derive(Clone, Debug)]
pub struct FocusedSimilarity {
    /// Sample id used as the highlight anchor.
    pub sample_id: String,
    /// Entry indices for near-duplicate matches.
    pub indices: Vec<usize>,
    /// Similarity scores aligned with `indices`.
    pub scores: Vec<f32>,
    /// Absolute index of the focused sample, when known.
    pub anchor_index: Option<usize>,
}

impl FocusedSimilarity {
    /// Return the raw similarity score for a given entry index.
    pub fn score_for_index(&self, entry_index: usize) -> Option<f32> {
        let position = self.indices.iter().position(|idx| *idx == entry_index)?;
        self.scores.get(position).copied()
    }
}

/// Visible list representation for the sample browser.
#[derive(Clone, Debug)]
pub enum VisibleRows {
    /// All rows are visible; total stores the count.
    All {
        /// Total number of rows.
        total: usize,
    },
    /// Only the provided indices are visible.
    List(Vec<usize>),
}

impl VisibleRows {
    /// Return the number of visible rows.
    pub fn len(&self) -> usize {
        match self {
            VisibleRows::All { total } => *total,
            VisibleRows::List(rows) => rows.len(),
        }
    }

    /// Map a visible row index to an absolute index.
    pub fn get(&self, row: usize) -> Option<usize> {
        match self {
            VisibleRows::All { total } => (row < *total).then_some(row),
            VisibleRows::List(rows) => rows.get(row).copied(),
        }
    }

    /// Reset the visible rows to an empty list.
    pub fn clear_to_list(&mut self) {
        *self = VisibleRows::List(Vec::new());
    }

    /// Iterate over visible absolute indices.
    pub fn iter(&self) -> Box<dyn Iterator<Item = usize> + '_> {
        match self {
            VisibleRows::All { total } => Box::new(0..*total),
            VisibleRows::List(rows) => Box::new(rows.iter().copied()),
        }
    }

    /// Find the visible position for an absolute index.
    pub fn position(&self, index: usize) -> Option<usize> {
        match self {
            VisibleRows::All { total } => (index < *total).then_some(index),
            VisibleRows::List(rows) => rows.iter().position(|i| *i == index),
        }
    }
}

/// Identifies a row inside one of the triage flag columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleBrowserIndex {
    /// Column containing the row.
    pub column: TriageFlagColumn,
    /// Row index within the column.
    pub row: usize,
}

/// Wav triage flag columns: trash, neutral, keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriageFlagColumn {
    /// Trash column.
    Trash,
    /// Neutral column.
    Neutral,
    /// Keep column.
    Keep,
}

/// Filter options for the single-column sample browser view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriageFlagFilter {
    /// Show all triage flags.
    All,
    /// Show keep-only rows.
    Keep,
    /// Show trash-only rows.
    Trash,
    /// Show untagged rows only.
    Untagged,
}

/// Sort modes for the sample browser list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleBrowserSort {
    /// Preserve the original list order.
    ListOrder,
    /// Sort by similarity score.
    Similarity,
    /// Sort by playback age ascending.
    PlaybackAgeAsc,
    /// Sort by playback age descending.
    PlaybackAgeDesc,
}

/// Pending inline action for the sample browser.
#[derive(Clone, Debug)]
pub enum SampleBrowserActionPrompt {
    /// Rename the selected entry.
    Rename {
        /// Path to rename.
        target: PathBuf,
        /// New name.
        name: String,
    },
}

/// Tabs for the sample browser area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleBrowserTab {
    /// List view tab.
    List,
    /// Map view tab.
    Map,
}
