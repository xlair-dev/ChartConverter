//! Parser and writer for the shared subset of the UGC chart format.

use chart::{Chart, ChartMode};

mod error;
mod parser;
mod syntax;
mod writer;

pub use error::UgcError;

/// Parses a UGC document into the shared chart model.
pub fn parse(source: &str) -> Result<Chart, UgcError> {
    parse_with_mode(source, ChartMode::Normal)
}

/// Parses a UGC document using the selected interpretation mode.
pub fn parse_with_mode(source: &str, mode: ChartMode) -> Result<Chart, UgcError> {
    parser::parse(source, mode)
}

/// Writes the representable shared chart model as a UGC document.
///
/// Measure-length changes stored in the shared chart are preserved in the
/// output and used when locating absolute note positions.
///
/// Unsupported or unrepresentable chart information is omitted and reported to stdout.
pub fn write(chart: &Chart) -> Result<String, UgcError> {
    write_with_mode(chart, ChartMode::Normal)
}

/// Writes a UGC document using the selected interpretation mode.
pub fn write_with_mode(chart: &Chart, mode: ChartMode) -> Result<String, UgcError> {
    writer::write_with_mode(chart, mode)
}

#[cfg(test)]
mod tests;
