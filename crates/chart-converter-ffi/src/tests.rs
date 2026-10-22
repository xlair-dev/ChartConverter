use std::ffi::CStr;

use super::*;

struct Parsed(*mut CcResult);

impl Parsed {
    fn new(format: CcFormat, source: &[u8]) -> Self {
        Self(unsafe {
            cc_parse(
                format as u32,
                CcMode::Xlair as u32,
                source.as_ptr(),
                source.len(),
            )
        })
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
    for (format, mode, source, length) in [
        (99, 0, ptr::null(), 0),
        (0, 99, ptr::null(), 0),
        (0, 0, ptr::null(), 1),
        (0, 0, ptr::null(), usize::MAX),
    ] {
        let result = Parsed(unsafe { cc_parse(format, mode, source, length) });
        assert!(!unsafe { cc_result_error(result.0) }.is_null());
        assert_eq!(unsafe { cc_result_chart(result.0) }.note_count, 0);
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
