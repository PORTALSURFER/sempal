use std::path::{Path, PathBuf};

use wavecrate::sample_sources::{
    SampleCollection, SourceMetadataStorage, SourceRole, config::SimilarityAspectSettings,
};

use super::{FolderEntry, collections::MissingCollectionSnapshot, path_helpers::rewrite_path_id};

pub(in crate::native_app) type SimilarityAspectStrengths =
    [Option<f32>; wavecrate_analysis::aspects::ASPECT_COUNT];

pub(in crate::native_app) const EMPTY_SIMILARITY_ASPECT_STRENGTHS: SimilarityAspectStrengths =
    [None; wavecrate_analysis::aspects::ASPECT_COUNT];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum SourceAvailability {
    Available,
    Missing,
}

impl SourceAvailability {
    fn from_root_path(path: &Path) -> Self {
        if wavecrate::sample_sources::harvest_file_ops::source_root_available(path) {
            Self::Available
        } else {
            Self::Missing
        }
    }

    pub(in crate::native_app) fn is_missing(self) -> bool {
        self == Self::Missing
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct SourceEntry {
    pub(in crate::native_app) id: String,
    pub(in crate::native_app) label: String,
    pub(super) root: PathBuf,
    pub(super) database_root: PathBuf,
    pub(in crate::native_app) role: SourceRole,
    pub(in crate::native_app) availability: SourceAvailability,
    pub(in crate::native_app) metadata_storage: SourceMetadataStorage,
    pub(in crate::native_app) primary_import_folder: PathBuf,
    pub(super) root_folder: Option<FolderEntry>,
    pub(super) parked_tree_loaded: bool,
    pub(super) missing_collection_snapshot: MissingCollectionSnapshot,
    pub(in crate::native_app) loading_task: Option<u64>,
    pub(super) projection_revision: Option<u64>,
}

impl SourceEntry {
    pub(in crate::native_app) fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        root: PathBuf,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            database_root: root.clone(),
            root,
            role: SourceRole::Normal,
            availability: SourceAvailability::Available,
            metadata_storage: SourceMetadataStorage::SourceFolder,
            primary_import_folder: wavecrate::sample_sources::default_primary_import_folder(),
            root_folder: None,
            parked_tree_loaded: false,
            missing_collection_snapshot: MissingCollectionSnapshot::default(),
            loading_task: None,
            projection_revision: None,
        }
    }

    pub(in crate::native_app) fn from_sample_source(
        source: &wavecrate::sample_sources::SampleSource,
    ) -> Self {
        let database_root = source.database_root().unwrap_or_else(|error| {
            tracing::warn!(
                source_id = source.id.as_str(),
                root = %source.root.display(),
                "failed to resolve source metadata root: {error}"
            );
            source.root.clone()
        });
        Self {
            id: source.id.as_str().to_string(),
            label: super::path_helpers::folder_label(&source.root),
            root: source.root.clone(),
            database_root,
            role: source.role,
            availability: SourceAvailability::from_root_path(&source.root),
            metadata_storage: source.metadata_storage,
            primary_import_folder: source.primary_import_folder.clone(),
            root_folder: None,
            parked_tree_loaded: false,
            missing_collection_snapshot: MissingCollectionSnapshot::default(),
            loading_task: None,
            projection_revision: None,
        }
    }

    pub(super) fn is_default_assets_source(&self) -> bool {
        self.id == "assets" && self.root.ends_with("assets")
    }

    pub(super) fn is_protected(&self) -> bool {
        self.role.protects_files()
    }

    pub(super) fn is_primary(&self) -> bool {
        self.role == SourceRole::Primary
    }

    pub(in crate::native_app) fn is_missing(&self) -> bool {
        self.availability.is_missing()
    }

    pub(super) fn refresh_availability_from_disk(&mut self) -> SourceAvailability {
        self.availability = SourceAvailability::from_root_path(&self.root);
        self.availability
    }

    pub(super) fn apply_observed_availability(&mut self, available: bool) -> SourceAvailability {
        self.availability = if available {
            SourceAvailability::Available
        } else {
            SourceAvailability::Missing
        };
        self.availability
    }

    pub(super) fn mark_available(&mut self) {
        self.availability = SourceAvailability::Available;
    }

    pub(super) fn mark_missing(&mut self) {
        self.availability = SourceAvailability::Missing;
        self.loading_task = None;
    }

    #[cfg(test)]
    pub(in crate::native_app) fn mark_missing_for_tests(&mut self) {
        self.mark_missing();
    }

    pub(super) fn apply_role(&mut self, role: SourceRole) -> Result<(), String> {
        let mut source = self.as_sample_source();
        source.role = role;
        source.metadata_storage = match role {
            SourceRole::Protected => SourceMetadataStorage::AppData,
            SourceRole::Primary | SourceRole::Normal => SourceMetadataStorage::SourceFolder,
        };
        let database_root = source
            .database_root()
            .map_err(|err| format!("Resolve source metadata location failed: {err}"))?;
        self.role = source.role;
        self.metadata_storage = source.metadata_storage;
        self.database_root = database_root;
        Ok(())
    }

    pub(crate) fn as_sample_source(&self) -> wavecrate::sample_sources::SampleSource {
        wavecrate::sample_sources::SampleSource {
            id: wavecrate::sample_sources::SourceId::from_string(self.id.clone()),
            root: self.root.clone(),
            role: self.role,
            metadata_storage: self.metadata_storage,
            primary_import_folder: self.primary_import_folder.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FolderRenameEdit {
    pub(super) folder_id: String,
    pub(super) draft: String,
    pub(super) input_id: u64,
    pub(super) kind: FolderRenameKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum FolderRenameKind {
    Rename,
    Create { parent_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileRenameEdit {
    pub(super) file_id: String,
    pub(super) draft: String,
    pub(super) input_id: u64,
    pub(super) selection_start: usize,
    pub(super) selection_end: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct FileColumn {
    pub(super) kind: FileColumnKind,
    pub(in crate::native_app) id: String,
    pub(in crate::native_app) label: String,
    pub(in crate::native_app) width: f32,
}

impl FileColumn {
    pub(in crate::native_app) fn kind(&self) -> FileColumnKind {
        self.kind
    }

    #[cfg(test)]
    pub(in crate::native_app) fn for_tests(id: &str, label: &str, width: f32) -> Self {
        file_column_with(
            FileColumnKind::from_id(id).unwrap_or(FileColumnKind::Name),
            label,
            width,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum FileColumnKind {
    Name,
    Curation,
    Harvest,
    Rating,
    PlaybackType,
    Collection,
    SourceFolder,
    Extension,
    Size,
    Modified,
    Kind,
    Path,
    Similarity,
}

impl FileColumnKind {
    pub(super) const DEFAULT_VISIBLE: [Self; 10] = [
        Self::Name,
        Self::Harvest,
        Self::SourceFolder,
        Self::Rating,
        Self::PlaybackType,
        Self::Curation,
        Self::Collection,
        Self::Extension,
        Self::Size,
        Self::Modified,
    ];

    pub(super) fn from_id(id: &str) -> Option<Self> {
        match id {
            "name" => Some(Self::Name),
            "curation" => Some(Self::Curation),
            "harvest" => Some(Self::Harvest),
            "rating" => Some(Self::Rating),
            "playback_type" => Some(Self::PlaybackType),
            "collection" => Some(Self::Collection),
            "source_folder" => Some(Self::SourceFolder),
            "extension" => Some(Self::Extension),
            "size" => Some(Self::Size),
            "modified" => Some(Self::Modified),
            "kind" => Some(Self::Kind),
            "path" => Some(Self::Path),
            "similarity" => Some(Self::Similarity),
            _ => None,
        }
    }

    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Curation => "curation",
            Self::Harvest => "harvest",
            Self::Rating => "rating",
            Self::PlaybackType => "playback_type",
            Self::Collection => "collection",
            Self::SourceFolder => "source_folder",
            Self::Extension => "extension",
            Self::Size => "size",
            Self::Modified => "modified",
            Self::Kind => "kind",
            Self::Path => "path",
            Self::Similarity => "similarity",
        }
    }

    fn default_label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Curation => "Curation",
            Self::Harvest => "Harvest",
            Self::Rating => "Rating",
            Self::PlaybackType => "Type",
            Self::Collection => "Col",
            Self::SourceFolder => "Folder",
            Self::Extension => "Ext",
            Self::Size => "Size",
            Self::Modified => "Last Played",
            Self::Kind => "Kind",
            Self::Path => "Path",
            Self::Similarity => "Sim",
        }
    }

    fn default_width(self) -> f32 {
        match self {
            Self::Name => 240.0,
            Self::Curation => 112.0,
            Self::Harvest => 74.0,
            Self::Rating => 68.0,
            Self::PlaybackType => 76.0,
            Self::Collection => 58.0,
            Self::SourceFolder => 220.0,
            Self::Extension => 54.0,
            Self::Size => 78.0,
            Self::Modified => 112.0,
            Self::Kind => 78.0,
            Self::Path => 220.0,
            Self::Similarity => 58.0,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum FolderBrowserDrag {
    Folder {
        folder_ids: Vec<String>,
    },
    Files {
        file_ids: Vec<String>,
        remove_from_collection: Option<SampleCollection>,
    },
    ExtractedFile {
        path: PathBuf,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct SimilarityBrowserState {
    anchor_id: String,
    controls: SimilarityAspectSettings,
    scores_by_file: std::collections::HashMap<String, f32>,
    aspect_scores_by_file: std::collections::HashMap<String, SimilarityAspectStrengths>,
    effective_scores_by_file: std::collections::HashMap<String, f32>,
    effective_score_bounds: Option<(f32, f32)>,
    aspect_score_bounds: [Option<(f32, f32)>; wavecrate_analysis::aspects::ASPECT_COUNT],
}

impl SimilarityBrowserState {
    pub(in crate::native_app) fn new(
        anchor_id: String,
        controls: SimilarityAspectSettings,
    ) -> Self {
        Self::with_scores(anchor_id, controls, std::collections::HashMap::new())
    }

    pub(in crate::native_app) fn with_scores(
        anchor_id: String,
        controls: SimilarityAspectSettings,
        scores_by_file: std::collections::HashMap<String, f32>,
    ) -> Self {
        Self::with_scores_and_aspects(
            anchor_id,
            controls,
            scores_by_file,
            std::collections::HashMap::new(),
        )
    }

    pub(in crate::native_app) fn with_scores_and_aspects(
        anchor_id: String,
        controls: SimilarityAspectSettings,
        mut scores_by_file: std::collections::HashMap<String, f32>,
        mut aspect_scores_by_file: std::collections::HashMap<String, SimilarityAspectStrengths>,
    ) -> Self {
        scores_by_file.retain(|_, score| score.is_finite());
        aspect_scores_by_file.retain(|_, row| row.iter().any(Option::is_some));
        scores_by_file.insert(anchor_id.clone(), 1.0);
        let aspect_score_bounds = aspect_score_bounds(aspect_scores_by_file.values());
        let controls = controls.normalized();
        let (effective_scores_by_file, effective_score_bounds) =
            effective_scores(&controls, &scores_by_file, &aspect_scores_by_file);
        Self {
            anchor_id,
            controls,
            scores_by_file,
            aspect_scores_by_file,
            effective_scores_by_file,
            effective_score_bounds,
            aspect_score_bounds,
        }
    }

    pub(in crate::native_app) fn anchor_id(&self) -> &str {
        &self.anchor_id
    }

    pub(in crate::native_app) fn effective_score_for(&self, file_id: &str) -> Option<f32> {
        self.effective_scores_by_file.get(file_id).copied()
    }

    pub(in crate::native_app) fn controls(&self) -> &SimilarityAspectSettings {
        &self.controls
    }

    pub(in crate::native_app) fn set_controls(&mut self, controls: SimilarityAspectSettings) {
        let controls = controls.normalized();
        if self.controls == controls {
            return;
        }
        self.controls = controls;
        let (scores, bounds) = effective_scores(
            &self.controls,
            &self.scores_by_file,
            &self.aspect_scores_by_file,
        );
        self.effective_scores_by_file = scores;
        self.effective_score_bounds = bounds;
    }

    pub(in crate::native_app) fn rewrite_path_prefix(
        &mut self,
        old_path: &Path,
        new_path: &Path,
    ) -> bool {
        let old_anchor_id = self.anchor_id.clone();
        self.anchor_id = rewrite_path_id(&self.anchor_id, old_path, new_path);
        let scores_by_file = rewrite_similarity_score_map(&self.scores_by_file, old_path, new_path);
        let aspect_scores_by_file =
            rewrite_similarity_score_map(&self.aspect_scores_by_file, old_path, new_path);
        let changed = old_anchor_id != self.anchor_id
            || scores_by_file != self.scores_by_file
            || aspect_scores_by_file != self.aspect_scores_by_file;
        if changed {
            self.scores_by_file = scores_by_file;
            self.aspect_scores_by_file = aspect_scores_by_file;
            self.aspect_score_bounds = aspect_score_bounds(self.aspect_scores_by_file.values());
            let (scores, bounds) = effective_scores(
                &self.controls,
                &self.scores_by_file,
                &self.aspect_scores_by_file,
            );
            self.effective_scores_by_file = scores;
            self.effective_score_bounds = bounds;
        }
        changed
    }

    pub(in crate::native_app) fn display_strength_for(&self, file_id: &str) -> Option<f32> {
        let score = self.effective_score_for(file_id)?.clamp(-1.0, 1.0);
        let (min_score, max_score) = self.effective_score_bounds?;
        let range = max_score - min_score;
        if range <= f32::EPSILON {
            return Some(absolute_display_strength(score));
        }
        Some(((score - min_score) / range).clamp(0.0, 1.0))
    }

    pub(in crate::native_app) fn aspect_display_strengths_for(
        &self,
        file_id: &str,
    ) -> SimilarityAspectStrengths {
        let Some(row) = self.aspect_scores_by_file.get(file_id) else {
            return EMPTY_SIMILARITY_ASPECT_STRENGTHS;
        };
        let mut strengths = EMPTY_SIMILARITY_ASPECT_STRENGTHS;
        for aspect in wavecrate_analysis::aspects::SimilarityAspect::ORDER {
            let index = aspect.index();
            if self.controls.aspect_enabled(aspect) {
                strengths[index] = self.aspect_display_strength(index, row[index]);
            }
        }
        strengths
    }

    fn aspect_display_strength(&self, index: usize, score: Option<f32>) -> Option<f32> {
        let score = score?.clamp(-1.0, 1.0);
        let (min_score, max_score) = self.aspect_score_bounds[index]?;
        let range = max_score - min_score;
        if range <= f32::EPSILON {
            return Some(absolute_display_strength(score));
        }
        Some(((score - min_score) / range).clamp(0.0, 1.0))
    }
}

fn rewrite_similarity_score_map<T: Clone>(
    scores: &std::collections::HashMap<String, T>,
    old_path: &Path,
    new_path: &Path,
) -> std::collections::HashMap<String, T> {
    scores
        .iter()
        .map(|(file_id, score)| (rewrite_path_id(file_id, old_path, new_path), score.clone()))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct VisibleFolder {
    pub(in crate::native_app) id: String,
    pub(in crate::native_app) name: String,
    pub(in crate::native_app) depth: usize,
    pub(in crate::native_app) is_source_root: bool,
    pub(in crate::native_app) has_children: bool,
    pub(in crate::native_app) empty: bool,
    pub(in crate::native_app) locked: bool,
    pub(in crate::native_app) lock_inherited: bool,
    pub(in crate::native_app) expanded: bool,
    pub(in crate::native_app) selected: bool,
    pub(in crate::native_app) focused: bool,
    pub(in crate::native_app) focus_alpha: u8,
    pub(in crate::native_app) selection_flash: bool,
    pub(in crate::native_app) drag_active: bool,
    pub(in crate::native_app) drag_source: bool,
    pub(in crate::native_app) drop_candidate: bool,
    pub(in crate::native_app) drop_target: bool,
    pub(in crate::native_app) drop_target_active: bool,
    pub(in crate::native_app) rename_draft: Option<String>,
    pub(in crate::native_app) rename_input_id: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FolderSelectionToggleResult {
    pub(in crate::native_app) folder_id: String,
    pub(in crate::native_app) selected: bool,
    pub(in crate::native_app) selected_count: usize,
}

pub(super) fn default_file_columns() -> Vec<FileColumn> {
    FileColumnKind::DEFAULT_VISIBLE
        .into_iter()
        .map(file_column)
        .collect()
}

fn file_column(kind: FileColumnKind) -> FileColumn {
    file_column_with(kind, kind.default_label(), kind.default_width())
}

fn file_column_with(kind: FileColumnKind, label: &str, width: f32) -> FileColumn {
    FileColumn {
        kind,
        id: kind.id().to_owned(),
        label: label.to_owned(),
        width,
    }
}

fn score_bounds(scores: impl IntoIterator<Item = f32>) -> Option<(f32, f32)> {
    let mut scores = scores.into_iter().map(|score| score.clamp(-1.0, 1.0));
    let first = scores.next()?;
    let mut min_score = first;
    let mut max_score = first;
    for score in scores {
        min_score = min_score.min(score);
        max_score = max_score.max(score);
    }
    Some((min_score, max_score))
}

fn aspect_score_bounds<'a>(
    rows: impl IntoIterator<Item = &'a SimilarityAspectStrengths>,
) -> [Option<(f32, f32)>; wavecrate_analysis::aspects::ASPECT_COUNT] {
    let rows = rows.into_iter().collect::<Vec<_>>();
    std::array::from_fn(|index| score_bounds(rows.iter().filter_map(|row| row[index])))
}

fn effective_scores(
    controls: &SimilarityAspectSettings,
    scores_by_file: &std::collections::HashMap<String, f32>,
    aspect_scores_by_file: &std::collections::HashMap<String, SimilarityAspectStrengths>,
) -> (std::collections::HashMap<String, f32>, Option<(f32, f32)>) {
    let mut effective_scores_by_file =
        std::collections::HashMap::with_capacity(scores_by_file.len());
    for (file_id, score) in scores_by_file {
        let row = aspect_scores_by_file
            .get(file_id)
            .unwrap_or(&EMPTY_SIMILARITY_ASPECT_STRENGTHS);
        if let Some(effective) = controls.effective_score(Some(*score), row) {
            effective_scores_by_file.insert(file_id.clone(), effective);
        }
    }
    let effective_score_bounds = score_bounds(effective_scores_by_file.values().copied());
    (effective_scores_by_file, effective_score_bounds)
}

fn absolute_display_strength(score: f32) -> f32 {
    let normalized = ((score.clamp(-1.0, 1.0) + 1.0) * 0.5).clamp(0.0, 1.0);
    normalized.powf(2.0)
}
