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
    /// Identifies a format from a file extension, with or without a leading dot.
    pub fn from_extension(extension: &str) -> Option<Self> {
        let extension = extension.strip_prefix('.').unwrap_or(extension);
        if extension.eq_ignore_ascii_case("c2s") {
            Some(Self::C2s)
        } else if extension.eq_ignore_ascii_case("sus") {
            Some(Self::Sus)
        } else if extension.eq_ignore_ascii_case("ugc") {
            Some(Self::Ugc)
        } else {
            None
        }
    }

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

#[derive(Debug, Error)]
pub enum SourceEncodingError {
    #[error("source is not valid UTF-8")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("source contains invalid UTF-16")]
    Utf16,
}

/// Errors returned by a format-independent conversion operation.
#[derive(Debug, Error)]
pub enum ConverterError {
    #[error("failed to decode {format} chart: {source}")]
    Encoding {
        format: Format,
        #[source]
        source: SourceEncodingError,
    },
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

/// Parses a chart from a text file, accepting UTF-8 and BOM-marked UTF-16.
pub fn parse_bytes(format: Format, source: &[u8]) -> Result<Chart, ConverterError> {
    let source =
        decode_source(source).map_err(|source| ConverterError::Encoding { format, source })?;
    parse(format, &source)
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
///
/// Unsupported or unrepresentable chart information is omitted and reported to stdout.
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

/// Converts a chart file from one supported format to another.
pub fn convert_bytes(
    source_format: Format,
    target_format: Format,
    source: &[u8],
) -> Result<String, ConverterError> {
    let chart = parse_bytes(source_format, source)?;
    write(target_format, &chart)
}

fn decode_source(source: &[u8]) -> Result<String, SourceEncodingError> {
    if let Some(source) = source.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return Ok(std::str::from_utf8(source)?.to_owned());
    }
    if let Some(source) = source.strip_prefix(&[0xff, 0xfe]) {
        return decode_utf16(source, u16::from_le_bytes);
    }
    if let Some(source) = source.strip_prefix(&[0xfe, 0xff]) {
        return decode_utf16(source, u16::from_be_bytes);
    }
    Ok(std::str::from_utf8(source)?.to_owned())
}

fn decode_utf16(
    source: &[u8],
    decode_unit: impl Fn([u8; 2]) -> u16,
) -> Result<String, SourceEncodingError> {
    if !source.len().is_multiple_of(2) {
        return Err(SourceEncodingError::Utf16);
    }
    let units = source
        .chunks(2)
        .map(|chunk| decode_unit([chunk[0], chunk[1]]));
    char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .map_err(|_| SourceEncodingError::Utf16)
}

#[cfg(test)]
mod tests {
    use chart::{Chart, Lane, Note, NoteKind, Position, TapKind};

    use super::{Format, convert, parse_bytes, write};

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
        assert_eq!(Format::from_extension(".C2S"), Some(Format::C2s));
        assert_eq!(Format::from_extension("sus"), Some(Format::Sus));
        assert_eq!(Format::from_extension("chart"), None);
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

    fn assert_round_trip(format: Format, source: &str) {
        let chart = super::parse(format, source).expect("valid source chart");
        let output = super::write(format, &chart).expect("representable chart");
        let round_tripped = super::parse(format, &output).expect("valid round-tripped chart");
        assert_eq!(round_tripped, chart);
    }

    #[test]
    fn preserves_c2s_information_through_the_shared_ir() {
        assert_round_trip(
            Format::C2s,
            "RESOLUTION\t384\nBPM\t0\t0\t120.000\nTAP\t0\t96\t4\t2\nHLD\t0\t192\t8\t4\t192\nSLD\t1\t0\t0\t4\t384\t8\t4\n",
        );
    }

    #[test]
    fn preserves_sus_information_through_the_shared_ir() {
        assert_round_trip(
            Format::Sus,
            "#BPM01: 120\n#00008: 01\n#00120A: 14\n#00220A: 24\n",
        );
    }

    #[test]
    fn preserves_ugc_information_through_the_shared_ir() {
        assert_round_trip(
            Format::Ugc,
            "@TICKS\t480\n@BEAT\t0\t4\t4\n@BPM\t0'0\t120\n@ENDHEAD\n#1'480:t04\n#1'960:h04\n#480>s04\n",
        );
    }

    #[test]
    fn parses_bom_marked_utf16_c2s() {
        let source = "RESOLUTION\t384\nTAP\t0\t0\t0\t4\n";
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(source.encode_utf16().flat_map(u16::to_le_bytes));

        let chart = parse_bytes(Format::C2s, &bytes).expect("valid UTF-16LE C2S");
        assert_eq!(chart.notes().len(), 1);
    }
}
