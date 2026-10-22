//! Parser and writer for the general SUS chart format and its XLAIR mode.
//!
//! Both modes accept Ched's `TIL00` / `HISPEED 00` / `MEASUREHS 00` notation.
//! Speed-group IDs are IR-local: `00` is assigned an unused group without changing
//! the existing `01`-to-group-0 mapping. SUS output uses only `01` through `zz`;
//! writing all 1296 distinct groups is therefore an error. `NOSPEED` remains separate.
//!
//! `REQUEST "xlair_directional_mode consume_tap"` opts into exact Tap 1 carrier matching;
//! `independent` preserves ordinary taps. Without a request, existing overlap rules apply.
//! Side XTap pairs always retain their existing XTap-parent / directional-AIR-child IR.

use chart::{Chart, ChartMode};

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
