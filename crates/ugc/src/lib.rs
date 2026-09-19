//! Parser for the timing and basic-note subset of the UGC chart format.

use chart::{
    Chart, ChartError, Lane, MeasureTimeline, Note, NoteKind, Position, ScrollScope,
    ScrollSpeedChange, SlidePoint, TapKind, TempoChange,
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

/// Writes the basic central-lane chart model as a fixed 4/4 UGC document.
pub fn write(chart: &Chart) -> Result<String, UgcError> {
    if !chart.scroll_speed_changes().is_empty() {
        return Err(UgcError::UnsupportedNote {
            note: "scroll speed changes".to_owned(),
        });
    }

    let mut tempo_records = Vec::new();
    for tempo in chart.tempo_changes() {
        let (measure, tick) = output_position(tempo.position())?;
        tempo_records.push((
            measure,
            tick,
            format!("@BPM\t{measure}'{tick}\t{:.6}\n", tempo.bpm()),
        ));
    }

    let mut note_records = Vec::new();
    for note in chart.notes() {
        note_records.push(write_note(note)?);
    }
    tempo_records.sort_by_key(|(measure, tick, _)| (*measure, *tick));
    note_records.sort_by_key(|record| (record.measure, record.tick));

    let mut output = String::from("@VER\t8\n@EXVER\t1\n@TICKS\t480\n@BEAT\t0\t4\t4\n");
    for (_, _, text) in tempo_records {
        output.push_str(&text);
    }
    output.push_str("@ENDHEAD\n");
    for record in note_records {
        output.push_str(&record.text);
    }
    Ok(output)
}

struct OutputRecord {
    measure: u32,
    tick: u64,
    text: String,
}

fn write_note(note: &Note) -> Result<OutputRecord, UgcError> {
    let (measure, tick) = output_position(note.position())?;
    let (lane, width) = central_lane(note.lane())?;
    let prefix = format!("#{measure}'{tick}:");
    let text = match note.kind() {
        NoteKind::Tap(kind) => {
            let type_code = match kind {
                TapKind::Tap => 't',
                TapKind::XTap => 'x',
                TapKind::Flick => 'f',
            };
            format!(
                "{prefix}{type_code}{}{}\n",
                encode_base36(lane),
                encode_base36(width)
            )
        }
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
        NoteKind::Mine
        | NoteKind::Air { .. }
        | NoteKind::AirHold { .. }
        | NoteKind::AirSlide { .. } => {
            return Err(UgcError::UnsupportedNote {
                note: "unsupported note kind".to_owned(),
            });
        }
    };
    Ok(OutputRecord {
        measure,
        tick,
        text,
    })
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

struct Parser {
    ticks_per_beat: u64,
    timeline: MeasureTimeline,
    chart: Chart,
    in_header: bool,
    current_speed_group: Option<u32>,
}

impl Parser {
    fn new() -> Self {
        Self {
            ticks_per_beat: 480,
            timeline: MeasureTimeline::new(Position::new(4, 1).unwrap()),
            chart: Chart::new(),
            in_header: true,
            current_speed_group: None,
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
                let note_id = self.chart.add_note(note);
                self.chart
                    .set_note_speed_group(note_id, self.current_speed_group)
                    .map_err(|source| UgcError::Chart {
                        line: line_number,
                        source,
                    })?;
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
            't' | 'x' | 'f' => {
                let lane = parse_lane(line, &code[1..3])?;
                let tap_kind = match kind {
                    't' => TapKind::Tap,
                    'x' => TapKind::XTap,
                    'f' => TapKind::Flick,
                    _ => unreachable!(),
                };
                let note = Note::new(position, lane, NoteKind::Tap(tap_kind))
                    .map_err(|source| UgcError::Chart { line, source })?;
                Ok((0, Some(note)))
            }
            'h' | 's' => self.parse_long_note(line, position, kind, &code[1..3], following),
            'a' | 'H' | 'S' | 'C' => Err(UgcError::UnsupportedRecord {
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

    fn parse_long_note(
        &self,
        line: usize,
        position: Position,
        kind: char,
        start_code: &str,
        following: &[&str],
    ) -> Result<(usize, Option<Note>), UgcError> {
        let start_lane = parse_lane(line, start_code)?;
        let mut points = vec![SlidePoint::new(position, start_lane)];
        let mut consumed = 0;
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
        Ok((consumed, Some(note)))
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
    use chart::{Chart, Lane, Note, NoteKind, Position, TapKind};

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
            Err(super::UgcError::UnsupportedRecord { .. })
        ));
        let speed = "@MAINTIL\t1\n@ENDHEAD\n";
        assert!(matches!(
            parse(speed),
            Err(super::UgcError::UnsupportedRecord { .. })
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
}
