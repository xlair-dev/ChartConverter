use thiserror::Error;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ChartError {
    #[error("position denominator must be greater than zero")]
    InvalidPositionDenominator,
    #[error("position arithmetic overflowed")]
    PositionOverflow,
    #[error("measure offset must be between zero and one")]
    InvalidMeasureOffset,
    #[error("measure length must be greater than zero")]
    InvalidMeasureLength,
    #[error("slider lane must fit within the sixteen central lanes")]
    InvalidSliderLane,
    #[error("slider lane width must be greater than zero")]
    InvalidSliderLaneWidth,
    #[error("hold end must be later than its start")]
    InvalidHoldEnd,
    #[error("slide must contain at least two points")]
    InvalidSlidePointCount,
    #[error("slide must start at the note position and lane")]
    InvalidSlideStart,
    #[error("slide points must be strictly ordered in time")]
    InvalidSlidePointOrder,
    #[error("tempo must be finite and greater than zero")]
    InvalidTempo,
    #[error("scroll speed must be finite")]
    InvalidScrollSpeed,
    #[error("scroll speed duration must be greater than zero")]
    InvalidScrollSpeedDuration,
    #[error("air height must be finite and non-negative")]
    InvalidAirHeight,
    #[error("air crush interval must be greater than zero")]
    InvalidAirCrushInterval,
    #[error("note id does not refer to a note in the chart")]
    InvalidNoteId,
}
