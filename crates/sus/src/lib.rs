//! Parser for the XLAIR-compatible subset of the SUS chart format.

use std::collections::{BTreeMap, HashMap};

use chart::{
    Chart, ChartError, Lane, Note, NoteKind, Position, ScrollScope, ScrollSpeedChange, SideButton,
    SlidePoint, TapKind, TempoChange,
};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum SusError {
    #[error("line {line}: malformed command")]
    MalformedCommand { line: usize },
    #[error("line {line}: invalid value `{value}`")]
    InvalidValue { line: usize, value: String },
    #[error("line {line}: unsupported command `{command}`")]
    UnsupportedCommand { line: usize, command: String },
    #[error("line {line}: {source}")]
    Chart { line: usize, source: ChartError },
    #[error("line {line}: channel `{channel}` has no start point")]
    MissingStart { line: usize, channel: char },
    #[error("channel `{channel}` has no end point")]
    MissingEnd { channel: char },
    #[error("cannot represent `{note}` in the supported SUS output")]
    UnsupportedNote { note: String },
    #[error("position cannot be represented at the SUS resolution")]
    UnrepresentablePosition,
}

#[derive(Clone, Debug)]
struct PendingSlide {
    points: Vec<SlidePoint>,
    side_button: Option<SideButton>,
}

/// Parses a SUS document into the shared chart model.
pub fn parse(source: &str) -> Result<Chart, SusError> {
    Parser::new().parse(source)
}

/// Writes a chart as an XLAIR-compatible SUS document.
pub fn write(chart: &Chart) -> Result<String, SusError> {
    let mut records = Vec::new();
    let mut bpm_definitions = Vec::new();
    let mut speed_definitions = BTreeMap::<u32, Vec<(u32, u64, f64)>>::new();
    for change in chart.scroll_speed_changes() {
        if change.duration().is_some() {
            return Err(SusError::UnsupportedNote {
                note: "scroll speed duration".to_owned(),
            });
        }
        let ScrollScope::Group(group) = change.scope() else {
            return Err(SusError::UnsupportedNote {
                note: "non-group scroll speed changes".to_owned(),
            });
        };
        let group = speed_group(group)?;
        let (measure, tick) = output_position(change.position())?;
        speed_definitions
            .entry(group)
            .or_default()
            .push((measure, tick, change.speed()));
    }
    for definitions in speed_definitions.values_mut() {
        definitions.sort_by_key(|(measure, tick, _)| (*measure, *tick));
    }
    for (index, tempo) in chart.tempo_changes().iter().enumerate() {
        let (measure, tick) = output_position(tempo.position())?;
        let id = format!("{:02}", index + 1);
        bpm_definitions.push(format!("#BPM{id}: {}\n", tempo.bpm()));
        records.push(Record {
            measure,
            tick,
            key: "08".to_owned(),
            token: id,
            speed_group: None,
        });
    }

    let mut channel_index = 0;
    for (note_index, note) in chart.notes().iter().enumerate() {
        if matches!(
            note.kind(),
            NoteKind::Air { .. }
                | NoteKind::AirHold { .. }
                | NoteKind::AirSlide { .. }
                | NoteKind::AirCrush { .. }
        ) {
            continue;
        }
        let channel = channel(channel_index)?;
        channel_index += 1;
        let record_start = records.len();
        match note.kind() {
            NoteKind::Tap(kind) => {
                let (measure, tick) = output_position(note.position())?;
                let token = match kind {
                    TapKind::Tap => '1',
                    TapKind::XTap => '2',
                    TapKind::Flick { .. } => '3',
                };
                match note.lane() {
                    Lane::Slider { start, width } => records.push(Record {
                        measure,
                        tick,
                        key: format!("1{}", base36_digit(start)),
                        token: format!("{token}{}", base36_digit(width)),
                        speed_group: None,
                    }),
                    Lane::Side(button) if *kind == TapKind::Tap => records.push(Record {
                        measure,
                        tick,
                        key: format!("5{}", base36_digit(side_lane(button))),
                        token: format!("{}1", side_direction(button)),
                        speed_group: None,
                    }),
                    Lane::Side(_) => {
                        return Err(SusError::UnsupportedNote {
                            note: "side ExTap/Flick".to_owned(),
                        });
                    }
                }
            }
            NoteKind::ExTap { .. } => {
                return Err(SusError::UnsupportedNote {
                    note: "ExTap".to_owned(),
                });
            }
            NoteKind::Hold { end } => match note.lane() {
                Lane::Slider { start, width } => {
                    add_hold_records(&mut records, note.position(), *end, start, width, channel)?
                }
                Lane::Side(button) => {
                    add_side_hold_records(&mut records, note.position(), *end, button, channel)?
                }
            },
            NoteKind::ExHold { .. } | NoteKind::ExSlide { .. } => {
                return Err(SusError::UnsupportedNote {
                    note: "ExLong".to_owned(),
                });
            }
            NoteKind::Slide { points } => {
                add_slide_records(&mut records, points, channel)?;
            }
            NoteKind::Mine => {
                return Err(SusError::UnsupportedNote {
                    note: "unsupported note kind".to_owned(),
                });
            }
            NoteKind::Air { .. }
            | NoteKind::AirHold { .. }
            | NoteKind::AirSlide { .. }
            | NoteKind::AirCrush { .. } => {
                unreachable!("AIR notes are filtered before channel allocation");
            }
        }
        let speed_group = chart
            .note_speed_group(chart::NoteId::new(note_index as u32))
            .map_err(|_| SusError::UnrepresentablePosition)?
            .map(speed_group)
            .transpose()?;
        for record in &mut records[record_start..] {
            record.speed_group = speed_group;
        }
    }

    records.sort_by_key(|record| (record.measure, record.tick, record.key.clone()));
    let mut output = String::from("#REQUEST \"ticks_per_beat 384\"\n#00002: 4\n");
    for definition in bpm_definitions {
        output.push_str(&definition);
    }
    for (group, definitions) in speed_definitions {
        let definitions = definitions
            .into_iter()
            .map(|(measure, tick, speed)| format!("{measure}'{tick}:{speed}"))
            .collect::<Vec<_>>();
        output.push_str(&format!(
            "#TIL{}: \"{}\"\n",
            speed_group_text(group),
            definitions.join(",")
        ));
    }
    let mut current_speed_group = None;
    for record in records {
        if record.speed_group != current_speed_group {
            match record.speed_group {
                Some(group) => output.push_str(&format!("#HISPEED {}\n", speed_group_text(group))),
                None => output.push_str("#NOSPEED\n"),
            }
            current_speed_group = record.speed_group;
        }
        output.push_str(&format_record(&record)?);
    }
    Ok(output)
}

struct Record {
    measure: u32,
    tick: u64,
    key: String,
    token: String,
    speed_group: Option<u32>,
}

fn add_hold_records(
    records: &mut Vec<Record>,
    start: Position,
    end: Position,
    lane: u8,
    width: u8,
    channel: char,
) -> Result<(), SusError> {
    let (start_measure, start_tick) = output_position(start)?;
    let (end_measure, end_tick) = output_position(end)?;
    records.push(Record {
        measure: start_measure,
        tick: start_tick,
        key: format!("3{}{channel}", base36_digit(lane)),
        token: format!("1{}", base36_digit(width)),
        speed_group: None,
    });
    records.push(Record {
        measure: end_measure,
        tick: end_tick,
        key: format!("3{}{channel}", base36_digit(lane)),
        token: format!("2{}", base36_digit(width)),
        speed_group: None,
    });
    Ok(())
}

fn add_side_hold_records(
    records: &mut Vec<Record>,
    start: Position,
    end: Position,
    button: SideButton,
    channel: char,
) -> Result<(), SusError> {
    let (start_measure, start_tick) = output_position(start)?;
    let (end_measure, end_tick) = output_position(end)?;
    let lane = side_lane(button);
    records.push(Record {
        measure: start_measure,
        tick: start_tick,
        key: format!("2{}{channel}", base36_digit(lane)),
        token: "11".to_owned(),
        speed_group: None,
    });
    records.push(Record {
        measure: end_measure,
        tick: end_tick,
        key: format!("2{}{channel}", base36_digit(lane)),
        token: "21".to_owned(),
        speed_group: None,
    });
    Ok(())
}

fn add_slide_records(
    records: &mut Vec<Record>,
    points: &[SlidePoint],
    channel: char,
) -> Result<(), SusError> {
    for (index, point) in points.iter().enumerate() {
        let lane = central_lane(point.lane(), "slide")?;
        let (measure, tick) = output_position(point.position())?;
        let kind = if index == 0 {
            '1'
        } else if index + 1 == points.len() {
            '2'
        } else {
            '3'
        };
        records.push(Record {
            measure,
            tick,
            key: format!("3{}{channel}", base36_digit(lane)),
            token: format!("{kind}{}", base36_digit(lane_width(point.lane())?)),
            speed_group: None,
        });
    }
    Ok(())
}

fn format_record(record: &Record) -> Result<String, SusError> {
    let padding = usize::try_from(record.tick).map_err(|_| SusError::UnrepresentablePosition)?;
    let trailing = 384usize
        .checked_sub(padding + 1)
        .ok_or(SusError::UnrepresentablePosition)?;
    Ok(format!(
        "#{:03}{}: {}{}{}\n",
        record.measure,
        record.key,
        "00".repeat(padding),
        record.token,
        "00".repeat(trailing),
    ))
}

fn output_position(position: Position) -> Result<(u32, u64), SusError> {
    let absolute_ticks = u128::from(position.numerator()) * 96;
    let denominator = u128::from(position.denominator());
    if absolute_ticks % denominator != 0 {
        return Err(SusError::UnrepresentablePosition);
    }
    let absolute_ticks = absolute_ticks / denominator;
    let measure = absolute_ticks / 384;
    let tick = absolute_ticks % 384;
    Ok((
        u32::try_from(measure).map_err(|_| SusError::UnrepresentablePosition)?,
        u64::try_from(tick).map_err(|_| SusError::UnrepresentablePosition)?,
    ))
}

fn channel(index: usize) -> Result<char, SusError> {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    DIGITS
        .get(index)
        .copied()
        .map(char::from)
        .ok_or(SusError::UnrepresentablePosition)
}

fn central_lane(lane: Lane, note: &str) -> Result<u8, SusError> {
    match lane {
        Lane::Slider { start, .. } => Ok(start),
        Lane::Side(_) => Err(SusError::UnsupportedNote {
            note: note.to_owned(),
        }),
    }
}

fn lane_width(lane: Lane) -> Result<u8, SusError> {
    match lane {
        Lane::Slider { width, .. } => Ok(width),
        Lane::Side(_) => Err(SusError::UnsupportedNote {
            note: "side lane".to_owned(),
        }),
    }
}

fn base36_digit(value: u8) -> char {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    char::from(DIGITS[usize::from(value)])
}

fn speed_group(group: u32) -> Result<u32, SusError> {
    if group < 36 * 36 {
        Ok(group)
    } else {
        Err(SusError::UnrepresentablePosition)
    }
}

fn speed_group_text(group: u32) -> String {
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let high = digits[(group / 36) as usize] as char;
    let low = digits[(group % 36) as usize] as char;
    format!("{high}{low}")
}

struct Parser {
    timeline: chart::MeasureTimeline,
    bpm_definitions: HashMap<String, f64>,
    pending_side_longs: HashMap<char, PendingSlide>,
    pending_sliders: HashMap<char, PendingSlide>,
    ticks_per_beat: u64,
    current_speed_group: Option<u32>,
    chart: Chart,
}

impl Parser {
    fn new() -> Self {
        Self {
            timeline: chart::MeasureTimeline::new(Position::new(4, 1).unwrap()),
            bpm_definitions: HashMap::new(),
            pending_side_longs: HashMap::new(),
            pending_sliders: HashMap::new(),
            ticks_per_beat: 480,
            current_speed_group: None,
            chart: Chart::new(),
        }
    }

    fn parse(mut self, source: &str) -> Result<Chart, SusError> {
        for (line_index, raw_line) in source.lines().enumerate() {
            let line = line_index + 1;
            let text = raw_line.trim();
            if text.is_empty() || !text.starts_with('#') || text.starts_with("#COMMENT") {
                continue;
            }

            let command = text[1..].trim();
            if command.starts_with("REQUEST") {
                self.parse_request(line, command)?;
                continue;
            }
            if command.starts_with("HISPEED") {
                self.current_speed_group = Some(parse_group(
                    line,
                    command
                        .split_whitespace()
                        .nth(1)
                        .ok_or(SusError::MalformedCommand { line })?,
                )?);
                continue;
            }
            if command == "NOSPEED" {
                self.current_speed_group = None;
                continue;
            }
            if command.starts_with("MEASUREHS") || command.starts_with("MEASUREBS") {
                return Err(SusError::UnsupportedCommand {
                    line,
                    command: command.to_owned(),
                });
            }

            let Some((header, data)) = command.split_once(':') else {
                if is_metadata(command) {
                    continue;
                }
                return Err(SusError::MalformedCommand { line });
            };
            let header = header.trim();
            let data = data.trim().replace(' ', "");
            if header.starts_with("BPM") {
                self.parse_bpm_definition(line, header, &data)?;
            } else if header.starts_with("TIL") {
                self.parse_til_definition(line, header, &data)?;
            } else if header.len() >= 3 && header[..3].chars().all(|c| c.is_ascii_digit()) {
                self.parse_data_line(line, header, &data)?;
            }
        }

        if let Some(channel) = self.pending_side_longs.keys().next().copied() {
            return Err(SusError::MissingEnd { channel });
        }
        if let Some(channel) = self.pending_sliders.keys().next().copied() {
            return Err(SusError::MissingEnd { channel });
        }
        Ok(self.chart)
    }

    fn parse_request(&mut self, line: usize, command: &str) -> Result<(), SusError> {
        let value = command.trim_start_matches("REQUEST").trim();
        let fields: Vec<_> = value.trim_matches('"').split_whitespace().collect();
        if fields.len() != 2 || fields[0] != "ticks_per_beat" {
            return Err(SusError::UnsupportedCommand {
                line,
                command: command.to_owned(),
            });
        }
        let ticks = fields[1]
            .parse::<u64>()
            .map_err(|_| SusError::InvalidValue {
                line,
                value: fields[1].to_owned(),
            })?;
        if ticks == 0 {
            return Err(SusError::InvalidValue {
                line,
                value: fields[1].to_owned(),
            });
        }
        self.ticks_per_beat = ticks;
        Ok(())
    }

    fn parse_bpm_definition(
        &mut self,
        line: usize,
        header: &str,
        data: &str,
    ) -> Result<(), SusError> {
        if header.len() != 5 || data.is_empty() {
            return Err(SusError::MalformedCommand { line });
        }
        let bpm = data.parse::<f64>().map_err(|_| SusError::InvalidValue {
            line,
            value: data.to_owned(),
        })?;
        self.bpm_definitions.insert(header[3..].to_owned(), bpm);
        Ok(())
    }

    fn parse_til_definition(
        &mut self,
        line: usize,
        header: &str,
        data: &str,
    ) -> Result<(), SusError> {
        if header.len() != 5 {
            return Err(SusError::MalformedCommand { line });
        }
        let group = parse_group(line, &header[3..])?;
        let definition = data.trim_matches('"');
        if definition.is_empty() {
            return Ok(());
        }
        for point in definition.split(',') {
            let (position, speed) = point
                .trim()
                .split_once(':')
                .ok_or(SusError::MalformedCommand { line })?;
            let (measure, tick) = parse_measure_tick(line, position)?;
            let speed = speed.parse::<f64>().map_err(|_| SusError::InvalidValue {
                line,
                value: speed.to_owned(),
            })?;
            let denominator = self.ticks_per_beat;
            let position = self
                .timeline
                .position(measure, tick, denominator)
                .map_err(|source| SusError::Chart { line, source })?;
            let change = ScrollSpeedChange::with_scope(position, speed, ScrollScope::Group(group))
                .map_err(|source| SusError::Chart { line, source })?;
            self.chart.add_scroll_speed_change(change);
        }
        Ok(())
    }

    fn parse_data_line(&mut self, line: usize, header: &str, data: &str) -> Result<(), SusError> {
        let measure = header[..3]
            .parse::<u32>()
            .map_err(|_| SusError::MalformedCommand { line })?;
        let kind = &header[3..];
        if kind == "02" {
            let length = parse_position(data).map_err(|_| SusError::InvalidValue {
                line,
                value: data.to_owned(),
            })?;
            self.timeline.set_length(measure, length);
            return Ok(());
        }
        if kind == "08" {
            return self.parse_bpm_change(line, measure, data);
        }
        if kind.len() != 2 && kind.len() != 3 {
            return Ok(());
        }

        let channel = kind.chars().nth(2);
        let note_kind = kind.chars().next().unwrap();
        let lane = base36(kind.chars().nth(1).unwrap(), line)?;
        let positions = self.positions(line, measure, data)?;
        match note_kind {
            '1' => self.parse_short_notes(line, lane, data, positions),
            '2' => self.parse_side_long(line, lane, channel.unwrap(), data, positions),
            '3' => self.parse_slider(line, lane, channel.unwrap(), data, positions),
            '5' => self.parse_directional_notes(line, data, positions),
            _ => Ok(()),
        }
    }

    fn parse_bpm_change(&mut self, line: usize, measure: u32, data: &str) -> Result<(), SusError> {
        for (index, token) in data.as_bytes().chunks(2).enumerate() {
            if token.len() != 2 {
                return Err(SusError::InvalidValue {
                    line,
                    value: data.to_owned(),
                });
            }
            let key = std::str::from_utf8(token).unwrap();
            if key == "00" {
                continue;
            }
            let bpm = *self
                .bpm_definitions
                .get(key)
                .ok_or_else(|| SusError::InvalidValue {
                    line,
                    value: key.to_owned(),
                })?;
            let position = self
                .timeline
                .position(measure, index as u64, (data.len() / 2) as u64)
                .map_err(|source| SusError::Chart { line, source })?;
            self.chart.add_tempo_change(
                TempoChange::new(position, bpm)
                    .map_err(|source| SusError::Chart { line, source })?,
            );
        }
        Ok(())
    }

    fn positions(&self, line: usize, measure: u32, data: &str) -> Result<Vec<Position>, SusError> {
        if !data.len().is_multiple_of(2) {
            return Err(SusError::InvalidValue {
                line,
                value: data.to_owned(),
            });
        }
        let count = data.len() / 2;
        (0..count)
            .map(|index| {
                self.timeline
                    .position(measure, index as u64, count as u64)
                    .map_err(|source| SusError::Chart { line, source })
            })
            .collect()
    }

    fn parse_short_notes(
        &mut self,
        line: usize,
        lane: u8,
        data: &str,
        positions: Vec<Position>,
    ) -> Result<(), SusError> {
        for (token, position) in data.as_bytes().chunks(2).zip(positions) {
            if token[0] == b'0' {
                continue;
            }
            let kind = match token[0] {
                b'1' => TapKind::Tap,
                b'2' => TapKind::XTap,
                b'3' => TapKind::Flick { direction: None },
                _ => return Err(invalid_token(line, token)),
            };
            let lane = Lane::slider(lane, base36_byte(token[1], line)?)
                .map_err(|source| SusError::Chart { line, source })?;
            self.add_note(
                line,
                Note::new(position, lane, NoteKind::Tap(kind))
                    .map_err(|source| SusError::Chart { line, source })?,
            )?;
        }
        Ok(())
    }

    fn parse_directional_notes(
        &mut self,
        line: usize,
        data: &str,
        positions: Vec<Position>,
    ) -> Result<(), SusError> {
        for (token, position) in data.as_bytes().chunks(2).zip(positions) {
            if token[0] == b'0' {
                continue;
            }
            let button = match token[0] {
                b'3' => SideButton::LeftUpper,
                b'4' => SideButton::RightUpper,
                b'5' => SideButton::LeftLower,
                b'6' => SideButton::RightLower,
                _ => return Err(invalid_token(line, token)),
            };
            let lane = Lane::Side(button);
            self.add_note(
                line,
                Note::new(position, lane, NoteKind::Tap(TapKind::Tap))
                    .map_err(|source| SusError::Chart { line, source })?,
            )?;
        }
        Ok(())
    }

    fn parse_side_long(
        &mut self,
        line: usize,
        lane: u8,
        channel: char,
        data: &str,
        positions: Vec<Position>,
    ) -> Result<(), SusError> {
        for (token, position) in data.as_bytes().chunks(2).zip(positions) {
            if token[0] == b'0' {
                continue;
            }
            let kind = token[0];
            if kind == b'1' {
                let button = side_button(lane).ok_or_else(|| SusError::InvalidValue {
                    line,
                    value: lane.to_string(),
                })?;
                if self.pending_side_longs.contains_key(&channel) {
                    return Err(SusError::InvalidValue {
                        line,
                        value: channel.to_string(),
                    });
                }
                self.pending_side_longs.insert(
                    channel,
                    PendingSlide {
                        points: vec![SlidePoint::new(position, Lane::Side(button))],
                        side_button: Some(button),
                    },
                );
            } else if kind == b'2' {
                let pending = self
                    .pending_side_longs
                    .remove(&channel)
                    .ok_or(SusError::MissingStart { line, channel })?;
                let point = SlidePoint::new(position, Lane::Side(pending.side_button.unwrap()));
                self.add_note(
                    line,
                    Note::new(
                        pending.points[0].position(),
                        pending.points[0].lane(),
                        NoteKind::Hold {
                            end: point.position(),
                        },
                    )
                    .map_err(|source| SusError::Chart { line, source })?,
                )?;
            } else if kind == b'3' {
                let pending = self
                    .pending_side_longs
                    .get_mut(&channel)
                    .ok_or(SusError::MissingStart { line, channel })?;
                pending.points.push(SlidePoint::new(
                    position,
                    Lane::Side(pending.side_button.unwrap()),
                ));
            } else {
                return Err(invalid_token(line, token));
            }
        }
        Ok(())
    }

    fn parse_slider(
        &mut self,
        line: usize,
        lane: u8,
        channel: char,
        data: &str,
        positions: Vec<Position>,
    ) -> Result<(), SusError> {
        for (token, position) in data.as_bytes().chunks(2).zip(positions) {
            if token[0] == b'0' {
                continue;
            }
            let width = base36_byte(token[1], line)?;
            let lane =
                Lane::slider(lane, width).map_err(|source| SusError::Chart { line, source })?;
            match token[0] {
                b'1' => {
                    if self.pending_sliders.contains_key(&channel) {
                        return Err(SusError::InvalidValue {
                            line,
                            value: channel.to_string(),
                        });
                    }
                    self.pending_sliders.insert(
                        channel,
                        PendingSlide {
                            points: vec![SlidePoint::new(position, lane)],
                            side_button: None,
                        },
                    );
                }
                b'2' => {
                    let pending = self
                        .pending_sliders
                        .remove(&channel)
                        .ok_or(SusError::MissingStart { line, channel })?;
                    let mut points = pending.points;
                    points.push(SlidePoint::new(position, lane));
                    let start = points[0].clone();
                    self.add_note(
                        line,
                        Note::new(start.position(), start.lane(), NoteKind::Slide { points })
                            .map_err(|source| SusError::Chart { line, source })?,
                    )?;
                }
                b'3'..=b'5' => {
                    let pending = self
                        .pending_sliders
                        .get_mut(&channel)
                        .ok_or(SusError::MissingStart { line, channel })?;
                    pending.points.push(SlidePoint::new(position, lane));
                }
                _ => return Err(invalid_token(line, token)),
            }
        }
        Ok(())
    }

    fn add_note(&mut self, line: usize, note: Note) -> Result<(), SusError> {
        let note_id = self.chart.add_note(note);
        self.chart
            .set_note_speed_group(note_id, self.current_speed_group)
            .map_err(|source| SusError::Chart { line, source })
    }
}

fn invalid_token(line: usize, token: &[u8]) -> SusError {
    SusError::InvalidValue {
        line,
        value: String::from_utf8_lossy(token).into_owned(),
    }
}

fn is_metadata(command: &str) -> bool {
    matches!(
        command.split_whitespace().next(),
        Some(
            "TITLE"
                | "ARTIST"
                | "DESIGNER"
                | "DIFFICULTY"
                | "PLAYLEVEL"
                | "SONGID"
                | "WAVE"
                | "WAVEOFFSET"
                | "JACKET"
        )
    )
}

fn base36(value: char, line: usize) -> Result<u8, SusError> {
    base36_byte(value as u8, line)
}

fn parse_group(line: usize, value: &str) -> Result<u32, SusError> {
    if value.len() != 2 {
        return Err(SusError::MalformedCommand { line });
    }
    let high = u32::from(base36_byte(value.as_bytes()[0], line)?);
    let low = u32::from(base36_byte(value.as_bytes()[1], line)?);
    Ok(high * 36 + low)
}

fn parse_measure_tick(line: usize, value: &str) -> Result<(u32, u64), SusError> {
    let (measure, tick) = value
        .split_once('\'')
        .ok_or(SusError::MalformedCommand { line })?;
    let measure = measure.parse::<u32>().map_err(|_| SusError::InvalidValue {
        line,
        value: measure.to_owned(),
    })?;
    let tick = tick.parse::<u64>().map_err(|_| SusError::InvalidValue {
        line,
        value: tick.to_owned(),
    })?;
    Ok((measure, tick))
}

fn base36_byte(value: u8, line: usize) -> Result<u8, SusError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'z' => Ok(value - b'a' + 10),
        b'A'..=b'Z' => Ok(value - b'A' + 10),
        _ => Err(SusError::InvalidValue {
            line,
            value: (value as char).to_string(),
        }),
    }
}

fn side_button(lane: u8) -> Option<SideButton> {
    match lane {
        0 | 1 => Some(SideButton::LeftUpper),
        2 | 3 => Some(SideButton::LeftLower),
        12 | 13 => Some(SideButton::RightLower),
        14 | 15 => Some(SideButton::RightUpper),
        _ => None,
    }
}

fn side_lane(button: SideButton) -> u8 {
    match button {
        SideButton::LeftUpper => 0,
        SideButton::LeftLower => 2,
        SideButton::RightLower => 12,
        SideButton::RightUpper => 14,
    }
}

fn side_direction(button: SideButton) -> char {
    match button {
        SideButton::LeftUpper => '3',
        SideButton::RightUpper => '4',
        SideButton::LeftLower => '5',
        SideButton::RightLower => '6',
    }
}

fn parse_position(value: &str) -> Result<Position, ()> {
    let (integer, fraction) = value.split_once('.').unwrap_or((value, ""));
    let integer = integer.parse::<u64>().map_err(|_| ())?;
    let fraction_value = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u64>().map_err(|_| ())?
    };
    let denominator = 10u64.checked_pow(fraction.len() as u32).ok_or(())?;
    Position::new(integer * denominator + fraction_value, denominator).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use chart::{
        Chart, Lane, Note, NoteKind, Position, ScrollScope, ScrollSpeedChange, SideButton, TapKind,
        TempoChange,
    };

    use super::{parse, write};

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
    fn parses_bpm_changes_and_side_longs() {
        let source = "#BPM01: 120\n#00008: 01\n#00120A: 14\n#00220A: 24\n";
        let chart = parse(source).expect("valid SUS");

        assert_eq!(chart.tempo_changes().len(), 1);
        assert_eq!(chart.notes().len(), 1);
        assert_eq!(chart.notes()[0].lane(), Lane::Side(SideButton::LeftUpper));
        assert!(matches!(chart.notes()[0].kind(), NoteKind::Hold { .. }));
    }

    #[test]
    fn parses_xlair_side_taps_and_relayed_holds() {
        let source = "#00150: 34\n#0015c: 64\n#00120A: 14\n#00220A: 34\n#00320A: 24";
        let chart = parse(source).expect("valid XLAIR SUS");

        assert_eq!(chart.notes().len(), 3);
        assert_eq!(chart.notes()[0].lane(), Lane::Side(SideButton::LeftUpper));
        assert_eq!(chart.notes()[1].lane(), Lane::Side(SideButton::RightLower));
        assert!(matches!(chart.notes()[2].kind(), NoteKind::Hold { .. }));
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
        let chart = parse("#00130A: 143g\n#0023cA: 24").expect("valid SUS");

        assert_eq!(chart.notes().len(), 1);
        let NoteKind::Slide { points } = chart.notes()[0].kind() else {
            panic!("expected a slide");
        };
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].lane(), Lane::slider(0, 4).unwrap());
        assert_eq!(points[1].lane(), Lane::slider(0, 16).unwrap());
        assert_eq!(points[2].lane(), Lane::slider(12, 4).unwrap());
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
    fn rejects_unsupported_measure_speed_commands() {
        let error = parse("#MEASUREHS 00").expect_err("unsupported measure speed state");
        assert!(matches!(error, super::SusError::UnsupportedCommand { .. }));
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
        assert!(sus.contains("#00008:"));
        assert!(sus.contains("#0001a:"));
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

        let sus = write(&chart).expect("valid speed output");
        assert!(sus.contains("#TIL0z: \"0'96:1.5\""));
        assert!(sus.contains("#HISPEED 0z"));
        let parsed = parse(&sus).expect("round-tripped speed output");
        assert_eq!(parsed.scroll_speed_changes(), chart.scroll_speed_changes());
        assert_eq!(parsed.note_speed_group(note_id).unwrap(), Some(35));
    }

    #[test]
    fn rejects_speed_changes_that_sus_cannot_represent() {
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

        let error = write(&chart).expect_err("duration is unsupported by SUS");
        assert_eq!(
            error,
            super::SusError::UnsupportedNote {
                note: "scroll speed duration".to_owned(),
            }
        );
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

        let sus = write(&chart).expect("valid XLAIR output");
        let parsed = parse(&sus).expect("round-tripped XLAIR output");
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

        let sus = write(&chart).expect("AIR is intentionally omitted in XLAIR output");
        let parsed = parse(&sus).expect("valid XLAIR output");
        assert_eq!(parsed.notes().len(), 1);
        assert_eq!(parsed.notes()[0].kind(), &NoteKind::Tap(TapKind::Tap));
    }
}
