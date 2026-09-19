//! Format-independent conversion entry points for supported chart formats.

use std::fmt;

use chart::Chart;
use thiserror::Error;

/// A chart format supported by the converter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// The C2S format.
    C2s,
    /// The XLAIR-compatible SUS subset implemented by the `sus` crate.
    Sus,
    /// The supported UGC subset implemented by the `ugc` crate.
    Ugc,
}

impl Format {
    /// Returns the file extension associated with the format.
    pub fn extension(self) -> &'static str {
        match self {
            Self::C2s => "c2s",
            Self::Sus => "sus",
            Self::Ugc => "ugc",
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::C2s => "C2S",
            Self::Sus => "SUS",
            Self::Ugc => "UGC",
        })
    }
}

/// Errors returned while parsing or writing a chart format.
#[derive(Debug, Error)]
pub enum FormatError {
    #[error(transparent)]
    C2s(#[from] c2s::C2sError),
    #[error(transparent)]
    Sus(#[from] sus::SusError),
    #[error(transparent)]
    Ugc(#[from] ugc::UgcError),
}

/// Errors returned by a format-independent conversion operation.
#[derive(Debug, Error)]
pub enum ConverterError {
    #[error("failed to parse {format} chart: {source}")]
    Parse {
        format: Format,
        #[source]
        source: FormatError,
    },
    #[error("failed to write {format} chart: {source}")]
    Write {
        format: Format,
        #[source]
        source: FormatError,
    },
}

/// Parses a chart using the frontend for `format`.
pub fn parse(format: Format, source: &str) -> Result<Chart, ConverterError> {
    let result = match format {
        Format::C2s => c2s::parse(source).map_err(FormatError::from),
        Format::Sus => sus::parse(source).map_err(FormatError::from),
        Format::Ugc => ugc::parse(source).map_err(FormatError::from),
    };
    result.map_err(|source| ConverterError::Parse { format, source })
}

/// Writes a chart using the backend for `format`.
pub fn write(format: Format, chart: &Chart) -> Result<String, ConverterError> {
    let result = match format {
        Format::C2s => c2s::write(chart).map_err(FormatError::from),
        Format::Sus => sus::write(chart).map_err(FormatError::from),
        Format::Ugc => ugc::write(chart).map_err(FormatError::from),
    };
    result.map_err(|source| ConverterError::Write { format, source })
}

/// Converts a chart document from one supported format to another.
pub fn convert(
    source_format: Format,
    target_format: Format,
    source: &str,
) -> Result<String, ConverterError> {
    let chart = parse(source_format, source)?;
    write(target_format, &chart)
}

#[cfg(test)]
mod tests {
    use chart::{Chart, Lane, Note, NoteKind, Position, TapKind};

    use super::{Format, convert, write};

    const FORMATS: [Format; 3] = [Format::C2s, Format::Sus, Format::Ugc];

    fn chart() -> Chart {
        let mut chart = Chart::new();
        chart.add_note(
            Note::new(
                Position::new(0, 1).unwrap(),
                Lane::slider(0, 4).unwrap(),
                NoteKind::Tap(TapKind::Tap),
            )
            .unwrap(),
        );
        chart
    }

    #[test]
    fn exposes_format_extensions() {
        assert_eq!(Format::C2s.extension(), "c2s");
        assert_eq!(Format::Sus.extension(), "sus");
        assert_eq!(Format::Ugc.extension(), "ugc");
    }

    #[test]
    fn converts_between_all_supported_formats() {
        for source_format in FORMATS {
            let source = write(source_format, &chart()).expect("valid source chart");
            for target_format in FORMATS {
                let converted = convert(source_format, target_format, &source)
                    .expect("valid cross-format conversion");
                let parsed = super::parse(target_format, &converted).expect("valid target chart");
                assert_eq!(parsed.notes(), chart().notes());
            }
        }
    }
}
