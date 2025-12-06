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

    /// Request the wav list to scroll to a given column/index pair.
    pub(super) fn scroll_wavs_to(&self, app: &HelloWorld, target: Option<(SampleTag, usize)>) {
        let Some((tag, index)) = target else {
            return;
        };
        app.invoke_scroll_wavs_to(Self::tag_to_column(tag), index as i32);
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
        let (tag_label, tag_bg, tag_fg, overlay) = Self::tag_display(entry.tag);
        let (bg, hover_bg, pressed_bg) = Self::row_background(selected, loaded);
        let (border_color, indicator_color) = Self::row_highlights(selected, loaded);
        WavRow {
            name: entry.relative_path.to_string_lossy().to_string().into(),
            path: entry.relative_path.to_string_lossy().to_string().into(),
            selected,
            loaded,
            bg,
            hover_bg,
            pressed_bg,
            border_color,
            indicator_color,
            overlay,
            tag_label,
            tag_bg,
            tag_fg,
        }
    }

    fn row_background(selected: bool, loaded: bool) -> (Color, Color, Color) {
        let base = if loaded {
            Color::from_rgb_u8(32, 52, 76)
        } else if selected {
            Color::from_rgb_u8(29, 29, 29)
        } else {
            Color::from_rgb_u8(20, 20, 20)
        };
        let hover = if selected || loaded {
            base
        } else {
            Color::from_rgb_u8(26, 26, 26)
        };
        let pressed = Color::from_rgb_u8(31, 31, 31);
        (base, hover, pressed)
    }

    fn row_highlights(selected: bool, loaded: bool) -> (Color, Color) {
        let primary = Color::from_rgb_u8(58, 156, 255);
        let secondary = Color::from_rgb_u8(47, 111, 177);
        let indicator = if loaded {
            primary
        } else if selected {
            secondary
        } else {
            Color::from_argb_u8(0, 0, 0, 0)
        };
        let border = if loaded { primary } else { secondary };
        (border, indicator)
    }

    fn tag_display(tag: SampleTag) -> (SharedString, Color, Color, Color) {
        match tag {
            SampleTag::Neutral => (
                "".into(),
                Color::from_argb_u8(0, 0, 0, 0),
                Color::from_argb_u8(0, 0, 0, 0),
                Color::from_argb_u8(0, 0, 0, 0),
            ),
            SampleTag::Keep => (
                "KEEP".into(),
                Color::from_argb_u8(180, 34, 78, 52),
                Color::from_rgb_u8(132, 214, 163),
                Color::from_argb_u8(42, 52, 164, 108),
            ),
            SampleTag::Trash => (
                "TRASH".into(),
                Color::from_argb_u8(180, 78, 35, 35),
                Color::from_rgb_u8(240, 138, 138),
                Color::from_argb_u8(42, 164, 78, 78),
            ),
        }
    }

    fn tag_to_column(tag: SampleTag) -> i32 {
        match tag {
            SampleTag::Trash => 0,
            SampleTag::Neutral => 1,
            SampleTag::Keep => 2,
        }
    }
}
