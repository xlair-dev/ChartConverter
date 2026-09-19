//! Parser and writer for the shared note and timing subset of the C2S format.

use chart::{
    AirColor, AirDirection, AirProperties, Chart, ChartError, ExDirection, Lane, Note, NoteId,
    NoteKind, Position, ScrollScope, ScrollSpeedChange, SlidePoint, TapKind, TempoChange,
};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum C2sError {
    #[error("line {line}: malformed record")]
    MalformedRecord { line: usize },
    #[error("line {line}: invalid value `{value}`")]
    InvalidValue { line: usize, value: String },
    #[error("line {line}: unsupported record `{record}`")]
    UnsupportedRecord { line: usize, record: String },
    #[error("line {line}: {source}")]
    Chart { line: usize, source: ChartError },
    #[error("the RESOLUTION header is missing")]
    MissingResolution,
    #[error("position cannot be represented at the C2S resolution")]
    UnrepresentablePosition,
}

/// Parses a C2S document into the shared chart model.
pub fn parse(source: &str) -> Result<Chart, C2sError> {
    Parser::new().parse(source)
}

/// Writes the representable shared chart model as a C2S document.
pub fn write(chart: &Chart) -> Result<String, C2sError> {
    let mut records = Vec::new();
    for (index, tempo) in chart.tempo_changes().iter().enumerate() {
        let (measure, tick) = output_position(tempo.position())?;
        records.push(Record::new(
            measure,
            tick,
            index,
            format!("BPM\t{measure}\t{tick}\t{:.6}", tempo.bpm()),
        ));
    }
    for (index, change) in chart.scroll_speed_changes().iter().enumerate() {
        let (measure, tick) = output_position(change.position())?;
        let duration = change.duration().ok_or(C2sError::UnsupportedRecord {
            line: 0,
            record: "scroll speed without duration".to_owned(),
        })?;
        let duration = output_ticks(duration)?;
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
        records.push(Record::new(
            measure,
            tick,
            index,
            format!(
                "{kind}\t{measure}\t{tick}\t{duration}\t{:.6}{suffix}",
                change.speed()
            ),
        ));
    }
    for (index, note) in chart.notes().iter().enumerate() {
        write_note(chart, index, note, &mut records)?;
    }
    records.sort_by_key(|record| (record.measure, record.tick, record.order));

    let mut output = String::from("RESOLUTION\t384\n");
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
) -> Result<(), C2sError> {
    let (measure, tick) = output_position(note.position())?;
    let (lane, width) = c2s_lane(note.lane())?;
    let id = NoteId::new(index as u32);
    let text = match note.kind() {
        NoteKind::Tap(kind) => {
            let tag = match kind {
                TapKind::Tap => "TAP",
                TapKind::XTap => {
                    return Err(unsupported("XTap cannot be represented by C2S"));
                }
                TapKind::Flick { direction: None } => "FLK",
                TapKind::Flick { direction: Some(_) } => {
                    return Err(unsupported(
                        "directional Flick cannot be represented by C2S",
                    ));
                }
            };
            format!("{tag}\t{measure}\t{tick}\t{lane}\t{width}")
        }
        NoteKind::ExTap { direction } => format!(
            "CHR\t{measure}\t{tick}\t{lane}\t{width}\t{}",
            encode_ex_direction(*direction)
        ),
        NoteKind::Mine => format!("MNE\t{measure}\t{tick}\t{lane}\t{width}"),
        NoteKind::Hold { end } => format!(
            "HLD\t{measure}\t{tick}\t{lane}\t{width}\t{}",
            duration_ticks(note.position(), *end)?
        ),
        NoteKind::ExHold { .. } | NoteKind::ExSlide { .. } => {
            return Err(unsupported("ExLong is not supported yet"));
        }
        NoteKind::Slide { points } => write_slide(points)?,
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
            properties,
            parent,
        } => format!(
            "AHD\t{measure}\t{tick}\t{lane}\t{width}\t{}\t{}\t{}",
            parent_type(chart, *parent)?,
            duration_ticks(note.position(), *end)?,
            encode_air_color(properties.color())
        ),
        NoteKind::AirSlide {
            points,
            properties,
            end_height: Some(end_height),
            parent,
        } => {
            if points.len() != 2 {
                return Err(unsupported("multi-segment AIR Slide"));
            }
            let end = &points[1];
            let (end_lane, end_width) = c2s_lane(end.lane())?;
            format!(
                "ASD\t{measure}\t{tick}\t{lane}\t{width}\t{}\t{}\t{}\t{end_lane}\t{end_width}\t{end_height:.6}\t{}",
                parent_type(chart, *parent)?,
                properties
                    .height()
                    .ok_or_else(|| unsupported("AIR Slide height is missing"))?,
                duration_ticks(note.position(), end.position())?,
                encode_air_color(properties.color())
            )
        }
        NoteKind::AirSlide { .. } => {
            return Err(unsupported("AIR Slide end height is missing"));
        }
    };
    records.push(Record::new(measure, tick, index, text));
    if let Some(group) = chart
        .note_speed_group(id)
        .map_err(|_| C2sError::MalformedRecord { line: 0 })?
    {
        records.push(Record::new(
            measure,
            tick,
            index,
            format!(
                "SLA\t{measure}\t{tick}\t{lane}\t{width}\t{}\t{group}",
                note_duration_ticks(note)?
            ),
        ));
    }
    Ok(())
}

fn write_slide(points: &[SlidePoint]) -> Result<String, C2sError> {
    if points.len() < 2 {
        return Err(unsupported("slide without an end point"));
    }
    points
        .windows(2)
        .enumerate()
        .map(|(index, segment)| {
            let (measure, tick) = output_position(segment[0].position())?;
            let (start_lane, start_width) = c2s_lane(segment[0].lane())?;
            let (end_lane, end_width) = c2s_lane(segment[1].lane())?;
            let kind = if index + 1 == points.len() - 1 {
                "SLD"
            } else {
                "SLC"
            };
            Ok(format!(
                "{kind}\t{measure}\t{tick}\t{start_lane}\t{start_width}\t{}\t{end_lane}\t{end_width}",
                duration_ticks(segment[0].position(), segment[1].position())?
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
        NoteKind::Hold { .. } => Ok("HLD"),
        NoteKind::Slide { .. } => Ok("SLD"),
        _ => Err(unsupported("unsupported AIR parent")),
    }
}

fn c2s_lane(lane: Lane) -> Result<(u8, u8), C2sError> {
    match lane {
        Lane::Slider { start, width } => Ok((start, width)),
        Lane::Side(_) => Err(unsupported("side lane cannot be represented by C2S")),
    }
}

fn output_position(position: Position) -> Result<(u32, u64), C2sError> {
    let ticks = output_ticks(position)?;
    Ok((
        u32::try_from(ticks / 384).map_err(|_| C2sError::UnrepresentablePosition)?,
        ticks % 384,
    ))
}

fn output_ticks(position: Position) -> Result<u64, C2sError> {
    let ticks = u128::from(position.numerator()) * 96;
    let denominator = u128::from(position.denominator());
    if ticks % denominator != 0 {
        return Err(C2sError::UnrepresentablePosition);
    }
    u64::try_from(ticks / denominator).map_err(|_| C2sError::UnrepresentablePosition)
}

fn duration_ticks(start: Position, end: Position) -> Result<u64, C2sError> {
    output_ticks(end)?
        .checked_sub(output_ticks(start)?)
        .ok_or(C2sError::UnrepresentablePosition)
}

fn note_duration_ticks(note: &Note) -> Result<u64, C2sError> {
    match note.kind() {
        NoteKind::Hold { end } | NoteKind::ExHold { end, .. } | NoteKind::AirHold { end, .. } => {
            duration_ticks(note.position(), *end)
        }
        NoteKind::Slide { points }
        | NoteKind::ExSlide { points, .. }
        | NoteKind::AirSlide { points, .. } => {
            let end = points.last().ok_or(C2sError::UnrepresentablePosition)?;
            duration_ticks(note.position(), end.position())
        }
        _ => Ok(1),
    }
}

fn encode_ex_direction(direction: ExDirection) -> &'static str {
    match direction {
        ExDirection::Up => "UP",
        ExDirection::Down => "DW",
        ExDirection::Center => "CE",
        ExDirection::All => "ALL",
        ExDirection::Wide => "VLT",
        ExDirection::Left => "LS",
        ExDirection::Right => "RS",
        ExDirection::Inward => "IN",
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

fn unsupported(record: &str) -> C2sError {
    C2sError::UnsupportedRecord {
        line: 0,
        record: record.to_owned(),
    }
}

struct Parser {
    resolution: Option<u64>,
    timeline: chart::MeasureTimeline,
    chart: Chart,
    last_ex_parent: Option<NoteId>,
    last_tap_parent: Option<NoteId>,
    last_flick_parent: Option<NoteId>,
    last_hold_parent: Option<NoteId>,
    last_slide_parent: Option<NoteId>,
    speed_assignments: Vec<(usize, Position, Lane, u64, u32)>,
}

impl Parser {
    fn new() -> Self {
        Self {
            resolution: None,
            timeline: chart::MeasureTimeline::new(Position::new(4, 1).unwrap()),
            chart: Chart::new(),
            last_ex_parent: None,
            last_tap_parent: None,
            last_flick_parent: None,
            last_hold_parent: None,
            last_slide_parent: None,
            speed_assignments: Vec::new(),
        }
    }

    fn parse(mut self, source: &str) -> Result<Chart, C2sError> {
        for (line_index, raw_line) in source.lines().enumerate() {
            let line = line_index + 1;
            let fields: Vec<_> = raw_line.trim().split('\t').collect();
            if fields.is_empty() || fields[0].is_empty() {
                continue;
            }
            match fields[0] {
                "RESOLUTION" => self.parse_resolution(line, &fields)?,
                "BPM" => self.parse_bpm(line, &fields)?,
                "MET" => self.parse_met(line, &fields)?,
                "TAP" => self.parse_tap(line, &fields, TapKind::Tap)?,
                "FLK" => self.parse_tap(line, &fields, TapKind::Flick { direction: None })?,
                "MNE" => self.parse_mine(line, &fields)?,
                "CHR" => self.parse_ex_tap(line, &fields)?,
                "AIR" => self.parse_air(line, &fields)?,
                "HLD" => self.parse_hold(line, &fields)?,
                "SLD" | "SLC" => self.parse_slide(line, &fields)?,
                "AHD" => self.parse_air_hold(line, &fields)?,
                "ASD" => self.parse_air_slide(line, &fields)?,
                "SLA" => self.parse_speed_assignment(line, &fields)?,
                "SFL" | "SLP" => self.parse_scroll_speed(line, &fields)?,
                "AHX" | "SXD" | "SXC" | "ALD" => {
                    return Err(C2sError::UnsupportedRecord {
                        line,
                        record: fields[0].to_owned(),
                    });
                }
                _ => {}
            }
        }

        if self.resolution.is_none() {
            return Err(C2sError::MissingResolution);
        }
        for (line, position, lane, duration, group) in self.speed_assignments {
            let note_id = self
                .chart
                .notes()
                .iter()
                .enumerate()
                .rev()
                .find(|(_, note)| {
                    note.position() == position
                        && note.lane() == lane
                        && note_duration_ticks(note).ok() == Some(duration)
                })
                .map(|(index, _)| NoteId::new(index as u32))
                .ok_or(C2sError::InvalidValue {
                    line,
                    value: "SLA without a matching note".to_owned(),
                })?;
            self.chart
                .set_note_speed_group(note_id, Some(group))
                .map_err(|source| C2sError::Chart { line, source })?;
        }
        Ok(self.chart)
    }

    fn parse_resolution(&mut self, line: usize, fields: &[&str]) -> Result<(), C2sError> {
        let value = fields.get(1).ok_or(C2sError::MalformedRecord { line })?;
        let resolution = parse_u64(line, value)?;
        if resolution == 0 {
            return Err(C2sError::InvalidValue {
                line,
                value: value.to_string(),
            });
        }
        self.resolution = Some(resolution);
        Ok(())
    }

    fn parse_bpm(&mut self, line: usize, fields: &[&str]) -> Result<(), C2sError> {
        let (measure, tick) = self.location(line, fields)?;
        let bpm = fields
            .get(3)
            .ok_or(C2sError::MalformedRecord { line })?
            .parse::<f64>()
            .map_err(|_| C2sError::InvalidValue {
                line,
                value: fields[3].to_owned(),
            })?;
        let position = self.position(line, measure, tick)?;
        let tempo =
            TempoChange::new(position, bpm).map_err(|source| C2sError::Chart { line, source })?;
        self.chart.add_tempo_change(tempo);
        Ok(())
    }

    fn parse_met(&mut self, line: usize, fields: &[&str]) -> Result<(), C2sError> {
        let measure = parse_u32(
            line,
            fields.get(1).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let numerator = parse_u64(
            line,
            fields.get(3).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let denominator = parse_u64(
            line,
            fields.get(4).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        if numerator == 0 || denominator == 0 {
            return Err(C2sError::InvalidValue {
                line,
                value: format!("{numerator}/{denominator}"),
            });
        }
        let length_numerator = numerator.checked_mul(4).ok_or(C2sError::InvalidValue {
            line,
            value: format!("{numerator}/{denominator}"),
        })?;
        let length = Position::new(length_numerator, denominator)
            .map_err(|source| C2sError::Chart { line, source })?;
        self.timeline.set_length(measure, length);
        Ok(())
    }

    fn parse_tap(&mut self, line: usize, fields: &[&str], kind: TapKind) -> Result<(), C2sError> {
        let (measure, tick) = self.location(line, fields)?;
        let lane = self.lane(
            line,
            fields.get(3).ok_or(C2sError::MalformedRecord { line })?,
            fields.get(4).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let position = self.position(line, measure, tick)?;
        let id = self.add_note(line, Note::new(position, lane, NoteKind::Tap(kind)))?;
        match kind {
            TapKind::Tap => self.last_tap_parent = Some(id),
            TapKind::Flick { .. } => self.last_flick_parent = Some(id),
            TapKind::XTap => {}
        }
        Ok(())
    }

    fn parse_mine(&mut self, line: usize, fields: &[&str]) -> Result<(), C2sError> {
        let (measure, tick) = self.location(line, fields)?;
        let lane = self.lane(
            line,
            fields.get(3).ok_or(C2sError::MalformedRecord { line })?,
            fields.get(4).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let position = self.position(line, measure, tick)?;
        self.add_note(line, Note::new(position, lane, NoteKind::Mine))
            .map(|_| ())
    }

    fn parse_ex_tap(&mut self, line: usize, fields: &[&str]) -> Result<(), C2sError> {
        let (measure, tick) = self.location(line, fields)?;
        let lane = self.lane(
            line,
            fields.get(3).ok_or(C2sError::MalformedRecord { line })?,
            fields.get(4).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let direction = parse_ex_direction(
            line,
            fields.get(5).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let position = self.position(line, measure, tick)?;
        let id = self.add_note(
            line,
            Note::new(position, lane, NoteKind::ExTap { direction }),
        )?;
        self.last_ex_parent = Some(id);
        Ok(())
    }

    fn parse_air(&mut self, line: usize, fields: &[&str]) -> Result<(), C2sError> {
        let parent_type = *fields.get(5).ok_or(C2sError::MalformedRecord { line })?;
        let parent = match parent_type {
            "CHR" => self.last_ex_parent,
            "TAP" => self.last_tap_parent,
            "FLK" => self.last_flick_parent,
            "HLD" => self.last_hold_parent,
            "SLD" => self.last_slide_parent,
            _ => None,
        }
        .ok_or(C2sError::InvalidValue {
            line,
            value: format!("AIR without {parent_type} parent"),
        })?;
        let (measure, tick) = self.location(line, fields)?;
        let lane = self.lane(
            line,
            fields.get(3).ok_or(C2sError::MalformedRecord { line })?,
            fields.get(4).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let position = self.position(line, measure, tick)?;
        let direction = parse_air_direction(line, fields[0])?;
        let color = parse_air_color(
            line,
            fields.get(6).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let properties = AirProperties::new(direction).with_color(color);
        self.add_note(
            line,
            Note::new(position, lane, NoteKind::Air { properties, parent }),
        )?;
        Ok(())
    }

    fn parse_scroll_speed(&mut self, line: usize, fields: &[&str]) -> Result<(), C2sError> {
        let (measure, tick) = self.location(line, fields)?;
        let duration = parse_u64(
            line,
            fields.get(3).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let speed = fields
            .get(4)
            .ok_or(C2sError::MalformedRecord { line })?
            .parse::<f64>()
            .map_err(|_| C2sError::InvalidValue {
                line,
                value: fields[4].to_owned(),
            })?;
        let scope = if fields[0] == "SLP" {
            ScrollScope::Group(parse_u32(
                line,
                fields.get(5).ok_or(C2sError::MalformedRecord { line })?,
            )?)
        } else {
            ScrollScope::Global
        };
        let position = self.position(line, measure, tick)?;
        let resolution = self.resolution.ok_or(C2sError::MissingResolution)?;
        let duration = Position::new(
            duration.checked_mul(4).ok_or(C2sError::InvalidValue {
                line,
                value: duration.to_string(),
            })?,
            resolution,
        )
        .map_err(|source| C2sError::Chart { line, source })?;
        let change = ScrollSpeedChange::with_duration(position, speed, scope, duration)
            .map_err(|source| C2sError::Chart { line, source })?;
        self.chart.add_scroll_speed_change(change);
        Ok(())
    }

    fn parse_speed_assignment(&mut self, line: usize, fields: &[&str]) -> Result<(), C2sError> {
        let (measure, tick) = self.location(line, fields)?;
        let lane = self.lane(
            line,
            fields.get(3).ok_or(C2sError::MalformedRecord { line })?,
            fields.get(4).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let duration = parse_u64(
            line,
            fields.get(5).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let group = parse_u32(
            line,
            fields.get(6).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let position = self.position(line, measure, tick)?;
        self.speed_assignments
            .push((line, position, lane, duration, group));
        Ok(())
    }

    fn parse_hold(&mut self, line: usize, fields: &[&str]) -> Result<(), C2sError> {
        let (measure, tick) = self.location(line, fields)?;
        let lane = self.lane(
            line,
            fields.get(3).ok_or(C2sError::MalformedRecord { line })?,
            fields.get(4).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let duration = parse_u64(
            line,
            fields.get(5).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let start = self.position(line, measure, tick)?;
        let end = self.position(
            line,
            measure,
            tick.checked_add(duration).ok_or(C2sError::InvalidValue {
                line,
                value: duration.to_string(),
            })?,
        )?;
        let id = self.add_note(line, Note::new(start, lane, NoteKind::Hold { end }))?;
        self.last_hold_parent = Some(id);
        Ok(())
    }

    fn parse_slide(&mut self, line: usize, fields: &[&str]) -> Result<(), C2sError> {
        let (measure, tick) = self.location(line, fields)?;
        let start_lane = self.lane(
            line,
            fields.get(3).ok_or(C2sError::MalformedRecord { line })?,
            fields.get(4).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let duration = parse_u64(
            line,
            fields.get(5).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let end_lane = self.lane(
            line,
            fields.get(6).ok_or(C2sError::MalformedRecord { line })?,
            fields.get(7).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let start = self.position(line, measure, tick)?;
        let end = self.position(
            line,
            measure,
            tick.checked_add(duration).ok_or(C2sError::InvalidValue {
                line,
                value: duration.to_string(),
            })?,
        )?;
        let end_point = SlidePoint::new(end, end_lane);
        if let Some(note_id) = self
            .chart
            .notes()
            .iter()
            .enumerate()
            .rev()
            .find(|(_, note)| {
                let NoteKind::Slide { points } = note.kind() else {
                    return false;
                };
                points
                    .last()
                    .is_some_and(|point| point.position() == start && point.lane() == start_lane)
            })
            .map(|(index, _)| NoteId::new(index as u32))
        {
            self.chart
                .append_slide_point(note_id, end_point)
                .map_err(|source| C2sError::Chart { line, source })?;
            self.last_slide_parent = Some(note_id);
            return Ok(());
        }
        let points = vec![SlidePoint::new(start, start_lane), end_point];
        let id = self.add_note(
            line,
            Note::new(start, start_lane, NoteKind::Slide { points }),
        )?;
        self.last_slide_parent = Some(id);
        Ok(())
    }

    fn parse_air_hold(&mut self, line: usize, fields: &[&str]) -> Result<(), C2sError> {
        let (measure, tick) = self.location(line, fields)?;
        let lane = self.lane(
            line,
            fields.get(3).ok_or(C2sError::MalformedRecord { line })?,
            fields.get(4).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let parent = self.parent_hold(line, fields.get(5))?;
        let duration = parse_u64(
            line,
            fields.get(6).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let color = parse_air_color(
            line,
            fields.get(7).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let start = self.position(line, measure, tick)?;
        let end = self.position(
            line,
            measure,
            tick.checked_add(duration).ok_or(C2sError::InvalidValue {
                line,
                value: duration.to_string(),
            })?,
        )?;
        let properties = AirProperties::without_direction().with_color(color);
        self.add_note(
            line,
            Note::new(
                start,
                lane,
                NoteKind::AirHold {
                    end,
                    properties,
                    parent,
                },
            ),
        )?;
        Ok(())
    }

    fn parse_air_slide(&mut self, line: usize, fields: &[&str]) -> Result<(), C2sError> {
        let (measure, tick) = self.location(line, fields)?;
        let start_lane = self.lane(
            line,
            fields.get(3).ok_or(C2sError::MalformedRecord { line })?,
            fields.get(4).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let parent = self.parent_slide(line, fields.get(5))?;
        let height = fields
            .get(6)
            .ok_or(C2sError::MalformedRecord { line })?
            .parse::<f64>()
            .map_err(|_| C2sError::InvalidValue {
                line,
                value: fields[6].to_owned(),
            })?;
        let duration = parse_u64(
            line,
            fields.get(7).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let end_lane = self.lane(
            line,
            fields.get(8).ok_or(C2sError::MalformedRecord { line })?,
            fields.get(9).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let end_height = fields
            .get(10)
            .ok_or(C2sError::MalformedRecord { line })?
            .parse::<f64>()
            .map_err(|_| C2sError::InvalidValue {
                line,
                value: fields[10].to_owned(),
            })?;
        if !end_height.is_finite() || end_height < 0.0 {
            return Err(C2sError::Chart {
                line,
                source: ChartError::InvalidAirHeight,
            });
        }
        let color = parse_air_color(
            line,
            fields.get(11).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let start = self.position(line, measure, tick)?;
        let end = self.position(
            line,
            measure,
            tick.checked_add(duration).ok_or(C2sError::InvalidValue {
                line,
                value: duration.to_string(),
            })?,
        )?;
        let properties = AirProperties::without_direction()
            .with_height(height)
            .map_err(|source| C2sError::Chart { line, source })?
            .with_color(color);
        let points = vec![
            SlidePoint::new(start, start_lane),
            SlidePoint::new(end, end_lane),
        ];
        let end_height = Some(end_height);
        self.add_note(
            line,
            Note::new(
                start,
                start_lane,
                NoteKind::AirSlide {
                    points,
                    properties,
                    end_height,
                    parent,
                },
            ),
        )?;
        Ok(())
    }

    fn parent_hold(&self, line: usize, value: Option<&&str>) -> Result<NoteId, C2sError> {
        match value.ok_or(C2sError::MalformedRecord { line })? {
            &"HLD" => self.last_hold_parent,
            target => {
                return Err(C2sError::InvalidValue {
                    line,
                    value: (*target).to_owned(),
                });
            }
        }
        .ok_or(C2sError::InvalidValue {
            line,
            value: "AIR hold without a parent".to_owned(),
        })
    }

    fn parent_slide(&self, line: usize, value: Option<&&str>) -> Result<NoteId, C2sError> {
        match value.ok_or(C2sError::MalformedRecord { line })? {
            &"SLD" => self.last_slide_parent,
            target => {
                return Err(C2sError::InvalidValue {
                    line,
                    value: (*target).to_owned(),
                });
            }
        }
        .ok_or(C2sError::InvalidValue {
            line,
            value: "AIR slide without a parent".to_owned(),
        })
    }

    fn location(&self, line: usize, fields: &[&str]) -> Result<(u32, u64), C2sError> {
        let measure = parse_u32(
            line,
            fields.get(1).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let tick = parse_u64(
            line,
            fields.get(2).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        Ok((measure, tick))
    }

    fn lane(&self, line: usize, start: &str, width: &str) -> Result<Lane, C2sError> {
        let start = parse_u8(line, start)?;
        let width = parse_u8(line, width)?;
        Lane::slider(start, width).map_err(|source| C2sError::Chart { line, source })
    }

    fn position(&self, line: usize, measure: u32, tick: u64) -> Result<Position, C2sError> {
        let resolution = self.resolution.ok_or(C2sError::MissingResolution)?;
        let measure_offset =
            u32::try_from(tick / resolution).map_err(|_| C2sError::InvalidValue {
                line,
                value: tick.to_string(),
            })?;
        let measure = measure
            .checked_add(measure_offset)
            .ok_or(C2sError::InvalidValue {
                line,
                value: tick.to_string(),
            })?;
        self.timeline
            .position(measure, tick % resolution, resolution)
            .map_err(|source| C2sError::Chart { line, source })
    }

    fn add_note(
        &mut self,
        line: usize,
        note: Result<Note, ChartError>,
    ) -> Result<NoteId, C2sError> {
        Ok(self
            .chart
            .add_note(note.map_err(|source| C2sError::Chart { line, source })?))
    }
}

fn parse_ex_direction(line: usize, value: &str) -> Result<ExDirection, C2sError> {
    match value {
        "UP" => Ok(ExDirection::Up),
        "DW" => Ok(ExDirection::Down),
        "CE" => Ok(ExDirection::Center),
        "ALL" => Ok(ExDirection::All),
        "VLT" => Ok(ExDirection::Wide),
        "LS" | "L" => Ok(ExDirection::Left),
        "RS" | "R" => Ok(ExDirection::Right),
        "IN" | "I" => Ok(ExDirection::Inward),
        _ => Err(C2sError::InvalidValue {
            line,
            value: value.to_owned(),
        }),
    }
}

fn parse_air_direction(line: usize, value: &str) -> Result<AirDirection, C2sError> {
    match value {
        "AIR" => Ok(AirDirection::Up),
        "AUR" => Ok(AirDirection::UpperRight),
        "AUL" => Ok(AirDirection::UpperLeft),
        "ADW" => Ok(AirDirection::Down),
        "ADR" => Ok(AirDirection::LowerRight),
        "ADL" => Ok(AirDirection::LowerLeft),
        _ => Err(C2sError::InvalidValue {
            line,
            value: value.to_owned(),
        }),
    }
}

fn parse_air_color(line: usize, value: &str) -> Result<chart::AirColor, C2sError> {
    match value {
        "DEF" => Ok(chart::AirColor::Normal),
        "GRN" | "PPL" => Ok(chart::AirColor::Inverted),
        _ => Err(C2sError::InvalidValue {
            line,
            value: value.to_owned(),
        }),
    }
}

fn parse_u64(line: usize, value: &str) -> Result<u64, C2sError> {
    value.parse().map_err(|_| C2sError::InvalidValue {
        line,
        value: value.to_owned(),
    })
}

fn parse_u32(line: usize, value: &str) -> Result<u32, C2sError> {
    parse_u64(line, value)?
        .try_into()
        .map_err(|_| C2sError::InvalidValue {
            line,
            value: value.to_owned(),
        })
}

fn parse_u8(line: usize, value: &str) -> Result<u8, C2sError> {
    parse_u64(line, value)?
        .try_into()
        .map_err(|_| C2sError::InvalidValue {
            line,
            value: value.to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use chart::{
        Chart, Lane, Note, NoteKind, Position, ScrollScope, ScrollSpeedChange, SlidePoint, TapKind,
        TempoChange,
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
                properties,
                end_height: Some(2.5),
                ..
            } if *parent == chart::NoteId::new(2) && properties.height() == Some(2.0)
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
    fn writes_and_parses_multi_segment_slides() {
        let mut chart = Chart::new();
        let points = vec![
            SlidePoint::new(Position::new(0, 1).unwrap(), Lane::slider(0, 4).unwrap()),
            SlidePoint::new(Position::new(1, 1).unwrap(), Lane::slider(4, 4).unwrap()),
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
}
