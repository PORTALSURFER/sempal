mod actions;
mod delegates;
mod drag_effects;
mod drag_state;
mod drag_transitions;
mod label_formatting;
mod path_resolution;

pub(crate) use actions::DragDropActions;
pub(crate) use drag_state::DragDropController;

use super::*;
use crate::egui_app::controller::collection_items_helpers::file_metadata;
use crate::egui_app::state::{DragPayload, DragSource, DragTarget};
use egui::Pos2;
use tracing::{debug, info};
