use std::ffi::CStr;

use super::*;

#[test]
fn keeps_side_xtap_parent_contract_with_every_sus_directional_mode() {
    for mode in [
        "",
        "#REQUEST \"xlair_directional_mode consume_tap\"\n",
        "#REQUEST \"xlair_directional_mode independent\"\n",
    ] {
        for records in ["#00010:24\n#00050:34\n", "#00050:34\n#00010:24\n"] {
            let source = format!("{mode}{records}");
            let parsed = Parsed::new(CcFormat::Sus, source.as_bytes());
            let view = parsed.view();
            assert_eq!(view.note_count, 2);
            let notes = unsafe { slice::from_raw_parts(view.notes, view.note_count) };
            assert!(matches!(notes[0].kind, CcNoteKind::XTap));
            assert!(matches!(notes[1].kind, CcNoteKind::Air));
            assert_eq!(notes[1].parent, notes[0].id);
            assert!(matches!(notes[1].air_direction, CcAirDirection::UpperLeft));
            assert_eq!(unsafe { cc_result_diagnostics(parsed.0) }.count, 0);
        }
    }
}

#[test]
fn exposes_opt_in_directional_modes_and_their_errors() {
    for (mode, count) in [("consume_tap", 1), ("independent", 2)] {
        let source = format!("#REQUEST \"xlair_directional_mode {mode}\"\n#00010:14\n#00050:34\n");
        let parsed = Parsed::new(CcFormat::Sus, source.as_bytes());
        assert_eq!(parsed.view().note_count, count);
    }
    let parsed = Parsed::new(
        CcFormat::Sus,
        b"#00010:14\n#REQUEST \"xlair_directional_mode invalid\"\n",
    );
    assert!(!unsafe { cc_result_error(parsed.0) }.is_null());
    let diagnostics = unsafe { cc_result_diagnostics(parsed.0) };
    assert_eq!(diagnostics.count, 1);
    assert_eq!(unsafe { (*diagnostics.diagnostics).line }, 2);
}

#[test]
fn exposes_sus_zero_timeline_as_a_distinct_group_in_notes_and_points() {
    let source = br#"#TIL00: "0'0:0.5"
#TIL01: "0'0:2.0"
#HISPEED 00
#MEASUREHS 00
#00030A:14
#HISPEED 01
#00134A:34
#NOSPEED
#00238A:24
"#;
    for mode in [CcMode::Normal, CcMode::Xlair] {
        let parsed = Parsed::with_mode(CcFormat::Sus, mode, source);
        let view = parsed.view();
        assert_eq!(view.note_count, 1);
        let note = unsafe { &*view.notes };
        assert_eq!(note.speed_group, 1);
        let details = unsafe { &*cc_result_note_details(parsed.0, note.id) };
        assert_eq!(details.point_count, note.point_count);
        let points = unsafe { slice::from_raw_parts(details.points, details.point_count) };
        assert_eq!(
            points
                .iter()
                .map(|point| point.speed_group)
                .collect::<Vec<_>>(),
            vec![1, 0, -1]
        );
        let scrolls = unsafe { slice::from_raw_parts(view.scrolls, view.scroll_count) };
        assert_eq!(scrolls.len(), 2);
        assert!(
            scrolls
                .iter()
                .all(|scroll| matches!(scroll.scope, CcScrollScope::Group))
        );
        assert_eq!((scrolls[0].target, scrolls[0].speed), (1, 0.5));
        assert_eq!((scrolls[1].target, scrolls[1].speed), (0, 2.0));
        assert_eq!(unsafe { cc_result_diagnostics(parsed.0) }.count, 0);
    }
}

#[test]
fn ex_long_replacement_keeps_start_and_point_groups() {
    let parsed = Parsed::with_mode(CcFormat::Ugc, CcMode::Normal,
        b"@TIL\t1\t0'0\t1.5\n@TIL\t2\t0'0\t2\n@ENDHEAD\n@USETIL\t1\n#0'0:x04L\n@USETIL\t2\n#0'0:s04\n@USETIL\t0\n#480>s44\n");
    let view = parsed.view();
    assert_eq!(view.note_count, 1);
    let note = unsafe { &*view.notes };
    assert!(matches!(note.kind, CcNoteKind::ExSlide));
    assert_eq!(note.speed_group, 2);
    let details = unsafe { &*cc_result_note_details(parsed.0, note.id) };
    assert!(matches!(details.ex_direction, CcExDirection::Left));
    let points = unsafe { slice::from_raw_parts(details.points, details.point_count) };
    assert_eq!(points[0].speed_group, 2);
    assert_eq!(points[1].speed_group, 0);
}

#[test]
fn exposes_omission_diagnostics_from_each_frontend() {
    for (format, source, expected_format) in [
        (CcFormat::Sus, b"#00010:44\n".as_slice(), "SUS"),
        (
            CcFormat::C2s,
            b"RESOLUTION\t384\nAIR\t0\t0\t0\t4\tTAP\tDEF\n".as_slice(),
            "C2S",
        ),
        (CcFormat::Ugc, b"@ENDHEAD\n#0'0:T04\n".as_slice(), "UGC"),
    ] {
        let parsed = Parsed::new(format, source);
        parsed.view();
        let diagnostics = unsafe { cc_result_diagnostics(parsed.0) };
        assert_eq!(diagnostics.count, 1);
        assert_eq!(
            unsafe { CStr::from_ptr((*diagnostics.diagnostics).format) }
                .to_str()
                .unwrap(),
            expected_format
        );
    }
}

#[test]
fn escapes_embedded_nuls_in_error_and_diagnostic_strings() {
    let parsed = Parsed::new(CcFormat::Sus, b"#BASEBPM 1\0\n");
    let error = unsafe { CStr::from_ptr(cc_result_error(parsed.0)) }
        .to_str()
        .unwrap();
    assert!(error.contains("\\0"));
    let diagnostics = unsafe { cc_result_diagnostics(parsed.0) };
    assert_eq!(diagnostics.count, 1);
    let message = unsafe { CStr::from_ptr((*diagnostics.diagnostics).message) }
        .to_str()
        .unwrap();
    assert!(message.contains("\\0"));
}

#[test]
fn exposes_metadata_and_distinguishes_absence_from_zero_and_false() {
    let parsed = {
        let source = String::from(
            r#"#WAVEOFFSET 0
#BASEBPM 150
#REQUEST "enable_priority false"
#00002:3
#00102:5
#ATR01:"rh:0,h:0,pr:0"
#ATTRIBUTE 01
#00010:14
"#,
        );
        Parsed::new(CcFormat::Sus, source.as_bytes())
    };
    let metadata = unsafe { cc_result_metadata(parsed.0) };
    assert!(metadata.audio_offset_seconds.has_value);
    assert_eq!(metadata.audio_offset_seconds.value, 0.0);
    assert!(metadata.base_bpm.has_value);
    assert_eq!(metadata.base_bpm.value, 150.0);
    assert!(metadata.priority_enabled.has_value);
    assert!(!metadata.priority_enabled.value);
    let measures =
        unsafe { slice::from_raw_parts(metadata.measure_lengths, metadata.measure_length_count) };
    assert_eq!(measures.len(), 2);
    assert_eq!(
        (
            measures[0].measure,
            measures[0].length.numerator,
            measures[0].length.denominator
        ),
        (0, 3, 1)
    );
    assert_eq!(measures[1].length.numerator, 5);
    let details = unsafe { &*cc_result_note_details(parsed.0, 0) };
    assert!(details.attributes.roll_speed.has_value);
    assert_eq!(details.attributes.roll_speed.value, 0.0);
    assert!(details.attributes.height.has_value);
    assert_eq!(details.attributes.height.value, 0.0);
    assert!(details.attributes.priority.has_value);
    assert_eq!(details.attributes.priority.value, 0);
    assert_eq!(details.point_count, 0);
    assert!(unsafe { cc_result_note_details(parsed.0, u32::MAX) }.is_null());
    let absent = Parsed::new(CcFormat::Sus, b"#00010:14\n");
    let metadata = unsafe { cc_result_metadata(absent.0) };
    assert!(!metadata.audio_offset_seconds.has_value);
    assert!(!metadata.priority_enabled.has_value);
    assert!(!metadata.base_bpm.has_value);
    assert!(
        !unsafe { &*cc_result_note_details(absent.0, 0) }
            .attributes
            .priority
            .has_value
    );
}

#[test]
fn exports_resolved_point_context_without_changing_legacy_paths() {
    let parsed = Parsed::new(
        CcFormat::Sus,
        br#"#TIL01:"0'0:1"
#TIL02:"0'0:2"
#HISPEED 01
#00030A:14
#HISPEED 02
#ATR01:"pr:12"
#ATTRIBUTE 01
#00134A:34
#NOSPEED
#NOATTRIBUTE
#00238A:24
"#,
    );
    let view = parsed.view();
    let note = unsafe { &*view.notes };
    let details = unsafe { &*cc_result_note_details(parsed.0, note.id) };
    assert_eq!(note.speed_group, 0);
    assert_eq!(details.id, note.id);
    assert_eq!(details.point_count, note.point_count);
    let points = unsafe { slice::from_raw_parts(details.points, details.point_count) };
    assert_eq!(
        points.iter().map(|p| p.speed_group).collect::<Vec<_>>(),
        vec![0, 1, -1]
    );
    assert!(points[1].attributes.priority.has_value);
    assert_eq!(points[1].attributes.priority.value, 12);
    assert!(!points[2].attributes.priority.has_value);
    assert!(matches!(
        unsafe { &*note.points.add(1) }.kind,
        CcPointKind::Visible
    ));
}

#[test]
fn exposes_ugc_zero_group_and_base_timeline() {
    let parsed = Parsed::new(
        CcFormat::Ugc,
        b"@MAINTIL\t2\n@TIL\t0\t0'0\t0.5\n@ENDHEAD\n#0'0:t04\n",
    );
    assert_eq!(unsafe { cc_result_metadata(parsed.0) }.main_speed_group, 2);
    assert_eq!(unsafe { &*parsed.view().notes }.speed_group, 0);
}

#[test]
fn exports_ex_directions_and_air_point_heights_and_colors() {
    let parsed = Parsed::with_mode(
        CcFormat::Ugc,
        CcMode::Normal,
        b"@ENDHEAD\n#0'0:x04L\n#0'480:f04R\n#0'960:t04\n#0'960:a04URI\n#0'960:S0410N\n#480>s841G\n",
    );
    let view = parsed.view();
    assert_eq!(view.note_count, 5);
    assert!(matches!(
        unsafe { &*cc_result_note_details(parsed.0, 0) }.ex_direction,
        CcExDirection::Left
    ));
    assert!(matches!(
        unsafe { &*cc_result_note_details(parsed.0, 1) }.ex_direction,
        CcExDirection::Right
    ));
    assert!(matches!(
        unsafe { &*cc_result_note_details(parsed.0, 3) }.air_color,
        CcAirColor::Inverted
    ));
    let details = unsafe { &*cc_result_note_details(parsed.0, 4) };
    let points = unsafe { slice::from_raw_parts(details.points, details.point_count) };
    assert_eq!(points.len(), 2);
    assert!(points[0].air_height.has_value);
    assert_eq!(points[0].air_height.value, 2.8);
    assert_eq!(points[1].air_height.value, 3.6);
}

#[test]
fn exports_all_air_crush_interval_semantics() {
    for (interval, kind) in [
        ("0", CcAirCrushIntervalKind::Trace),
        ("$", CcAirCrushIntervalKind::Start),
        ("240", CcAirCrushIntervalKind::Every),
    ] {
        let source = format!("@ENDHEAD\n#0'0:t04\n#0'0:C04101,{interval}\n#480>c8410\n");
        let parsed = Parsed::with_mode(CcFormat::Ugc, CcMode::Normal, source.as_bytes());
        assert_eq!(parsed.view().note_count, 2);
        let details = unsafe { &*cc_result_note_details(parsed.0, 1) };
        assert!(matches!(details.air_crush_color, CcAirCrushColor::Red));
        assert_eq!(details.air_crush_interval.kind as u32, kind as u32);
        if matches!(kind, CcAirCrushIntervalKind::Every) {
            assert_eq!(
                (
                    details.air_crush_interval.interval.numerator,
                    details.air_crush_interval.interval.denominator
                ),
                (1, 2)
            );
        }
    }
}

#[test]
fn returns_warnings_and_fatal_errors_with_source_lines() {
    let parsed = Parsed::new(CcFormat::Sus, b"#00010:44\n#00010:xx\n");
    assert!(!unsafe { cc_result_error(parsed.0) }.is_null());
    let view = unsafe { cc_result_diagnostics(parsed.0) };
    let diagnostics = unsafe { slice::from_raw_parts(view.diagnostics, view.count) };
    assert_eq!(diagnostics.len(), 2);
    assert!(matches!(
        diagnostics[0].severity,
        CcDiagnosticSeverity::Warning
    ));
    assert_eq!(diagnostics[0].line, 1);
    assert!(matches!(
        diagnostics[1].severity,
        CcDiagnosticSeverity::Error
    ));
    assert_eq!(diagnostics[1].line, 2);
    assert_eq!(
        unsafe { CStr::from_ptr(diagnostics[0].format) }
            .to_str()
            .unwrap(),
        "SUS"
    );
    assert!(
        unsafe { CStr::from_ptr(diagnostics[0].message) }
            .to_str()
            .unwrap()
            .contains("omitted")
    );
}

#[test]
fn warnings_are_owned_and_isolated_across_threads_and_calls() {
    let threads: Vec<_> = [false, true]
        .into_iter()
        .map(|warn| {
            std::thread::spawn(move || {
                for _ in 0..16 {
                    let source = if warn { b"#00010:44\n" } else { b"#00010:14\n" };
                    let parsed = Parsed::new(CcFormat::Sus, source);
                    let diagnostics = unsafe { cc_result_diagnostics(parsed.0) };
                    assert_eq!(diagnostics.count, usize::from(warn));
                    assert!(unsafe { cc_result_error(parsed.0) }.is_null());
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
}

struct Parsed(*mut CcResult);

impl Parsed {
    fn new(format: CcFormat, source: &[u8]) -> Self {
        Self::with_mode(format, CcMode::Xlair, source)
    }

    fn with_mode(format: CcFormat, mode: CcMode, source: &[u8]) -> Self {
        Self(unsafe { cc_parse(format as u32, mode as u32, source.as_ptr(), source.len()) })
    }

    fn view(&self) -> CcChartView {
        assert!(unsafe { cc_result_error(self.0) }.is_null());
        unsafe { cc_result_chart(self.0) }
    }
}

impl Drop for Parsed {
    fn drop(&mut self) {
        unsafe { cc_result_free(self.0) };
    }
}

#[test]
fn validates_arguments_and_null_getters() {
    assert_eq!(cc_api_version(), 2);
    assert!(unsafe { cc_result_note_details(ptr::null(), 0) }.is_null());
    assert_eq!(
        unsafe { cc_result_metadata(ptr::null()) }.main_speed_group,
        -1
    );
    assert_eq!(unsafe { cc_result_diagnostics(ptr::null()) }.count, 0);
    for (format, mode, source, length) in [
        (99, 0, ptr::null(), 0),
        (0, 99, ptr::null(), 0),
        (0, 0, ptr::null(), 1),
        (0, 0, ptr::null(), usize::MAX),
    ] {
        let result = Parsed(unsafe { cc_parse(format, mode, source, length) });
        assert!(!unsafe { cc_result_error(result.0) }.is_null());
        assert_eq!(unsafe { cc_result_chart(result.0) }.note_count, 0);
        assert_eq!(unsafe { cc_result_diagnostics(result.0) }.count, 1);
        assert_eq!(
            unsafe { cc_result_metadata(result.0) }.measure_length_count,
            0
        );
        assert!(unsafe { cc_result_note_details(result.0, 0) }.is_null());
    }
    let empty = Parsed(unsafe { cc_parse(0, 1, ptr::null(), 0) });
    assert_eq!(empty.view().note_count, 0);
    assert!(unsafe { cc_result_error(ptr::null()) }.is_null());
    assert_eq!(unsafe { cc_result_chart(ptr::null()) }.note_count, 0);
    unsafe { cc_result_free(ptr::null_mut()) };
}

#[test]
fn preserves_format_errors_and_decodes_utf16() {
    let invalid = Parsed::new(CcFormat::Sus, &[0xff]);
    let error = unsafe { CStr::from_ptr(cc_result_error(invalid.0)) };
    assert!(error.to_str().unwrap().contains("UTF-8"));

    for (format, text) in [
        (CcFormat::Sus, "#00010: 14\n"),
        (CcFormat::C2s, "RESOLUTION\t384\nTAP\t0\t0\t0\t4\n"),
        (
            CcFormat::Ugc,
            "@TICKS\t480\n@BEAT\t0\t4\t4\n@BPM\t0'0\t120\n@ENDHEAD\n#0'0:t04\n",
        ),
    ] {
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        let parsed = Parsed::new(format, &bytes);
        assert_eq!(parsed.view().note_count, 1);
    }
}

#[test]
fn owns_paths_and_preserves_note_identity_after_input_is_dropped() {
    let parsed = {
        let mut text =
            String::from("#00010: 24\n#00050: 34\n#00030A: 14\n#00134A: 34\n#00238A: 24\n");
        for measure in 3..35 {
            text.push_str(&format!("#{measure:03}10: 14\n"));
        }
        Parsed::new(CcFormat::Sus, text.as_bytes())
    };
    let view = parsed.view();
    let notes = unsafe { slice::from_raw_parts(view.notes, view.note_count) };
    assert!(notes.len() > 32);
    for (index, note) in notes.iter().enumerate() {
        assert_eq!(note.id as usize, index);
    }
    let air = notes
        .iter()
        .find(|note| matches!(note.kind, CcNoteKind::Air))
        .unwrap();
    assert!(matches!(notes[air.parent as usize].kind, CcNoteKind::XTap));
    assert!(matches!(air.air_direction, CcAirDirection::UpperLeft));
    let slide = notes
        .iter()
        .find(|note| matches!(note.kind, CcNoteKind::Slide))
        .unwrap();
    let points = unsafe { slice::from_raw_parts(slide.points, slide.point_count) };
    assert_eq!(points.len(), 3);
    assert_eq!(points[1].lane.start, 4);
    assert_eq!(points[2].position.numerator, 8);
}

#[test]
fn exports_stable_timing_order_and_explicit_hold_checkpoints() {
    let parsed = Parsed::new(
        CcFormat::Sus,
        concat!(
            "#BPM01: 60\n#BPM02: 180\n#00208: 01\n#00008: 02\n",
            "#TIL01: \"2'0:0.5, 0'0:2.0, 0'0:3.0\"\n#HISPEED 01\n",
            "#00020A: 14\n#00120A: 34\n#00220A: 24\n",
        )
        .as_bytes(),
    );
    let view = parsed.view();
    let tempos = unsafe { slice::from_raw_parts(view.tempos, view.tempo_count) };
    assert_eq!(tempos[0].bpm, 180.0);
    assert_eq!(tempos[1].bpm, 60.0);
    let scrolls = unsafe { slice::from_raw_parts(view.scrolls, view.scroll_count) };
    assert_eq!(scrolls.len(), 3);
    assert_eq!(scrolls[0].speed, 2.0);
    assert_eq!(scrolls[1].speed, 3.0);
    assert_eq!(scrolls[2].speed, 0.5);
    let note = unsafe { &*view.notes };
    assert!(matches!(note.kind, CcNoteKind::Hold));
    assert!(matches!(note.lane.kind, CcLaneKind::Side));
    assert_eq!(note.speed_group, 0);
    let points = unsafe { slice::from_raw_parts(note.points, note.point_count) };
    assert_eq!(points.len(), 3);
    assert_eq!(points[1].position.numerator, 4);
}
