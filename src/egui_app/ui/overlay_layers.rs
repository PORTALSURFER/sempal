use eframe::egui::{self, Color32, Id, LayerId, Order};

/// Overlay ordering tiers for consistent stacking across the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OverlayLayer {
    /// Non-modal overlays like drag previews and status popups.
    Overlay,
    /// Modal overlays that must remain on top and block input.
    Modal,
}

impl OverlayLayer {
    /// Return the egui order used for the overlay tier.
    pub(super) fn order(self) -> Order {
        match self {
            Self::Overlay => Order::Foreground,
            Self::Modal => Order::Tooltip,
        }
    }

    /// Create a layer id in the overlay tier for custom painters.
    pub(super) fn layer_id(self, id: impl Into<Id>) -> LayerId {
        LayerId::new(self.order(), id.into())
    }
}

/// Paint a modal backdrop and capture pointer input behind the modal.
pub(super) fn modal_backdrop(ctx: &egui::Context, id: impl Into<Id>, color: Color32) {
    let id = id.into();
    let rect = ctx.viewport_rect();
    let painter = ctx.layer_painter(OverlayLayer::Modal.layer_id(id.with("backdrop_paint")));
    painter.rect_filled(rect, 0.0, color);
    egui::Area::new(id.with("backdrop_blocker"))
        .order(OverlayLayer::Modal.order())
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            ui.allocate_rect(rect, egui::Sense::click_and_drag());
        });
}
