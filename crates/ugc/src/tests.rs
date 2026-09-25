use chart::{
    AirCrushColor, AirCrushInterval, AirCrushPoint, Chart, Lane, Note, NoteKind, Position,
    ScrollScope, ScrollSpeedChange, SlidePoint, SlidePointKind, TapKind,
};

use super::{parse, write};

#[test]
fn parses_ugc_timing_and_basic_notes() {
    let source = "@TICKS\t480\n@BEAT\t0\t4\t4\n@BEAT\t2\t5\t4\n@BPM\t0'0\t120\n@ENDHEAD\n#1'480:t04\n#1'960:h04\n#480>s04\n#2'0:s04\n#960>s84\n";
    let chart = parse(source).expect("valid UGC");

    assert_eq!(chart.tempo_changes().len(), 1);
    assert_eq!(chart.notes().len(), 3);
    assert_eq!(chart.notes()[0].kind(), &NoteKind::Tap(TapKind::Tap));
    assert_eq!(chart.notes()[0].position(), Position::new(5, 1).unwrap());
    assert!(matches!(chart.notes()[1].kind(), NoteKind::Hold { .. }));
    assert!(matches!(chart.notes()[2].kind(), NoteKind::Slide { .. }));
    assert_eq!(chart.notes()[0].lane(), Lane::slider(0, 4).unwrap());
}

#[test]
fn parses_holds_with_implicit_end_lane() {
    let chart = parse("@TICKS\t480\n@ENDHEAD\n#0'0:h04\n#480>s\n")
        .expect("valid UGC hold with an implicit end lane");
    assert!(matches!(
        chart.notes()[0].kind(),
        NoteKind::Hold { end } if *end == Position::new(1, 1).unwrap()
    ));
}

#[test]
fn preserves_variable_measure_lengths_when_writing_ugc() {
    let source = concat!(
        "@TICKS\t480\n",
        "@BEAT\t0\t3\t4\n",
        "@BEAT\t1\t5\t4\n",
        "@ENDHEAD\n",
        "#0'0:t04\n",
        "#1'0:t04\n",
    );
    let chart = parse(source).expect("valid variable-length UGC");
    let written = write(&chart).expect("valid variable-length UGC output");
    assert!(written.contains("@BEAT\t0\t3\t4\n"));
    assert!(written.contains("@BEAT\t1\t5\t4\n"));

    let reparsed = parse(&written).expect("round-tripped variable-length UGC");
    assert_eq!(reparsed.measure_lengths(), chart.measure_lengths());
    assert_eq!(reparsed.notes(), chart.notes());
}

#[test]
fn preserves_main_bpm_when_writing_ugc() {
    let source = "@TICKS\t480\n@MAINBPM\t132.5\n@ENDHEAD\n#0'0:t04\n";
    let chart = parse(source).expect("valid UGC main BPM");
    assert_eq!(chart.base_bpm(), Some(132.5));

    let written = write(&chart).expect("valid UGC main BPM output");
    assert!(written.contains("@MAINBPM\t132.5\n"));
    let reparsed = parse(&written).expect("round-tripped UGC main BPM");
    assert_eq!(reparsed.base_bpm(), chart.base_bpm());
}

#[test]
fn applies_ugc_soffset_to_chart_positions() {
    let source = "@TICKS\t480\n@FLAG\tSOFFSET\tTRUE\n@BEAT\t0\t3\t4\n@ENDHEAD\n#0'0:t04\n";
    let chart = parse(source).expect("valid UGC SOFFSET");
    assert_eq!(chart.notes()[0].position(), Position::new(3, 1).unwrap());

    let written = write(&chart).expect("valid normalized UGC output");
    assert!(written.contains("#1'0:t04\n"));
    let reparsed = parse(&written).expect("round-tripped normalized UGC");
    assert_eq!(reparsed.notes(), chart.notes());
}

#[test]
fn parses_long_notes_with_directives_between_parent_and_follower() {
    let chart = parse("@ENDHEAD\n#0'0:h04\n@USETIL\t1\n#480>s\n")
        .expect("valid UGC long note with an intervening directive");

    assert!(matches!(chart.notes()[0].kind(), NoteKind::Hold { .. }));
}

#[test]
fn parses_air_holds_with_an_implicit_action_end() {
    let chart = parse("@ENDHEAD\n#0'0:t04\n#0'0:H04N\n#480>c\n")
        .expect("valid UGC AIR-Hold with an implicit action end");

    assert!(
        matches!(chart.notes()[1].kind(), NoteKind::AirHold { end, .. } if *end == Position::new(1, 1).unwrap())
    );
}

#[test]
fn parses_air_crush_followers_with_implicit_lane_and_height() {
    let chart = parse("@TICKS\t480\n@ENDHEAD\n#0'0:t04\n#0'0:C0400\n#480>s\n#960>s048\n")
        .expect("valid UGC AIR Crush with implicit follower data");

    let NoteKind::AirCrush { points, .. } = chart.notes()[1].kind() else {
        panic!("expected AIR Crush");
    };
    assert_eq!(points.len(), 3);
}

#[test]
fn omits_unsupported_legacy_t_notes() {
    let chart = parse("@ENDHEAD\n#0'0:T0400\n#480>s04\n#0'0:t04\n")
        .expect("unsupported legacy T notes are lossy but parseable");

    assert_eq!(chart.notes().len(), 1);
    assert_eq!(chart.notes()[0].kind(), &NoteKind::Tap(TapKind::Tap));
}

#[test]
fn rejects_air_and_speed_records_without_a_lossy_conversion() {
    let air = "@ENDHEAD\n#0'0:a04UR";
    assert!(matches!(
        parse(air),
        Err(super::UgcError::InvalidValue { .. })
    ));
    let speed = "@MAINTIL\t1\n@ENDHEAD\n";
    assert!(matches!(
        parse(speed),
        Err(super::UgcError::UnsupportedRecord { .. })
    ));

    let speed_field = "@SPDDEF\t1\t0\t1.0\n@ENDHEAD\n";
    assert!(matches!(
        parse(speed_field),
        Err(super::UgcError::UnsupportedRecord { .. })
    ));
}

#[test]
fn omits_notes_that_ugc_cannot_represent() {
    let mut chart = Chart::new();
    chart.add_note(
        Note::new(
            Position::new(0, 1).unwrap(),
            Lane::Side(chart::SideButton::LeftUpper),
            NoteKind::Tap(TapKind::Tap),
        )
        .expect("valid side note"),
    );

    let ugc = write(&chart).expect("unsupported note is omitted");
    assert_eq!(
        ugc,
        "@VER\t8\n@EXVER\t1\n@TICKS\t480\n@BEAT\t0\t4\t4\n@ENDHEAD\n"
    );
}

#[test]
fn parses_ex_flick_and_air_notes_with_attributes() {
    let source = "@ENDHEAD\n#0'0:x04U\n#0'0:f04L\n#0'0:a04UL01I\n";
    let chart = parse(source).expect("valid extended UGC notes");

    assert_eq!(chart.notes().len(), 3);
    assert_eq!(
        chart.notes()[0].kind(),
        &NoteKind::ExTap {
            direction: chart::ExDirection::Up
        }
    );
    assert_eq!(
        chart.notes()[1].kind(),
        &NoteKind::Tap(TapKind::Flick {
            direction: Some(chart::ExDirection::Left)
        })
    );
    assert_eq!(
        chart.notes()[2].kind(),
        &NoteKind::Air {
            properties: chart::AirProperties::new(chart::AirDirection::UpperLeft)
                .with_height(1.05)
                .unwrap()
                .with_color(chart::AirColor::Inverted),
            parent: chart::NoteId::new(1),
        }
    );
}

#[test]
fn parses_air_long_notes_after_their_parent() {
    let source = "@ENDHEAD\n#0'0:h04\n#480>s04\n#0'0:H04I\n#480>s04\n";
    let chart = parse(source).expect("valid air hold");

    assert!(matches!(
        chart.notes()[1].kind(),
        NoteKind::AirHold {
            end,
            properties,
            parent,
        } if *end == Position::new(1, 1).unwrap()
            && *parent == chart::NoteId::new(0)
            && properties.direction().is_none()
            && properties.color() == chart::AirColor::Inverted
    ));
}

#[test]
fn applies_ugc_speed_groups_to_following_notes() {
    let source = "@TICKS\t480\n@BEAT\t0\t4\t4\n@TIL\t2\t0'0\t1.5\n@ENDHEAD\n@USETIL\t2\n#0'0:t04\n";
    let chart = parse(source).expect("valid speed group");

    assert_eq!(chart.scroll_speed_changes().len(), 1);
    assert_eq!(
        chart.note_speed_group(chart::NoteId::new(0)).unwrap(),
        Some(2)
    );
}

#[test]
fn writes_basic_notes_and_round_trips_them() {
    let mut chart = Chart::new();
    let start = Position::new(1, 1).unwrap();
    let end = Position::new(2, 1).unwrap();
    chart.add_note(
        Note::new(
            start,
            Lane::slider(10, 6).unwrap(),
            NoteKind::Tap(TapKind::XTap),
        )
        .unwrap(),
    );
    chart.add_note(Note::new(start, Lane::slider(0, 4).unwrap(), NoteKind::Hold { end }).unwrap());

    let ugc = write(&chart).expect("valid output");
    assert!(ugc.contains("@ENDHEAD"));
    assert_eq!(parse(&ugc).unwrap().notes(), chart.notes());
}

#[test]
fn writes_extended_taps_and_round_trips_them() {
    let mut chart = Chart::new();
    let position = Position::new(1, 1).unwrap();
    chart.add_note(
        Note::new(
            position,
            Lane::slider(0, 4).unwrap(),
            NoteKind::ExTap {
                direction: chart::ExDirection::Right,
            },
        )
        .unwrap(),
    );
    chart.add_note(
        Note::new(
            position,
            Lane::slider(4, 4).unwrap(),
            NoteKind::Tap(TapKind::Flick {
                direction: Some(chart::ExDirection::Left),
            }),
        )
        .unwrap(),
    );

    let ugc = write(&chart).expect("valid extended output");
    assert_eq!(parse(&ugc).unwrap().notes(), chart.notes());
}

#[test]
fn writes_and_parses_ex_long_carriers() {
    let mut chart = Chart::new();
    let start = Position::new(1, 1).unwrap();
    let middle = Position::new(2, 1).unwrap();
    let end = Position::new(3, 1).unwrap();
    chart.add_note(
        Note::new(
            start,
            Lane::slider(0, 4).unwrap(),
            NoteKind::ExHold {
                end: middle,
                direction: chart::ExDirection::Inward,
            },
        )
        .unwrap(),
    );
    chart.add_note(
        Note::new(
            middle,
            Lane::slider(4, 4).unwrap(),
            NoteKind::ExSlide {
                points: vec![
                    SlidePoint::new(middle, Lane::slider(4, 4).unwrap()),
                    SlidePoint::new(end, Lane::slider(8, 4).unwrap()),
                ],
                direction: chart::ExDirection::Left,
            },
        )
        .unwrap(),
    );

    let ugc = write(&chart).expect("valid UGC output");
    assert!(ugc.contains(":x04I"));
    assert!(ugc.contains(":s44"));
    let parsed = parse(&ugc).expect("round-tripped UGC output");
    assert_eq!(parsed.notes(), chart.notes());
}

#[test]
fn writes_and_parses_slide_control_points() {
    let start = Position::new(0, 1).unwrap();
    let middle = Position::new(1, 2).unwrap();
    let end = Position::new(1, 1).unwrap();
    let points = vec![
        SlidePoint::new(start, Lane::slider(0, 4).unwrap()),
        SlidePoint::new(middle, Lane::slider(4, 4).unwrap()).with_kind(SlidePointKind::Control),
        SlidePoint::new(end, Lane::slider(8, 4).unwrap()),
    ];
    let mut chart = Chart::new();
    chart.add_note(
        Note::new(
            start,
            Lane::slider(0, 4).unwrap(),
            NoteKind::Slide { points },
        )
        .unwrap(),
    );

    let ugc = write(&chart).expect("valid UGC output");
    assert!(ugc.contains("#240>c44"));
    assert_eq!(parse(&ugc).unwrap().notes(), chart.notes());
}

#[test]
fn preserves_an_ex_carrier_that_has_an_air_child() {
    let source = "@ENDHEAD\n#0'0:x04U\n#0'0:a04UC\n#0'0:h04\n#480>s04\n";
    let chart = parse(source).expect("valid ExLong carrier with AIR");

    assert!(matches!(chart.notes()[0].kind(), NoteKind::ExTap { .. }));
    assert!(matches!(
        chart.notes()[1].kind(),
        NoteKind::Air { parent, .. } if *parent == chart::NoteId::new(0)
    ));
    assert!(matches!(chart.notes()[2].kind(), NoteKind::ExHold { .. }));
}

#[test]
fn writes_and_parses_air_crush_notes() {
    let mut chart = Chart::new();
    let parent = chart.add_note(
        Note::new(
            Position::new(1, 1).unwrap(),
            Lane::slider(0, 4).unwrap(),
            NoteKind::Tap(TapKind::Tap),
        )
        .unwrap(),
    );
    let start = Position::new(1, 1).unwrap();
    let end = Position::new(2, 1).unwrap();
    chart.add_note(
        Note::new(
            start,
            Lane::slider(0, 4).unwrap(),
            NoteKind::AirCrush {
                points: vec![
                    AirCrushPoint::new(start, Lane::slider(0, 4).unwrap(), 5.0).unwrap(),
                    AirCrushPoint::new(end, Lane::slider(4, 4).unwrap(), 6.0).unwrap(),
                ],
                color: AirCrushColor::Purple,
                interval: AirCrushInterval::Every(Position::new(1, 4).unwrap()),
                parent,
            },
        )
        .unwrap(),
    );

    let ugc = write(&chart).expect("valid UGC output");
    assert!(ugc.contains("C04"));
    assert!(ugc.contains(",120"));
    let parsed = parse(&ugc).expect("round-tripped UGC output");
    assert_eq!(parsed.notes(), chart.notes());
}

#[test]
fn writes_air_mines_and_scroll_speed_records() {
    let mut chart = Chart::new();
    let position = Position::new(0, 1).unwrap();
    let parent = chart.add_note(
        Note::new(
            position,
            Lane::slider(0, 4).unwrap(),
            NoteKind::Tap(TapKind::Tap),
        )
        .unwrap(),
    );
    let air_properties = chart::AirProperties::new(chart::AirDirection::UpperLeft)
        .with_height(1.05)
        .unwrap()
        .with_color(chart::AirColor::Inverted);
    chart.add_note(
        Note::new(
            position,
            Lane::slider(0, 4).unwrap(),
            NoteKind::Air {
                properties: air_properties,
                parent,
            },
        )
        .unwrap(),
    );
    chart.add_note(Note::new(position, Lane::slider(4, 2).unwrap(), NoteKind::Mine).unwrap());
    chart.add_scroll_speed_change(
        ScrollSpeedChange::with_scope(position, 1.5, ScrollScope::Group(2)).unwrap(),
    );
    let grouped_note = chart.add_note(
        Note::new(
            Position::new(1, 1).unwrap(),
            Lane::slider(8, 2).unwrap(),
            NoteKind::Tap(TapKind::Tap),
        )
        .unwrap(),
    );
    chart.set_note_speed_group(grouped_note, Some(2)).unwrap();

    let ugc = write(&chart).expect("valid extended UGC output");
    let parsed = parse(&ugc).expect("round-tripped extended UGC output");
    assert_eq!(parsed.notes(), chart.notes());
    assert_eq!(parsed.scroll_speed_changes(), chart.scroll_speed_changes());
    assert_eq!(
        parsed.note_speed_group(chart::NoteId::new(3)).unwrap(),
        Some(2)
    );
}

#[test]
fn writes_air_long_notes_and_round_trips_their_heights() {
    let mut chart = Chart::new();
    let parent = chart.add_note(
        Note::new(
            Position::new(0, 1).unwrap(),
            Lane::slider(0, 4).unwrap(),
            NoteKind::Hold {
                end: Position::new(1, 1).unwrap(),
            },
        )
        .unwrap(),
    );
    chart.add_note(
        Note::new(
            Position::new(0, 1).unwrap(),
            Lane::slider(0, 4).unwrap(),
            NoteKind::AirHold {
                end: Position::new(1, 1).unwrap(),
                properties: chart::AirProperties::without_direction()
                    .with_color(chart::AirColor::Inverted),
                parent,
            },
        )
        .unwrap(),
    );
    let start = Position::new(1, 1).unwrap();
    let middle = Position::new(3, 2).unwrap();
    let end = Position::new(2, 1).unwrap();
    chart.add_note(
        Note::new(
            start,
            Lane::slider(4, 4).unwrap(),
            NoteKind::AirSlide {
                points: vec![
                    chart::AirPoint::new(start, Lane::slider(4, 4).unwrap(), 2.0).unwrap(),
                    chart::AirPoint::new(middle, Lane::slider(6, 4).unwrap(), 2.25)
                        .unwrap()
                        .with_kind(SlidePointKind::Control),
                    chart::AirPoint::new(end, Lane::slider(8, 4).unwrap(), 2.5).unwrap(),
                ],
                color: chart::AirColor::Normal,
                parent,
            },
        )
        .unwrap(),
    );

    let ugc = write(&chart).expect("valid AIR long UGC output");
    assert!(ugc.contains(">c6"));
    let parsed = parse(&ugc).expect("round-tripped AIR long UGC output");
    assert_eq!(parsed.notes(), chart.notes());
}

#[test]
fn applies_xlair_mode_to_ugc_extended_taps() {
    let source = "@TICKS\t480\n@BEAT\t0\t4\t4\n@ENDHEAD\n#0'0:x04D\n";
    let normal = super::parse_with_mode(source, chart::ChartMode::Normal).unwrap();
    assert!(matches!(normal.notes()[0].kind(), NoteKind::ExTap { .. }));
    let xlair = super::parse_with_mode(source, chart::ChartMode::Xlair).unwrap();
    assert_eq!(xlair.notes()[0].kind(), &NoteKind::Tap(TapKind::XTap));

    let output = super::write_with_mode(&xlair, chart::ChartMode::Xlair).unwrap();
    let reparsed = super::parse_with_mode(&output, chart::ChartMode::Xlair).unwrap();
    assert_eq!(reparsed.notes(), xlair.notes());
}

#[test]
fn maps_xlair_diagonal_air_to_side_button_taps() {
    for (direction, button) in [
        ("UL", chart::SideButton::LeftUpper),
        ("UR", chart::SideButton::RightUpper),
        ("DL", chart::SideButton::LeftLower),
        ("DR", chart::SideButton::RightLower),
    ] {
        let source =
            format!("@TICKS\t480\n@BEAT\t0\t4\t4\n@ENDHEAD\n#0'0:t04\n#0'0:a04{direction}N\n");
        let chart = super::parse_with_mode(&source, chart::ChartMode::Xlair).unwrap();
        assert_eq!(chart.notes().len(), 1);
        assert_eq!(chart.notes()[0].lane(), Lane::Side(button));
        assert_eq!(chart.notes()[0].kind(), &NoteKind::Tap(TapKind::Tap));
    }
}

#[test]
fn keeps_non_overlapping_xlair_side_air_taps_alongside_their_parent_taps() {
    let source = concat!(
        "@TICKS\t480\n",
        "@BEAT\t0\t4\t4\n",
        "@ENDHEAD\n",
        "#0'0:t04\n",
        "#0'120:a04ULN\n",
    );
    let chart = super::parse_with_mode(source, chart::ChartMode::Xlair).unwrap();

    assert_eq!(chart.notes().len(), 2);
    assert_eq!(chart.notes()[0].lane(), Lane::slider(0, 4).unwrap());
    assert_eq!(
        chart.notes()[1].lane(),
        Lane::Side(chart::SideButton::LeftUpper)
    );
}

#[test]
fn parses_xlair_side_holds_as_side_button_notes() {
    for (lane, button) in [
        ("02", chart::SideButton::LeftUpper),
        ("22", chart::SideButton::LeftLower),
        ("c2", chart::SideButton::RightLower),
        ("e2", chart::SideButton::RightUpper),
    ] {
        let source = format!("@TICKS\t480\n@BEAT\t0\t4\t4\n@ENDHEAD\n#0'0:h{lane}\n#480>s{lane}\n");
        let chart = super::parse_with_mode(&source, chart::ChartMode::Xlair).unwrap();
        assert_eq!(chart.notes().len(), 1);
        assert_eq!(chart.notes()[0].lane(), Lane::Side(button));
        assert!(matches!(chart.notes()[0].kind(), NoteKind::Hold { .. }));

        let normal = super::parse_with_mode(&source, chart::ChartMode::Normal).unwrap();
        assert!(matches!(normal.notes()[0].lane(), Lane::Slider { .. }));
    }
}

#[test]
fn omits_non_side_air_notes_in_xlair_mode() {
    let source = concat!(
        "@TICKS\t480\n",
        "@BEAT\t0\t4\t4\n",
        "@ENDHEAD\n",
        "#0'0:t04\n",
        "#0'0:a04UCN\n",
    );
    let chart = super::parse_with_mode(source, chart::ChartMode::Xlair).unwrap();

    assert_eq!(chart.notes().len(), 1);
    assert!(matches!(
        chart.notes()[0].kind(),
        NoteKind::Tap(TapKind::Tap)
    ));
}
