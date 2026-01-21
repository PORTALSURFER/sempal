mod actions;
mod delegates;
pub(crate) mod helpers;

pub(crate) use actions::BrowserActions;
pub(crate) use helpers::BrowserController;

use crate::egui_app::controller::library::wav_io::file_metadata;
use super::*;
use std::path::{Path, PathBuf};
