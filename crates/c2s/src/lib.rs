//! Parser and writer for the shared note and timing subset of the C2S format.

use chart::{Chart, ChartMode};
mod error;
mod parser;
mod syntax;
mod writer;

pub use error::C2sError;

/// Parses a C2S document into the shared chart model.
pub fn parse(source: &str) -> Result<Chart, C2sError> {
    parse_with_mode(source, ChartMode::Normal)
}

/// Parses a C2S document using the selected interpretation mode.
pub fn parse_with_mode(source: &str, mode: ChartMode) -> Result<Chart, C2sError> {
    parser::parse(source, mode)
}

/// Writes the representable shared chart model as a C2S document.
///
/// Unsupported or unrepresentable chart information is omitted and reported to stdout.
pub fn write(chart: &Chart) -> Result<String, C2sError> {
    write_with_mode(chart, ChartMode::Normal)
}

/// Writes a C2S document using the selected interpretation mode.
pub fn write_with_mode(chart: &Chart, mode: ChartMode) -> Result<String, C2sError> {
    writer::write_with_mode(chart, mode)
}
#[cfg(test)]
mod tests;
