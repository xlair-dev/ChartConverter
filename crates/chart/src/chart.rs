use crate::{
    Position,
    error::ChartError,
    note::{AirPoint, Note, SlidePoint},
    timing::{ScrollSpeedChange, TempoChange},
};

#[cfg(test)]
mod tests {
    use super::Chart;
    use crate::{Lane, Note, NoteKind, Position, TapKind};

    #[test]
    fn assigns_speed_groups_to_note_ids() {
        let mut chart = Chart::new();
        let note_id = chart.add_note(
            Note::new(
                Position::new(0, 1).unwrap(),
                Lane::slider(0, 1).unwrap(),
                NoteKind::Tap(TapKind::Tap),
            )
            .unwrap(),
        );

        chart
            .set_note_speed_group(note_id, Some(2))
            .expect("valid note id");
        assert_eq!(chart.note_speed_group(note_id).unwrap(), Some(2));
    }
}

/// Identifies a note by its insertion index in a chart.
///
/// A chart implementation must preserve this identity when it reorders notes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NoteId(u32);

impl NoteId {
    pub fn new(value: u32) -> Self {
        Self(value)
    }

    pub fn value(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chart {
    notes: Vec<Note>,
    note_speed_groups: Vec<Option<u32>>,
    tempo_changes: Vec<TempoChange>,
    scroll_speed_changes: Vec<ScrollSpeedChange>,
    measure_lengths: Vec<(u32, Position)>,
    priority_enabled: Option<bool>,
}

impl Chart {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_note(&mut self, note: Note) -> NoteId {
        let id = NoteId::new(self.notes.len() as u32);
        self.notes.push(note);
        self.note_speed_groups.push(None);
        id
    }

    pub fn add_tempo_change(&mut self, tempo_change: TempoChange) {
        self.tempo_changes.push(tempo_change);
    }

    pub fn add_scroll_speed_change(&mut self, change: ScrollSpeedChange) {
        self.scroll_speed_changes.push(change);
    }

    /// Stores the SUS priority-rendering request when the source specifies it.
    pub fn set_priority_enabled(&mut self, enabled: bool) {
        self.priority_enabled = Some(enabled);
    }

    /// Stores the length used by a SUS measure from this measure onward.
    ///
    /// The change is format metadata; note positions remain absolute beat positions.
    pub fn set_measure_length(&mut self, measure: u32, length: Position) -> Result<(), ChartError> {
        if length.numerator() == 0 {
            return Err(ChartError::InvalidMeasureLength);
        }
        if let Some((_, current)) = self
            .measure_lengths
            .iter_mut()
            .find(|(current_measure, _)| *current_measure == measure)
        {
            *current = length;
        } else {
            self.measure_lengths.push((measure, length));
            self.measure_lengths
                .sort_by_key(|(current_measure, _)| *current_measure);
        }
        Ok(())
    }

    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// Returns the note identified by its stable insertion-order identity.
    pub fn note(&self, note_id: NoteId) -> Result<&Note, ChartError> {
        self.notes
            .get(note_id.value() as usize)
            .ok_or(ChartError::InvalidNoteId)
    }

    pub fn append_slide_point(
        &mut self,
        note_id: NoteId,
        point: SlidePoint,
    ) -> Result<(), ChartError> {
        self.notes
            .get_mut(note_id.value() as usize)
            .ok_or(ChartError::InvalidNoteId)?
            .append_slide_point(point)
    }

    pub fn append_air_slide_point(
        &mut self,
        note_id: NoteId,
        point: AirPoint,
    ) -> Result<(), ChartError> {
        self.notes
            .get_mut(note_id.value() as usize)
            .ok_or(ChartError::InvalidNoteId)?
            .append_air_slide_point(point)
    }

    pub fn replace_note(&mut self, note_id: NoteId, note: Note) -> Result<(), ChartError> {
        *self
            .notes
            .get_mut(note_id.value() as usize)
            .ok_or(ChartError::InvalidNoteId)? = note;
        Ok(())
    }

    pub fn set_note_speed_group(
        &mut self,
        note_id: NoteId,
        group: Option<u32>,
    ) -> Result<(), ChartError> {
        let speed_group = self
            .note_speed_groups
            .get_mut(note_id.value() as usize)
            .ok_or(ChartError::InvalidNoteId)?;
        *speed_group = group;
        Ok(())
    }

    pub fn note_speed_group(&self, note_id: NoteId) -> Result<Option<u32>, ChartError> {
        self.note_speed_groups
            .get(note_id.value() as usize)
            .copied()
            .ok_or(ChartError::InvalidNoteId)
    }

    pub fn tempo_changes(&self) -> &[TempoChange] {
        &self.tempo_changes
    }

    pub fn scroll_speed_changes(&self) -> &[ScrollSpeedChange] {
        &self.scroll_speed_changes
    }

    /// Returns SUS measure-length changes in ascending measure order.
    pub fn measure_lengths(&self) -> &[(u32, Position)] {
        &self.measure_lengths
    }

    /// Returns the SUS priority-rendering request, if one was specified.
    pub fn priority_enabled(&self) -> Option<bool> {
        self.priority_enabled
    }
}
