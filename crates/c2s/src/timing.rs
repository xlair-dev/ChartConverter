use chart::{Chart, Note, NoteKind, Position};

use crate::C2sError;

/// C2S timestamps and durations share a document-wide tick count per measure.
#[derive(Clone, Copy)]
pub(super) struct Timing {
    resolution: u64,
}

impl Timing {
    pub(super) fn new(resolution: u64) -> Self {
        Self { resolution }
    }

    pub(super) fn for_chart(chart: &Chart) -> Result<Self, C2sError> {
        // Retain the traditional 384-tick output when sufficient; otherwise
        // extend it exactly, rather than dropping notes with finer path points.
        let mut ticks_per_beat = 96;
        let mut include = |position: Position| -> Result<(), C2sError> {
            let denominator = position.denominator();
            ticks_per_beat = (ticks_per_beat / gcd(ticks_per_beat, denominator))
                .checked_mul(denominator)
                .ok_or(C2sError::UnrepresentablePosition)?;
            Ok(())
        };
        for change in chart.tempo_changes() {
            include(change.position())?;
        }
        for change in chart.scroll_speed_changes() {
            include(change.position())?;
            if let Some(duration) = change.duration() {
                include(duration)?;
            }
        }
        for note in chart.notes() {
            include(note.position())?;
            match note.kind() {
                NoteKind::Hold { end, checkpoints }
                | NoteKind::ExHold {
                    end, checkpoints, ..
                }
                | NoteKind::AirHold {
                    end, checkpoints, ..
                } => {
                    include(*end)?;
                    for checkpoint in checkpoints {
                        include(*checkpoint)?;
                    }
                }
                NoteKind::Slide { points } | NoteKind::ExSlide { points, .. } => {
                    for point in points {
                        include(point.position())?;
                    }
                }
                NoteKind::AirSlide { points, .. } | NoteKind::AirCrush { points, .. } => {
                    for point in points {
                        include(point.position())?;
                    }
                }
                NoteKind::Tap(_)
                | NoteKind::ExTap { .. }
                | NoteKind::Mine
                | NoteKind::Air { .. } => {}
            }
        }
        let resolution = ticks_per_beat
            .checked_mul(4)
            .ok_or(C2sError::UnrepresentablePosition)?;
        Ok(Self::new(resolution))
    }

    pub(super) fn resolution(self) -> u64 {
        self.resolution
    }

    pub(super) fn position(self, position: Position) -> Result<(u32, u64), C2sError> {
        let ticks = self.ticks(position)?;
        Ok((
            u32::try_from(ticks / self.resolution)
                .map_err(|_| C2sError::UnrepresentablePosition)?,
            ticks % self.resolution,
        ))
    }

    pub(super) fn ticks(self, position: Position) -> Result<u64, C2sError> {
        let ticks = u128::from(position.numerator()) * u128::from(self.resolution);
        let denominator = u128::from(position.denominator()) * 4;
        if ticks % denominator != 0 {
            return Err(C2sError::UnrepresentablePosition);
        }
        u64::try_from(ticks / denominator).map_err(|_| C2sError::UnrepresentablePosition)
    }

    pub(super) fn duration_ticks(self, start: Position, end: Position) -> Result<u64, C2sError> {
        self.ticks(end)?
            .checked_sub(self.ticks(start)?)
            .ok_or(C2sError::UnrepresentablePosition)
    }

    pub(super) fn note_duration_ticks(self, note: &Note) -> Result<u64, C2sError> {
        let end = match note.kind() {
            NoteKind::Hold { end, .. }
            | NoteKind::ExHold { end, .. }
            | NoteKind::AirHold { end, .. } => *end,
            NoteKind::Slide { points } | NoteKind::ExSlide { points, .. } => points
                .last()
                .ok_or(C2sError::UnrepresentablePosition)?
                .position(),
            NoteKind::AirSlide { points, .. } => points
                .last()
                .ok_or(C2sError::UnrepresentablePosition)?
                .position(),
            NoteKind::AirCrush { points, .. } => points
                .last()
                .ok_or(C2sError::UnrepresentablePosition)?
                .position(),
            _ => return Ok(1),
        };
        self.duration_ticks(note.position(), end)
    }
}

fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}
