use chart::ChartError;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum UgcError {
    #[error("line {line}: malformed record")]
    MalformedRecord { line: usize },
    #[error("line {line}: invalid value `{value}`")]
    InvalidValue { line: usize, value: String },
    #[error("line {line}: unsupported record `{record}`")]
    UnsupportedRecord { line: usize, record: String },
    #[error("line {line}: {source}")]
    Chart { line: usize, source: ChartError },
    #[error("line {line}: a note is missing a follower line")]
    MissingFollower { line: usize },
    #[error("cannot represent `{note}` in the supported UGC output")]
    UnsupportedNote { note: String },
    #[error("position cannot be represented at the UGC resolution")]
    UnrepresentablePosition,
}
