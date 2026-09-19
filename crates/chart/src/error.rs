use thiserror::Error;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ChartError {
    #[error("beat denominator must be greater than zero")]
    InvalidBeatDenominator,
    #[error("beat numerator must be less than denominator")]
    InvalidBeatFraction,
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
}
