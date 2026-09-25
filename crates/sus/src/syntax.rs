use chart::{Note, NoteKind, Position, SideButton, SlidePoint};

pub(super) fn standard_parent_code(note: &Note) -> &'static str {
    match note.kind() {
        NoteKind::Tap(chart::TapKind::Flick { .. }) => "FLK",
        NoteKind::Tap(_) => "TAP",
        NoteKind::ExTap { .. } => "CHR",
        NoteKind::Mine => "MNE",
        NoteKind::Hold { .. } | NoteKind::ExHold { .. } | NoteKind::AirHold { .. } => "HLD",
        NoteKind::Slide { .. } | NoteKind::ExSlide { .. } => "SLD",
        NoteKind::Air { .. } | NoteKind::AirSlide { .. } | NoteKind::AirCrush { .. } => "TAP",
    }
}

pub(super) fn note_end_position(note: &Note) -> Position {
    match note.kind() {
        NoteKind::Hold { end } | NoteKind::ExHold { end, .. } | NoteKind::AirHold { end, .. } => {
            *end
        }
        NoteKind::Slide { points } | NoteKind::ExSlide { points, .. } => points
            .last()
            .map(SlidePoint::position)
            .unwrap_or(note.position()),
        NoteKind::AirSlide { points, .. } | NoteKind::AirCrush { points, .. } => points
            .last()
            .map(chart::AirPoint::position)
            .unwrap_or(note.position()),
        _ => note.position(),
    }
}

pub(super) fn side_lane(button: SideButton) -> u8 {
    button.xlair_lane_start()
}

pub(super) fn side_direction(button: SideButton) -> char {
    match button {
        SideButton::LeftUpper => '3',
        SideButton::RightUpper => '4',
        SideButton::LeftLower => '5',
        SideButton::RightLower => '6',
    }
}
