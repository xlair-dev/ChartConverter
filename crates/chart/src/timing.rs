#[cfg(test)]
mod tests {
    use super::{ScrollScope, ScrollSpeedChange, TempoChange};
    use crate::{NoteId, Position};

    #[test]
    fn rejects_non_positive_tempo() {
        let position = Position::new(0, 1).expect("valid position");

        assert!(TempoChange::new(position, 0.0).is_err());
    }

    #[test]
    fn rejects_non_finite_scroll_speed() {
        let position = Position::new(0, 1).expect("valid position");

        assert!(ScrollSpeedChange::new(position, f64::NAN).is_err());
    }

    #[test]
    fn preserves_the_scope_of_a_speed_change() {
        let position = Position::new(0, 1).expect("valid position");
        let change =
            ScrollSpeedChange::with_scope(position, 1.0, ScrollScope::Note(NoteId::new(2)))
                .expect("valid speed change");

        assert_eq!(change.scope(), ScrollScope::Note(NoteId::new(2)));
    }
}
use crate::{ChartError, Lane, NoteId, Position};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollScope {
    /// Applies to all notes after the change.
    Global,
    /// Applies only to notes intersecting the specified slider lane.
    Lane(Lane),
    /// Applies only to one note and is used for format-specific note speed records.
    Note(NoteId),
    /// Identifies a format-specific speed definition applied to notes in a group.
    Group(u32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoChange {
    position: Position,
    bpm: f64,
}

impl TempoChange {
    pub fn new(position: Position, bpm: f64) -> Result<Self, ChartError> {
        if !bpm.is_finite() || bpm <= 0.0 {
            return Err(ChartError::InvalidTempo);
        }
        Ok(Self { position, bpm })
    }

    pub fn position(&self) -> Position {
        self.position
    }

    pub fn bpm(&self) -> f64 {
        self.bpm
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollSpeedChange {
    position: Position,
    speed: f64,
    scope: ScrollScope,
}

impl ScrollSpeedChange {
    pub fn new(position: Position, speed: f64) -> Result<Self, ChartError> {
        Self::with_scope(position, speed, ScrollScope::Global)
    }

    pub fn with_scope(
        position: Position,
        speed: f64,
        scope: ScrollScope,
    ) -> Result<Self, ChartError> {
        if !speed.is_finite() {
            return Err(ChartError::InvalidScrollSpeed);
        }
        Ok(Self {
            position,
            speed,
            scope,
        })
    }

    pub fn position(&self) -> Position {
        self.position
    }

    pub fn speed(&self) -> f64 {
        self.speed
    }

    pub fn scope(&self) -> ScrollScope {
        self.scope
    }
}
