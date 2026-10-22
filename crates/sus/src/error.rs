use chart::ChartError;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum SusError {
    #[error("line {line}: malformed command")]
    MalformedCommand { line: usize },
    #[error("line {line}: invalid value `{value}`")]
    InvalidValue { line: usize, value: String },
    #[error("line {line}: unsupported command `{command}`")]
    UnsupportedCommand { line: usize, command: String },
    #[error("line {line}: {source}")]
    Chart { line: usize, source: ChartError },
    #[error("line {line}: channel `{channel}` has no start point")]
    MissingStart { line: usize, channel: char },
    #[error("channel `{channel}` has no end point")]
    MissingEnd { channel: char },
    #[error("line {line}: cannot expand SUS Bezier curve: {reason}")]
    InvalidCurve { line: usize, reason: &'static str },
    #[error("cannot represent `{note}` in the supported SUS output")]
    UnsupportedNote { note: String },
    #[error("position cannot be represented at the SUS resolution")]
    UnrepresentablePosition,
}

impl SusError {
    pub fn line(&self) -> Option<usize> {
        match self {
            Self::MalformedCommand { line, .. }
            | Self::InvalidValue { line, .. }
            | Self::UnsupportedCommand { line, .. }
            | Self::Chart { line, .. }
            | Self::MissingStart { line, .. }
            | Self::InvalidCurve { line, .. } => Some(*line),
            _ => None,
        }
    }
}
