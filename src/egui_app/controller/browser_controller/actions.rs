use super::*;

pub(crate) trait BrowserActions {
    fn tag_browser_sample(&mut self, row: usize, tag: SampleTag) -> Result<(), String>;
    fn tag_browser_samples(
        &mut self,
        rows: &[usize],
        tag: SampleTag,
        primary_visible_row: usize,
    ) -> Result<(), String>;
    fn normalize_browser_sample(&mut self, row: usize) -> Result<(), String>;
    fn normalize_browser_samples(&mut self, rows: &[usize]) -> Result<(), String>;
    fn rename_browser_sample(&mut self, row: usize, new_name: &str) -> Result<(), String>;
    fn delete_browser_sample(&mut self, row: usize) -> Result<(), String>;
    fn delete_browser_samples(&mut self, rows: &[usize]) -> Result<(), String>;
}

impl BrowserActions for BrowserController<'_> {
    fn tag_browser_sample(&mut self, row: usize, tag: SampleTag) -> Result<(), String> {
        let result: Result<(), String> = (|| {
            let ctx = self.resolve_browser_sample(row)?;
            self.set_sample_tag_for_source(&ctx.source, &ctx.entry.relative_path, tag, true)?;
            self.set_status(
                format!("Tagged {} as {:?}", ctx.entry.relative_path.display(), tag),
                StatusTone::Info,
            );
            Ok(())
        })();
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    fn tag_browser_samples(
        &mut self,
        rows: &[usize],
        tag: SampleTag,
        primary_visible_row: usize,
    ) -> Result<(), String> {
        let mut last_error = None;
        for &row in rows {
            if let Err(err) = self.tag_browser_sample(row, tag) {
                last_error = Some(err);
            }
        }
        self.refocus_after_filtered_removal(primary_visible_row);
        if let Some(err) = last_error {
            Err(err)
        } else {
            Ok(())
        }
    }

    fn normalize_browser_sample(&mut self, row: usize) -> Result<(), String> {
        let result = self.try_normalize_browser_sample(row);
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    fn normalize_browser_samples(&mut self, rows: &[usize]) -> Result<(), String> {
        let mut last_error = None;
        for &row in rows {
            if let Err(err) = self.normalize_browser_sample(row) {
                last_error = Some(err);
            }
        }
        if let Some(err) = last_error {
            Err(err)
        } else {
            Ok(())
        }
    }

    fn rename_browser_sample(&mut self, row: usize, new_name: &str) -> Result<(), String> {
        let result = self.try_rename_browser_sample(row, new_name);
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    fn delete_browser_sample(&mut self, row: usize) -> Result<(), String> {
        self.delete_browser_samples(&[row])
    }

    fn delete_browser_samples(&mut self, rows: &[usize]) -> Result<(), String> {
        let next_focus = self.next_browser_focus_after_delete(rows);
        let mut last_error = None;
        for &row in rows {
            if let Err(err) = self.try_delete_browser_sample(row) {
                last_error = Some(err);
            }
        }
        if let Some(path) = next_focus
            && self.wav_lookup.contains_key(&path)
        {
            if let Some(row) = self.visible_row_for_path(&path) {
                self.focus_browser_row_only(row);
            } else {
                self.select_wav_by_path_with_rebuild(&path, true);
            }
        }
        if let Some(err) = last_error {
            Err(err)
        } else {
            Ok(())
        }
    }
}
