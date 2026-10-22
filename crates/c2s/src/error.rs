use chart::ChartError;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum C2sError {
    #[error("line {line}: malformed record")]
    MalformedRecord { line: usize },
    #[error("line {line}: invalid value `{value}`")]
    InvalidValue { line: usize, value: String },
    #[error("line {line}: unsupported record `{record}`")]
    UnsupportedRecord { line: usize, record: String },
    #[error("line {line}: {source}")]
    Chart { line: usize, source: ChartError },
    #[error("the RESOLUTION header is missing")]
    MissingResolution,
    #[error("position cannot be represented at the C2S resolution")]
    UnrepresentablePosition,
}

impl C2sError {
    pub fn line(&self) -> Option<usize> {
        match self {
            Self::MalformedRecord { line, .. }
            | Self::InvalidValue { line, .. }
            | Self::UnsupportedRecord { line, .. }
            | Self::Chart { line, .. } => Some(*line),
            _ => None,
        }
    }
}
