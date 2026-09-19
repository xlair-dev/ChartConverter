//! Parser for the XLAIR-compatible subset of the SUS chart format.

use std::collections::HashMap;

use chart::{
    Chart, ChartError, Lane, Note, NoteKind, Position, SideButton, SlidePoint, TapKind, TempoChange,
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

/// Writes the basic central-lane chart model as a SUS document.
pub fn write(chart: &Chart) -> Result<String, SusError> {
    let mut records = Vec::new();
    let mut bpm_definitions = Vec::new();
    for (index, tempo) in chart.tempo_changes().iter().enumerate() {
        let (measure, tick) = output_position(tempo.position())?;
        let id = format!("{:02}", index + 1);
        bpm_definitions.push(format!("#BPM{id}: {}\n", tempo.bpm()));
        records.push(Record {
            measure,
            tick,
            key: "08".to_owned(),
            token: id,
        });
    }

    for (index, note) in chart.notes().iter().enumerate() {
        let channel = channel(index)?;
        match note.kind() {
            NoteKind::Tap(kind) => {
                let (measure, tick) = output_position(note.position())?;
                let lane = central_lane(note.lane(), "tap")?;
                let token = match kind {
                    TapKind::Tap => '1',
                    TapKind::XTap => '2',
                    TapKind::Flick => '3',
                };
                records.push(Record {
                    measure,
                    tick,
                    key: format!("1{}", base36_digit(lane)),
                    token: format!("{token}{}", base36_digit(lane_width(note.lane())?)),
                });
            }
            NoteKind::Hold { end } => {
                let (lane, width) = central_lane_parts(note.lane(), "hold")?;
                add_hold_records(&mut records, note.position(), *end, lane, width, channel)?;
            }
            NoteKind::Slide { points } => {
                add_slide_records(&mut records, points, channel)?;
            }
            NoteKind::Air { .. } | NoteKind::AirHold { .. } | NoteKind::AirSlide { .. } => {
                return Err(SusError::UnsupportedNote {
                    note: "air".to_owned(),
                });
            }
        }
    }

    records.sort_by_key(|record| (record.measure, record.tick, record.key.clone()));
    let mut output = String::from("#REQUEST \"ticks_per_beat 384\"\n#00002: 4\n");
    for definition in bpm_definitions {
        output.push_str(&definition);
    }
    for record in records {
        output.push_str(&format_record(&record)?);
    }
    Ok(output)
}

struct Record {
    measure: u32,
    tick: u64,
    key: String,
    token: String,
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
    });
    records.push(Record {
        measure: end_measure,
        tick: end_tick,
        key: format!("3{lane}{channel}"),
        token: format!("2{width}"),
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

fn central_lane_parts(lane: Lane, note: &str) -> Result<(u8, u8), SusError> {
    match lane {
        Lane::Slider { start, width } => Ok((start, width)),
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

struct Parser {
    timeline: chart::MeasureTimeline,
    bpm_definitions: HashMap<String, f64>,
    pending_side_longs: HashMap<char, PendingSlide>,
    pending_sliders: HashMap<char, PendingSlide>,
    chart: Chart,
}

impl Parser {
    fn new() -> Self {
        Self {
            timeline: chart::MeasureTimeline::new(Position::new(4, 1).unwrap()),
            bpm_definitions: HashMap::new(),
            pending_side_longs: HashMap::new(),
            pending_sliders: HashMap::new(),
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
                continue;
            }
            if command.starts_with("TIL")
                || command.starts_with("HISPEED")
                || command.starts_with("NOSPEED")
                || command.starts_with("MEASUREHS")
                || command.starts_with("MEASUREBS")
            {
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
            '5' => self.parse_directional_notes(line),
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
                b'3' => TapKind::Flick,
                _ => return Err(invalid_token(line, token)),
            };
            let lane = Lane::slider(lane, base36_byte(token[1], line)?)
                .map_err(|source| SusError::Chart { line, source })?;
            self.chart.add_note(
                Note::new(position, lane, NoteKind::Tap(kind))
                    .map_err(|source| SusError::Chart { line, source })?,
            );
        }
        Ok(())
    }

    fn parse_directional_notes(&mut self, line: usize) -> Result<(), SusError> {
        Err(SusError::UnsupportedCommand {
            line,
            command: "directional notes".to_owned(),
        })
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
                self.chart.add_note(
                    Note::new(
                        pending.points[0].position(),
                        pending.points[0].lane(),
                        NoteKind::Hold {
                            end: point.position(),
                        },
                    )
                    .map_err(|source| SusError::Chart { line, source })?,
                );
            } else if kind == b'3' {
                return Err(SusError::UnsupportedCommand {
                    line,
                    command: "side relay".to_owned(),
                });
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
                    self.chart.add_note(
                        Note::new(start.position(), start.lane(), NoteKind::Slide { points })
                            .map_err(|source| SusError::Chart { line, source })?,
                    );
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
    use chart::{Chart, Lane, Note, NoteKind, Position, SideButton, TapKind, TempoChange};

    use super::{parse, write};

    #[test]
    fn parses_short_notes_and_variable_measure_lengths() {
        let chart = parse("#00002: 3.5\n#00110: 12\n#00114: 0031\n").expect("valid SUS");

        assert_eq!(chart.notes().len(), 2);
        assert_eq!(chart.notes()[0].position(), Position::new(7, 2).unwrap());
        assert_eq!(chart.notes()[1].position(), Position::new(21, 4).unwrap());
        assert_eq!(chart.notes()[0].lane(), Lane::slider(0, 2).unwrap());
        assert_eq!(chart.notes()[1].kind(), &NoteKind::Tap(TapKind::Flick));
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
    fn rejects_speed_commands_until_the_model_can_represent_them() {
        let error = parse("#HISPEED 00").expect_err("unsupported speed state");
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
}
