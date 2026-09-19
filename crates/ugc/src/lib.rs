//! Parser and writer for the shared subset of the UGC chart format.

use chart::{
    AirColor, AirDirection, AirProperties, Chart, ChartError, ExDirection, Lane, MeasureTimeline,
    Note, NoteId, NoteKind, Position, ScrollScope, ScrollSpeedChange, SlidePoint, TapKind,
    TempoChange,
};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum UgcError {
    #[error("line {line}: malformed record")]
    MalformedRecord { line: usize },
    #[error("line {line}: invalid value `{value}`")]
    InvalidValue { line: usize, value: String },
    #[error("line {line}: unsupported record `{record}`")]
    UnsupportedRecord { line: usize, record: String },
    #[error("line {line}: {source}")]
    Chart { line: usize, source: ChartError },
    #[error("line {line}: a note is missing a follower line")]
    MissingFollower { line: usize },
    #[error("cannot represent `{note}` in the supported UGC output")]
    UnsupportedNote { note: String },
    #[error("position cannot be represented at the UGC resolution")]
    UnrepresentablePosition,
}

/// Parses a UGC document into the shared chart model.
pub fn parse(source: &str) -> Result<Chart, UgcError> {
    Parser::new().parse(source)
}

/// Writes the representable shared chart model as a fixed 4/4 UGC document.
pub fn write(chart: &Chart) -> Result<String, UgcError> {
    let mut tempo_records = Vec::new();
    for tempo in chart.tempo_changes() {
        let (measure, tick) = output_position(tempo.position())?;
        tempo_records.push((
            measure,
            tick,
            format!("@BPM\t{measure}'{tick}\t{:.6}\n", tempo.bpm()),
        ));
    }

    let mut speed_records = Vec::new();
    for change in chart.scroll_speed_changes() {
        if change.duration().is_some() {
            return Err(UgcError::UnsupportedNote {
                note: "scroll speed duration".to_owned(),
            });
        }
        let (measure, tick) = output_position(change.position())?;
        let speed = format!("{:.6}", change.speed());
        let text = match change.scope() {
            ScrollScope::Global => format!("@SPDMOD\t{measure}'{tick}\t{speed}\n"),
            ScrollScope::Group(group) => {
                format!("@TIL\t{group}\t{measure}'{tick}\t{speed}\n")
            }
            _ => {
                return Err(UgcError::UnsupportedNote {
                    note: "unsupported scroll speed scope".to_owned(),
                });
            }
        };
        speed_records.push((measure, tick, text));
    }

    let mut note_records = Vec::new();
    for (index, note) in chart.notes().iter().enumerate() {
        let note_id = NoteId::new(index as u32);
        note_records.push(write_note(chart, note_id, note)?);
    }
    tempo_records.sort_by_key(|(measure, tick, _)| (*measure, *tick));
    speed_records.sort_by_key(|(measure, tick, _)| (*measure, *tick));
    note_records.sort_by_key(|record| {
        (
            record.measure,
            record.tick,
            record.parent_order,
            record.is_air,
            record.order,
        )
    });

    let mut output = String::from("@VER\t8\n@EXVER\t1\n@TICKS\t480\n@BEAT\t0\t4\t4\n");
    for (_, _, text) in tempo_records {
        output.push_str(&text);
    }
    for (_, _, text) in speed_records {
        output.push_str(&text);
    }
    output.push_str("@ENDHEAD\n");
    let mut current_speed_group = None;
    for record in note_records {
        if record.speed_group != current_speed_group {
            if let Some(group) = record.speed_group {
                output.push_str(&format!("@USETIL\t{group}\n"));
            } else if current_speed_group.is_some() {
                output.push_str("@USETIL\t0\n");
            }
            current_speed_group = record.speed_group;
        }
        output.push_str(&record.text);
    }
    Ok(output)
}

struct OutputRecord {
    measure: u32,
    tick: u64,
    order: u32,
    parent_order: u32,
    is_air: bool,
    speed_group: Option<u32>,
    text: String,
}

fn write_note(chart: &Chart, note_id: NoteId, note: &Note) -> Result<OutputRecord, UgcError> {
    let (measure, tick) = output_position(note.position())?;
    let (lane, width) = central_lane(note.lane())?;
    let prefix = format!("#{measure}'{tick}:");
    let is_air = matches!(
        note.kind(),
        NoteKind::Air { .. } | NoteKind::AirHold { .. } | NoteKind::AirSlide { .. }
    );
    let (parent, text) = match note.kind() {
        NoteKind::Air { parent, .. }
        | NoteKind::AirHold { parent, .. }
        | NoteKind::AirSlide { parent, .. } => (Some(*parent), write_air_note(note, &prefix)?),
        _ => (None, write_non_air_note(note, &prefix, lane, width)?),
    };
    if let Some(parent) = parent {
        let parent_note = chart.note(parent).map_err(|_| UgcError::InvalidValue {
            line: 0,
            value: "invalid AIR parent".to_owned(),
        })?;
        if parent_note.position() > note.position() {
            return Err(UgcError::InvalidValue {
                line: 0,
                value: "AIR parent appears after its child".to_owned(),
            });
        }
    }
    let speed_group = chart
        .note_speed_group(note_id)
        .map_err(|_| UgcError::InvalidValue {
            line: 0,
            value: "invalid note id".to_owned(),
        })?;
    Ok(OutputRecord {
        measure,
        tick,
        order: note_id.value(),
        parent_order: parent.map_or(note_id.value(), NoteId::value),
        is_air,
        speed_group,
        text,
    })
}

fn write_non_air_note(note: &Note, prefix: &str, lane: u8, width: u8) -> Result<String, UgcError> {
    let text = match note.kind() {
        NoteKind::Tap(kind) => {
            let type_code = match kind {
                TapKind::Tap => 't',
                TapKind::XTap => 'x',
                TapKind::Flick { .. } => 'f',
            };
            let suffix = match kind {
                TapKind::Tap => "".to_owned(),
                TapKind::XTap => "".to_owned(),
                TapKind::Flick { direction } => encode_flick_direction(*direction)?,
            };
            format!(
                "{prefix}{type_code}{}{}{suffix}\n",
                encode_base36(lane),
                encode_base36(width)
            )
        }
        NoteKind::ExTap { direction } => {
            format!(
                "{prefix}x{}{}{}\n",
                encode_base36(lane),
                encode_base36(width),
                encode_ex_direction(*direction)
            )
        }
        NoteKind::Mine => format!("{prefix}d{}{}\n", encode_base36(lane), encode_base36(width)),
        NoteKind::Hold { end } => {
            let offset = relative_tick(note.position(), *end)?;
            format!(
                "{prefix}h{}{}\n#{}>s{}{}\n",
                encode_base36(lane),
                encode_base36(width),
                offset,
                encode_base36(lane),
                encode_base36(width),
            )
        }
        NoteKind::Slide { points } => {
            let mut text = format!("{prefix}s{}{}\n", encode_base36(lane), encode_base36(width));
            for point in points.iter().skip(1) {
                let (point_lane, point_width) = central_lane(point.lane())?;
                let offset = relative_tick(note.position(), point.position())?;
                text.push_str(&format!(
                    "#{}>s{}{}\n",
                    offset,
                    encode_base36(point_lane),
                    encode_base36(point_width),
                ));
            }
            text
        }
        NoteKind::ExHold { .. } | NoteKind::ExSlide { .. } => {
            return Err(UgcError::UnsupportedNote {
                note: "ExLong".to_owned(),
            });
        }
        NoteKind::Air { .. } | NoteKind::AirHold { .. } | NoteKind::AirSlide { .. } => {
            unreachable!("AIR notes are written by write_air_note")
        }
    };
    Ok(text)
}

fn write_air_note(note: &Note, prefix: &str) -> Result<String, UgcError> {
    match note.kind() {
        NoteKind::Air { properties, .. } => {
            let (lane, width) = central_lane(note.lane())?;
            properties.direction().ok_or(UgcError::UnsupportedNote {
                note: "AIR direction".to_owned(),
            })?;
            let attributes = encode_air_attributes(*properties)?;
            Ok(format!(
                "{prefix}a{}{}{}\n",
                encode_base36(lane),
                encode_base36(width),
                attributes
            ))
        }
        NoteKind::AirHold {
            end, properties, ..
        } => {
            let (lane, width) = central_lane(note.lane())?;
            let attributes = encode_air_attributes(*properties)?;
            Ok(format!(
                "{prefix}H{}{}{}\n#{}>s{}{}\n",
                encode_base36(lane),
                encode_base36(width),
                attributes,
                relative_tick(note.position(), *end)?,
                encode_base36(lane),
                encode_base36(width)
            ))
        }
        NoteKind::AirSlide {
            points,
            properties,
            end_height,
            ..
        } => {
            if points.len() != 2 {
                return Err(UgcError::UnsupportedNote {
                    note: "multi-segment AIR Slide".to_owned(),
                });
            }
            let (lane, width) = central_lane(note.lane())?;
            let attributes = encode_air_attributes(*properties)?;
            let end = &points[1];
            let (end_lane, end_width) = central_lane(end.lane())?;
            let end_height = end_height
                .map(encode_air_height)
                .transpose()?
                .unwrap_or_default();
            Ok(format!(
                "{prefix}S{}{}{}\n#{}>s{}{}{}\n",
                encode_base36(lane),
                encode_base36(width),
                attributes,
                relative_tick(note.position(), end.position())?,
                encode_base36(end_lane),
                encode_base36(end_width),
                end_height
            ))
        }
        _ => unreachable!("non-AIR note passed to write_air_note"),
    }
}

fn central_lane(lane: Lane) -> Result<(u8, u8), UgcError> {
    match lane {
        Lane::Slider { start, width } => Ok((start, width)),
        Lane::Side(_) => Err(UgcError::UnsupportedNote {
            note: "side lane".to_owned(),
        }),
    }
}

fn output_position(position: Position) -> Result<(u32, u64), UgcError> {
    let absolute_ticks = u128::from(position.numerator()) * 480;
    let denominator = u128::from(position.denominator());
    if absolute_ticks % denominator != 0 {
        return Err(UgcError::UnrepresentablePosition);
    }
    let absolute_ticks = absolute_ticks / denominator;
    let measure = absolute_ticks / 1920;
    let tick = absolute_ticks % 1920;
    Ok((
        u32::try_from(measure).map_err(|_| UgcError::UnrepresentablePosition)?,
        u64::try_from(tick).map_err(|_| UgcError::UnrepresentablePosition)?,
    ))
}

fn relative_tick(start: Position, end: Position) -> Result<u64, UgcError> {
    let start = absolute_tick(start)?;
    let end = absolute_tick(end)?;
    end.checked_sub(start)
        .ok_or(UgcError::UnrepresentablePosition)
}

fn absolute_tick(position: Position) -> Result<u64, UgcError> {
    let ticks = u128::from(position.numerator()) * 480;
    let denominator = u128::from(position.denominator());
    if ticks % denominator != 0 {
        return Err(UgcError::UnrepresentablePosition);
    }
    u64::try_from(ticks / denominator).map_err(|_| UgcError::UnrepresentablePosition)
}

fn encode_base36(value: u8) -> char {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    char::from(DIGITS[usize::from(value)])
}

fn encode_ex_direction(direction: ExDirection) -> &'static str {
    match direction {
        ExDirection::Up => "U",
        ExDirection::Down => "D",
        ExDirection::Center => "C",
        ExDirection::All => "A",
        ExDirection::Wide => "W",
        ExDirection::Left => "L",
        ExDirection::Right => "R",
        ExDirection::Inward => "I",
    }
}

fn encode_flick_direction(direction: Option<ExDirection>) -> Result<String, UgcError> {
    match direction {
        None => Ok(String::new()),
        Some(ExDirection::All) => Ok("A".to_owned()),
        Some(ExDirection::Left) => Ok("L".to_owned()),
        Some(ExDirection::Right) => Ok("R".to_owned()),
        Some(_) => Err(UgcError::UnsupportedNote {
            note: "unsupported Flick direction".to_owned(),
        }),
    }
}

fn encode_air_attributes(properties: AirProperties) -> Result<String, UgcError> {
    let direction = properties.direction().map(|direction| match direction {
        AirDirection::Up => "UC",
        AirDirection::UpperLeft => "UL",
        AirDirection::UpperRight => "UR",
        AirDirection::Down => "DC",
        AirDirection::LowerLeft => "DL",
        AirDirection::LowerRight => "DR",
    });
    let height = properties
        .height()
        .map(encode_air_height)
        .transpose()?
        .unwrap_or_default();
    let color = match properties.color() {
        AirColor::Normal => 'N',
        AirColor::Inverted => 'I',
    };
    Ok(format!("{}{}{}", direction.unwrap_or(""), height, color))
}

fn encode_air_height(height: f64) -> Result<String, UgcError> {
    if !height.is_finite() || height < 1.0 {
        return Err(UgcError::UnsupportedNote {
            note: "AIR height cannot be represented by UGC".to_owned(),
        });
    }
    let raw = (height - 1.0) * 20.0;
    let rounded = raw.round();
    if (raw - rounded).abs() > 1e-9 || rounded > 1295.0 {
        return Err(UgcError::UnsupportedNote {
            note: "AIR height cannot be represented by UGC".to_owned(),
        });
    }
    let raw = rounded as u32;
    Ok(format!(
        "{}{}",
        encode_base36_u32(raw / 36),
        encode_base36_u32(raw % 36)
    ))
}

fn encode_base36_u32(value: u32) -> char {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    char::from(DIGITS[value as usize])
}

struct Parser {
    ticks_per_beat: u64,
    timeline: MeasureTimeline,
    chart: Chart,
    in_header: bool,
    current_speed_group: Option<u32>,
    last_parent: Option<NoteId>,
}

impl Parser {
    fn new() -> Self {
        Self {
            ticks_per_beat: 480,
            timeline: MeasureTimeline::new(Position::new(4, 1).unwrap()),
            chart: Chart::new(),
            in_header: true,
            current_speed_group: None,
            last_parent: None,
        }
    }

    fn parse(mut self, source: &str) -> Result<Chart, UgcError> {
        let lines: Vec<_> = source.lines().collect();
        let mut index = 0;
        while index < lines.len() {
            let line_number = index + 1;
            let line = lines[index].trim();
            if line.is_empty() || line.starts_with('\'') {
                index += 1;
                continue;
            }
            if self.in_header {
                if line == "@ENDHEAD" {
                    self.in_header = false;
                } else if line.starts_with('@') {
                    self.parse_header(line_number, line)?;
                } else {
                    return Err(UgcError::MalformedRecord { line: line_number });
                }
                index += 1;
                continue;
            }
            if line.starts_with('@') {
                self.parse_directive(line_number, line)?;
                index += 1;
                continue;
            }
            let (consumed, note) = self.parse_note(line_number, line, &lines[index + 1..])?;
            if let Some(note) = note {
                let is_air = matches!(
                    note.kind(),
                    NoteKind::Air { .. } | NoteKind::AirHold { .. } | NoteKind::AirSlide { .. }
                );
                let note_id = self.chart.add_note(note);
                self.chart
                    .set_note_speed_group(note_id, self.current_speed_group)
                    .map_err(|source| UgcError::Chart {
                        line: line_number,
                        source,
                    })?;
                if !is_air {
                    self.last_parent = Some(note_id);
                }
            }
            index += consumed + 1;
        }
        Ok(self.chart)
    }

    fn parse_header(&mut self, line: usize, text: &str) -> Result<(), UgcError> {
        let (tag, value) = split_directive(text)?;
        match tag {
            "@TICKS" => {
                let ticks = parse_u64(line, value)?;
                if ticks == 0 {
                    return Err(UgcError::InvalidValue {
                        line,
                        value: value.to_owned(),
                    });
                }
                self.ticks_per_beat = ticks;
            }
            "@BEAT" => self.parse_beat(line, value)?,
            "@BPM" => self.parse_bpm(line, value)?,
            "@TIL" => self.parse_group_speed(line, value)?,
            "@SPDMOD" => self.parse_global_speed(line, value)?,
            "@MAINTIL" => {
                if value != "0" {
                    return Err(UgcError::UnsupportedRecord {
                        line,
                        record: tag.to_owned(),
                    });
                }
            }
            "@USETIL" => self.parse_speed_group(line, value)?,
            _ => {}
        }
        Ok(())
    }

    fn parse_directive(&mut self, line: usize, text: &str) -> Result<(), UgcError> {
        let (tag, value) = split_directive(text)?;
        match tag {
            "@TIL" => self.parse_group_speed(line, value)?,
            "@SPDMOD" => self.parse_global_speed(line, value)?,
            "@USETIL" => self.parse_speed_group(line, value)?,
            _ => {}
        }
        Ok(())
    }

    fn parse_group_speed(&mut self, line: usize, value: &str) -> Result<(), UgcError> {
        let fields: Vec<_> = value.split_whitespace().collect();
        if fields.len() != 3 {
            return Err(UgcError::MalformedRecord { line });
        }
        let group = parse_u32(line, fields[0])?;
        let (measure, tick) = parse_measure_tick(line, fields[1])?;
        let speed = parse_speed(line, fields[2])?;
        let position = self.position(line, measure, tick)?;
        let change = ScrollSpeedChange::with_scope(position, speed, ScrollScope::Group(group))
            .map_err(|source| UgcError::Chart { line, source })?;
        self.chart.add_scroll_speed_change(change);
        Ok(())
    }

    fn parse_global_speed(&mut self, line: usize, value: &str) -> Result<(), UgcError> {
        let fields: Vec<_> = value.split_whitespace().collect();
        if fields.len() != 2 {
            return Err(UgcError::MalformedRecord { line });
        }
        let (measure, tick) = parse_measure_tick(line, fields[0])?;
        let speed = parse_speed(line, fields[1])?;
        let position = self.position(line, measure, tick)?;
        let change = ScrollSpeedChange::new(position, speed)
            .map_err(|source| UgcError::Chart { line, source })?;
        self.chart.add_scroll_speed_change(change);
        Ok(())
    }

    fn parse_speed_group(&mut self, line: usize, value: &str) -> Result<(), UgcError> {
        let group = parse_u32(line, value)?;
        self.current_speed_group = (group != 0).then_some(group);
        Ok(())
    }

    fn parse_beat(&mut self, line: usize, value: &str) -> Result<(), UgcError> {
        let fields: Vec<_> = value.split_whitespace().collect();
        if fields.len() != 3 {
            return Err(UgcError::MalformedRecord { line });
        }
        let measure = parse_u32(line, fields[0])?;
        let numerator = parse_u64(line, fields[1])?;
        let denominator = parse_u64(line, fields[2])?;
        if numerator == 0 || denominator == 0 {
            return Err(UgcError::InvalidValue {
                line,
                value: value.to_owned(),
            });
        }
        let length = Position::new(
            numerator
                .checked_mul(4)
                .ok_or_else(|| UgcError::InvalidValue {
                    line,
                    value: value.to_owned(),
                })?,
            denominator,
        )
        .map_err(|source| UgcError::Chart { line, source })?;
        self.timeline.set_length(measure, length);
        Ok(())
    }

    fn parse_bpm(&mut self, line: usize, value: &str) -> Result<(), UgcError> {
        let fields: Vec<_> = value.split_whitespace().collect();
        if fields.len() != 2 {
            return Err(UgcError::MalformedRecord { line });
        }
        let (measure, tick) = parse_measure_tick(line, fields[0])?;
        let bpm = fields[1]
            .parse::<f64>()
            .map_err(|_| UgcError::InvalidValue {
                line,
                value: fields[1].to_owned(),
            })?;
        let position = self.position(line, measure, tick)?;
        let tempo =
            TempoChange::new(position, bpm).map_err(|source| UgcError::Chart { line, source })?;
        self.chart.add_tempo_change(tempo);
        Ok(())
    }

    fn parse_note(
        &self,
        line: usize,
        text: &str,
        following: &[&str],
    ) -> Result<(usize, Option<Note>), UgcError> {
        let (position_text, code) = text
            .split_once(':')
            .ok_or(UgcError::MalformedRecord { line })?;
        let position_text = position_text
            .strip_prefix('#')
            .ok_or(UgcError::MalformedRecord { line })?;
        let (measure, tick) = parse_measure_tick(line, position_text)?;
        if code.len() < 3 {
            return Err(UgcError::MalformedRecord { line });
        }
        let position = self.position(line, measure, tick)?;
        let kind = code.as_bytes()[0] as char;
        match kind {
            't' | 'x' | 'f' | 'd' => {
                let lane = parse_lane(line, &code[1..3])?;
                let tap_kind = match kind {
                    't' => TapKind::Tap,
                    'x' if code[3..].is_empty() => TapKind::XTap,
                    'x' => return self.parse_ex_tap(line, position, lane, &code[3..]),
                    'f' => TapKind::Flick {
                        direction: parse_flick_direction(line, &code[3..])?,
                    },
                    'd' => {
                        let note = Note::new(position, lane, NoteKind::Mine)
                            .map_err(|source| UgcError::Chart { line, source })?;
                        return Ok((0, Some(note)));
                    }
                    _ => unreachable!(),
                };
                let note = Note::new(position, lane, NoteKind::Tap(tap_kind))
                    .map_err(|source| UgcError::Chart { line, source })?;
                Ok((0, Some(note)))
            }
            'h' | 's' => {
                let (consumed, note, _) =
                    self.parse_long_note(line, position, kind, &code[1..3], following, false)?;
                Ok((consumed, note))
            }
            'a' => self.parse_air(line, position, &code[1..3], &code[3..]),
            'H' | 'S' => {
                self.parse_air_long(line, position, kind, &code[1..3], &code[3..], following)
            }
            'C' => Err(UgcError::UnsupportedRecord {
                line,
                record: kind.to_string(),
            }),
            'c' => Ok((0, None)),
            _ => Err(UgcError::UnsupportedRecord {
                line,
                record: kind.to_string(),
            }),
        }
    }

    fn parse_ex_tap(
        &self,
        line: usize,
        position: Position,
        lane: Lane,
        direction: &str,
    ) -> Result<(usize, Option<Note>), UgcError> {
        let direction = parse_ex_direction(line, direction)?;
        let note = Note::new(position, lane, NoteKind::ExTap { direction })
            .map_err(|source| UgcError::Chart { line, source })?;
        Ok((0, Some(note)))
    }

    fn parse_air(
        &self,
        line: usize,
        position: Position,
        lane_code: &str,
        attributes: &str,
    ) -> Result<(usize, Option<Note>), UgcError> {
        let parent = self.last_parent.ok_or(UgcError::InvalidValue {
            line,
            value: "AIR without a parent note".to_owned(),
        })?;
        let lane = parse_lane(line, lane_code)?;
        let direction = parse_air_direction(line, attributes.get(..2).unwrap_or(""))?;
        let properties =
            parse_air_properties(line, Some(direction), attributes.get(2..).unwrap_or(""))?;
        let note = Note::new(position, lane, NoteKind::Air { properties, parent })
            .map_err(|source| UgcError::Chart { line, source })?;
        Ok((0, Some(note)))
    }

    fn parse_air_long(
        &self,
        line: usize,
        position: Position,
        kind: char,
        lane_code: &str,
        attributes: &str,
        following: &[&str],
    ) -> Result<(usize, Option<Note>), UgcError> {
        let parent = self.last_parent.ok_or(UgcError::InvalidValue {
            line,
            value: "AIR long note without a parent note".to_owned(),
        })?;
        let properties = parse_air_properties(line, None, attributes)?;
        let (consumed, note, end_height) = self.parse_long_note(
            line,
            position,
            if kind == 'H' { 'h' } else { 's' },
            lane_code,
            following,
            true,
        )?;
        let Some(note) = note else {
            return Ok((consumed, None));
        };
        let note_kind = match (kind, note.kind()) {
            ('H', NoteKind::Hold { end }) => NoteKind::AirHold {
                end: *end,
                properties,
                parent,
            },
            ('S', NoteKind::Slide { points }) => NoteKind::AirSlide {
                points: points.clone(),
                properties,
                end_height,
                parent,
            },
            _ => return Err(UgcError::MalformedRecord { line }),
        };
        let note = Note::new(note.position(), note.lane(), note_kind)
            .map_err(|source| UgcError::Chart { line, source })?;
        Ok((consumed, Some(note)))
    }

    fn parse_long_note(
        &self,
        line: usize,
        position: Position,
        kind: char,
        start_code: &str,
        following: &[&str],
        air: bool,
    ) -> Result<(usize, Option<Note>, Option<f64>), UgcError> {
        let start_lane = parse_lane(line, start_code)?;
        let mut points = vec![SlidePoint::new(position, start_lane)];
        let mut consumed = 0;
        let mut end_height = None;
        for follower in following {
            let follower = follower.trim();
            if follower.is_empty() || follower.starts_with('\'') {
                consumed += 1;
                continue;
            }
            let Some((offset, data)) = parse_follower(follower) else {
                break;
            };
            if data.len() < 3 {
                return Err(UgcError::MalformedRecord {
                    line: line + consumed + 1,
                });
            }
            let marker = data.as_bytes()[0] as char;
            if marker != 's' && marker != 'c' {
                return Err(UgcError::UnsupportedRecord {
                    line: line + consumed + 1,
                    record: marker.to_string(),
                });
            }
            let endpoint = position
                .checked_add(
                    Position::new(offset, self.ticks_per_beat).map_err(|source| {
                        UgcError::Chart {
                            line: line + consumed + 1,
                            source,
                        }
                    })?,
                )
                .map_err(|source| UgcError::Chart {
                    line: line + consumed + 1,
                    source,
                })?;
            let lane = parse_lane(line + consumed + 1, &data[1..3])?;
            if air && kind == 's' && data.len() > 3 {
                end_height = Some(parse_air_height(line + consumed + 1, &data[3..])?);
            }
            points.push(SlidePoint::new(endpoint, lane));
            consumed += 1;
        }
        if points.len() == 1 {
            return Err(UgcError::MissingFollower { line });
        }
        let note = if kind == 'h' {
            let first_lane = points[0].lane();
            if points.iter().any(|point| point.lane() != first_lane) {
                return Err(UgcError::UnsupportedRecord {
                    line,
                    record: "variable hold lane".to_owned(),
                });
            }
            Note::new(
                position,
                first_lane,
                NoteKind::Hold {
                    end: points.last().unwrap().position(),
                },
            )
        } else {
            Note::new(position, start_lane, NoteKind::Slide { points })
        }
        .map_err(|source| UgcError::Chart { line, source })?;
        Ok((consumed, Some(note), end_height))
    }

    fn position(&self, line: usize, measure: u32, tick: u64) -> Result<Position, UgcError> {
        let ticks_per_measure =
            self.ticks_per_beat
                .checked_mul(4)
                .ok_or_else(|| UgcError::InvalidValue {
                    line,
                    value: self.ticks_per_beat.to_string(),
                })?;
        self.timeline
            .position(measure, tick, ticks_per_measure)
            .map_err(|source| UgcError::Chart { line, source })
    }
}

fn split_directive(line: &str) -> Result<(&str, &str), UgcError> {
    let Some((tag, value)) = line.split_once('\t') else {
        return Ok((line, ""));
    };
    Ok((tag, value.trim()))
}

fn parse_measure_tick(line: usize, value: &str) -> Result<(u32, u64), UgcError> {
    let (measure, tick) = value
        .split_once('\'')
        .ok_or(UgcError::MalformedRecord { line })?;
    Ok((parse_u32(line, measure)?, parse_u64(line, tick)?))
}

fn parse_follower(line: &str) -> Option<(u64, &str)> {
    let line = line.strip_prefix('#')?;
    let (offset, data) = line.split_once('>')?;
    Some((offset.parse().ok()?, data))
}

fn parse_lane(line: usize, value: &str) -> Result<Lane, UgcError> {
    if value.len() != 2 {
        return Err(UgcError::MalformedRecord { line });
    }
    let start = base36(line, value.as_bytes()[0])?;
    let width = base36(line, value.as_bytes()[1])?;
    Lane::slider(start, width).map_err(|source| UgcError::Chart { line, source })
}

fn parse_ex_direction(line: usize, value: &str) -> Result<ExDirection, UgcError> {
    match value {
        "U" => Ok(ExDirection::Up),
        "D" => Ok(ExDirection::Down),
        "C" => Ok(ExDirection::Center),
        "A" => Ok(ExDirection::All),
        "W" => Ok(ExDirection::Wide),
        "L" => Ok(ExDirection::Left),
        "R" => Ok(ExDirection::Right),
        "I" => Ok(ExDirection::Inward),
        _ => Err(UgcError::InvalidValue {
            line,
            value: value.to_owned(),
        }),
    }
}

fn parse_flick_direction(line: usize, value: &str) -> Result<Option<ExDirection>, UgcError> {
    if value.is_empty() || value == "A" {
        Ok(None)
    } else {
        Ok(Some(parse_ex_direction(line, value)?))
    }
}

fn parse_air_direction(line: usize, value: &str) -> Result<AirDirection, UgcError> {
    match value {
        "UC" => Ok(AirDirection::Up),
        "UL" => Ok(AirDirection::UpperLeft),
        "UR" => Ok(AirDirection::UpperRight),
        "DC" => Ok(AirDirection::Down),
        "DL" => Ok(AirDirection::LowerLeft),
        "DR" => Ok(AirDirection::LowerRight),
        _ => Err(UgcError::InvalidValue {
            line,
            value: value.to_owned(),
        }),
    }
}

fn parse_air_properties(
    line: usize,
    direction: Option<AirDirection>,
    attributes: &str,
) -> Result<AirProperties, UgcError> {
    if attributes.is_empty() {
        return Ok(direction.map_or_else(AirProperties::without_direction, AirProperties::new));
    }
    let (height_text, color_text) = attributes.split_at(attributes.len() - 1);
    let color = match color_text {
        "N" => AirColor::Normal,
        "I" => AirColor::Inverted,
        _ => {
            return Err(UgcError::InvalidValue {
                line,
                value: attributes.to_owned(),
            });
        }
    };
    let properties = direction
        .map_or_else(AirProperties::without_direction, AirProperties::new)
        .with_color(color);
    if height_text.is_empty() {
        return Ok(properties);
    }
    let height = parse_air_height(line, height_text)?;
    properties
        .with_height(height)
        .map_err(|source| UgcError::Chart { line, source })
}

fn parse_air_height(line: usize, text: &str) -> Result<f64, UgcError> {
    let raw = text.bytes().try_fold(0u32, |value, digit| {
        let digit = match digit {
            b'0'..=b'9' => u32::from(digit - b'0'),
            b'a'..=b'z' => u32::from(digit - b'a' + 10),
            b'A'..=b'Z' => u32::from(digit - b'A' + 10),
            _ => return None,
        };
        value.checked_mul(36)?.checked_add(digit)
    });
    let raw = raw.ok_or_else(|| UgcError::InvalidValue {
        line,
        value: text.to_owned(),
    })?;
    Ok(if text.len() == 1 {
        f64::from(raw) / 2.0 + 1.0
    } else {
        f64::from(raw) / 20.0 + 1.0
    })
}

fn base36(line: usize, value: u8) -> Result<u8, UgcError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'z' => Ok(value - b'a' + 10),
        b'A'..=b'Z' => Ok(value - b'A' + 10),
        _ => Err(UgcError::InvalidValue {
            line,
            value: (value as char).to_string(),
        }),
    }
}

fn parse_u64(line: usize, value: &str) -> Result<u64, UgcError> {
    value.parse().map_err(|_| UgcError::InvalidValue {
        line,
        value: value.to_owned(),
    })
}

fn parse_speed(line: usize, value: &str) -> Result<f64, UgcError> {
    value.parse().map_err(|_| UgcError::InvalidValue {
        line,
        value: value.to_owned(),
    })
}

fn parse_u32(line: usize, value: &str) -> Result<u32, UgcError> {
    parse_u64(line, value)?
        .try_into()
        .map_err(|_| UgcError::InvalidValue {
            line,
            value: value.to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use chart::{Chart, Lane, Note, NoteKind, Position, ScrollScope, ScrollSpeedChange, TapKind};

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
        let source =
            "@TICKS\t480\n@BEAT\t0\t4\t4\n@TIL\t2\t0'0\t1.5\n@ENDHEAD\n@USETIL\t2\n#0'0:t04\n";
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
        chart.add_note(
            Note::new(start, Lane::slider(0, 4).unwrap(), NoteKind::Hold { end }).unwrap(),
        );

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
        let end = Position::new(2, 1).unwrap();
        chart.add_note(
            Note::new(
                start,
                Lane::slider(4, 4).unwrap(),
                NoteKind::AirSlide {
                    points: vec![
                        chart::SlidePoint::new(start, Lane::slider(4, 4).unwrap()),
                        chart::SlidePoint::new(end, Lane::slider(8, 4).unwrap()),
                    ],
                    properties: chart::AirProperties::without_direction()
                        .with_height(2.0)
                        .unwrap(),
                    end_height: Some(2.5),
                    parent,
                },
            )
            .unwrap(),
        );

        let ugc = write(&chart).expect("valid AIR long UGC output");
        let parsed = parse(&ugc).expect("round-tripped AIR long UGC output");
        assert_eq!(parsed.notes(), chart.notes());
    }
}
