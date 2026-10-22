use std::borrow::Cow;

use chart::{Note, NoteKind, Position, SideButton, SlidePoint};

/// Normalize only the directive/header token, never its string arguments.
pub(super) fn normalize_command(line: &str) -> Option<Cow<'_, str>> {
    let command = line.trim().strip_prefix('#')?.trim();
    let end = command
        .find(|c: char| c == ':' || c.is_whitespace())
        .unwrap_or(command.len());
    if !command.as_bytes()[..end].iter().any(u8::is_ascii_lowercase) {
        return Some(Cow::Borrowed(command));
    }
    let mut normalized = command.to_owned();
    normalized[..end].make_ascii_uppercase();
    Some(Cow::Owned(normalized))
}

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
        NoteKind::Hold { end, .. }
        | NoteKind::ExHold { end, .. }
        | NoteKind::AirHold { end, .. } => *end,
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

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::normalize_command;

    #[test]
    fn normalizes_headers_without_rewriting_arguments() {
        assert_eq!(
            normalize_command("  #jAcKeT \"Images/MiXeD-日本語.png\"  ").unwrap(),
            "JACKET \"Images/MiXeD-日本語.png\""
        );
        assert_eq!(normalize_command("#0003aa:12ab").unwrap(), "0003AA:12ab");
        assert!(matches!(
            normalize_command("#TITLE \"MiXeD\""),
            Some(Cow::Borrowed(_))
        ));
        assert!(normalize_command("not a directive").is_none());
    }
}
