use std::collections::BTreeSet;

use chart::{
    AirColor, AirCrushColor, AirCrushInterval, AirDirection, Chart, ChartMode, ExDirection, Lane,
    Note, NoteId, NoteKind, ScrollScope, SlidePoint, SlidePointKind, TapKind, report_loss,
};

use crate::{C2sError, timing::Timing};

pub(super) fn write_with_mode(chart: &Chart, mode: ChartMode) -> Result<String, C2sError> {
    let timing = Timing::for_chart(chart)?;
    if chart.priority_enabled().is_some() {
        report_loss("C2S", "SUS enable_priority request");
    }
    if chart.base_bpm().is_some() {
        report_loss("C2S", "SUS BASEBPM");
    }
    if chart.audio_offset_seconds().is_some() {
        report_loss("C2S", "SUS WAVEOFFSET");
    }
    let mut records = Vec::new();
    let mut speed_groups = BTreeSet::new();
    for (index, tempo) in chart.tempo_changes().iter().enumerate() {
        match timing.position(tempo.position()) {
            Ok((measure, tick)) => records.push(Record::new(
                measure,
                tick,
                index,
                format!("BPM\t{measure}\t{tick}\t{:.6}", tempo.bpm()),
            )),
            Err(error) if is_loss(&error) => report_loss("C2S", error),
            Err(error) => return Err(error),
        }
    }
    for (index, change) in chart.scroll_speed_changes().iter().enumerate() {
        let record = (|| {
            let (measure, tick) = timing.position(change.position())?;
            let duration = change.duration().ok_or(C2sError::UnsupportedRecord {
                line: 0,
                record: "scroll speed without duration".to_owned(),
            })?;
            let duration = timing.ticks(duration)?;
            let (kind, group) = match change.scope() {
                ScrollScope::Global => ("SFL", None),
                ScrollScope::Group(group) => ("SLP", Some(group.to_string())),
                _ => {
                    return Err(C2sError::UnsupportedRecord {
                        line: 0,
                        record: "unsupported scroll speed scope".to_owned(),
                    });
                }
            };
            let suffix = group.map_or_else(String::new, |group| format!("\t{group}"));
            Ok(Record::new(
                measure,
                tick,
                index,
                format!(
                    "{kind}\t{measure}\t{tick}\t{duration}\t{:.6}{suffix}",
                    change.speed()
                ),
            ))
        })();
        match record {
            Ok(record) => {
                if let ScrollScope::Group(group) = change.scope() {
                    speed_groups.insert(group);
                }
                records.push(record);
            }
            Err(error) if is_loss(&error) => report_loss("C2S", error),
            Err(error) => return Err(error),
        }
    }
    for (index, note) in chart.notes().iter().enumerate() {
        if note.point_metadata().iter().any(Option::is_some) {
            report_loss(
                "C2S",
                "per-point rendering context; SLA applies to an entire note",
            );
        }
        let record_start = records.len();
        match write_note(
            chart,
            index,
            note,
            &mut records,
            &speed_groups,
            mode,
            timing,
        ) {
            Ok(()) => {}
            Err(error) if is_loss(&error) => {
                records.truncate(record_start);
                report_loss("C2S", error);
            }
            Err(error) => return Err(error),
        }
    }
    records.sort_by_key(|record| (record.measure, record.tick, record.order));

    let mut output = format!("RESOLUTION\t{}\n", timing.resolution());
    for record in records {
        output.push_str(&record.text);
        output.push('\n');
    }
    Ok(output)
}

struct Record {
    measure: u32,
    tick: u64,
    order: usize,
    text: String,
}

impl Record {
    fn new(measure: u32, tick: u64, order: usize, text: String) -> Self {
        Self {
            measure,
            tick,
            order,
            text,
        }
    }
}

fn write_note(
    chart: &Chart,
    index: usize,
    note: &Note,
    records: &mut Vec<Record>,
    speed_groups: &BTreeSet<u32>,
    mode: ChartMode,
    timing: Timing,
) -> Result<(), C2sError> {
    if !note.attributes().is_empty() {
        report_loss("C2S", "SUS note attributes");
    }
    if mode == ChartMode::Xlair
        && matches!(
            note.kind(),
            NoteKind::Air { .. }
                | NoteKind::AirHold { .. }
                | NoteKind::AirSlide { .. }
                | NoteKind::AirCrush { .. }
        )
    {
        return Err(unsupported("AIR notation in XLAIR mode"));
    }
    let (measure, tick) = timing.position(note.position())?;
    let (lane, width) = c2s_lane(note.lane())?;
    let id = NoteId::new(index as u32);
    let text = match note.kind() {
        NoteKind::Tap(kind) => {
            let tag = match kind {
                TapKind::Tap => "TAP",
                TapKind::XTap => "CHR",
                TapKind::Flick { direction: None } => "FLK",
                TapKind::Flick { direction: Some(_) } => {
                    return Err(unsupported(
                        "directional Flick cannot be represented by C2S",
                    ));
                }
                TapKind::Tap4 | TapKind::Tap5 | TapKind::Tap6 => {
                    return Err(unsupported("SUS tap variant cannot be represented by C2S"));
                }
            };
            if *kind == TapKind::XTap {
                format!("{tag}\t{measure}\t{tick}\t{lane}\t{width}\tUP")
            } else {
                format!("{tag}\t{measure}\t{tick}\t{lane}\t{width}")
            }
        }
        NoteKind::ExTap { direction } => {
            let direction = encode_ex_direction(*direction)?;
            format!("CHR\t{measure}\t{tick}\t{lane}\t{width}\t{direction}")
        }
        NoteKind::Mine => format!("MNE\t{measure}\t{tick}\t{lane}\t{width}"),
        NoteKind::Hold { end, checkpoints } => {
            if !checkpoints.is_empty() {
                report_loss("C2S", "hold checkpoints");
            }
            format!(
                "HLD\t{measure}\t{tick}\t{lane}\t{width}\t{}",
                timing.duration_ticks(note.position(), *end)?
            )
        }
        NoteKind::ExHold {
            end, checkpoints, ..
        } if mode == ChartMode::Xlair => {
            if !checkpoints.is_empty() {
                report_loss("C2S", "hold checkpoints");
            }
            format!(
                "HLD\t{measure}\t{tick}\t{lane}\t{width}\t{}",
                timing.duration_ticks(note.position(), *end)?
            )
        }
        NoteKind::ExHold {
            end,
            direction,
            checkpoints,
        } => {
            if !checkpoints.is_empty() {
                report_loss("C2S", "hold checkpoints");
            }
            let direction = encode_ex_direction(*direction)?;
            format!(
                "HXD\t{measure}\t{tick}\t{lane}\t{width}\t{}\t{direction}",
                timing.duration_ticks(note.position(), *end)?
            )
        }
        NoteKind::Slide { points } => write_slide(points, None, timing)?,
        NoteKind::ExSlide { points, .. } if mode == ChartMode::Xlair => {
            write_slide(points, None, timing)?
        }
        NoteKind::ExSlide { points, direction } => write_slide(points, Some(*direction), timing)?,
        NoteKind::AirCrush {
            points,
            color,
            interval,
            ..
        } => {
            let end = points.last().ok_or(C2sError::UnrepresentablePosition)?;
            let (end_lane, end_width) = c2s_lane(end.lane())?;
            format!(
                "ALD\t{measure}\t{tick}\t{lane}\t{width}\t{}\t{:.6}\t{}\t{end_lane}\t{end_width}\t{:.6}\t{}",
                match interval {
                    AirCrushInterval::Trace => 0,
                    AirCrushInterval::Start => 9600,
                    // ALD interval codes use their own 96-tick beat scale,
                    // independently of the document's timestamp resolution.
                    AirCrushInterval::Every(interval) => Timing::new(384).ticks(*interval)?,
                },
                points[0].height(),
                timing.duration_ticks(note.position(), end.position())?,
                end.height(),
                encode_air_crush_color(*color)
            )
        }
        NoteKind::Air { properties, parent } => format!(
            "{}\t{measure}\t{tick}\t{lane}\t{width}\t{}\t{}",
            encode_air_direction(
                properties
                    .direction()
                    .ok_or_else(|| { unsupported("AIR direction is missing") })?
            ),
            parent_type(chart, *parent)?,
            encode_air_color(properties.color())
        ),
        NoteKind::AirHold {
            end,
            checkpoints,
            properties,
            parent,
        } => {
            if !checkpoints.is_empty() {
                report_loss("C2S", "AIR hold checkpoints");
            }
            format!(
                "AHD\t{measure}\t{tick}\t{lane}\t{width}\t{}\t{}\t{}",
                parent_type(chart, *parent)?,
                timing.duration_ticks(note.position(), *end)?,
                encode_air_color(properties.color())
            )
        }
        NoteKind::AirSlide {
            points,
            color,
            parent,
        } => write_air_slide(points, *color, *parent, chart, timing)?,
    };
    records.push(Record::new(measure, tick, index, text));
    if let Some(group) = chart
        .note_speed_group(id)
        .map_err(|_| C2sError::MalformedRecord { line: 0 })?
    {
        if speed_groups.contains(&group) {
            records.push(Record::new(
                measure,
                tick,
                index,
                format!(
                    "SLA\t{measure}\t{tick}\t{lane}\t{width}\t{}\t{group}",
                    timing.note_duration_ticks(note)?
                ),
            ));
        } else {
            report_loss("C2S", format!("note speed group {group}"));
        }
    }
    Ok(())
}

fn write_slide(
    points: &[SlidePoint],
    direction: Option<ExDirection>,
    timing: Timing,
) -> Result<String, C2sError> {
    if points.len() < 2 {
        return Err(unsupported("slide without an end point"));
    }
    if !matches!(
        points.first().map(SlidePoint::kind),
        Some(SlidePointKind::Visible)
    ) || !matches!(
        points.last().map(SlidePoint::kind),
        Some(SlidePointKind::Visible)
    ) {
        return Err(unsupported("slide endpoint kind"));
    }
    if points[1..points.len() - 1]
        .iter()
        .any(|point| point.kind() == &SlidePointKind::Visible)
    {
        return Err(unsupported("slide control point kind"));
    }
    points
        .windows(2)
        .enumerate()
        .map(|(index, segment)| {
            let (measure, tick) = timing.position(segment[0].position())?;
            let (start_lane, start_width) = c2s_lane(segment[0].lane())?;
            let (end_lane, end_width) = c2s_lane(segment[1].lane())?;
            let kind = if index + 1 == points.len() - 1 {
                if direction.is_some() { "SXD" } else { "SLD" }
            } else {
                if direction.is_some() { "SXC" } else { "SLC" }
            };
            let mut record = format!(
                "{kind}\t{measure}\t{tick}\t{start_lane}\t{start_width}\t{}\t{end_lane}\t{end_width}",
                timing.duration_ticks(segment[0].position(), segment[1].position())?
            );
            if let Some(direction) = direction {
                record.push('\t');
                record.push_str(encode_ex_direction(direction)?);
            }
            Ok(record)
        })
        .collect::<Result<Vec<_>, C2sError>>()
        .map(|records| records.join("\n"))
}

fn write_air_slide(
    points: &[chart::AirPoint],
    color: AirColor,
    parent: NoteId,
    chart: &Chart,
    timing: Timing,
) -> Result<String, C2sError> {
    if points.len() < 2 {
        return Err(unsupported("AIR Slide without an end point"));
    }
    if points
        .iter()
        .any(|point| point.kind() != &SlidePointKind::Visible)
    {
        return Err(unsupported("AIR Slide point kind"));
    }
    points
        .windows(2)
        .enumerate()
        .map(|(index, segment)| {
            let start = &segment[0];
            let end = &segment[1];
            let (measure, tick) = timing.position(start.position())?;
            let (start_lane, start_width) = c2s_lane(start.lane())?;
            let (end_lane, end_width) = c2s_lane(end.lane())?;
            let kind = "ASD";
            let target = if index == 0 {
                parent_type(chart, parent)?
            } else {
                "ASC"
            };
            Ok(format!(
                "{kind}\t{measure}\t{tick}\t{start_lane}\t{start_width}\t{target}\t{:.6}\t{}\t{end_lane}\t{end_width}\t{:.6}\t{}",
                start.height(),
                timing.duration_ticks(start.position(), end.position())?,
                end.height(),
                encode_air_color(color)
            ))
        })
        .collect::<Result<Vec<_>, C2sError>>()
        .map(|records| records.join("\n"))
}

fn parent_type(chart: &Chart, parent: NoteId) -> Result<&'static str, C2sError> {
    match chart
        .note(parent)
        .map_err(|_| C2sError::InvalidValue {
            line: 0,
            value: "invalid note parent".to_owned(),
        })?
        .kind()
    {
        NoteKind::Tap(TapKind::Tap) => Ok("TAP"),
        NoteKind::Tap(TapKind::Flick { .. }) => Ok("FLK"),
        NoteKind::ExTap { .. } => Ok("CHR"),
        NoteKind::Mine => Ok("MNE"),
        NoteKind::Hold { .. } => Ok("HLD"),
        NoteKind::ExHold { .. } => Ok("HLD"),
        NoteKind::AirHold { .. } => Ok("AHD"),
        NoteKind::Slide { .. } => Ok("SLD"),
        NoteKind::ExSlide { .. } => Ok("SLD"),
        _ => Err(unsupported("unsupported AIR parent")),
    }
}

fn c2s_lane(lane: Lane) -> Result<(u8, u8), C2sError> {
    match lane {
        Lane::Slider { start, width } => Ok((start, width)),
        Lane::Side(_) => Err(unsupported("side lane cannot be represented by C2S")),
    }
}

fn encode_ex_direction(direction: ExDirection) -> Result<&'static str, C2sError> {
    match direction {
        ExDirection::Up => Ok("UP"),
        ExDirection::Down => Ok("DW"),
        ExDirection::Center => Ok("CE"),
        ExDirection::All => Ok("RC"),
        ExDirection::Wide => Ok("LC"),
        ExDirection::Left => Ok("LS"),
        ExDirection::Right => Ok("RS"),
        ExDirection::Inward => Ok("BS"),
        ExDirection::UpperLeft
        | ExDirection::UpperRight
        | ExDirection::LowerLeft
        | ExDirection::LowerRight => Err(unsupported("diagonal Ex direction")),
    }
}

fn encode_air_direction(direction: AirDirection) -> &'static str {
    match direction {
        AirDirection::Up => "AIR",
        AirDirection::UpperRight => "AUR",
        AirDirection::UpperLeft => "AUL",
        AirDirection::Down => "ADW",
        AirDirection::LowerRight => "ADR",
        AirDirection::LowerLeft => "ADL",
    }
}

fn encode_air_color(color: AirColor) -> &'static str {
    match color {
        AirColor::Normal => "DEF",
        AirColor::Inverted => "PPL",
    }
}

fn encode_air_crush_color(color: AirCrushColor) -> &'static str {
    match color {
        AirCrushColor::Normal => "DEF",
        AirCrushColor::Transparent => "NON",
        AirCrushColor::Red => "RED",
        AirCrushColor::Orange => "ORN",
        AirCrushColor::Yellow => "YEL",
        AirCrushColor::Lime => "LIM",
        AirCrushColor::Green => "GRN",
        AirCrushColor::Aqua => "AQA",
        AirCrushColor::Cyan => "CYN",
        AirCrushColor::DarkBlue => "DGR",
        AirCrushColor::Blue => "BLU",
        AirCrushColor::Violet => "VLT",
        AirCrushColor::Purple => "PPL",
        AirCrushColor::Pink => "PNK",
        AirCrushColor::Gray => "GRY",
        AirCrushColor::Black => "BLK",
    }
}

fn unsupported(record: &str) -> C2sError {
    C2sError::UnsupportedRecord {
        line: 0,
        record: record.to_owned(),
    }
}

fn is_loss(error: &C2sError) -> bool {
    matches!(
        error,
        C2sError::UnsupportedRecord { .. } | C2sError::UnrepresentablePosition
    )
}
