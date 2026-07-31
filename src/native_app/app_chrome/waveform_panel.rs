use radiant::prelude as ui;

#[cfg(test)]
use crate::native_app::app::DEFAULT_WAVEFORM_PANEL_HEIGHT;
use crate::native_app::app::{GuiMessage, MIN_WAVEFORM_PANEL_HEIGHT};
use crate::native_app::app_chrome::view_models::waveform_panel::WaveformPanelViewModel;
use crate::native_app::ui::ids as widget_ids;
use crate::native_app::waveform::{
    self, InstantWaveformPreviewTier, WaveformInteraction, WaveformState,
};

const WAVEFORM_STATUS_HEIGHT: f32 = 16.0;
const WAVEFORM_SCROLLBAR_HEIGHT: f32 = 6.0;
const WAVEFORM_RESIZE_HANDLE_HEIGHT: f32 = 8.0;
const WAVEFORM_SAMPLE_DRAG_HANDLE_WIDTH: f32 = 14.0;
#[cfg(test)]
pub(in crate::native_app) const WAVEFORM_PANEL_HEIGHT: f32 = DEFAULT_WAVEFORM_PANEL_HEIGHT;
#[cfg(test)]
pub(in crate::native_app) const WAVEFORM_VIEW_HEIGHT: f32 = WAVEFORM_PANEL_HEIGHT
    - WAVEFORM_STATUS_HEIGHT
    - WAVEFORM_SCROLLBAR_HEIGHT
    - WAVEFORM_RESIZE_HANDLE_HEIGHT;

pub(in crate::native_app) fn waveform_panel(
    model: WaveformPanelViewModel<'_>,
) -> ui::View<GuiMessage> {
    ui::column([
        waveform_title_row(
            model.waveform,
            model.instant_preview_active,
            model.instant_preview_label.as_deref(),
            model.instant_preview_tier,
            model.loading_label,
            model.failed_label.as_deref(),
            model.help_tooltips_enabled,
        ),
        waveform_viewport_with_loading_state(&model, waveform_view_height(model.panel_height)),
        waveform_scrollbar(model.waveform),
        waveform_resize_handle(model.help_tooltips_enabled),
    ])
    .spacing(0.0)
    .fill_width()
    .height(model.panel_height)
}

fn waveform_view_height(panel_height: f32) -> f32 {
    (panel_height
        - WAVEFORM_STATUS_HEIGHT
        - WAVEFORM_SCROLLBAR_HEIGHT
        - WAVEFORM_RESIZE_HANDLE_HEIGHT)
        .max(
            MIN_WAVEFORM_PANEL_HEIGHT
                - WAVEFORM_STATUS_HEIGHT
                - WAVEFORM_SCROLLBAR_HEIGHT
                - WAVEFORM_RESIZE_HANDLE_HEIGHT,
        )
}

fn waveform_viewport_with_loading_state(
    model: &WaveformPanelViewModel<'_>,
    viewport_height: f32,
) -> ui::View<GuiMessage> {
    let tooltip = model.help_tooltips_enabled.then_some(
        "Waveform: click to set playback start, drag to select, Z zooms to selection, X zooms out.",
    );
    let viewport = waveform::waveform_viewport_view_with_tooltip(
        model.waveform,
        viewport_height,
        tooltip,
        model.beat_guides_enabled,
        model.bpm_snap_enabled,
        model.beat_guide_count,
        model.normalized_audition_enabled,
        model.playhead_occlusion_rect,
    )
    .fill_width()
    .height(viewport_height);
    ui::overlay_stack(viewport)
        .overlay_opt(
            model
                .drop_hover
                .map(|hover| waveform_drop_hover_visual(hover.supported, viewport_height)),
        )
        .input_opt(waveform_loading_input_blocker(model))
        .into_view()
        .accepts_native_file_drop()
        .on_native_file_drop(GuiMessage::WaveformFileDrop)
        .fill_width()
        .height(viewport_height)
}

fn waveform_loading_input_blocker(
    model: &WaveformPanelViewModel<'_>,
) -> Option<ui::View<GuiMessage>> {
    model.block_input_while_loading.then(|| {
        ui::pointer_shield(true)
            .consume()
            .key("waveform-loading-input-blocker")
            .input_only()
            .fill_width()
            .height(waveform_view_height(model.panel_height))
    })
}

fn waveform_drop_hover_visual(supported: bool, viewport_height: f32) -> ui::View<GuiMessage> {
    let color = if supported {
        ui::Rgba8::new(74, 178, 116, 255)
    } else {
        ui::Rgba8::new(214, 62, 62, 255)
    };
    ui::feedback_overlay()
        .background(color.with_alpha(56))
        .edge(
            color.with_alpha(210),
            3.0,
            ui::BorderSides {
                top: true,
                bottom: true,
                left: false,
                right: false,
            },
        )
        .view()
        .key("waveform-drop-hover-visual")
        .fill_width()
        .height(viewport_height)
}

fn waveform_resize_handle(help_tooltips_enabled: bool) -> ui::View<GuiMessage> {
    ui::drag_handle()
        .hover_chrome_only()
        .mapped(GuiMessage::ResizeWaveformPanel)
        .key("waveform-resize-handle")
        .id(widget_ids::WAVEFORM_RESIZE_HANDLE_ID)
        .style(ui::WidgetStyle::subtle(ui::WidgetTone::Accent))
        .tooltip_if(help_tooltips_enabled, "Resize waveform height")
        .fill_width()
        .height(WAVEFORM_RESIZE_HANDLE_HEIGHT)
}

fn waveform_title_row(
    waveform: &WaveformState,
    instant_preview_active: bool,
    instant_preview_label: Option<&str>,
    instant_preview_tier: Option<InstantWaveformPreviewTier>,
    loading_label: Option<&str>,
    failed_label: Option<&str>,
    help_tooltips_enabled: bool,
) -> ui::View<GuiMessage> {
    let title = waveform_title(
        waveform,
        instant_preview_label,
        instant_preview_tier,
        loading_label,
        failed_label,
    );
    if instant_preview_active
        || loading_label.is_some()
        || failed_label.is_some()
        || !waveform.has_loaded_sample()
    {
        return ui::text_line(title, WAVEFORM_STATUS_HEIGHT);
    }
    ui::row([
        loaded_sample_drag_handle(help_tooltips_enabled),
        ui::text_line(title, WAVEFORM_STATUS_HEIGHT),
    ])
    .spacing(3.0)
    .fill_width()
    .height(WAVEFORM_STATUS_HEIGHT)
}

fn loaded_sample_drag_handle(help_tooltips_enabled: bool) -> ui::View<GuiMessage> {
    ui::drag_handle()
        .mapped(|drag| GuiMessage::Waveform(WaveformInteraction::DragLoadedSample(drag)))
        .id(widget_ids::WAVEFORM_LOADED_SAMPLE_DRAG_HANDLE_ID)
        .style(ui::WidgetStyle::subtle(ui::WidgetTone::Accent))
        .tooltip_if(help_tooltips_enabled, "Drag loaded sample")
        .size(WAVEFORM_SAMPLE_DRAG_HANDLE_WIDTH, WAVEFORM_STATUS_HEIGHT)
}

fn waveform_title(
    waveform: &WaveformState,
    instant_preview_label: Option<&str>,
    instant_preview_tier: Option<InstantWaveformPreviewTier>,
    loading_label: Option<&str>,
    failed_label: Option<&str>,
) -> String {
    if let Some(label) = instant_preview_label {
        return match instant_preview_tier {
            Some(InstantWaveformPreviewTier::Head) => format!("Previewing {label} | preview"),
            None => format!("Loading preview {label}"),
        };
    }
    if let Some(label) = loading_label {
        return format!("Loading {label}");
    }
    if let Some(label) = failed_label {
        return format!("Could not load {label}");
    }
    if !waveform.has_loaded_sample() {
        return String::from("No sample loaded");
    }
    format!(
        "{} | {} Hz | {} channel{} -> mono | {} frames",
        waveform.file_name(),
        waveform.sample_rate(),
        waveform.channels(),
        if waveform.channels() == 1 { "" } else { "s" },
        waveform.frames()
    )
}

fn waveform_scrollbar(waveform: &WaveformState) -> ui::View<GuiMessage> {
    if waveform.fully_zoomed_out() {
        return ui::empty().fill_width();
    }
    ui::scrollbar(ui::ScrollbarAxis::Horizontal)
        .viewport_fraction(waveform.visible_fraction())
        .offset_fraction(waveform.offset_fraction())
        .message(|offset_fraction| {
            GuiMessage::Waveform(WaveformInteraction::ScrollTo { offset_fraction })
        })
        .fill_width()
        .height(WAVEFORM_SCROLLBAR_HEIGHT)
}
