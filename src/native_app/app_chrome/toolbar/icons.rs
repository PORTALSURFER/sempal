use radiant::prelude as ui;

use crate::native_app::app_chrome::palette::{ACCENT, TEXT_MUTED, TEXT_PRIMARY};

const TOOLBAR_ICON_ACTIVE_COLOR: ui::Rgba8 = ACCENT;
const TOOLBAR_ICON_ENABLED_COLOR: ui::Rgba8 = TEXT_PRIMARY;
const TOOLBAR_ICON_DISABLED_COLOR: ui::Rgba8 = TEXT_MUTED;
const TOOLBAR_ICON_TINTS: ui::SvgIconTintPalette = ui::SvgIconTintPalette::new(
    TOOLBAR_ICON_ENABLED_COLOR,
    TOOLBAR_ICON_ACTIVE_COLOR,
    TOOLBAR_ICON_DISABLED_COLOR,
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum ToolbarIcon {
    FocusLoaded,
    Loop,
    Random,
    SimilarSections,
    ZeroCrossingSnap,
    BpmSnap,
    BeatGuides,
    Metronome,
    Play,
    Stop,
}

impl ToolbarIcon {
    fn cache(self) -> &'static ui::SvgIconTintCache {
        match self {
            Self::FocusLoaded => &FOCUS_LOADED_ICON,
            Self::Loop => &LOOP_ICON,
            Self::Random => &RANDOM_ICON,
            Self::SimilarSections => &SIMILAR_SECTIONS_ICON,
            Self::ZeroCrossingSnap => &ZERO_CROSSING_SNAP_ICON,
            Self::BpmSnap => &BPM_SNAP_ICON,
            Self::BeatGuides => &BEAT_GUIDES_ICON,
            Self::Metronome => &METRONOME_ICON,
            Self::Play => &PLAY_ICON,
            Self::Stop => &STOP_ICON,
        }
    }
}

pub(in crate::native_app) fn toolbar_icon_color(enabled: bool, active: bool) -> ui::Rgba8 {
    TOOLBAR_ICON_TINTS.color(enabled, active)
}

pub(in crate::native_app) fn toolbar_icon_glyph(
    icon: ToolbarIcon,
    enabled: bool,
    active: bool,
) -> ui::SvgIcon {
    icon.cache()
        .icon_for_state(TOOLBAR_ICON_TINTS, enabled, active)
}

static FOCUS_LOADED_ICON: ui::SvgIconTintCache = ui::SvgIconTintCache::new(
    r#"<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg">
  <path fill-rule="evenodd" d="M8 1.5C4.41 1.5 1.5 4.41 1.5 8c0 2.2 1.5 4.6 6.5 7.5 5-2.9 6.5-5.3 6.5-7.5 0-3.59-2.91-6.5-6.5-6.5ZM8 10A2.5 2.5 0 1 0 8 5a2.5 2.5 0 0 0 0 5Z"/>
</svg>"#,
);

static LOOP_ICON: ui::SvgIconTintCache = ui::SvgIconTintCache::new(
    r#"<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg">
  <path d="M4 3h5.4V1.5L14 5l-4.6 3.5V7H4.2C3 7 2 8 2 9.2V10H.5v-.8C.5 5.8 2 3 4 3z"/>
  <path d="M12 13H6.6v1.5L2 11l4.6-3.5V9H12c1.2 0 2-1 2-2.2V6h1.5v.8C15.5 10.2 14 13 12 13z"/>
</svg>"#,
);

static RANDOM_ICON: ui::SvgIconTintCache = ui::SvgIconTintCache::new(
    r#"<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg">
  <path d="M2 4h2.1c1.8 0 2.9.8 4.1 2.5l.8 1.1c.8 1.1 1.4 1.4 2.6 1.4H12V7l3 3-3 3v-2h-.4c-1.9 0-3.1-.7-4.2-2.4l-.8-1.1C5.8 6.3 5.2 6 4.1 6H2z"/>
  <path d="M11.6 4H12V2l3 3-3 3V6h-.4c-1.2 0-1.8.3-2.6 1.4l-.2.3-.9-1.4.5-.7C8.5 4.7 9.7 4 11.6 4z"/>
  <path d="M2 10h2.1c1.1 0 1.7-.3 2.5-1.5l.9 1.4c-1 1.4-2 2.1-3.4 2.1H2z"/>
</svg>"#,
);

static SIMILAR_SECTIONS_ICON: ui::SvgIconTintCache = ui::SvgIconTintCache::new(
    r#"<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg">
  <rect x="2" y="3" width="3.5" height="3"/>
  <rect x="10.5" y="3" width="3.5" height="3"/>
  <rect x="2" y="10" width="3.5" height="3"/>
  <rect x="10.5" y="10" width="3.5" height="3"/>
  <rect x="6.5" y="4.1" width="3" height="1.2"/>
  <rect x="6.5" y="11.1" width="3" height="1.2"/>
  <rect x="3.15" y="7" width="1.2" height="2"/>
  <rect x="11.65" y="7" width="1.2" height="2"/>
</svg>"#,
);

static ZERO_CROSSING_SNAP_ICON: ui::SvgIconTintCache = ui::SvgIconTintCache::new(
    r#"<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg">
  <rect x="11.5" y="2" width="1.5" height="12"/>
  <path d="M2 9.5c1.8 0 2.3-5.5 4.4-5.5S8.8 9.5 11.5 9.5V11C8 11 7.6 5.5 6.4 5.5S4.2 11 2 11z"/>
</svg>"#,
);

static BPM_SNAP_ICON: ui::SvgIconTintCache = ui::SvgIconTintCache::new(
    r#"<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg">
  <rect x="1.5" y="2.5" width="1.5" height="11"/>
  <rect x="13" y="2.5" width="1.5" height="11"/>
  <path d="M5 4h3.1c1.5 0 2.4.8 2.4 2 0 .8-.4 1.4-1.1 1.7.9.3 1.4 1 1.4 2 0 1.5-1 2.3-2.7 2.3H5V4zm1.5 1.3v1.8h1.4c.7 0 1.1-.3 1.1-.9s-.4-.9-1.1-.9H6.5zm0 3v2.4H8c.8 0 1.2-.4 1.2-1.2s-.4-1.2-1.2-1.2H6.5z"/>
</svg>"#,
);

static BEAT_GUIDES_ICON: ui::SvgIconTintCache = ui::SvgIconTintCache::new(
    r#"<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg">
  <rect x="2" y="3" width="1.5" height="10"/>
  <rect x="12.5" y="3" width="1.5" height="10"/>
  <rect x="5.5" y="5" width="1" height="6"/>
  <rect x="9.5" y="5" width="1" height="6"/>
  <rect x="2" y="7.25" width="12" height="1.5"/>
</svg>"#,
);

static METRONOME_ICON: ui::SvgIconTintCache = ui::SvgIconTintCache::new(
    r#"<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg">
  <path d="M3 3h.5c1.7 0 3 2.2 3 5s-1.3 5-3 5H3z"/>
  <path d="M13 3h-.5c-1.7 0-3 2.2-3 5s1.3 5 3 5h.5z"/>
</svg>"#,
);

static PLAY_ICON: ui::SvgIconTintCache = ui::SvgIconTintCache::new(
    r#"<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg">
  <polygon points="4,3 13,8 4,13"/>
</svg>"#,
);

static STOP_ICON: ui::SvgIconTintCache = ui::SvgIconTintCache::new(
    r#"<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg">
  <rect x="4" y="4" width="8" height="8"/>
</svg>"#,
);
