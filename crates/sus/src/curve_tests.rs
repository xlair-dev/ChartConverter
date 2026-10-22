use chart::{Chart, ChartMode, Lane, NoteId, NoteKind, Position, SlidePoint, SlidePointKind};

use crate::{parse, parse_with_mode, write_with_mode};

fn points(chart: &Chart) -> &[SlidePoint] {
    let NoteKind::Slide { points } = chart.notes()[0].kind() else {
        panic!("expected a slide");
    };
    points
}

#[test]
fn expands_quadratic_handles_like_xlair_without_adding_checkpoints() {
    let source = "#00030A:12\n#0003DA:0042\n#00130A:22\n";
    for mode in [ChartMode::Normal, ChartMode::Xlair] {
        let chart = parse_with_mode(source, mode).unwrap();
        let path = points(&chart);
        assert_eq!(path.len(), 41);
        assert_eq!(path[0].position(), Position::new(0, 1).unwrap());
        assert_eq!(path[40].position(), Position::new(4, 1).unwrap());
        assert_eq!(path[20].position(), Position::new(2, 1).unwrap());
        assert_eq!(path[20].lane(), Lane::slider(6, 3).unwrap());
        assert!(
            path[1..40]
                .iter()
                .all(|point| point.kind() == &SlidePointKind::Control)
        );
        assert_eq!(
            path.iter()
                .filter(|point| point.kind() == &SlidePointKind::Visible)
                .count(),
            2
        );
        assert!(
            path.windows(2)
                .all(|pair| pair[0].position() < pair[1].position())
        );
        assert!(path.iter().all(|point| {
            let Lane::Slider { start, width } = point.lane() else {
                return false;
            };
            width > 0 && u16::from(start) + u16::from(width) <= 16
        }));
    }
}

#[test]
fn supports_multiple_handles_and_time_linear_width_changes() {
    let cubic = parse("#00030A:12\n#0003DA:00420042\n#00130A:22\n").unwrap();
    assert_eq!(points(&cubic).len(), 41);
    assert_eq!(points(&cubic)[20].position(), Position::new(2, 1).unwrap());
    assert_eq!(points(&cubic)[20].lane(), Lane::slider(9, 3).unwrap());

    let widths = parse("#00030A:12\n#0003DA:0041\n#00138A:26\n").unwrap();
    assert_eq!(points(&widths)[20].lane(), Lane::slider(7, 5).unwrap());
    assert_eq!(points(&widths)[0].lane(), Lane::slider(0, 2).unwrap());
    assert_eq!(points(&widths)[40].lane(), Lane::slider(8, 6).unwrap());
}

#[test]
fn keeps_invisible_and_visible_authored_anchors_without_expanding_them() {
    let chart = parse("#00030A:12\n#0003DA:0052\n#00130A:22\n").unwrap();
    let path = points(&chart);
    assert_eq!(path.len(), 3);
    assert_eq!(path[1].kind(), &SlidePointKind::Invisible);
    assert_eq!(path[1].lane(), Lane::slider(13, 2).unwrap());
}

#[test]
fn resolves_tempo_changes_before_time_based_curve_expansion() {
    let chart = parse("#00030A:12\n#0003DA:0042\n#00130A:22\n#00008:0001\n#BPM01:240\n").unwrap();
    let path = points(&chart);
    // 120 BPM for two beats, then 240 BPM: the segment lasts 1.5 seconds.
    assert_eq!(path.len(), 31);
    // At t=1/2 the Bezier time is 0.875 s, which is beat 1.75.
    assert_eq!(path[15].position(), Position::new(7, 4).unwrap());
    assert_eq!(path[15].lane(), Lane::slider(6, 3).unwrap());

    let simultaneous = parse(
        "#00008:01\n#00008:02\n#BPM01:60\n#BPM02:120\n#00030A:12\n#0003DA:0042\n#00130A:22\n",
    )
    .unwrap();
    assert_eq!(points(&simultaneous).len(), 41); // Last event at the origin wins.
}

#[test]
fn keeps_anchor_context_and_uses_it_for_generated_points() {
    let chart = parse(
        r#"#TIL01:"0'0:1"
#TIL02:"0'0:2"
#ATR01:"pr:10"
#ATR02:"pr:99"
#HISPEED 01
#ATTRIBUTE 01
#00030A:12
#HISPEED 02
#ATTRIBUTE 02
#0003DA:0042
#NOATTRIBUTE
#00134A:32
#NOSPEED
#00138A:0042
#00230A:22
"#,
    )
    .unwrap();
    let path = points(&chart);
    assert_eq!(path.len(), 81);
    assert_eq!(path[40].position(), Position::new(4, 1).unwrap());
    assert_eq!(path[40].kind(), &SlidePointKind::Visible);
    assert_eq!(
        path.iter()
            .filter(|point| point.kind() == &SlidePointKind::Visible)
            .count(),
        3
    );
    for index in 0..40 {
        let metadata = chart.point_metadata(NoteId::new(0), index).unwrap();
        assert_eq!(metadata.speed_group, Some(0));
        assert_eq!(metadata.attributes.priority(), Some(10));
    }
    for index in 40..80 {
        let metadata = chart.point_metadata(NoteId::new(0), index).unwrap();
        assert_eq!(metadata.speed_group, Some(1));
        assert_eq!(metadata.attributes.priority(), None);
    }
    assert_eq!(
        chart
            .point_metadata(NoteId::new(0), 80)
            .unwrap()
            .speed_group,
        None
    );
    for mode in [ChartMode::Normal, ChartMode::Xlair] {
        let output = write_with_mode(&chart, mode).unwrap();
        let restored = parse_with_mode(&output, mode).unwrap();
        assert_eq!(points(&restored).len(), path.len());
        for (index, original) in path.iter().enumerate() {
            assert_eq!(points(&restored)[index].position(), original.position());
            assert_eq!(points(&restored)[index].lane(), original.lane());
            assert_eq!(
                restored.point_metadata(NoteId::new(0), index).unwrap(),
                chart.point_metadata(NoteId::new(0), index).unwrap()
            );
        }
    }
}

#[test]
fn keeps_curve_geometry_independent_of_the_source_tick_resolution() {
    let source = "#00030A:12\n#0003DA:0042\n#00130A:22\n";
    let expected = parse(source).unwrap();
    for resolution in [1, 2, 96, 480, 960, u64::MAX] {
        let chart = parse(&format!(
            "#REQUEST \"ticks_per_beat {resolution}\"\n{source}"
        ))
        .unwrap();
        let path = points(&chart);
        assert_eq!(path, points(&expected), "source resolution: {resolution}");
        assert!(
            path.windows(2)
                .all(|pair| pair[0].position() < pair[1].position())
        );
        assert_eq!(path[0].position(), Position::new(0, 1).unwrap());
        assert_eq!(
            path.last().unwrap().position(),
            Position::new(4, 1).unwrap()
        );
        // This symmetric quadratic must stay symmetric, including at a source
        // resolution of one tick per beat. First-wins tick collisions skewed it.
        for (left, right) in path.iter().zip(path.iter().rev()) {
            assert_eq!(left.lane(), right.lane());
        }
        assert_eq!(path[10].lane(), Lane::slider(4, 3).unwrap());
        assert_eq!(path[30].lane(), Lane::slider(4, 3).unwrap());
        let late_request = parse(&format!(
            "{source}#REQUEST \"ticks_per_beat {resolution}\"\n"
        ))
        .unwrap();
        assert_eq!(points(&late_request), path);
    }
}

#[test]
fn evaluates_geometry_at_the_generated_position_after_time_rounding() {
    // The handle is at beat 1/4, not halfway between the anchors. Consequently
    // beat(t) = 0.5t + 3.5t^2. Check lanes and time-linear widths against an
    // analytic inverse, rather than the implementation's binary search.
    for (start_width, end_width) in [(1, 1), (1, 5), (5, 1)] {
        let chart = parse(&format!(
            "#00030A:1{start_width}\n#0003AA:0046{}\n#00130A:2{end_width}\n",
            "00".repeat(14)
        ))
        .unwrap();
        for point in &points(&chart)[1..points(&chart).len() - 1] {
            let beat = point.position().numerator() as f64 / point.position().denominator() as f64;
            let t = ((0.25 + 14.0 * beat).sqrt() - 0.5) / 7.0;
            let center = f64::from(start_width) * 0.5 * (1.0 - t).powi(2)
                + 26.0 * t * (1.0 - t)
                + f64::from(end_width) * 0.5 * t * t;
            let width = f64::from(start_width) + f64::from(end_width - start_width) * beat / 4.0;
            let left = (center - width * 0.5 + 1e-9).floor() as u8;
            let right = (center + width * 0.5 - 1e-9).ceil() as u8;
            assert_eq!(point.lane(), Lane::slider(left, right - left).unwrap());
        }
    }
}

#[test]
fn generates_an_interior_point_even_for_short_curves() {
    let chart = parse("#BPM01:10000\n#00008:01\n#00030A:12\n#0003DA:0042\n#00130A:22\n").unwrap();
    assert_eq!(points(&chart).len(), 3); // At least one interior point for a short curve.
}

#[test]
fn accepts_handles_at_anchor_times_and_preserves_unrelated_interleaved_channels() {
    let chart = parse("#00030A:12\n#0003DA:42\n#00130A:22\n#00034B:14\n#00138B:24\n").unwrap();
    assert_eq!(chart.notes().len(), 2);
    assert_eq!(points(&chart).len(), 41);
    let NoteKind::Slide { points: straight } = chart.notes()[1].kind() else {
        panic!("expected slide");
    };
    assert_eq!(straight.len(), 2);
}

#[test]
fn writing_expanded_curves_does_not_reexpand_them_on_read() {
    let chart = parse("#00030A:12\n#0003DA:0042\n#00130A:22\n").unwrap();
    for mode in [ChartMode::Normal, ChartMode::Xlair] {
        let output = write_with_mode(&chart, mode).unwrap();
        let restored = parse_with_mode(&output, mode).unwrap();
        let before = points(&chart);
        let after = points(&restored);
        assert_eq!(after.len(), before.len());
        for (before, after) in before.iter().zip(after) {
            assert_eq!(after.position(), before.position());
            assert_eq!(after.lane(), before.lane());
            assert_eq!(
                after.kind() == &SlidePointKind::Visible,
                before.kind() == &SlidePointKind::Visible
            );
        }
        assert!(
            after[1..after.len() - 1]
                .iter()
                .all(|point| point.kind() == &SlidePointKind::Invisible)
        );
    }
}

#[test]
fn reports_unbounded_expansion_and_timing_ranges_as_diagnostics() {
    for bpm in ["0.000001", "1e-320"] {
        let source = format!("#BPM01:{bpm}\n#00008:01\n#00030A:12\n#0003DA:0042\n#00130A:22\n");
        let error = parse(&source).unwrap_err();
        assert!(matches!(error, crate::SusError::InvalidCurve { .. }));
        assert!(error.line().is_some());
    }

    let expensive = format!("#00030A:12\n{}#00130A:22\n", "#0003DA:0042\n".repeat(2000));
    assert!(matches!(
        parse(&expensive),
        Err(crate::SusError::InvalidCurve {
            reason: "curve evaluation budget exceeded",
            ..
        })
    ));
}
