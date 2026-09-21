//! Parser and writer for the general SUS chart format and its XLAIR mode.

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
