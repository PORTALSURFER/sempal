use super::*;
use crate::ui::WavRow;
use slint::{Model, VecModel};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Shared wav list models that can be updated without rebuilding on every selection change.
pub(super) struct WavModels {
    trash: Rc<VecModel<WavRow>>,
    neutral: Rc<VecModel<WavRow>>,
    keep: Rc<VecModel<WavRow>>,
    trash_lookup: HashMap<PathBuf, usize>,
    neutral_lookup: HashMap<PathBuf, usize>,
    keep_lookup: HashMap<PathBuf, usize>,
    current_selected: Option<(SampleTag, usize)>,
    current_loaded: Option<(SampleTag, usize)>,
}

impl Default for WavModels {
    fn default() -> Self {
        Self::new()
    }
}

impl WavModels {
    /// Create empty wav list models ready for reuse.
    pub fn new() -> Self {
        Self {
            trash: Rc::new(VecModel::default()),
            neutral: Rc::new(VecModel::default()),
            keep: Rc::new(VecModel::default()),
            trash_lookup: HashMap::new(),
            neutral_lookup: HashMap::new(),
            keep_lookup: HashMap::new(),
            current_selected: None,
            current_loaded: None,
        }
    }

    pub fn models(
        &self,
    ) -> (
        Rc<VecModel<WavRow>>,
        Rc<VecModel<WavRow>>,
        Rc<VecModel<WavRow>>,
    ) {
        (self.trash.clone(), self.neutral.clone(), self.keep.clone())
    }

    /// True when the cached lookups cover the current entry set.
    pub fn is_synced(&self, entry_count: usize) -> bool {
        self.trash_lookup.len() + self.neutral_lookup.len() + self.keep_lookup.len() == entry_count
    }

    /// Rebuild all models from the wav entries, updating selection and loaded flags.
    pub fn rebuild(
        &mut self,
        entries: &[WavEntry],
        selected_index: Option<usize>,
        loaded_index: Option<usize>,
    ) -> (Option<(SampleTag, usize)>, Option<String>) {
        self.trash_lookup.clear();
        self.neutral_lookup.clear();
        self.keep_lookup.clear();
        let mut trash_rows = Vec::new();
        let mut neutral_rows = Vec::new();
        let mut keep_rows = Vec::new();
        let mut selected_target = None;
        let mut loaded_target = None;
        let mut loaded_path = None;

        let mut push_row =
            |tag: SampleTag, row: WavRow, path: &PathBuf, selected: bool, loaded: bool| {
                let (rows, lookup) = match tag {
                    SampleTag::Trash => (&mut trash_rows, &mut self.trash_lookup),
                    SampleTag::Neutral => (&mut neutral_rows, &mut self.neutral_lookup),
                    SampleTag::Keep => (&mut keep_rows, &mut self.keep_lookup),
                };
                let index = rows.len();
                rows.push(row);
                lookup.insert(path.clone(), index);
                if selected {
                    selected_target = Some((tag, index));
                }
                if loaded {
                    loaded_target = Some((tag, index));
                }
            };

        for (i, entry) in entries.iter().enumerate() {
            let selected = Some(i) == selected_index;
            let loaded = Some(i) == loaded_index;
            if loaded {
                loaded_path = Some(entry.relative_path.to_string_lossy().to_string());
            }
            let row = DropHandler::wav_row(entry, selected, loaded);
            push_row(entry.tag, row, &entry.relative_path, selected, loaded);
        }

        self.trash = Rc::new(VecModel::from(trash_rows));
        self.neutral = Rc::new(VecModel::from(neutral_rows));
        self.keep = Rc::new(VecModel::from(keep_rows));
        self.current_selected = selected_target;
        self.current_loaded = loaded_target;
        (selected_target, loaded_path)
    }

    /// Update selection and loaded flags in place when the entry set has not changed.
    pub fn update_selection(
        &mut self,
        entries: &[WavEntry],
        selected_path: Option<&Path>,
        loaded_path: Option<&Path>,
    ) -> (Option<(SampleTag, usize)>, Option<String>) {
        if !self.is_synced(entries.len()) {
            let selected_index = locate_entry(entries, selected_path);
            let loaded_index = locate_entry(entries, loaded_path);
            return self.rebuild(entries, selected_index, loaded_index);
        }

        let desired_selected = selected_path.and_then(|path| self.lookup(path));
        let desired_loaded = loaded_path.and_then(|path| self.lookup(path));
        self.update_flags(desired_selected, desired_loaded);

        let loaded_string = loaded_path.map(|p| p.to_string_lossy().to_string());

        (desired_selected, loaded_string)
    }

    fn lookup(&self, path: &Path) -> Option<(SampleTag, usize)> {
        self.trash_lookup
            .get(path)
            .copied()
            .map(|idx| (SampleTag::Trash, idx))
            .or_else(|| {
                self.neutral_lookup
                    .get(path)
                    .copied()
                    .map(|idx| (SampleTag::Neutral, idx))
            })
            .or_else(|| {
                self.keep_lookup
                    .get(path)
                    .copied()
                    .map(|idx| (SampleTag::Keep, idx))
            })
    }

    fn update_flags(
        &mut self,
        desired_selected: Option<(SampleTag, usize)>,
        desired_loaded: Option<(SampleTag, usize)>,
    ) {
        if let Some((tag, idx)) = self.current_selected.take() {
            set_flags_for(&self.model_for(tag), idx, Some(false), None);
        }
        if let Some((tag, idx)) = self.current_loaded.take() {
            set_flags_for(&self.model_for(tag), idx, None, Some(false));
        }
        if let Some((tag, idx)) = desired_selected {
            set_flags_for(&self.model_for(tag), idx, Some(true), None);
        }
        if let Some((tag, idx)) = desired_loaded {
            set_flags_for(&self.model_for(tag), idx, None, Some(true));
        }
        self.current_selected = desired_selected;
        self.current_loaded = desired_loaded;
    }

    fn model_for(&self, tag: SampleTag) -> Rc<VecModel<WavRow>> {
        match tag {
            SampleTag::Trash => self.trash.clone(),
            SampleTag::Neutral => self.neutral.clone(),
            SampleTag::Keep => self.keep.clone(),
        }
    }
}

fn set_flags_for(
    model: &Rc<VecModel<WavRow>>,
    index: usize,
    selected: Option<bool>,
    loaded: Option<bool>,
) {
    if index >= model.row_count() {
        return;
    }
    let Some(mut row) = model.row_data(index) else {
        return;
    };
    if let Some(selected) = selected {
        row.selected = selected;
    }
    if let Some(loaded) = loaded {
        row.loaded = loaded;
    }
    model.set_row_data(index, row);
}

fn locate_entry(entries: &[WavEntry], target: Option<&Path>) -> Option<usize> {
    let target = target?;
    entries
        .iter()
        .position(|entry| entry.relative_path == target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, tag: SampleTag) -> WavEntry {
        WavEntry {
            relative_path: PathBuf::from(path),
            file_size: 0,
            modified_ns: 0,
            tag,
        }
    }

    #[test]
    fn rebuild_splits_by_tag_and_marks_selection() {
        let mut models = WavModels::new();
        let entries = vec![
            entry("a.wav", SampleTag::Trash),
            entry("b.wav", SampleTag::Neutral),
            entry("c.wav", SampleTag::Keep),
        ];
        let (selected, loaded) = models.rebuild(&entries, Some(1), Some(2));
        assert_eq!(selected, Some((SampleTag::Neutral, 0)));
        assert_eq!(loaded, Some("c.wav".to_string()));
        assert_eq!(models.trash.row_count(), 1);
        assert_eq!(models.neutral.row_count(), 1);
        assert_eq!(models.keep.row_count(), 1);
    }

    #[test]
    fn update_selection_marks_rows_without_rebuild() {
        let mut models = WavModels::new();
        let entries = vec![entry("a.wav", SampleTag::Neutral)];
        models.rebuild(&entries, Some(0), None);
        let (selected, loaded) =
            models.update_selection(&entries, Some(Path::new("a.wav")), Some(Path::new("a.wav")));
        assert_eq!(selected, Some((SampleTag::Neutral, 0)));
        assert_eq!(loaded, Some("a.wav".to_string()));
        let row = models.neutral.row_data(0).unwrap();
        assert!(row.selected);
        assert!(row.loaded);
    }
}
