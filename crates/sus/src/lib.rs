//! Parser and writer for the general SUS chart format and its XLAIR mode.
//! Directive names and symbolic tokens are ASCII case-insensitive; string
//! arguments retain their original spelling.
//!
//! Both modes accept Ched's `TIL00` / `HISPEED 00` / `MEASUREHS 00` notation.
//! Speed-group IDs are IR-local: `00` is assigned an unused group without changing
//! the existing `01`-to-group-0 mapping. SUS output uses only `01` through `zz`;
//! writing all 1296 distinct groups is therefore an error. `NOSPEED` remains separate.
//!
//! `REQUEST "xlair_directional_mode consume_tap"` opts into exact Tap 1 carrier matching;
//! `independent` preserves ordinary taps. Without a request, existing overlap rules apply.
//! Side XTap pairs always retain their existing XTap-parent / directional-AIR-child IR.
//!
//! SUS token 4 is a Bezier handle, unlike the shared IR's on-path `Control`.
//! Parsing expands these handles with 20 subdivisions per second, using the
//! chart's tempo map (default BPM 120), and outward u8 lane rounding. Generated
//! positions use a bounded 480-tick beat grid, independent of the source
//! `ticks_per_beat` request; geometry is evaluated at the generated time after
//! rounding. Authored anchors are kept exact.
//! Generated points are `Control` (non-checkpoints), inherit the preceding
//! anchor's context, and can be joined directly like UGC/C2S paths. Writing emits
//! on-path Control/Invisible points as token 5, never as new Bezier handles.
//! This is a bounded geometric approximation, not lossless authoring round-trip.

use chart::{Chart, ChartMode};

mod curve;
mod error;
mod parser;
mod syntax;
mod writer;

pub use error::SusError;

/// Parses a SUS document into the shared chart model.
pub fn parse(source: &str) -> Result<Chart, SusError> {
    parse_with_mode(source, ChartMode::Normal)
}

/// Parses a SUS document using the selected interpretation mode.
pub fn parse_with_mode(source: &str, mode: ChartMode) -> Result<Chart, SusError> {
    parser::parse(source, mode)
}

/// Parses without printing warnings; diagnostics also survive a parse failure.
pub fn parse_with_diagnostics(source: &str, mode: ChartMode) -> chart::ParseReport<SusError> {
    parser::parse_with_diagnostics(source, mode)
}

/// Writes a chart as a general SUS document.
///
/// Unsupported or unrepresentable chart information is omitted and reported to stdout.
pub fn write(chart: &Chart) -> Result<String, SusError> {
    write_with_mode(chart, ChartMode::Normal)
}

/// Writes a SUS document using the selected interpretation mode.
pub fn write_with_mode(chart: &Chart, mode: ChartMode) -> Result<String, SusError> {
    writer::write_with_mode(chart, mode)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod curve_tests;

#[cfg(test)]
mod case_tests;
