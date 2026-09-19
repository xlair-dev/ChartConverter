use crate::{
    note::Note,
    timing::{ScrollSpeedChange, TempoChange},
};

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
    tempo_changes: Vec<TempoChange>,
    scroll_speed_changes: Vec<ScrollSpeedChange>,
}

impl Chart {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_note(&mut self, note: Note) -> NoteId {
        let id = NoteId::new(self.notes.len() as u32);
        self.notes.push(note);
        id
    }

    pub fn add_tempo_change(&mut self, tempo_change: TempoChange) {
        self.tempo_changes.push(tempo_change);
    }

    pub fn add_scroll_speed_change(&mut self, change: ScrollSpeedChange) {
        self.scroll_speed_changes.push(change);
    }

    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    pub fn tempo_changes(&self) -> &[TempoChange] {
        &self.tempo_changes
    }

    pub fn scroll_speed_changes(&self) -> &[ScrollSpeedChange] {
        &self.scroll_speed_changes
    }
}
