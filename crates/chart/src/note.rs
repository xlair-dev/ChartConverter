#[cfg(test)]
mod tests {
    use super::{
        AirColor, AirCrushColor, AirCrushInterval, AirCrushPoint, AirPoint, AirProperties, Lane,
        Note, NoteKind, SideButton, SlidePoint, TapKind,
    };
    use crate::{ExDirection, NoteId, Position};

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
    fn accepts_control_points_at_the_same_time_as_the_previous_point() {
        let start = Position::new(0, 1).expect("valid position");
        let end = Position::new(1, 1).expect("valid position");
        let lane = Lane::slider(0, 4).expect("valid lane");
        let points = vec![
            SlidePoint::new(start, lane),
            SlidePoint::new(start, Lane::slider(1, 2).expect("valid lane"))
                .with_kind(super::SlidePointKind::Control),
            SlidePoint::new(end, Lane::slider(4, 4).expect("valid lane")),
        ];

        assert!(Note::new(start, lane, NoteKind::Slide { points }).is_ok());
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

    #[test]
    fn accepts_ex_long_notes_and_preserves_their_direction() {
        let start = Position::new(0, 1).expect("valid position");
        let end = Position::new(1, 1).expect("valid position");
        let note = Note::new(
            start,
            Lane::slider(0, 4).expect("valid lane"),
            NoteKind::ExHold {
                end,
                direction: ExDirection::Left,
            },
        )
        .expect("valid Ex Hold");

        assert_eq!(
            note.kind(),
            &NoteKind::ExHold {
                end,
                direction: ExDirection::Left,
            }
        );
    }

    #[test]
    fn validates_air_crush_points_and_interval() {
        let start = Position::new(0, 1).expect("valid position");
        let end = Position::new(1, 1).expect("valid position");
        let lane = Lane::slider(0, 4).expect("valid lane");
        let points = vec![
            AirCrushPoint::new(start, lane, 5.0).expect("valid height"),
            AirCrushPoint::new(end, Lane::slider(4, 4).expect("valid lane"), 6.0)
                .expect("valid height"),
        ];
        let note = Note::new(
            start,
            lane,
            NoteKind::AirCrush {
                points,
                color: AirCrushColor::Normal,
                interval: AirCrushInterval::Every(Position::new(1, 4).unwrap()),
                parent: NoteId::new(0),
            },
        )
        .expect("valid Air Crush");

        assert!(matches!(note.kind(), NoteKind::AirCrush { .. }));
        assert!(
            Note::new(
                start,
                lane,
                NoteKind::AirCrush {
                    points: vec![
                        AirCrushPoint::new(start, lane, 5.0).unwrap(),
                        AirCrushPoint::new(end, Lane::slider(4, 4).unwrap(), 6.0).unwrap(),
                    ],
                    color: AirCrushColor::Normal,
                    interval: AirCrushInterval::Every(Position::new(0, 1).unwrap()),
                    parent: NoteId::new(0),
                },
            )
            .is_err()
        );
    }

    #[test]
    fn preserves_heights_on_air_slide_points() {
        let start = Position::new(0, 1).unwrap();
        let middle = Position::new(1, 2).unwrap();
        let end = Position::new(1, 1).unwrap();
        let start_lane = Lane::slider(0, 4).unwrap();
        let points = vec![
            AirPoint::new(start, start_lane, 2.0).unwrap(),
            AirPoint::new(middle, Lane::slider(4, 4).unwrap(), 2.5).unwrap(),
            AirPoint::new(end, Lane::slider(8, 4).unwrap(), 3.0).unwrap(),
        ];
        let note = Note::new(
            start,
            start_lane,
            NoteKind::AirSlide {
                points,
                color: AirColor::Normal,
                parent: NoteId::new(0),
            },
        )
        .expect("valid AIR Slide");

        let NoteKind::AirSlide { points, .. } = note.kind() else {
            panic!("expected AIR Slide");
        };
        assert_eq!(points[1].height(), 2.5);
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
    Flick { direction: Option<ExDirection> },
    Tap4,
    Tap5,
    Tap6,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExDirection {
    Up,
    Down,
    Center,
    All,
    Wide,
    Left,
    Right,
    Inward,
    UpperLeft,
    UpperRight,
    LowerLeft,
    LowerRight,
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

/// Attributes defined by SUS `ATR` records and applied to subsequent notes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NoteAttributes {
    roll_speed: Option<f64>,
    height: Option<f64>,
    priority: Option<i32>,
}

impl NoteAttributes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_roll_speed(mut self, roll_speed: f64) -> Result<Self, crate::ChartError> {
        if !roll_speed.is_finite() {
            return Err(crate::ChartError::InvalidNoteAttribute);
        }
        self.roll_speed = Some(roll_speed);
        Ok(self)
    }

    pub fn with_height(mut self, height: f64) -> Result<Self, crate::ChartError> {
        if !height.is_finite() || height < 0.0 {
            return Err(crate::ChartError::InvalidNoteAttribute);
        }
        self.height = Some(height);
        Ok(self)
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = Some(priority);
        self
    }

    pub fn roll_speed(self) -> Option<f64> {
        self.roll_speed
    }

    pub fn height(self) -> Option<f64> {
        self.height
    }

    pub fn priority(self) -> Option<i32> {
        self.priority
    }

    pub fn is_empty(self) -> bool {
        self == Self::default()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AirColor {
    Normal,
    Inverted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AirCrushColor {
    Normal,
    Transparent,
    Red,
    Orange,
    Yellow,
    Lime,
    Green,
    Aqua,
    Cyan,
    DarkBlue,
    Blue,
    Violet,
    Purple,
    Pink,
    Gray,
    Black,
}

/// Describes how an AIR Crush creates combo points along its path.
///
/// C2S and UGC use different textual values for the same three semantics: a
/// zero interval is an AIR-TRACE, `$` is a start-only crush, and a positive
/// interval repeats combo points at that distance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AirCrushInterval {
    /// Do not create combo points along the path.
    Trace,
    /// Create a combo point only at the start of the path.
    Start,
    /// Repeat combo points at the given positive interval.
    Every(Position),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirProperties {
    direction: Option<AirDirection>,
    height: Option<f64>,
    color: AirColor,
}

impl AirProperties {
    pub fn new(direction: AirDirection) -> Self {
        Self {
            direction: Some(direction),
            height: None,
            color: AirColor::Normal,
        }
    }

    /// Creates properties for an AIR long note, whose format does not encode a direction.
    pub fn without_direction() -> Self {
        Self {
            direction: None,
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

    /// Returns `None` when the source format does not define an AIR direction.
    pub fn direction(self) -> Option<AirDirection> {
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
pub enum SlidePointKind {
    Visible,
    Control,
    Invisible,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlidePoint {
    position: Position,
    lane: Lane,
    kind: SlidePointKind,
}

impl SlidePoint {
    pub fn new(position: Position, lane: Lane) -> Self {
        Self {
            position,
            lane,
            kind: SlidePointKind::Visible,
        }
    }

    pub fn with_kind(mut self, kind: SlidePointKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn position(&self) -> Position {
        self.position
    }

    pub fn lane(&self) -> Lane {
        self.lane
    }

    pub fn kind(&self) -> &SlidePointKind {
        &self.kind
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NoteKind {
    Tap(TapKind),
    ExTap {
        direction: ExDirection,
    },
    Mine,
    Hold {
        end: Position,
    },
    ExHold {
        end: Position,
        direction: ExDirection,
    },
    Slide {
        points: Vec<SlidePoint>,
    },
    ExSlide {
        points: Vec<SlidePoint>,
        direction: ExDirection,
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
        points: Vec<AirPoint>,
        color: AirColor,
        parent: NoteId,
    },
    AirCrush {
        points: Vec<AirPoint>,
        color: AirCrushColor,
        interval: AirCrushInterval,
        parent: NoteId,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    position: Position,
    lane: Lane,
    kind: NoteKind,
    attributes: NoteAttributes,
}

impl Note {
    pub fn new(position: Position, lane: Lane, kind: NoteKind) -> Result<Self, ChartError> {
        match &kind {
            NoteKind::Hold { end } | NoteKind::ExHold { end, .. } if *end <= position => {
                return Err(ChartError::InvalidHoldEnd);
            }
            NoteKind::Slide { points } | NoteKind::ExSlide { points, .. } => {
                validate_slide(position, lane, points)?;
            }
            NoteKind::AirSlide { points, .. } => validate_air_path(position, lane, points)?,
            NoteKind::AirHold { end, .. } if *end <= position => {
                return Err(ChartError::InvalidHoldEnd);
            }
            NoteKind::AirCrush {
                points,
                interval: AirCrushInterval::Every(interval),
                ..
            } => {
                if *interval == Position::new(0, 1).expect("valid position") {
                    return Err(ChartError::InvalidAirCrushInterval);
                }
                validate_air_path(position, lane, points)?;
            }
            NoteKind::AirCrush { points, .. } => {
                validate_air_path(position, lane, points)?;
            }
            _ => {}
        }

        Ok(Self {
            position,
            lane,
            kind,
            attributes: NoteAttributes::default(),
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

    pub fn with_attributes(mut self, attributes: NoteAttributes) -> Self {
        self.attributes = attributes;
        self
    }

    pub fn attributes(&self) -> NoteAttributes {
        self.attributes
    }

    /// Appends a point to a slide while preserving its temporal invariants.
    pub fn append_slide_point(&mut self, point: SlidePoint) -> Result<(), ChartError> {
        let points = match &mut self.kind {
            NoteKind::Slide { points } | NoteKind::ExSlide { points, .. } => points,
            _ => return Err(ChartError::InvalidSlidePointCount),
        };
        if points
            .last()
            .is_some_and(|last| is_invalid_point_order(last.position(), point.position()))
        {
            return Err(ChartError::InvalidSlidePointOrder);
        }
        points.push(point);
        Ok(())
    }

    /// Appends a height-aware point to an AIR Slide while preserving its temporal invariants.
    pub fn append_air_slide_point(&mut self, point: AirPoint) -> Result<(), ChartError> {
        let points = match &mut self.kind {
            NoteKind::AirSlide { points, .. } => points,
            _ => return Err(ChartError::InvalidSlidePointCount),
        };
        if points
            .last()
            .is_some_and(|last| is_invalid_point_order(last.position(), point.position()))
        {
            return Err(ChartError::InvalidSlidePointOrder);
        }
        points.push(point);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AirPoint {
    position: Position,
    lane: Lane,
    height: f64,
    kind: SlidePointKind,
}

impl AirPoint {
    pub fn new(position: Position, lane: Lane, height: f64) -> Result<Self, ChartError> {
        if !height.is_finite() || height < 0.0 {
            return Err(ChartError::InvalidAirHeight);
        }
        Ok(Self {
            position,
            lane,
            height,
            kind: SlidePointKind::Visible,
        })
    }

    pub fn with_kind(mut self, kind: SlidePointKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn position(&self) -> Position {
        self.position
    }

    pub fn lane(&self) -> Lane {
        self.lane
    }

    pub fn height(&self) -> f64 {
        self.height
    }

    pub fn kind(&self) -> &SlidePointKind {
        &self.kind
    }
}

/// Height-aware path point shared by AIR Slide and AIR Crush notes.
pub type AirSlidePoint = AirPoint;

/// Compatibility alias for the height-aware AIR Crush path point.
pub type AirCrushPoint = AirPoint;

fn validate_air_path(
    position: Position,
    lane: Lane,
    points: &[AirPoint],
) -> Result<(), ChartError> {
    if points.len() < 2 {
        return Err(ChartError::InvalidSlidePointCount);
    }
    if points[0].position() != position || points[0].lane() != lane {
        return Err(ChartError::InvalidSlideStart);
    }
    if points.windows(2).any(|points| {
        is_invalid_point_order(points[0].position(), points[1].position())
            || !points[0].height().is_finite()
            || points[0].height() < 0.0
    }) || points
        .last()
        .is_some_and(|point| !point.height().is_finite() || point.height() < 0.0)
    {
        return Err(ChartError::InvalidSlidePointOrder);
    }
    Ok(())
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
        .any(|points| is_invalid_point_order(points[0].position(), points[1].position()))
    {
        return Err(ChartError::InvalidSlidePointOrder);
    }
    Ok(())
}

/// C2S can encode zero-duration segments, so path points are non-decreasing.
fn is_invalid_point_order(previous: Position, current: Position) -> bool {
    previous > current
}
