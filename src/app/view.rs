use super::*;

impl DropHandler {
    /// Update the status text and badge styling in the UI.
    pub(super) fn set_status(
        &self,
        app: &HelloWorld,
        text: impl Into<SharedString>,
        state: StatusState,
    ) {
        let (badge, color) = Self::status_badge(state);
        app.set_status_badge_text(badge);
        app.set_status_badge_color(color);
        app.set_status_text(text.into());
    }

    /// Request the source list to scroll to a given index.
    pub(super) fn scroll_sources_to(&self, app: &HelloWorld, index: i32) {
        if index >= 0 {
            app.invoke_scroll_sources_to(index);
        }
    }

    /// Request the wav list to scroll to a given index.
    pub(super) fn scroll_wavs_to(&self, app: &HelloWorld, index: i32) {
        if index >= 0 {
            app.invoke_scroll_wavs_to(index);
        }
    }

    fn status_badge(state: StatusState) -> (SharedString, Color) {
        match state {
            StatusState::Idle => ("Idle".into(), Color::from_rgb_u8(42, 42, 42)),
            StatusState::Busy => ("Scanning".into(), Color::from_rgb_u8(31, 139, 255)),
            StatusState::Info => ("Info".into(), Color::from_rgb_u8(64, 140, 112)),
            StatusState::Warning => ("Warning".into(), Color::from_rgb_u8(192, 138, 43)),
            StatusState::Error => ("Error".into(), Color::from_rgb_u8(192, 57, 43)),
        }
    }

    pub(super) fn source_row(source: &SampleSource) -> SourceRow {
        let name = source
            .root
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.to_string())
            .unwrap_or_else(|| source.root.to_string_lossy().to_string());
        SourceRow {
            name: name.into(),
            path: source.root.to_string_lossy().to_string().into(),
        }
    }

    /// Convert a wav entry into its UI row representation.
    pub(super) fn wav_row(entry: &WavEntry, selected: bool, loaded: bool) -> WavRow {
        let (tag_label, tag_bg, tag_fg) = Self::tag_display(entry.tag);
        WavRow {
            name: entry.relative_path.to_string_lossy().to_string().into(),
            path: entry.relative_path.to_string_lossy().to_string().into(),
            selected,
            loaded,
            tag_label,
            tag_bg,
            tag_fg,
        }
    }

    fn tag_display(tag: SampleTag) -> (SharedString, Color, Color) {
        match tag {
            SampleTag::Neutral => (
                "".into(),
                Color::from_argb_u8(0, 0, 0, 0),
                Color::from_argb_u8(0, 0, 0, 0),
            ),
            SampleTag::Keep => (
                "KEEP".into(),
                Color::from_argb_u8(180, 34, 78, 52),
                Color::from_rgb_u8(132, 214, 163),
            ),
            SampleTag::Trash => (
                "TRASH".into(),
                Color::from_argb_u8(180, 78, 35, 35),
                Color::from_rgb_u8(240, 138, 138),
            ),
        }
    }
}
