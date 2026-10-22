use chart::{Chart, Lane, NoteKind, Position, SlidePoint, SlidePointKind};

use crate::{Format, parse, write};

fn path(chart: &Chart) -> &[SlidePoint] {
    let NoteKind::Slide { points } = chart.notes()[0].kind() else {
        panic!("expected slide");
    };
    points
}

fn assert_same_path(before: &Chart, after: &Chart) {
    assert_eq!(after.notes().len(), before.notes().len());
    let before = path(before);
    let after = path(after);
    assert_eq!(after.len(), before.len());
    for (before, after) in before.iter().zip(after) {
        assert_eq!(after.position(), before.position());
        assert_eq!(after.lane(), before.lane());
        assert_eq!(
            after.kind() == &SlidePointKind::Visible,
            before.kind() == &SlidePointKind::Visible
        );
    }
}

#[test]
fn serializes_normalized_sus_curves_through_ugc_and_back_without_reexpansion() {
    let before = parse(Format::Sus, "#00030A:12\n#0003DA:0042\n#00130A:22\n").unwrap();
    assert_eq!(path(&before).len(), 41);
    let ugc = write(Format::Ugc, &before).unwrap();
    assert!(ugc.contains(">c"));
    let after_ugc = parse(Format::Ugc, &ugc).unwrap();
    assert_same_path(&before, &after_ugc);
    let sus = write(Format::Sus, &after_ugc).unwrap();
    let after_sus = parse(Format::Sus, &sus).unwrap();
    assert_same_path(&before, &after_sus);
    // SUS token 5 must also be writable to UGC, not silently dropped.
    let ugc_again = write(Format::Ugc, &after_sus).unwrap();
    assert_same_path(&before, &parse(Format::Ugc, &ugc_again).unwrap());
}

#[test]
fn keeps_authored_ugc_control_points_as_on_path_anchors_when_writing_sus() {
    let source = "@ENDHEAD\n#0'0:s84\n#40>c94\n#104>cA4\n#200>cB4\n#360>cC4\n#480>cC4\n#600>cC4\n#760>cB4\n#856>cA4\n#920>c94\n#960>s84\n";
    let before = parse(Format::Ugc, source).unwrap();
    assert_eq!(path(&before).len(), 11);
    assert_eq!(path(&before)[4].lane(), Lane::slider(12, 4).unwrap());
    assert_eq!(path(&before)[4].position(), Position::new(3, 4).unwrap());
    let after = parse(Format::Sus, &write(Format::Sus, &before).unwrap()).unwrap();
    assert_same_path(&before, &after);
    assert_same_path(
        &before,
        &parse(Format::Ugc, &write(Format::Ugc, &after).unwrap()).unwrap(),
    );
}

#[test]
fn preserves_expanded_curves_through_c2s_at_different_tempos_and_source_resolutions() {
    for bpm in [120, 150, 180] {
        for resolution in [1, 480, 960] {
            let source = format!(
                "#REQUEST \"ticks_per_beat {resolution}\"\n#BPM01:{bpm}\n#00008:01\n#00030A:12\n#0003DA:0042\n#00130A:22\n"
            );
            let before = parse(Format::Sus, &source).unwrap();
            assert!(path(&before).len() > 2);
            let c2s = write(Format::C2s, &before).unwrap();
            assert!(c2s.contains("SLC\t"));
            let after = parse(Format::C2s, &c2s).unwrap();
            assert_same_path(&before, &after);
            assert_eq!(after.tempo_changes(), before.tempo_changes());
            // Also cover SUS token-5 aliases on the way back to C2S.
            let sus = parse(Format::Sus, &write(Format::Sus, &after).unwrap()).unwrap();
            assert_same_path(
                &before,
                &parse(Format::C2s, &write(Format::C2s, &sus).unwrap()).unwrap(),
            );
        }
    }
}
