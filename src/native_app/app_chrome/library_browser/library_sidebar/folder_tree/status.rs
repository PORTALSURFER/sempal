use radiant::prelude as ui;

use crate::native_app::app::GuiMessage;
use crate::native_app::app_chrome::palette::{ACCENT, DANGER, TEXT_MUTED, TEXT_PRIMARY};
use crate::native_app::sample_library::folder_browser::commands::FolderBrowserMessage;
use crate::native_app::ui::ids as widget_ids;

const ICON_ACTIVE_COLOR: ui::Rgba8 = ACCENT;
const ICON_ENABLED_COLOR: ui::Rgba8 = TEXT_PRIMARY;
const ICON_DISABLED_COLOR: ui::Rgba8 = TEXT_MUTED;
const SOURCE_MISSING_STATUS_COLOR: ui::Rgba8 = DANGER.with_alpha(230);
const STATUS_ICON_TINTS: ui::SvgIconTintPalette =
    ui::SvgIconTintPalette::new(ICON_ENABLED_COLOR, ICON_ACTIVE_COLOR, ICON_DISABLED_COLOR);

pub(super) fn selected_folder_status(
    label: String,
    source_missing: bool,
    include_subfolders_available: bool,
    include_subfolders: bool,
    show_empty_folders: bool,
    help_tooltips_enabled: bool,
) -> ui::View<GuiMessage> {
    ui::row([
        selected_folder_status_label(label, source_missing),
        include_subfolders_button(include_subfolders_available, include_subfolders).tooltip_if(
            help_tooltips_enabled,
            "Include samples from subfolders in the sample list.",
        ),
        show_empty_folders_button(show_empty_folders).tooltip_if(
            help_tooltips_enabled,
            "Show folders that contain no audio files.",
        ),
    ])
    .spacing(4.0)
    .padding_x(3.0)
    .fill_width()
    .height(24.0)
}

fn selected_folder_status_label(label: String, source_missing: bool) -> ui::View<GuiMessage> {
    let label = ui::text(label.to_ascii_uppercase())
        .height(20.0)
        .fill_width();
    if source_missing {
        label.text_color(ui::TextColorRole::Custom(SOURCE_MISSING_STATUS_COLOR))
    } else {
        label
    }
}

fn include_subfolders_button(available: bool, active: bool) -> ui::View<GuiMessage> {
    ui::icon_button(include_subfolders_icon(available, active))
        .enabled(available)
        .active(active)
        .message(GuiMessage::FolderBrowser(
            FolderBrowserMessage::ToggleFolderSubtreeListing,
        ))
        .id(widget_ids::FOLDER_TREE_INCLUDE_SUBFOLDERS_TOGGLE_ID)
        .size(24.0, 20.0)
}

fn show_empty_folders_button(active: bool) -> ui::View<GuiMessage> {
    ui::icon_button(show_empty_folders_icon(active))
        .active(active)
        .message(GuiMessage::FolderBrowser(
            FolderBrowserMessage::ToggleEmptyFolderVisibility,
        ))
        .id(widget_ids::FOLDER_TREE_SHOW_EMPTY_FOLDERS_TOGGLE_ID)
        .size(24.0, 20.0)
}

fn include_subfolders_icon(available: bool, active: bool) -> ui::SvgIcon {
    INCLUDE_SUBFOLDERS_ICON.icon_for_state(STATUS_ICON_TINTS, available, active)
}

fn show_empty_folders_icon(active: bool) -> ui::SvgIcon {
    SHOW_EMPTY_FOLDERS_ICON.icon_for_state(STATUS_ICON_TINTS, true, active)
}

static INCLUDE_SUBFOLDERS_ICON: ui::SvgIconTintCache = ui::SvgIconTintCache::new(
    r#"<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg">
  <path d="M4 3.25v8.5" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round"/>
  <path d="M4 5.25h3.2M4 10.75h3.2" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round"/>
  <rect x="8.2" y="3.65" width="4.55" height="3.2" rx=".6" fill="none" stroke="currentColor" stroke-width="1.2"/>
  <rect x="8.2" y="9.15" width="4.55" height="3.2" rx=".6" fill="none" stroke="currentColor" stroke-width="1.2"/>
</svg>"#,
);

static SHOW_EMPTY_FOLDERS_ICON: ui::SvgIconTintCache = ui::SvgIconTintCache::new(
    r#"<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg">
  <path d="M2.25 5.05h3.4l1.1 1.25h7v5.1c0 .7-.45 1.15-1.15 1.15H3.4c-.7 0-1.15-.45-1.15-1.15V5.05Z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/>
  <path d="M2.25 5.05V4.6c0-.7.45-1.15 1.15-1.15h2.45l1.05 1.1" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>
  <circle cx="8" cy="9.15" r="1.55" fill="none" stroke="currentColor" stroke-width="1.15"/>
</svg>"#,
);

#[cfg(test)]
mod tests {
    use super::*;
    use radiant::prelude::IntoView;
    use radiant::widgets::ButtonMessage;

    #[test]
    fn selected_folder_status_projects_subfolder_toggle_button() {
        let frame = selected_folder_status(
            String::from("drums | 2 audio incl subfolders | 1 item"),
            false,
            true,
            true,
            false,
            false,
        )
        .view_frame_at_size_with_default_theme(ui::Vector2::new(260.0, 24.0));

        assert!(
            frame
                .paint_plan
                .first_widget_rect(widget_ids::FOLDER_TREE_INCLUDE_SUBFOLDERS_TOGGLE_ID)
                .is_some()
        );
        assert_eq!(
            selected_folder_status(
                String::from("drums | 1 audio | 1 item"),
                false,
                true,
                false,
                false,
                false
            )
            .view_dispatch_widget_output(
                widget_ids::FOLDER_TREE_INCLUDE_SUBFOLDERS_TOGGLE_ID,
                ui::WidgetOutput::typed(ButtonMessage::Activate {
                    provenance: ui::InteractionProvenance::Programmatic,
                }),
            ),
            Some(GuiMessage::FolderBrowser(
                FolderBrowserMessage::ToggleFolderSubtreeListing
            ))
        );
    }

    #[test]
    fn selected_folder_status_projects_empty_folder_toggle_button() {
        let frame = selected_folder_status(
            String::from("drums | 1 audio | 1 item"),
            false,
            true,
            false,
            true,
            false,
        )
        .view_frame_at_size_with_default_theme(ui::Vector2::new(260.0, 24.0));

        assert!(
            frame
                .paint_plan
                .first_widget_rect(widget_ids::FOLDER_TREE_SHOW_EMPTY_FOLDERS_TOGGLE_ID)
                .is_some()
        );
        assert_eq!(
            selected_folder_status(
                String::from("drums | 1 audio | 1 item"),
                false,
                true,
                false,
                false,
                false
            )
            .view_dispatch_widget_output(
                widget_ids::FOLDER_TREE_SHOW_EMPTY_FOLDERS_TOGGLE_ID,
                ui::WidgetOutput::typed(ButtonMessage::Activate {
                    provenance: ui::InteractionProvenance::Programmatic,
                }),
            ),
            Some(GuiMessage::FolderBrowser(
                FolderBrowserMessage::ToggleEmptyFolderVisibility
            ))
        );
    }

    #[test]
    fn selected_folder_status_paints_missing_source_as_warning() {
        let frame = selected_folder_status(
            String::from("Source missing | . | 0 audio | 0 items"),
            true,
            false,
            false,
            false,
            false,
        )
        .view_frame_at_size_with_default_theme(ui::Vector2::new(300.0, 24.0));

        assert_eq!(
            frame
                .paint_plan
                .first_text_color("Source missing | . | 0 audio | 0 items"),
            Some(SOURCE_MISSING_STATUS_COLOR)
        );
    }
}
