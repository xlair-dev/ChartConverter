use chart::{
    Chart, Lane, Note, NoteId, NoteKind, Position, ScrollScope, ScrollSpeedChange, SideButton,
    SlidePoint, SlidePointKind, TapKind, TempoChange,
};

use super::{parse, parse_with_mode, write, write_with_mode};

#[test]
fn parses_short_notes_and_variable_measure_lengths() {
    let chart = parse("#00002: 3.5\n#00110: 12\n#00114: 0031\n").expect("valid SUS");

    assert_eq!(chart.notes().len(), 2);
    assert_eq!(chart.notes()[0].position(), Position::new(7, 2).unwrap());
    assert_eq!(chart.notes()[1].position(), Position::new(21, 4).unwrap());
    assert_eq!(chart.notes()[0].lane(), Lane::slider(0, 2).unwrap());
    assert_eq!(
        chart.notes()[1].kind(),
        &NoteKind::Tap(TapKind::Flick { direction: None })
    );
}

#[test]
fn preserves_variable_measure_lengths_when_writing_standard_sus() {
    let source = "#00102: 3\n#00210: 14";
    let chart = parse(source).expect("valid SUS");
    let written = write(&chart).expect("variable measure length is representable");
    let reparsed = parse(&written).expect("written SUS is valid");

    assert!(written.contains("#00102: 3"));
    assert_eq!(reparsed.measure_lengths(), chart.measure_lengths());
    assert_eq!(reparsed.notes(), chart.notes());
}

#[test]
fn preserves_variable_measure_lengths_when_writing_xlair_sus() {
    let chart = parse("#00102: 3\n#00210: 14").expect("valid SUS");
    let written = write_with_mode(&chart, chart::ChartMode::Xlair).expect("valid XLAIR SUS");
    let reparsed = parse_with_mode(&written, chart::ChartMode::Xlair).expect("valid XLAIR SUS");

    assert!(written.contains("#00102: 3"));
    assert_eq!(reparsed.measure_lengths(), chart.measure_lengths());
    assert_eq!(reparsed.notes(), chart.notes());
}

#[test]
fn increases_xlair_resolution_for_fine_note_positions() {
    let position = Position::new(479, 120).expect("valid position");
    let mut chart = Chart::new();
    chart.add_note(
        Note::new(
            position,
            Lane::slider(0, 4).expect("valid lane"),
            NoteKind::Tap(TapKind::Tap),
        )
        .expect("valid note"),
    );

    let written = write_with_mode(&chart, chart::ChartMode::Xlair).expect("valid XLAIR SUS");
    let reparsed = parse_with_mode(&written, chart::ChartMode::Xlair).expect("valid XLAIR SUS");

    assert!(written.starts_with("#REQUEST \"ticks_per_beat 1920\""));
    assert_eq!(reparsed.notes(), chart.notes());
}

#[test]
fn preserves_note_attributes_when_writing_xlair_sus() {
    let source = "#ATR01: \"rh: 1.5, h: 2.0, pr: 100\"\n#ATTRIBUTE 01\n#00010: 14";
    let chart = parse(source).expect("valid SUS attributes");
    let written = write_with_mode(&chart, chart::ChartMode::Xlair).expect("valid XLAIR SUS");
    let reparsed = parse_with_mode(&written, chart::ChartMode::Xlair).expect("valid XLAIR SUS");

    assert_eq!(reparsed.notes(), chart.notes());
}

#[test]
fn parses_bpm_changes_and_side_longs() {
    let source = "#BPM01: 120\n#00008: 01\n#00120A: 14\n#00220A: 24\n";
    let chart = super::parse_with_mode(source, chart::ChartMode::Xlair).expect("valid XLAIR SUS");

    assert_eq!(chart.tempo_changes().len(), 1);
    assert_eq!(chart.notes().len(), 1);
    assert_eq!(chart.notes()[0].lane(), Lane::Side(SideButton::LeftUpper));
    assert!(matches!(chart.notes()[0].kind(), NoteKind::Hold { .. }));
}

#[test]
fn does_not_parse_long_bpm_change_data_as_a_note() {
    let source = "#BPM01: 120\n#00008: 010000000000\n";
    let chart =
        super::parse_with_mode(source, chart::ChartMode::Xlair).expect("valid XLAIR BPM change");

    assert_eq!(chart.tempo_changes().len(), 1);
    assert!(chart.notes().is_empty());
}

#[test]
fn parses_xlair_side_taps_and_relayed_holds() {
    let source = "#00150: 34\n#0015c: 64\n#00120A: 14\n#00220A: 34\n#00320A: 24";
    let chart = super::parse_with_mode(source, chart::ChartMode::Xlair).expect("valid XLAIR SUS");

    assert_eq!(chart.notes().len(), 3);
    assert_eq!(chart.notes()[0].lane(), Lane::Side(SideButton::LeftUpper));
    assert_eq!(chart.notes()[1].lane(), Lane::Side(SideButton::RightLower));
    assert!(matches!(chart.notes()[2].kind(), NoteKind::Hold { .. }));
}

#[test]
fn maps_each_xlair_directional_code_to_its_side_button() {
    for (direction, button) in [
        ('3', SideButton::LeftUpper),
        ('4', SideButton::RightUpper),
        ('5', SideButton::LeftLower),
        ('6', SideButton::RightLower),
    ] {
        let source = format!("#00150: {direction}1");
        let chart = super::parse_with_mode(&source, chart::ChartMode::Xlair)
            .expect("valid XLAIR directional note");

        assert_eq!(chart.notes().len(), 1);
        assert_eq!(chart.notes()[0].lane(), Lane::Side(button));
        assert_eq!(chart.notes()[0].kind(), &NoteKind::Tap(TapKind::Tap));
    }
}

#[test]
fn maps_xlair_side_holds_to_the_buttons_defined_by_their_start_lanes() {
    for (lane, button) in [
        ('0', SideButton::LeftUpper),
        ('1', SideButton::LeftUpper),
        ('2', SideButton::LeftLower),
        ('3', SideButton::LeftLower),
        ('c', SideButton::RightUpper),
        ('d', SideButton::RightUpper),
        ('e', SideButton::RightLower),
        ('f', SideButton::RightLower),
    ] {
        let source = format!("#0012{lane}A: 14\n#0022{lane}A: 24");
        let chart = super::parse_with_mode(&source, chart::ChartMode::Xlair)
            .expect("valid XLAIR side hold");

        assert_eq!(chart.notes().len(), 1);
        assert_eq!(chart.notes()[0].lane(), Lane::Side(button));
        assert!(matches!(chart.notes()[0].kind(), NoteKind::Hold { .. }));
    }
}

#[test]
fn xlair_side_taps_replace_overlapping_central_taps_in_either_record_order() {
    for source in [
        "#00110: 11\n#00150: 31",
        "#00150: 31\n#00110: 11",
        "#00110: 21\n#00150: 31",
        "#00110: 31\n#00150: 31",
    ] {
        let chart =
            super::parse_with_mode(source, chart::ChartMode::Xlair).expect("valid XLAIR SUS");
        assert_eq!(chart.notes().len(), 1);
        assert_eq!(chart.notes()[0].lane(), Lane::Side(SideButton::LeftUpper));
        assert_eq!(chart.notes()[0].kind(), &NoteKind::Tap(TapKind::Tap));
    }
}

#[test]
fn xlair_side_tap_keeps_the_speed_group_of_its_own_record() {
    let source = concat!(
        "#TIL00: \"0'0:0.50\"\n",
        "#HISPEED 00\n",
        "#00110: 11\n",
        "#NOSPEED\n",
        "#00150: 31",
    );
    let chart = super::parse_with_mode(source, chart::ChartMode::Xlair).unwrap();

    assert_eq!(chart.notes().len(), 1);
    assert_eq!(chart.note_speed_group(chart::NoteId::new(0)).unwrap(), None);
}

#[test]
fn keeps_a_slider_slide_that_overlaps_an_xlair_side_hold() {
    let source = concat!(
        "#00120A: 14\n",
        "#00220A: 24\n",
        "#00130B: 14\n",
        "#00230B: 24",
    );
    let chart = super::parse_with_mode(source, chart::ChartMode::Xlair).unwrap();

    assert_eq!(chart.notes().len(), 2);
    assert_eq!(chart.notes()[0].lane(), Lane::Side(SideButton::LeftUpper));
    assert!(matches!(chart.notes()[0].kind(), NoteKind::Hold { .. }));
    assert_eq!(chart.notes()[1].lane(), Lane::slider(0, 4).unwrap());
    assert!(matches!(chart.notes()[1].kind(), NoteKind::Slide { .. }));
}

#[test]
fn xlair_side_taps_keep_non_overlapping_central_taps() {
    let chart = super::parse_with_mode(
        "#00110: 11\n#00112: 11\n#00150: 31",
        chart::ChartMode::Xlair,
    )
    .expect("valid XLAIR SUS");

    assert_eq!(chart.notes().len(), 2);
    assert_eq!(chart.notes()[0].lane(), Lane::Side(SideButton::LeftUpper));
    assert_eq!(chart.notes()[1].lane(), Lane::slider(2, 1).unwrap());
}

#[test]
fn ignores_standard_metadata() {
    let source = r#"
#TITLE "title"
#SUBTITLE "subtitle"
#ARTIST "artist"
#GENRE "genre"
#DESIGNER "designer"
#DIFFICULTY 3
#PLAYLEVEL 12
#SONGID "song"
#WAVE "song.wav"
#WAVEOFFSET 0
#JACKET "jacket.jpg"
#BACKGROUND "background.jpg"
#MOVIE "movie.mp4"
#MOVIEOFFSET 0
#BASEBPM 154
#00110: 14
"#;

    let chart = parse(source).expect("standard SUS metadata is not chart data");
    assert_eq!(chart.notes().len(), 1);
    assert_eq!(chart.base_bpm(), Some(154.0));
    assert!(write(&chart).unwrap().contains("#BASEBPM 154"));
}

#[test]
fn rejects_unclosed_slider_channels() {
    let error = parse("#00130A: 14").expect_err("missing end");
    assert!(matches!(
        error,
        super::SusError::MissingEnd { channel: 'A' }
    ));
}

#[test]
fn parses_central_slider_points_across_data_lines() {
    let chart = parse("#00130A: 144g\n#0023cA: 24").expect("valid SUS");

    assert_eq!(chart.notes().len(), 1);
    let NoteKind::Slide { points } = chart.notes()[0].kind() else {
        panic!("expected a slide");
    };
    assert_eq!(points.len(), 3);
    assert_eq!(points[0].lane(), Lane::slider(0, 4).unwrap());
    assert_eq!(points[1].lane(), Lane::slider(0, 16).unwrap());
    assert_eq!(points[2].lane(), Lane::slider(12, 4).unwrap());
    assert_eq!(points[1].kind(), &SlidePointKind::Control);
}

#[test]
fn parses_slide_two_channels() {
    let chart = parse("#00140A: 144g\n#0024cA: 24").expect("valid SUS");

    assert_eq!(chart.notes().len(), 1);
    assert!(matches!(chart.notes()[0].kind(), NoteKind::Slide { points } if points.len() == 3));
}

#[test]
fn parses_hispeed_definitions_and_applies_the_group_to_notes() {
    let source = "#TIL00: \"0'0:0.50, 1'0:2.00\"\n#HISPEED 00\n#00110: 14";
    let chart = parse(source).expect("valid speed definition");

    assert_eq!(chart.scroll_speed_changes().len(), 2);
    assert_eq!(
        chart.note_speed_group(chart::NoteId::new(0)).unwrap(),
        Some(0)
    );
}

#[test]
fn ignores_measure_line_speed_commands() {
    let chart = parse("#MEASUREHS 00\n#REQUEST \"enable_priority true\"\n#00110: 14")
        .expect("valid SUS display commands");
    assert_eq!(chart.notes().len(), 1);
    assert_eq!(chart.priority_enabled(), Some(true));
    assert!(write(&chart).unwrap().contains("enable_priority true"));
}

#[test]
fn preserves_sus_note_attributes() {
    let source = concat!(
        "#ATR01: \"rh: 1.5, h: 2.0, pr: 100\"\n",
        "#ATTRIBUTE 01\n",
        "#00010: 14\n",
        "#NOATTRIBUTE\n",
        "#00110: 14\n",
    );
    let chart = parse(source).expect("valid SUS attributes");

    assert_eq!(chart.notes()[0].attributes().roll_speed(), Some(1.5));
    assert_eq!(chart.notes()[0].attributes().height(), Some(2.0));
    assert_eq!(chart.notes()[0].attributes().priority(), Some(100));
    assert!(chart.notes()[1].attributes().is_empty());

    let written = write(&chart).expect("attributes are representable in SUS");
    let reparsed = parse(&written).expect("written SUS attributes");
    assert_eq!(reparsed.notes(), chart.notes());
}

#[test]
fn applies_measure_base_to_note_positions() {
    let chart = parse("#MEASUREBS 10\n#00010: 14").expect("valid SUS");

    assert_eq!(chart.notes()[0].position(), Position::new(40, 1).unwrap());
}

#[test]
fn writes_basic_notes_and_timing() {
    let position = Position::new(1, 1).unwrap();
    let mut chart = Chart::new();
    chart.add_tempo_change(TempoChange::new(position, 120.0).unwrap());
    chart.add_note(
        Note::new(
            position,
            Lane::slider(10, 6).unwrap(),
            NoteKind::Tap(TapKind::XTap),
        )
        .unwrap(),
    );

    let sus = write(&chart).expect("valid output");
    assert!(sus.contains("#BPM01: 120"));
    assert!(sus.contains("#00001: 02140C"));
    let parsed = parse(&sus).unwrap();
    assert_eq!(
        parsed.notes()[0].kind(),
        &NoteKind::ExTap {
            direction: chart::ExDirection::Up
        }
    );
}

#[test]
fn parses_standard_sus_note_codes() {
    let source = concat!(
        "#REQUEST \"ticks_per_beat 480\"\n",
        "#00000: 010008\n",
        "#000A0: 020008\n",
        "#00140: 05000801E0\n",
        "#00280: 06000801E00008\n",
        "#003C0: 100008\n",
    );
    let chart = parse(source).expect("valid standard SUS");
    assert_eq!(chart.notes().len(), 5);
    assert!(matches!(chart.notes()[1].kind(), NoteKind::ExTap { .. }));
    assert!(matches!(chart.notes()[2].kind(), NoteKind::Hold { .. }));
    assert!(matches!(chart.notes()[3].kind(), NoteKind::Slide { .. }));
    assert_eq!(chart.notes()[4].kind(), &NoteKind::Mine);
}

#[test]
fn omits_damage_notes_in_xlair_mode() {
    let chart = super::parse_with_mode("#00000: 100008", chart::ChartMode::Xlair)
        .expect("valid XLAIR SUS with an unsupported damage note");

    assert!(chart.notes().is_empty());
}

#[test]
fn omits_unsupported_tap_variants_in_xlair_mode() {
    let source = "#00010: 41 51 61";
    let xlair = super::parse_with_mode(source, chart::ChartMode::Xlair).unwrap();
    assert!(xlair.notes().is_empty());

    let normal = super::parse_with_mode(source, chart::ChartMode::Normal).unwrap();
    assert_eq!(normal.notes().len(), 3);
    assert_eq!(normal.notes()[0].kind(), &NoteKind::Tap(TapKind::Tap4));
    assert_eq!(normal.notes()[1].kind(), &NoteKind::Tap(TapKind::Tap5));
    assert_eq!(normal.notes()[2].kind(), &NoteKind::Tap(TapKind::Tap6));
}

#[test]
fn preserves_generic_tap_two_as_xtap() {
    let chart = parse("#00010: 22").expect("valid generic SUS");

    assert_eq!(chart.notes()[0].kind(), &NoteKind::Tap(TapKind::XTap));
}

#[test]
fn rejects_unknown_generic_sus_data_instead_of_dropping_it() {
    assert!(matches!(
        parse("#00060: 14"),
        Err(super::SusError::UnsupportedCommand { .. })
    ));
}

#[test]
fn resolves_standard_sus_air_to_its_parent() {
    let source = concat!(
        "#REQUEST \"ticks_per_beat 480\"\n",
        "#00000: 010008\n",
        "#00000: 070008\n",
        "#00000: 08000801E0\n",
    );
    let chart = parse(source).expect("valid standard SUS AIR");
    assert_eq!(chart.notes().len(), 3);
    assert!(matches!(
        chart.notes()[1].kind(),
        NoteKind::Air { parent, .. } if *parent == chart::NoteId::new(0)
    ));
    assert!(matches!(
        chart.notes()[2].kind(),
        NoteKind::AirHold { parent, .. } if *parent == chart::NoteId::new(0)
    ));
}

#[test]
fn preserves_speed_groups_on_standard_air_notes() {
    let source = concat!(
        "#TIL00: \"0'0:1.0\"\n",
        "#HISPEED 00\n",
        "#00000: 010008\n",
        "#00000: 070008\n",
    );
    let chart = parse(source).expect("valid standard SUS AIR");

    assert_eq!(
        chart.note_speed_group(chart::NoteId::new(1)).unwrap(),
        Some(0)
    );
}

#[test]
fn writes_standard_sus_air_without_losing_the_parent_relation() {
    let position = Position::new(1, 1).unwrap();
    let lane = Lane::slider(4, 4).unwrap();
    let mut chart = Chart::new();
    let parent = chart
        .add_note(Note::new(position, lane, NoteKind::Tap(TapKind::Tap)).expect("valid parent"));
    chart.add_note(
        Note::new(
            position,
            lane,
            NoteKind::Air {
                properties: chart::AirProperties::new(chart::AirDirection::Up),
                parent,
            },
        )
        .expect("valid AIR"),
    );
    let source = write(&chart).expect("valid standard SUS output");
    let parsed = parse(&source).expect("valid standard SUS round trip");
    assert!(matches!(
        parsed.notes()[1].kind(),
        NoteKind::Air { parent, .. } if *parent == chart::NoteId::new(0)
    ));
}

#[test]
fn preserves_standard_sus_scroll_speed_groups() {
    let position = Position::new(1, 1).unwrap();
    let mut chart = Chart::new();
    chart.add_scroll_speed_change(
        ScrollSpeedChange::with_scope(position, 1.5, ScrollScope::Group(2))
            .expect("valid speed change"),
    );
    let note_id = chart.add_note(
        Note::new(
            position,
            Lane::slider(0, 4).unwrap(),
            NoteKind::Tap(TapKind::Tap),
        )
        .expect("valid note"),
    );
    chart
        .set_note_speed_group(note_id, Some(2))
        .expect("valid speed group");
    let source = write(&chart).expect("valid standard SUS output");
    let parsed = parse(&source).expect("valid standard SUS round trip");
    assert_eq!(parsed.scroll_speed_changes(), chart.scroll_speed_changes());
    assert_eq!(parsed.note_speed_group(NoteId::new(0)).unwrap(), Some(2));
}

#[test]
fn preserves_generic_sus_tap_variants_and_directions() {
    let mut chart = Chart::new();
    for (index, kind) in [TapKind::Tap4, TapKind::Tap5, TapKind::Tap6]
        .into_iter()
        .enumerate()
    {
        chart.add_note(
            Note::new(
                Position::new(index as u64, 1).unwrap(),
                Lane::slider(index as u8, 2).unwrap(),
                NoteKind::Tap(kind),
            )
            .unwrap(),
        );
    }
    chart.add_note(
        Note::new(
            Position::new(3, 1).unwrap(),
            Lane::slider(6, 2).unwrap(),
            NoteKind::ExTap {
                direction: chart::ExDirection::LowerRight,
            },
        )
        .unwrap(),
    );
    let source = write(&chart).expect("valid generic SUS output");
    let parsed = parse(&source).expect("valid generic SUS round trip");
    assert_eq!(parsed.notes(), chart.notes());
}

#[test]
fn writes_slide_point_kinds() {
    let start = Position::new(0, 1).unwrap();
    let middle = Position::new(1, 4).unwrap();
    let invisible = Position::new(1, 2).unwrap();
    let end = Position::new(1, 1).unwrap();
    let mut chart = Chart::new();
    chart.add_note(
        Note::new(
            start,
            Lane::slider(0, 4).unwrap(),
            NoteKind::Slide {
                points: vec![
                    SlidePoint::new(start, Lane::slider(0, 4).unwrap()),
                    SlidePoint::new(middle, Lane::slider(4, 4).unwrap())
                        .with_kind(SlidePointKind::Control),
                    SlidePoint::new(invisible, Lane::slider(8, 4).unwrap())
                        .with_kind(SlidePointKind::Invisible),
                    SlidePoint::new(end, Lane::slider(12, 4).unwrap()),
                ],
            },
        )
        .unwrap(),
    );

    let sus = super::write_with_mode(&chart, chart::ChartMode::Xlair).expect("valid SUS output");
    assert_eq!(parse(&sus).unwrap().notes(), chart.notes());
}

#[test]
fn writes_speed_definitions_and_note_speed_groups() {
    let position = Position::new(1, 1).unwrap();
    let mut chart = Chart::new();
    chart.add_scroll_speed_change(
        ScrollSpeedChange::with_scope(position, 1.5, ScrollScope::Group(35))
            .expect("valid speed change"),
    );
    let note_id = chart.add_note(
        Note::new(
            position,
            Lane::slider(0, 4).unwrap(),
            NoteKind::Tap(TapKind::Tap),
        )
        .expect("valid note"),
    );
    chart
        .set_note_speed_group(note_id, Some(35))
        .expect("valid note id");

    let sus = super::write_with_mode(&chart, chart::ChartMode::Xlair).expect("valid speed output");
    assert!(sus.contains("#TIL0z: \"0'96:1.5\""));
    assert!(sus.contains("#HISPEED 0z"));
    let parsed =
        super::parse_with_mode(&sus, chart::ChartMode::Xlair).expect("round-tripped speed output");
    assert_eq!(parsed.scroll_speed_changes(), chart.scroll_speed_changes());
    assert_eq!(parsed.note_speed_group(note_id).unwrap(), Some(35));
}

#[test]
fn omits_speed_changes_that_sus_cannot_represent() {
    let mut chart = Chart::new();
    chart.add_scroll_speed_change(
        ScrollSpeedChange::with_duration(
            Position::new(0, 1).unwrap(),
            1.0,
            ScrollScope::Group(0),
            Position::new(1, 4).unwrap(),
        )
        .expect("valid speed duration"),
    );

    let sus = write(&chart).expect("unsupported speed is omitted");
    assert_eq!(sus, "#REQUEST \"ticks_per_beat 4\"\n");
}

#[test]
fn writes_xlair_side_notes() {
    let mut chart = Chart::new();
    chart.add_note(
        Note::new(
            Position::new(0, 1).unwrap(),
            Lane::Side(SideButton::LeftUpper),
            NoteKind::Tap(TapKind::Tap),
        )
        .unwrap(),
    );
    chart.add_note(
        Note::new(
            Position::new(1, 1).unwrap(),
            Lane::Side(SideButton::RightLower),
            NoteKind::Hold {
                end: Position::new(2, 1).unwrap(),
            },
        )
        .unwrap(),
    );

    let sus = super::write_with_mode(&chart, chart::ChartMode::Xlair).expect("valid XLAIR output");
    let parsed =
        super::parse_with_mode(&sus, chart::ChartMode::Xlair).expect("round-tripped XLAIR output");
    assert_eq!(parsed.notes().len(), 2);
    assert!(
        parsed
            .notes()
            .iter()
            .any(|note| note.lane() == Lane::Side(SideButton::LeftUpper))
    );
    assert!(
        parsed
            .notes()
            .iter()
            .any(|note| note.lane() == Lane::Side(SideButton::RightLower))
    );
}

#[test]
fn intentionally_drops_air_notes_in_xlair_output() {
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
    chart.add_note(
        Note::new(
            position,
            Lane::slider(0, 4).unwrap(),
            NoteKind::Air {
                properties: chart::AirProperties::new(chart::AirDirection::Up),
                parent,
            },
        )
        .unwrap(),
    );

    let sus = super::write_with_mode(&chart, chart::ChartMode::Xlair)
        .expect("AIR is intentionally omitted in XLAIR output");
    let parsed = super::parse_with_mode(&sus, chart::ChartMode::Xlair).expect("valid XLAIR output");
    assert_eq!(parsed.notes().len(), 1);
    assert_eq!(parsed.notes()[0].kind(), &NoteKind::Tap(TapKind::Tap));
}

#[test]
fn preserves_slide_points_that_share_a_position() {
    let start = Position::new(0, 1).unwrap();
    let junction = Position::new(1, 1).unwrap();
    let mut chart = Chart::new();
    chart.add_note(
        Note::new(
            start,
            Lane::slider(0, 4).unwrap(),
            NoteKind::Slide {
                points: vec![
                    SlidePoint::new(start, Lane::slider(0, 4).unwrap()),
                    SlidePoint::new(junction, Lane::slider(4, 4).unwrap())
                        .with_kind(SlidePointKind::Control),
                    SlidePoint::new(junction, Lane::slider(8, 4).unwrap()),
                ],
            },
        )
        .unwrap(),
    );

    let sus = write_with_mode(&chart, chart::ChartMode::Xlair).expect("valid XLAIR SUS");
    let reparsed = parse_with_mode(&sus, chart::ChartMode::Xlair).expect("valid XLAIR SUS");

    assert_eq!(reparsed.notes(), chart.notes());
}

#[test]
fn reuses_xlair_channels_for_non_overlapping_long_notes() {
    let mut chart = Chart::new();
    for index in 0..40 {
        let start = Position::new(index * 2, 1).unwrap();
        let end = Position::new(index * 2 + 1, 1).unwrap();
        chart.add_note(
            Note::new(
                start,
                Lane::slider(0, 4).unwrap(),
                NoteKind::Slide {
                    points: vec![
                        SlidePoint::new(start, Lane::slider(0, 4).unwrap()),
                        SlidePoint::new(end, Lane::slider(0, 4).unwrap()),
                    ],
                },
            )
            .unwrap(),
        );
    }

    let sus = write_with_mode(&chart, chart::ChartMode::Xlair).expect("valid XLAIR SUS");
    let reparsed = parse_with_mode(&sus, chart::ChartMode::Xlair).expect("valid XLAIR SUS");

    assert_eq!(reparsed.notes().len(), chart.notes().len());
    assert!(
        reparsed
            .notes()
            .iter()
            .zip(chart.notes())
            .all(|(reparsed, original)| reparsed.position() == original.position())
    );
}
