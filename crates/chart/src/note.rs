#[cfg(test)]
mod tests {
    use super::{AirColor, AirProperties, Lane, Note, NoteKind, SideButton, SlidePoint, TapKind};
    use crate::{NoteId, Position};

    #[test]
    fn rejects_a_hold_that_does_not_advance_in_time() {
        let position = Position::new(1, 1).expect("valid position");

        assert!(
            Note::new(
                position,
                Lane::slider(0, 1).expect("valid lane"),
                NoteKind::Hold { end: position },
            )
            .is_err()
        );
    }

    #[test]
    fn accepts_xlair_side_button_notes() {
        let position = Position::new(1, 1).expect("valid position");

        assert!(
            Note::new(
                position,
                Lane::Side(SideButton::LeftUpper),
                NoteKind::Tap(TapKind::Tap),
            )
            .is_ok()
        );
    }

    #[test]
    fn air_notes_keep_their_parent_reference() {
        let position = Position::new(1, 1).expect("valid position");

        let note = Note::new(
            position,
            Lane::slider(0, 1).expect("valid lane"),
            NoteKind::Air {
                properties: AirProperties::new(super::AirDirection::Up),
                parent: NoteId::new(3),
            },
        )
        .expect("valid air note");

        assert_eq!(
            note.kind(),
            &NoteKind::Air {
                properties: AirProperties::new(super::AirDirection::Up),
                parent: NoteId::new(3),
            }
        );
    }

    #[test]
    fn rejects_a_slide_with_only_one_point() {
        let position = Position::new(1, 1).expect("valid position");

        assert!(
            Note::new(
                position,
                Lane::slider(0, 1).expect("valid lane"),
                NoteKind::Slide {
                    points: vec![SlidePoint::new(
                        position,
                        Lane::slider(0, 1).expect("valid lane")
                    )],
                },
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_an_air_hold_that_does_not_advance_in_time() {
        let position = Position::new(1, 1).expect("valid position");

        assert!(
            Note::new(
                position,
                Lane::slider(0, 1).expect("valid lane"),
                NoteKind::AirHold {
                    end: position,
                    properties: AirProperties::new(super::AirDirection::Up),
                    parent: NoteId::new(0),
                },
            )
            .is_err()
        );
    }

    #[test]
    fn preserves_air_attributes_and_rejects_invalid_height() {
        let properties = AirProperties::new(super::AirDirection::UpperRight)
            .with_height(2.5)
            .expect("valid height")
            .with_color(AirColor::Inverted);
        assert_eq!(properties.height(), Some(2.5));
        assert_eq!(properties.color(), AirColor::Inverted);
        assert!(
            AirProperties::new(super::AirDirection::Up)
                .with_height(f64::NAN)
                .is_err()
        );
    }
}
use crate::{ChartError, NoteId, Position};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lane {
    Slider { start: u8, width: u8 },
    Side(SideButton),
}

impl Lane {
    pub fn slider(start: u8, width: u8) -> Result<Self, ChartError> {
        if width == 0 {
            return Err(ChartError::InvalidSliderLaneWidth);
        }
        if start >= 16 || width > 16 || u16::from(start) + u16::from(width) > 16 {
            return Err(ChartError::InvalidSliderLane);
        }
        Ok(Self::Slider { start, width })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SideButton {
    LeftUpper,
    LeftLower,
    RightLower,
    RightUpper,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TapKind {
    Tap,
    XTap,
    Flick,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AirDirection {
    Up,
    UpperLeft,
    UpperRight,
    Down,
    LowerLeft,
    LowerRight,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AirColor {
    Normal,
    Inverted,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirProperties {
    direction: AirDirection,
    height: Option<f64>,
    color: AirColor,
}

impl AirProperties {
    pub fn new(direction: AirDirection) -> Self {
        Self {
            direction,
            height: None,
            color: AirColor::Normal,
        }
    }

    pub fn with_height(mut self, height: f64) -> Result<Self, crate::ChartError> {
        if !height.is_finite() || height < 0.0 {
            return Err(crate::ChartError::InvalidAirHeight);
        }
        self.height = Some(height);
        Ok(self)
    }

    pub fn with_color(mut self, color: AirColor) -> Self {
        self.color = color;
        self
    }

    pub fn direction(self) -> AirDirection {
        self.direction
    }

    pub fn height(self) -> Option<f64> {
        self.height
    }

    pub fn color(self) -> AirColor {
        self.color
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlidePoint {
    position: Position,
    lane: Lane,
}

impl SlidePoint {
    pub fn new(position: Position, lane: Lane) -> Self {
        Self { position, lane }
    }

    pub fn position(&self) -> Position {
        self.position
    }

    pub fn lane(&self) -> Lane {
        self.lane
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NoteKind {
    Tap(TapKind),
    Mine,
    Hold {
        end: Position,
    },
    Slide {
        points: Vec<SlidePoint>,
    },
    Air {
        properties: AirProperties,
        parent: NoteId,
    },
    AirHold {
        end: Position,
        properties: AirProperties,
        parent: NoteId,
    },
    AirSlide {
        points: Vec<SlidePoint>,
        properties: AirProperties,
        parent: NoteId,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    position: Position,
    lane: Lane,
    kind: NoteKind,
}

impl Note {
    pub fn new(position: Position, lane: Lane, kind: NoteKind) -> Result<Self, ChartError> {
        match &kind {
            NoteKind::Hold { end } if *end <= position => {
                return Err(ChartError::InvalidHoldEnd);
            }
            NoteKind::Slide { points } | NoteKind::AirSlide { points, .. } => {
                validate_slide(position, lane, points)?;
            }
            NoteKind::AirHold { end, .. } if *end <= position => {
                return Err(ChartError::InvalidHoldEnd);
            }
            _ => {}
        }

        Ok(Self {
            position,
            lane,
            kind,
        })
    }

    pub fn position(&self) -> Position {
        self.position
    }

    pub fn lane(&self) -> Lane {
        self.lane
    }

    pub fn kind(&self) -> &NoteKind {
        &self.kind
    }
}

fn validate_slide(position: Position, lane: Lane, points: &[SlidePoint]) -> Result<(), ChartError> {
    if points.len() < 2 {
        return Err(ChartError::InvalidSlidePointCount);
    }
    if points[0].position != position || points[0].lane != lane {
        return Err(ChartError::InvalidSlideStart);
    }
    if points
        .windows(2)
        .any(|points| points[0].position >= points[1].position)
    {
        return Err(ChartError::InvalidSlidePointOrder);
    }
    Ok(())
}
