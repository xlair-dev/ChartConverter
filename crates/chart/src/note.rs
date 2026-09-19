#[cfg(test)]
mod tests {
    use super::{Lane, Note, NoteKind, SideButton, SlidePoint, TapKind};
    use crate::{Beat, NoteId};

    #[test]
    fn rejects_a_hold_that_does_not_advance_in_time() {
        let position = Beat::new(1, 0, 1).expect("valid beat");

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
        let position = Beat::new(1, 0, 1).expect("valid beat");

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
        let position = Beat::new(1, 0, 1).expect("valid beat");

        let note = Note::new(
            position,
            Lane::slider(0, 1).expect("valid lane"),
            NoteKind::Air {
                direction: super::AirDirection::Up,
                parent: NoteId::new(3),
            },
        )
        .expect("valid air note");

        assert_eq!(
            note.kind(),
            &NoteKind::Air {
                direction: super::AirDirection::Up,
                parent: NoteId::new(3),
            }
        );
    }

    #[test]
    fn rejects_a_slide_with_only_one_point() {
        let position = Beat::new(1, 0, 1).expect("valid beat");

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
}
use crate::{Beat, ChartError, NoteId};

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

#[derive(Clone, Debug, PartialEq)]
pub struct SlidePoint {
    position: Beat,
    lane: Lane,
}

impl SlidePoint {
    pub fn new(position: Beat, lane: Lane) -> Self {
        Self { position, lane }
    }

    pub fn position(&self) -> Beat {
        self.position
    }

    pub fn lane(&self) -> Lane {
        self.lane
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NoteKind {
    Tap(TapKind),
    Hold {
        end: Beat,
    },
    Slide {
        points: Vec<SlidePoint>,
    },
    Air {
        direction: AirDirection,
        parent: NoteId,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    position: Beat,
    lane: Lane,
    kind: NoteKind,
}

impl Note {
    pub fn new(position: Beat, lane: Lane, kind: NoteKind) -> Result<Self, ChartError> {
        match &kind {
            NoteKind::Hold { end } if *end <= position => {
                return Err(ChartError::InvalidHoldEnd);
            }
            NoteKind::Slide { points } => validate_slide(position, lane, points)?,
            _ => {}
        }

        Ok(Self {
            position,
            lane,
            kind,
        })
    }

    pub fn position(&self) -> Beat {
        self.position
    }

    pub fn lane(&self) -> Lane {
        self.lane
    }

    pub fn kind(&self) -> &NoteKind {
        &self.kind
    }
}

fn validate_slide(position: Beat, lane: Lane, points: &[SlidePoint]) -> Result<(), ChartError> {
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
