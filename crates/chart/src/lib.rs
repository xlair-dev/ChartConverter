//! Shared chart domain model.

use std::fmt::Display;

mod chart;
mod error;
mod note;
mod position;
mod timeline;
mod timing;

pub use chart::{Chart, NoteId};
pub use error::ChartError;
pub use note::{
    AirColor, AirCrushColor, AirCrushInterval, AirCrushPoint, AirDirection, AirPoint,
    AirProperties, AirSlidePoint, ExDirection, Lane, Note, NoteKind, SideButton, SlidePoint,
    SlidePointKind, TapKind,
};
pub use position::Position;
pub use timeline::MeasureTimeline;
pub use timing::{ScrollScope, ScrollSpeedChange, TempoChange};

/// Selects the interpretation rules used by a format frontend or backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChartMode {
    /// Preserve the format's general-purpose semantics.
    Normal,
    /// Apply XLAIR's shared interpretation rules.
    Xlair,
}

/// Reports information omitted because a target format cannot represent it.
pub fn report_loss(format: &str, detail: impl Display) {
    println!("{format}: unsupported notation was omitted; information was lost: {detail}");
}
