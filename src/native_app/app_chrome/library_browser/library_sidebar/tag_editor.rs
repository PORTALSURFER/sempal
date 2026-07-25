use radiant::prelude as ui;

use crate::native_app::app::{BrowserScrollSurface, GuiMessage};
use crate::native_app::app_chrome::view_models::library_sidebar::TagEditorViewModel;
use crate::native_app::sample_library::folder_browser::commands::FolderBrowserMessage;
use crate::native_app::sample_library::folder_browser::view_contract::{
    SIDEBAR_PANEL_HEADER_CONTENT_SPACING, SIDEBAR_PANEL_HEADER_HEIGHT,
};
use crate::native_app::ui::ids as widget_ids;

use super::edge_aligned_resize_panel;
use super::tag_entry_layout::TAG_FIELD_LINE_GAP;

mod identity;
mod projection;
mod rows;

use projection::{TagEditorFieldProjection, TagEditorProjection};
use rows::tag_entry_row;

use super::METADATA_PANEL_PADDING;
const METADATA_PANEL_HEADER_CONTENT_SPACING: f32 = SIDEBAR_PANEL_HEADER_CONTENT_SPACING;
const METADATA_TAG_LIBRARY_TOGGLE_WIDTH: f32 = 22.0;

pub(super) fn tag_editor_section(
    model: &TagEditorViewModel,
    sidebar_width: f32,
    panel_height: f32,
) -> ui::View<GuiMessage> {
    let projection =
        TagEditorProjection::from_model(model, sidebar_width, METADATA_TAG_LIBRARY_TOGGLE_WIDTH);
    metadata_section(projection, panel_height)
}

fn metadata_section(projection: TagEditorProjection, panel_height: f32) -> ui::View<GuiMessage> {
    let Some(field) = projection.field else {
        return metadata_sidebar_panel(ui::empty().fill_width().fill_height(), panel_height);
    };
    let field_height = field.layout.field_height;

    metadata_sidebar_panel(
        tag_entry_field(field)
            .key(identity::TAG_ENTRY_FIELD_KEY)
            .fill_width()
            .height(field_height),
        panel_height,
    )
}

fn tag_entry_field(field: TagEditorFieldProjection) -> ui::View<GuiMessage> {
    let content = ui::column(
        field
            .rows
            .into_iter()
            .enumerate()
            .map(|(row_index, row)| tag_entry_row(row, row_index))
            .collect::<Vec<_>>(),
    )
    .fill_width()
    .height(field.layout.content_height)
    .spacing(TAG_FIELD_LINE_GAP);

    if field.layout.requires_scroll {
        ui::scroll(content)
            .id(widget_ids::METADATA_TAG_SCROLL_NODE_ID)
            .on_scroll_update(|_| {
                GuiMessage::BrowserScrollAccepted(BrowserScrollSurface::MetadataTags)
            })
            .padding(3.0)
            .fill_width()
            .height(field.layout.field_height)
    } else {
        content.fill_width().height(field.layout.field_height)
    }
}

fn metadata_sidebar_panel(content: ui::View<GuiMessage>, height: f32) -> ui::View<GuiMessage> {
    let panel = edge_aligned_resize_panel(
        "metadata-resize-header",
        identity::metadata_resize_header_id(),
        SIDEBAR_PANEL_HEADER_HEIGHT,
        content,
        height,
        METADATA_PANEL_PADDING,
        METADATA_PANEL_HEADER_CONTENT_SPACING,
        |message| GuiMessage::FolderBrowser(FolderBrowserMessage::ResizeMetadataPanel(message)),
    );
    #[cfg(test)]
    {
        panel.id(identity::metadata_sidebar_panel_id())
    }
    #[cfg(not(test))]
    {
        panel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radiant::prelude::IntoView;

    #[test]
    fn metadata_resize_header_aligns_with_panel_top_edge() {
        let layout = metadata_sidebar_panel(ui::empty(), 120.0)
            .view_layout_at_size(ui::Vector2::new(240.0, 120.0));
        let panel = layout
            .rects
            .get(&identity::metadata_sidebar_panel_id())
            .expect("metadata panel layout rect");
        let header = layout
            .rects
            .get(&identity::metadata_resize_header_id())
            .expect("metadata resize header layout rect");

        assert_eq!(header.min.x, panel.min.x);
        assert_eq!(header.max.x, panel.max.x);
        assert_eq!(header.min.y, panel.min.y);
    }
}
