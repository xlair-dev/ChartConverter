use chart::{
    AirCrushColor, AirCrushInterval, AirCrushPoint, Chart, Lane, Note, NoteKind, Position,
    ScrollScope, ScrollSpeedChange, SlidePoint, SlidePointKind, TapKind, TempoChange,
};

use super::{parse, write};

#[test]
fn parses_timing_taps_holds_and_slides() {
    let source = "RESOLUTION\t384\nBPM\t0\t0\t120.000\nMET\t1\t0\t3\t4\nTAP\t0\t96\t4\t2\nHLD\t0\t192\t8\t4\t192\nSLD\t1\t0\t0\t4\t384\t8\t4\n";
    let chart = parse(source).expect("valid C2S");

    assert_eq!(chart.tempo_changes().len(), 1);
    assert_eq!(chart.notes().len(), 3);
    assert_eq!(chart.notes()[0].position(), Position::new(1, 1).unwrap());
    assert_eq!(chart.notes()[0].lane(), Lane::slider(4, 2).unwrap());
    assert_eq!(chart.notes()[0].kind(), &NoteKind::Tap(TapKind::Tap));
    assert_eq!(chart.notes()[2].position(), Position::new(4, 1).unwrap());
    assert!(matches!(chart.notes()[1].kind(), NoteKind::Hold { .. }));
    assert!(matches!(chart.notes()[2].kind(), NoteKind::Slide { .. }));
}

#[test]
fn parses_mines_air_parents_and_scroll_speed_records() {
    let source = "RESOLUTION\t384\nCHR\t0\t0\t4\t2\tDW\nAIR\t0\t0\t4\t2\tCHR\tDEF\nMNE\t0\t96\t8\t1\nSFL\t0\t0\t96\t1.5\nSLP\t1\t0\t96\t2.0\t3\n";
    let chart = parse(source).expect("valid extended C2S");

    assert!(matches!(chart.notes()[0].kind(), NoteKind::ExTap { .. }));
    let NoteKind::Air { parent, .. } = chart.notes()[1].kind() else {
        panic!("expected an air note");
    };
    assert_eq!(*parent, chart::NoteId::new(0));
    assert_eq!(chart.notes()[2].kind(), &NoteKind::Mine);
    assert_eq!(chart.scroll_speed_changes().len(), 2);
    assert_eq!(
        chart.scroll_speed_changes()[1].scope(),
        chart::ScrollScope::Group(3)
    );
}

#[test]
fn parses_air_hold_and_slide_with_explicit_parents() {
    let source = "RESOLUTION\t384\nHLD\t0\t0\t0\t4\t96\nAHD\t0\t96\t0\t4\tHLD\t96\tDEF\nSLD\t1\t0\t0\t4\t96\t4\t4\nASD\t1\t96\t4\t4\tSLD\t2.0\t96\t8\t4\t2.5\tDEF\n";
    let chart = parse(source).expect("valid AIR long notes");

    assert!(matches!(
        chart.notes()[1].kind(),
        NoteKind::AirHold { parent, .. } if *parent == chart::NoteId::new(0)
    ));
    assert!(matches!(
        chart.notes()[3].kind(),
        NoteKind::AirSlide {
            parent,
            points,
            ..
        } if *parent == chart::NoteId::new(2)
            && points[0].height() == 2.0
            && points[1].height() == 2.5
    ));
}

#[test]
fn defaults_legacy_air_colors_to_normal() {
    let source = "RESOLUTION\t384\nHLD\t0\t0\t0\t4\t192\nAIR\t0\t192\t0\t4\tHLD\nAHD\t1\t0\t0\t4\tHLD\t192\n";
    let chart = parse(source).expect("valid legacy AIR record");

    let NoteKind::Air { properties, .. } = chart.notes()[1].kind() else {
        panic!("expected AIR note");
    };
    assert_eq!(properties.color(), chart::AirColor::Normal);
    let NoteKind::AirHold { properties, .. } = chart.notes()[2].kind() else {
        panic!("expected AIR Hold");
    };
    assert_eq!(properties.color(), chart::AirColor::Normal);
}

#[test]
fn parses_air_slide_control_records() {
    let source = "RESOLUTION\t384\nSLD\t0\t0\t0\t4\t96\t4\t4\nASC\t0\t96\t4\t4\tSLD\t2.0\t96\t8\t4\t2.5\tDEF\nASC\t0\t192\t8\t4\tASC\t2.5\t96\t12\t4\t3.0\tDEF\n";
    let chart = parse(source).expect("valid C2S AIR Slide control records");

    assert_eq!(chart.notes().len(), 2);
    let NoteKind::AirSlide { points, .. } = chart.notes()[1].kind() else {
        panic!("expected AIR Slide");
    };
    assert_eq!(points.len(), 3);
}

#[test]
fn parses_air_long_notes_with_tap_parents_and_ahx() {
    let source = "RESOLUTION\t384\nTAP\t0\t0\t0\t4\nAHX\t0\t0\t0\t4\tTAP\t96\tDEF\nASD\t0\t96\t0\t4\tTAP\t2.0\t96\t4\t4\t2.5\tDEF\n";
    let chart = parse(source).expect("valid AIR long notes");

    assert!(matches!(
        chart.notes()[1].kind(),
        NoteKind::AirHold { parent, .. } if *parent == chart::NoteId::new(0)
    ));
    assert!(matches!(
        chart.notes()[2].kind(),
        NoteKind::AirSlide { parent, .. } if *parent == chart::NoteId::new(0)
    ));
}

#[test]
fn rejects_zero_resolution() {
    let error = parse("RESOLUTION\t0").expect_err("invalid resolution");
    assert!(matches!(error, super::C2sError::InvalidValue { .. }));
}

#[test]
fn writes_basic_and_extended_notes_and_round_trips_them() {
    let mut chart = Chart::new();
    chart.add_tempo_change(TempoChange::new(Position::new(0, 1).unwrap(), 120.0).unwrap());
    let position = Position::new(1, 1).unwrap();
    let end = Position::new(2, 1).unwrap();
    chart.add_note(
        Note::new(
            position,
            Lane::slider(0, 4).unwrap(),
            NoteKind::Tap(TapKind::Tap),
        )
        .unwrap(),
    );
    chart.add_note(
        Note::new(
            position,
            Lane::slider(2, 2).unwrap(),
            NoteKind::Air {
                properties: chart::AirProperties::new(chart::AirDirection::Up),
                parent: chart::NoteId::new(0),
            },
        )
        .unwrap(),
    );
    chart.add_note(
        Note::new(
            position,
            Lane::slider(4, 2).unwrap(),
            NoteKind::Tap(TapKind::Flick { direction: None }),
        )
        .unwrap(),
    );
    chart.add_note(
        Note::new(
            position,
            Lane::slider(8, 2).unwrap(),
            NoteKind::ExTap {
                direction: chart::ExDirection::Up,
            },
        )
        .unwrap(),
    );
    chart.add_note(Note::new(position, Lane::slider(10, 2).unwrap(), NoteKind::Mine).unwrap());
    chart.add_note(
        Note::new(
            position,
            Lane::slider(12, 2).unwrap(),
            NoteKind::Hold { end },
        )
        .unwrap(),
    );

    let c2s = write(&chart).expect("valid C2S output");
    let parsed = parse(&c2s).expect("round-tripped C2S output");
    assert_eq!(parsed.notes(), chart.notes());
    assert_eq!(parsed.tempo_changes(), chart.tempo_changes());
}

#[test]
fn writes_and_parses_note_speed_assignments() {
    let mut chart = Chart::new();
    let position = Position::new(1, 1).unwrap();
    let note_id = chart.add_note(
        Note::new(
            position,
            Lane::slider(0, 4).unwrap(),
            NoteKind::Tap(TapKind::Tap),
        )
        .expect("valid note"),
    );
    chart
        .set_note_speed_group(note_id, Some(7))
        .expect("valid note id");
    chart.add_scroll_speed_change(
        ScrollSpeedChange::with_duration(
            position,
            1.5,
            ScrollScope::Group(7),
            Position::new(1, 1).unwrap(),
        )
        .expect("valid speed change"),
    );

    let c2s = write(&chart).expect("valid C2S output");
    assert!(c2s.contains("SLA\t0\t96\t0\t4\t1\t7"));
    let parsed = parse(&c2s).expect("round-tripped C2S output");
    assert_eq!(parsed.note_speed_group(note_id).unwrap(), Some(7));
    assert_eq!(parsed.scroll_speed_changes(), chart.scroll_speed_changes());
}

#[test]
fn ignores_short_speed_assignments_for_long_notes() {
    let source = "RESOLUTION\t384\nTAP\t0\t0\t7\t2\nALD\t0\t96\t7\t2\t0\t1.0\t96\t7\t2\t1.0\tDEF\nSLA\t0\t96\t7\t2\t1\t1\n";
    let chart = parse(source).expect("unmatched SLA records are ignorable");

    assert_eq!(chart.notes().len(), 2);
    assert_eq!(chart.note_speed_group(chart::NoteId::new(1)).unwrap(), None);
}

#[test]
fn writes_and_parses_multi_segment_slides() {
    let mut chart = Chart::new();
    let points = vec![
        SlidePoint::new(Position::new(0, 1).unwrap(), Lane::slider(0, 4).unwrap()),
        SlidePoint::new(Position::new(1, 1).unwrap(), Lane::slider(4, 4).unwrap())
            .with_kind(SlidePointKind::Control),
        SlidePoint::new(Position::new(2, 1).unwrap(), Lane::slider(8, 4).unwrap()),
    ];
    chart.add_note(
        Note::new(
            points[0].position(),
            points[0].lane(),
            NoteKind::Slide { points },
        )
        .expect("valid slide"),
    );

    let c2s = write(&chart).expect("valid C2S output");
    assert!(c2s.contains("SLC\t0\t0\t0\t4\t96\t4\t4"));
    assert!(c2s.contains("SLD\t0\t96\t4\t4\t96\t8\t4"));
    let parsed = parse(&c2s).expect("round-tripped C2S output");
    assert_eq!(parsed.notes(), chart.notes());
}

#[test]
fn omits_slide_point_kinds_that_c2s_cannot_represent() {
    let points = vec![
        SlidePoint::new(Position::new(0, 1).unwrap(), Lane::slider(0, 4).unwrap()),
        SlidePoint::new(Position::new(1, 1).unwrap(), Lane::slider(4, 4).unwrap())
            .with_kind(SlidePointKind::Invisible),
        SlidePoint::new(Position::new(2, 1).unwrap(), Lane::slider(8, 4).unwrap()),
    ];
    let mut chart = Chart::new();
    chart.add_note(
        Note::new(
            points[0].position(),
            points[0].lane(),
            NoteKind::Slide { points },
        )
        .expect("valid slide"),
    );

    let c2s = write(&chart).expect("unsupported slide is omitted");
    assert_eq!(c2s, "RESOLUTION\t384\n");
}

#[test]
fn writes_and_parses_multi_segment_air_slides() {
    let mut chart = Chart::new();
    let start = Position::new(0, 1).unwrap();
    let middle = Position::new(1, 2).unwrap();
    let end = Position::new(1, 1).unwrap();
    let start_lane = Lane::slider(0, 4).unwrap();
    let parent = chart.add_note(Note::new(start, start_lane, NoteKind::Tap(TapKind::Tap)).unwrap());
    chart.add_note(
        Note::new(
            start,
            start_lane,
            NoteKind::AirSlide {
                points: vec![
                    chart::AirPoint::new(start, start_lane, 2.0).unwrap(),
                    chart::AirPoint::new(middle, Lane::slider(4, 4).unwrap(), 2.5).unwrap(),
                    chart::AirPoint::new(end, Lane::slider(8, 4).unwrap(), 3.0).unwrap(),
                ],
                color: chart::AirColor::Normal,
                parent,
            },
        )
        .unwrap(),
    );

    let c2s = write(&chart).expect("valid C2S output");
    assert!(c2s.contains("ASD\t0\t0\t0\t4\tTAP\t2.000000\t48\t4\t4\t2.500000\tDEF"));
    assert!(c2s.contains("ASD\t0\t48\t4\t4\tASC\t2.500000\t48\t8\t4\t3.000000\tDEF"));
    let parsed = parse(&c2s).expect("round-tripped C2S output");
    assert_eq!(parsed.notes(), chart.notes());
}

#[test]
fn writes_and_parses_air_crush_notes() {
    let mut chart = Chart::new();
    let start = Position::new(1, 1).unwrap();
    let end = Position::new(2, 1).unwrap();
    let lane = Lane::slider(0, 4).unwrap();
    chart.add_note(Note::new(start, lane, NoteKind::Tap(TapKind::Tap)).expect("valid parent"));
    chart.add_note(
        Note::new(
            start,
            lane,
            NoteKind::AirCrush {
                points: vec![
                    AirCrushPoint::new(start, lane, 5.0).unwrap(),
                    AirCrushPoint::new(end, Lane::slider(4, 4).unwrap(), 6.0).unwrap(),
                ],
                color: AirCrushColor::Purple,
                interval: AirCrushInterval::Every(Position::new(1, 4).unwrap()),
                parent: chart::NoteId::new(0),
            },
        )
        .expect("valid AIR Crush"),
    );

    let c2s = write(&chart).expect("valid C2S output");
    assert!(c2s.contains("ALD\t0\t96\t0\t4\t24\t5.000000\t96\t4\t4\t6.000000\tPPL"));
    let parsed = parse(&c2s).expect("round-tripped C2S output");
    assert_eq!(parsed.notes(), chart.notes());
}

#[test]
fn parses_air_crush_without_a_same_lane_parent() {
    let source = "RESOLUTION\t384\nTAP\t0\t0\t0\t4\nALD\t1\t0\t8\t4\t9600\t5\t96\t8\t4\t5\tDEF\n";
    let chart = parse(source).expect("valid C2S AIR Crush");

    assert!(matches!(
        chart.notes()[1].kind(),
        NoteKind::AirCrush { parent, .. } if *parent == chart::NoteId::new(0)
    ));
}

#[test]
fn writes_and_parses_ex_long_notes() {
    let mut chart = Chart::new();
    let start = Position::new(0, 1).unwrap();
    let end = Position::new(1, 1).unwrap();
    chart.add_note(
        Note::new(
            start,
            Lane::slider(0, 4).unwrap(),
            NoteKind::ExHold {
                end,
                direction: chart::ExDirection::Inward,
            },
        )
        .expect("valid Ex Hold"),
    );
    let points = vec![
        SlidePoint::new(end, Lane::slider(4, 4).unwrap()),
        SlidePoint::new(Position::new(2, 1).unwrap(), Lane::slider(8, 4).unwrap()),
    ];
    chart.add_note(
        Note::new(
            points[0].position(),
            points[0].lane(),
            NoteKind::ExSlide {
                points,
                direction: chart::ExDirection::Inward,
            },
        )
        .expect("valid Ex Slide"),
    );

    let c2s = write(&chart).expect("valid C2S output");
    assert!(c2s.contains("HXD\t0\t0\t0\t4\t96\tBS"));
    assert!(c2s.contains("SXD\t0\t96\t4\t4\t96\t8\t4\tBS"));
    let parsed = parse(&c2s).expect("round-tripped C2S output");
    assert_eq!(parsed.notes(), chart.notes());
}

#[test]
fn applies_xlair_mode_to_c2s_chr_notes() {
    let source = "RESOLUTION\t384\nCHR\t0\t0\t4\t2\tDW\n";
    let normal = super::parse_with_mode(source, chart::ChartMode::Normal).unwrap();
    assert!(matches!(normal.notes()[0].kind(), NoteKind::ExTap { .. }));
    let xlair = super::parse_with_mode(source, chart::ChartMode::Xlair).unwrap();
    assert_eq!(xlair.notes()[0].kind(), &NoteKind::Tap(TapKind::XTap));

    let output = super::write_with_mode(&xlair, chart::ChartMode::Xlair).unwrap();
    let reparsed = super::parse_with_mode(&output, chart::ChartMode::Xlair).unwrap();
    assert_eq!(reparsed.notes(), xlair.notes());
}

#[test]
fn applies_xlair_mode_to_c2s_extended_long_notes() {
    let source = concat!(
        "RESOLUTION\t384\n",
        "HXD\t0\t0\t0\t4\t96\tBS\n",
        "SXD\t1\t0\t4\t4\t96\t8\t4\tBS\n",
    );
    let normal = super::parse_with_mode(source, chart::ChartMode::Normal).unwrap();
    assert!(matches!(normal.notes()[0].kind(), NoteKind::ExHold { .. }));
    assert!(matches!(normal.notes()[1].kind(), NoteKind::ExSlide { .. }));

    let xlair = super::parse_with_mode(source, chart::ChartMode::Xlair).unwrap();
    assert!(matches!(xlair.notes()[0].kind(), NoteKind::Hold { .. }));
    assert!(matches!(xlair.notes()[1].kind(), NoteKind::Slide { .. }));

    let output = super::write_with_mode(&normal, chart::ChartMode::Xlair).unwrap();
    assert!(output.contains("HLD\t0\t0\t0\t4\t96"));
    assert!(output.contains("SLD\t1\t0\t4\t4\t96\t8\t4"));
    assert!(!output.contains("HXD"));
    assert!(!output.contains("SXD"));
    let reparsed = super::parse_with_mode(&output, chart::ChartMode::Xlair).unwrap();
    assert_eq!(reparsed.notes(), xlair.notes());
}

#[test]
fn omits_air_notes_in_xlair_mode() {
    let source = concat!(
        "RESOLUTION\t384\n",
        "TAP\t0\t0\t4\t2\n",
        "AIR\t0\t0\t4\t2\tTAP\tDEF\n",
    );
    let chart = super::parse_with_mode(source, chart::ChartMode::Xlair).unwrap();
    assert_eq!(chart.notes().len(), 1);
    assert!(matches!(
        chart.notes()[0].kind(),
        NoteKind::Tap(TapKind::Tap)
    ));
}
