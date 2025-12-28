//! State for the loop crossfade prompt and settings.

use crate::sample_sources::SourceId;
use std::path::PathBuf;

/// Units for loop crossfade depth controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopCrossfadeUnit {
    /// Depth measured in milliseconds.
    Milliseconds,
    /// Depth measured in sample frames.
    Samples,
}

/// User-configurable settings for loop crossfades.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopCrossfadeSettings {
    /// Depth in milliseconds.
    pub depth_ms: u32,
    /// Depth in sample frames.
    pub depth_samples: u32,
    /// Active unit for the depth control.
    pub unit: LoopCrossfadeUnit,
}

impl Default for LoopCrossfadeSettings {
    fn default() -> Self {
        Self {
            depth_ms: 5,
            depth_samples: 220,
            unit: LoopCrossfadeUnit::Milliseconds,
        }
    }
}

/// Pending prompt state for the loop crossfade workflow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopCrossfadePrompt {
    /// Source id that owns the targeted sample.
    pub source_id: SourceId,
    /// Relative path of the targeted sample.
    pub relative_path: PathBuf,
    /// Settings to apply for the crossfade.
    pub settings: LoopCrossfadeSettings,
}
