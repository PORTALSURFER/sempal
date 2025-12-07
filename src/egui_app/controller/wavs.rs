use super::*;
use crate::waveform::DecodedWaveform;

impl EguiController {
    /// Expose wav indices for a given triage column (used by virtualized rendering).
    pub fn triage_indices(&self, column: TriageColumn) -> &[usize] {
        match column {
            TriageColumn::Trash => &self.ui.triage.trash,
            TriageColumn::Neutral => &self.ui.triage.neutral,
            TriageColumn::Keep => &self.ui.triage.keep,
        }
    }

    /// Visible wav indices after applying the active triage filter.
    pub fn visible_triage_indices(&self) -> &[usize] {
        &self.ui.triage.visible
    }

    /// Select a wav row based on its path.
    pub fn select_wav_by_path(&mut self, path: &Path) {
        if self.wav_lookup.contains_key(path) {
            self.selected_wav = Some(path.to_path_buf());
            if let Some(source) = self.current_source() {
                if let Err(err) = self.load_waveform_for_selection(&source, path) {
                    self.set_status(err, StatusTone::Error);
                } else if self.feature_flags.autoplay_selection && !self.suppress_autoplay_once {
                    let _ = self.play_audio(self.ui.waveform.loop_enabled, None);
                } else {
                    self.suppress_autoplay_once = false;
                }
            }
            self.rebuild_triage_lists();
        }
    }

    /// Map the current triage filter into a drop target tag for drag-and-drop retagging.
    pub fn triage_drop_target(&self) -> TriageColumn {
        match self.ui.triage.filter {
            TriageFilter::All | TriageFilter::Untagged => TriageColumn::Neutral,
            TriageFilter::Keep => TriageColumn::Keep,
            TriageFilter::Trash => TriageColumn::Trash,
        }
    }

    /// Current tag of the selected wav, if any.
    pub fn selected_tag(&self) -> Option<SampleTag> {
        self.selected_row_index()
            .and_then(|idx| self.wav_entries.get(idx))
            .map(|entry| entry.tag)
    }

    /// Apply a new triage filter and refresh visible rows.
    pub fn set_triage_filter(&mut self, filter: TriageFilter) {
        if self.ui.triage.filter != filter {
            self.ui.triage.filter = filter;
            self.rebuild_triage_lists();
        }
    }

    /// Select a wav by absolute index into the full wav list.
    pub fn select_wav_by_index(&mut self, index: usize) {
        let path = match self.wav_entries.get(index) {
            Some(entry) => entry.relative_path.clone(),
            None => return,
        };
        self.select_wav_by_path(&path);
    }

    /// Select a wav coming from the triage columns and clear collection focus.
    pub fn select_from_triage(&mut self, path: &Path) {
        self.ui.collections.selected_sample = None;
        self.select_wav_by_path(path);
    }

    /// Retrieve a wav entry by absolute index.
    pub fn wav_entry(&self, index: usize) -> Option<&WavEntry> {
        self.wav_entries.get(index)
    }

    /// Retrieve a cached label for a wav entry by index.
    pub fn wav_label(&mut self, index: usize) -> Option<String> {
        self.label_for(index)
    }

    pub(super) fn rebuild_wav_lookup(&mut self) {
        self.wav_lookup.clear();
        for (index, entry) in self.wav_entries.iter().enumerate() {
            self.wav_lookup.insert(entry.relative_path.clone(), index);
        }
    }

    pub(super) fn rebuild_triage_lists(&mut self) {
        if self.ui.collections.selected_sample.is_some() {
            self.ui.triage.autoscroll = false;
        }
        let highlight_selection = self.ui.collections.selected_sample.is_none();
        let selected_index = if highlight_selection {
            self.selected_row_index()
        } else {
            None
        };
        let loaded_index = if highlight_selection {
            self.loaded_row_index()
        } else {
            None
        };
        self.reset_triage_ui();

        for i in 0..self.wav_entries.len() {
            let tag = self.wav_entries[i].tag;
            let flags = RowFlags {
                selected: Some(i) == selected_index,
                loaded: Some(i) == loaded_index,
            };
            self.push_triage_row(i, tag, flags);
        }
    }

    pub(super) fn selected_row_index(&self) -> Option<usize> {
        self.selected_wav
            .as_ref()
            .and_then(|path| self.wav_lookup.get(path).copied())
    }

    pub(super) fn loaded_row_index(&self) -> Option<usize> {
        self.loaded_wav
            .as_ref()
            .and_then(|path| self.wav_lookup.get(path).copied())
    }

    fn reset_triage_ui(&mut self) {
        let autoscroll = self.ui.triage.autoscroll;
        let collections_selected = self.ui.collections.selected_sample.is_some();
        self.ui.triage.trash.clear();
        self.ui.triage.neutral.clear();
        self.ui.triage.keep.clear();
        self.ui.triage.visible.clear();
        self.ui.triage.selected_visible = None;
        if collections_selected {
            self.ui.triage.selected = None;
        }
        self.ui.triage.loaded = None;
        self.ui.triage.loaded_visible = None;
        self.ui.triage.autoscroll = autoscroll && !collections_selected;
        self.ui.loaded_wav = None;
    }

    fn push_triage_row(&mut self, entry_index: usize, tag: SampleTag, flags: RowFlags) {
        let target = match tag {
            SampleTag::Trash => &mut self.ui.triage.trash,
            SampleTag::Neutral => &mut self.ui.triage.neutral,
            SampleTag::Keep => &mut self.ui.triage.keep,
        };
        let row_index = target.len();
        target.push(entry_index);
        if self.triage_filter_accepts(tag) {
            let visible_row = self.ui.triage.visible.len();
            self.ui.triage.visible.push(entry_index);
            if flags.selected {
                self.ui.triage.selected_visible = Some(visible_row);
            }
            if flags.loaded {
                self.ui.triage.loaded_visible = Some(visible_row);
            }
        }
        if flags.selected {
            self.ui.triage.selected = Some(view_model::triage_index_for(tag, row_index));
        }
        if flags.loaded {
            self.ui.triage.loaded = Some(view_model::triage_index_for(tag, row_index));
            if let Some(path) = self.wav_entries.get(entry_index) {
                self.ui.loaded_wav = Some(path.relative_path.clone());
            }
        }
    }

    fn triage_filter_accepts(&self, tag: SampleTag) -> bool {
        match self.ui.triage.filter {
            TriageFilter::All => true,
            TriageFilter::Keep => matches!(tag, SampleTag::Keep),
            TriageFilter::Trash => matches!(tag, SampleTag::Trash),
            TriageFilter::Untagged => matches!(tag, SampleTag::Neutral),
        }
    }

    pub(super) fn set_sample_tag(
        &mut self,
        path: &Path,
        column: TriageColumn,
    ) -> Result<(), String> {
        let target_tag = match column {
            TriageColumn::Trash => SampleTag::Trash,
            TriageColumn::Neutral => SampleTag::Neutral,
            TriageColumn::Keep => SampleTag::Keep,
        };
        self.set_sample_tag_value(path, target_tag)
    }

    pub(super) fn set_sample_tag_value(
        &mut self,
        path: &Path,
        target_tag: SampleTag,
    ) -> Result<(), String> {
        let Some(source) = self.current_source() else {
            return Err("Select a source first".into());
        };
        self.set_sample_tag_for_source(&source, path, target_tag, true)
    }

    pub(super) fn set_sample_tag_for_source(
        &mut self,
        source: &SampleSource,
        path: &Path,
        target_tag: SampleTag,
        require_present: bool,
    ) -> Result<(), String> {
        let db = self.database_for(source).map_err(|err| err.to_string())?;
        self.apply_tag_to_caches(source, path, target_tag, require_present)?;
        let _ = db.set_tag(path, target_tag);
        if self.selected_source.as_ref() == Some(&source.id) {
            self.rebuild_triage_lists();
            if require_present {
                self.select_wav_by_path(path);
            }
        }
        Ok(())
    }

    fn apply_tag_to_caches(
        &mut self,
        source: &SampleSource,
        path: &Path,
        target_tag: SampleTag,
        require_present: bool,
    ) -> Result<(), String> {
        if self.selected_source.as_ref() == Some(&source.id) {
            if let Some(index) = self.wav_lookup.get(path).copied() {
                if let Some(entry) = self.wav_entries.get_mut(index) {
                    entry.tag = target_tag;
                }
            } else if require_present {
                return Err("Sample not found".into());
            }
        }
        if let Some(cache) = self.wav_cache.get_mut(&source.id) {
            if let Some(entry) = cache.iter_mut().find(|entry| entry.relative_path == path) {
                entry.tag = target_tag;
            }
        }
        Ok(())
    }

    fn label_for(&mut self, index: usize) -> Option<String> {
        let source_id = self.selected_source.clone()?;
        if let Some(cache) = self.label_cache.get(&source_id) {
            if let Some(label) = cache.get(index) {
                return Some(label.clone());
            }
        }
        let labels: Vec<String> = self
            .wav_entries
            .iter()
            .map(|entry| entry.relative_path.to_string_lossy().into_owned())
            .collect();
        let result = labels.get(index).cloned();
        self.label_cache.insert(source_id, labels);
        result
    }

    pub(super) fn build_label_cache(&self, entries: &[WavEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| entry.relative_path.to_string_lossy().into_owned())
            .collect()
    }

    pub(super) fn load_waveform_for_selection(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        if self.loaded_wav.as_deref() == Some(relative_path) {
            self.clear_waveform_selection();
            self.set_status(
                format!("Loaded {}", relative_path.display()),
                StatusTone::Info,
            );
            return Ok(());
        }
        let bytes = self.read_waveform_bytes(source, relative_path)?;
        let decoded = self.renderer.decode_from_bytes(&bytes)?;
        let duration_seconds = decoded.duration_seconds;
        let sample_rate = decoded.sample_rate;
        let channels = decoded.channels;
        self.apply_waveform_image(decoded);
        self.clear_waveform_selection();
        self.loaded_wav = Some(relative_path.to_path_buf());
        self.ui.loaded_wav = Some(relative_path.to_path_buf());
        self.sync_loaded_audio(
            source,
            relative_path,
            duration_seconds,
            sample_rate,
            channels,
            bytes,
        )?;
        self.set_status(
            format!("Loaded {}", relative_path.display()),
            StatusTone::Info,
        );
        Ok(())
    }

    pub(super) fn load_collection_waveform(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        let bytes = self.read_waveform_bytes(source, relative_path)?;
        let decoded = self.renderer.decode_from_bytes(&bytes)?;
        let duration_seconds = decoded.duration_seconds;
        let sample_rate = decoded.sample_rate;
        let channels = decoded.channels;
        self.apply_waveform_image(decoded);
        self.clear_waveform_selection();
        self.loaded_wav = None;
        self.ui.loaded_wav = None;
        self.sync_loaded_audio(
            source,
            relative_path,
            duration_seconds,
            sample_rate,
            channels,
            bytes,
        )?;
        self.set_status(
            format!("Loaded {}", relative_path.display()),
            StatusTone::Info,
        );
        Ok(())
    }

    fn clear_waveform_selection(&mut self) {
        self.ui.waveform.playhead = PlayheadState::default();
        self.ui.waveform.selection = None;
        self.selection.clear();
    }

    fn apply_waveform_image(&mut self, decoded: DecodedWaveform) {
        self.decoded_waveform = Some(decoded);
        self.refresh_waveform_image();
    }

    /// Update the waveform render target to match the current view size.
    pub fn update_waveform_size(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if self.waveform_size == [width, height] {
            return;
        }
        self.waveform_size = [width, height];
        self.refresh_waveform_image();
    }

    fn refresh_waveform_image(&mut self) {
        let Some(decoded) = self.decoded_waveform.as_ref() else {
            return;
        };
        let [width, height] = self.waveform_size;
        let color_image =
            self.renderer
                .render_color_image_with_size(&decoded.samples, width, height);
        self.ui.waveform.image = Some(WaveformImage { image: color_image });
    }

    fn read_waveform_bytes(
        &self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<Vec<u8>, String> {
        let full_path = source.root.join(relative_path);
        fs::read(&full_path).map_err(|err| format!("Failed to read {}: {err}", full_path.display()))
    }

    fn sync_loaded_audio(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        duration_seconds: f32,
        sample_rate: u32,
        channels: u16,
        bytes: Vec<u8>,
    ) -> Result<(), String> {
        self.loaded_audio = Some(LoadedAudio {
            source_id: source.id.clone(),
            relative_path: relative_path.to_path_buf(),
            bytes: bytes.clone(),
            duration_seconds,
            sample_rate,
            channels,
        });
        match self.ensure_player() {
            Ok(Some(player)) => {
                let mut player = player.borrow_mut();
                player.stop();
                player.set_audio(bytes, duration_seconds);
            }
            Ok(None) => {}
            Err(err) => self.set_status(err, StatusTone::Warning),
        }
        Ok(())
    }
}
