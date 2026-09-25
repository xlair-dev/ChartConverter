use chart::{
    AirColor, AirCrushColor, AirCrushInterval, AirCrushPoint, AirDirection, AirPoint,
    AirProperties, Chart, ChartError, ChartMode, ExDirection, Lane, MeasureTimeline, Note, NoteId,
    NoteKind, Position, ScrollScope, ScrollSpeedChange, SideButton, SlidePoint, SlidePointKind,
    TapKind, TempoChange, report_loss,
};

use crate::{UgcError, syntax::is_xlair_side_air_direction};

pub(super) fn parse(source: &str, mode: ChartMode) -> Result<Chart, UgcError> {
    Parser::new(mode).parse(source)
}

pub(super) struct Parser {
    mode: ChartMode,
    ticks_per_beat: u64,
    timeline: MeasureTimeline,
    chart: Chart,
    in_header: bool,
    current_speed_group: Option<u32>,
    last_parent: Option<NoteId>,
    offset_measure: bool,
}

struct ParsedLongNote {
    consumed: usize,
    note: Option<Note>,
    air_points: Option<Vec<AirPoint>>,
}

impl Parser {
    fn new(mode: ChartMode) -> Self {
        Self {
            mode,
            ticks_per_beat: 480,
            timeline: MeasureTimeline::new(Position::new(4, 1).unwrap()),
            chart: Chart::new(),
            in_header: true,
            current_speed_group: None,
            last_parent: None,
            offset_measure: false,
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
                if let Some((carrier_id, direction)) = self.ex_long_carrier(&note) {
                    let note = ex_long_note(note, direction).map_err(|source| UgcError::Chart {
                        line: line_number,
                        source,
                    })?;
                    if self.has_air_child(carrier_id) {
                        let note_id = self.chart.add_note(note);
                        self.chart
                            .set_note_speed_group(note_id, self.current_speed_group)
                            .map_err(|source| UgcError::Chart {
                                line: line_number,
                                source,
                            })?;
                        self.last_parent = Some(note_id);
                    } else {
                        self.chart
                            .replace_note(carrier_id, note)
                            .map_err(|source| UgcError::Chart {
                                line: line_number,
                                source,
                            })?;
                        self.last_parent = Some(carrier_id);
                    }
                } else {
                    let is_air = matches!(
                        note.kind(),
                        NoteKind::Air { .. }
                            | NoteKind::AirHold { .. }
                            | NoteKind::AirSlide { .. }
                            | NoteKind::AirCrush { .. }
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
            }
            index += consumed + 1;
        }
        Ok(self.chart)
    }

    fn ex_long_carrier(&self, note: &Note) -> Option<(NoteId, ExDirection)> {
        if !matches!(note.kind(), NoteKind::Hold { .. } | NoteKind::Slide { .. }) {
            return None;
        }
        self.chart
            .notes()
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, carrier)| {
                (carrier.position() == note.position()
                    && carrier.lane() == note.lane()
                    && matches!(carrier.kind(), NoteKind::ExTap { .. }))
                .then(|| {
                    let direction = match carrier.kind() {
                        NoteKind::ExTap { direction } => *direction,
                        _ => unreachable!(),
                    };
                    (NoteId::new(index as u32), direction)
                })
            })
    }

    fn has_air_child(&self, parent: NoteId) -> bool {
        self.chart.notes().iter().any(|note| {
            matches!(
                note.kind(),
                NoteKind::Air { parent: note_parent, .. }
                    | NoteKind::AirHold {
                        parent: note_parent,
                        ..
                    }
                    | NoteKind::AirSlide {
                        parent: note_parent,
                        ..
                    }
                    | NoteKind::AirCrush {
                        parent: note_parent,
                        ..
                    } if *note_parent == parent
            )
        })
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
            "@MAINBPM" => {
                let bpm = parse_speed(line, value)?;
                self.chart
                    .set_base_bpm(bpm)
                    .map_err(|source| UgcError::Chart { line, source })?;
            }
            "@FLAG" => self.parse_flag(line, value)?,
            "@SPDDEF" | "@SPDFLD" => {
                return Err(UgcError::UnsupportedRecord {
                    line,
                    record: tag.to_owned(),
                });
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

    fn parse_flag(&mut self, line: usize, value: &str) -> Result<(), UgcError> {
        let fields: Vec<_> = value.split_whitespace().collect();
        if fields.len() != 2 {
            return Err(UgcError::MalformedRecord { line });
        }
        if fields[0] == "SOFFSET" {
            self.offset_measure = parse_bool(line, fields[1])?;
        }
        Ok(())
    }

    fn parse_directive(&mut self, line: usize, text: &str) -> Result<(), UgcError> {
        let (tag, value) = split_directive(text)?;
        match tag {
            "@TIL" => self.parse_group_speed(line, value)?,
            "@SPDMOD" => self.parse_global_speed(line, value)?,
            "@USETIL" => self.parse_speed_group(line, value)?,
            "@SPDDEF" | "@SPDFLD" => {
                return Err(UgcError::UnsupportedRecord {
                    line,
                    record: tag.to_owned(),
                });
            }
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
        self.chart
            .set_measure_length(measure, length)
            .map_err(|source| UgcError::Chart { line, source })?;
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
        &mut self,
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
                    'x' if self.mode == ChartMode::Xlair => TapKind::XTap,
                    'x' => return self.parse_ex_tap(line, position, lane, &code[3..]),
                    'f' => TapKind::Flick {
                        direction: parse_flick_direction(line, &code[3..])?,
                    },
                    'd' => {
                        if self.mode == ChartMode::Xlair {
                            report_loss("UGC", "damage note in XLAIR mode");
                            return Ok((0, None));
                        }
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
                let parsed =
                    self.parse_long_note(line, position, kind, &code[1..3], following, None)?;
                Ok((parsed.consumed, parsed.note))
            }
            'a' => self.parse_air(line, position, &code[1..3], &code[3..]),
            'H' | 'S' => {
                self.parse_air_long(line, position, kind, &code[1..3], &code[3..], following)
            }
            'C' => self.parse_air_crush(line, position, &code[1..3], &code[3..], following),
            'T' => {
                report_loss("UGC", "unsupported legacy T note was omitted");
                let consumed = following
                    .iter()
                    .take_while(|follower| parse_follower(follower.trim()).is_some())
                    .count();
                Ok((consumed, None))
            }
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

    /// XLAIR reuses UGC's diagonal AIR records to encode side-button taps.
    fn parse_air(
        &mut self,
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
        if self.mode == ChartMode::Xlair && !is_xlair_side_air_direction(direction) {
            report_loss("UGC", "non-side AIR notation in XLAIR mode");
            return Ok((0, None));
        }
        if self.mode == ChartMode::Xlair {
            let button = match direction {
                AirDirection::UpperLeft => SideButton::LeftUpper,
                AirDirection::UpperRight => SideButton::RightUpper,
                AirDirection::LowerLeft => SideButton::LeftLower,
                AirDirection::LowerRight => SideButton::RightLower,
                _ => unreachable!(),
            };
            let side_note = Note::new(position, Lane::Side(button), NoteKind::Tap(TapKind::Tap))
                .map_err(|source| UgcError::Chart { line, source })?;
            let overlapping_tap =
                self.chart
                    .notes()
                    .iter()
                    .enumerate()
                    .find_map(|(index, note)| {
                        (note.position() == position
                            && lane.overlaps(note.lane())
                            && matches!(note.kind(), NoteKind::Tap(_)))
                        .then_some(NoteId::new(index as u32))
                    });
            if let Some(note_id) = overlapping_tap {
                self.chart
                    .replace_note(note_id, side_note)
                    .map_err(|source| UgcError::Chart { line, source })?;
                self.chart
                    .set_note_speed_group(note_id, self.current_speed_group)
                    .map_err(|source| UgcError::Chart { line, source })?;
                return Ok((0, None));
            }
            return Ok((0, Some(side_note)));
        }
        let properties =
            parse_air_properties(line, Some(direction), attributes.get(2..).unwrap_or(""))?;
        let note = Note::new(position, lane, NoteKind::Air { properties, parent })
            .map_err(|source| UgcError::Chart { line, source })?;
        Ok((0, Some(note)))
    }

    fn parse_air_crush(
        &mut self,
        line: usize,
        position: Position,
        lane_code: &str,
        attributes: &str,
        following: &[&str],
    ) -> Result<(usize, Option<Note>), UgcError> {
        if self.mode == ChartMode::Xlair {
            report_loss("UGC", "AIR Crush notation in XLAIR mode");
            let consumed = following
                .iter()
                .take_while(|follower| parse_follower(follower.trim()).is_some())
                .count();
            return Ok((consumed, None));
        }
        let parent = self.last_parent.ok_or(UgcError::InvalidValue {
            line,
            value: "AIR Crush without a parent note".to_owned(),
        })?;
        let lane = parse_lane(line, lane_code)?;
        let (attributes, interval) = attributes.split_once(',').unwrap_or((attributes, ""));
        if attributes.is_empty() {
            return Err(UgcError::MalformedRecord { line });
        }
        let color = parse_air_crush_color(
            line,
            attributes
                .get(attributes.len() - 1..)
                .ok_or(UgcError::MalformedRecord { line })?,
        )?;
        let height = parse_air_height(
            line,
            attributes
                .get(..attributes.len() - 1)
                .ok_or(UgcError::MalformedRecord { line })?,
        )?;
        let interval = if interval.is_empty() || interval == "$" {
            AirCrushInterval::Start
        } else if interval == "0" {
            AirCrushInterval::Trace
        } else {
            AirCrushInterval::Every(
                Position::new(parse_u64(line, interval)?, self.ticks_per_beat)
                    .map_err(|source| UgcError::Chart { line, source })?,
            )
        };
        let mut points = vec![
            AirCrushPoint::new(position, lane, height)
                .map_err(|source| UgcError::Chart { line, source })?,
        ];
        let mut consumed = 0;
        for follower in following {
            let follower = follower.trim();
            if follower.starts_with('@') {
                self.parse_directive(line + consumed + 1, follower)?;
                consumed += 1;
                continue;
            }
            let Some((offset, data)) = parse_follower(follower) else {
                break;
            };
            if data == "s" {
                let endpoint =
                    position
                        .checked_add(Position::new(offset, self.ticks_per_beat).map_err(
                            |source| UgcError::Chart {
                                line: line + consumed + 1,
                                source,
                            },
                        )?)
                        .map_err(|source| UgcError::Chart {
                            line: line + consumed + 1,
                            source,
                        })?;
                points.push(
                    AirCrushPoint::new(endpoint, lane, height).map_err(|source| {
                        UgcError::Chart {
                            line: line + consumed + 1,
                            source,
                        }
                    })?,
                );
                consumed += 1;
                continue;
            }
            if data.len() < 4 {
                return Err(UgcError::MalformedRecord {
                    line: line + consumed + 1,
                });
            }
            let marker = data.as_bytes()[0] as char;
            if marker != 'c' && marker != 's' {
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
            let point_lane = parse_lane(line + consumed + 1, &data[1..3])?;
            let point_height = parse_air_height(line + consumed + 1, &data[3..])?;
            points.push(
                AirCrushPoint::new(endpoint, point_lane, point_height).map_err(|source| {
                    UgcError::Chart {
                        line: line + consumed + 1,
                        source,
                    }
                })?,
            );
            consumed += 1;
        }
        if points.len() == 1 {
            return Err(UgcError::MissingFollower { line });
        }
        let note = Note::new(
            position,
            lane,
            NoteKind::AirCrush {
                points,
                color,
                interval,
                parent,
            },
        )
        .map_err(|source| UgcError::Chart { line, source })?;
        Ok((consumed, Some(note)))
    }

    fn parse_air_long(
        &mut self,
        line: usize,
        position: Position,
        kind: char,
        lane_code: &str,
        attributes: &str,
        following: &[&str],
    ) -> Result<(usize, Option<Note>), UgcError> {
        if self.mode == ChartMode::Xlair {
            report_loss("UGC", "AIR long notation in XLAIR mode");
            let consumed = following
                .iter()
                .take_while(|follower| parse_follower(follower.trim()).is_some())
                .count();
            return Ok((consumed, None));
        }
        let parent = self.last_parent.ok_or(UgcError::InvalidValue {
            line,
            value: "AIR long note without a parent note".to_owned(),
        })?;
        let properties = parse_air_properties(line, None, attributes)?;
        let parsed = self.parse_long_note(
            line,
            position,
            if kind == 'H' { 'h' } else { 's' },
            lane_code,
            following,
            properties.height(),
        )?;
        let consumed = parsed.consumed;
        let note = parsed.note;
        let air_points = parsed.air_points;
        let Some(note) = note else {
            return Ok((consumed, None));
        };
        let note_kind = match (kind, note.kind()) {
            ('H', NoteKind::Hold { end }) => NoteKind::AirHold {
                end: *end,
                properties,
                parent,
            },
            ('S', NoteKind::Slide { .. }) => NoteKind::AirSlide {
                points: air_points.ok_or(UgcError::MalformedRecord { line })?,
                color: properties.color(),
                parent,
            },
            _ => return Err(UgcError::MalformedRecord { line }),
        };
        let note = Note::new(note.position(), note.lane(), note_kind)
            .map_err(|source| UgcError::Chart { line, source })?;
        Ok((consumed, Some(note)))
    }

    fn parse_long_note(
        &mut self,
        line: usize,
        position: Position,
        kind: char,
        start_code: &str,
        following: &[&str],
        air_start_height: Option<f64>,
    ) -> Result<ParsedLongNote, UgcError> {
        let start_lane = parse_lane(line, start_code)?;
        let mut points = vec![SlidePoint::new(position, start_lane)];
        let mut consumed = 0;
        let mut air_points = if kind == 's' && air_start_height.is_some() {
            let height = air_start_height.ok_or(UgcError::MalformedRecord { line })?;
            Some(vec![
                AirPoint::new(position, start_lane, height)
                    .map_err(|source| UgcError::Chart { line, source })?,
            ])
        } else {
            None
        };
        for follower in following {
            let follower = follower.trim();
            if follower.is_empty() || follower.starts_with('\'') {
                consumed += 1;
                continue;
            }
            if follower.starts_with('@') {
                self.parse_directive(line + consumed + 1, follower)?;
                consumed += 1;
                continue;
            }
            let Some((offset, data)) = parse_follower(follower) else {
                break;
            };
            if kind == 'h' && (data == "s" || data == "c") {
                let endpoint =
                    position
                        .checked_add(Position::new(offset, self.ticks_per_beat).map_err(
                            |source| UgcError::Chart {
                                line: line + consumed + 1,
                                source,
                            },
                        )?)
                        .map_err(|source| UgcError::Chart {
                            line: line + consumed + 1,
                            source,
                        })?;
                points.push(SlidePoint::new(endpoint, start_lane));
                consumed += 1;
                continue;
            }
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
            let point_kind = if marker == 'c' {
                SlidePointKind::Control
            } else {
                SlidePointKind::Visible
            };
            if kind == 's' && air_points.is_some() {
                let height = parse_air_height(line + consumed + 1, &data[3..])?;
                if let Some(air_points) = &mut air_points {
                    air_points.push(
                        AirPoint::new(endpoint, lane, height)
                            .map_err(|source| UgcError::Chart {
                                line: line + consumed + 1,
                                source,
                            })?
                            .with_kind(point_kind.clone()),
                    );
                }
            }
            points.push(SlidePoint::new(endpoint, lane).with_kind(point_kind));
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
            let hold_lane = if self.mode == ChartMode::Xlair {
                match first_lane {
                    Lane::Slider { start, .. } => SideButton::from_xlair_lane_start(start)
                        .map(Lane::Side)
                        .unwrap_or(first_lane),
                    Lane::Side(_) => first_lane,
                }
            } else {
                first_lane
            };
            Note::new(
                position,
                hold_lane,
                NoteKind::Hold {
                    end: points.last().unwrap().position(),
                },
            )
        } else {
            Note::new(position, start_lane, NoteKind::Slide { points })
        }
        .map_err(|source| UgcError::Chart { line, source })?;
        Ok(ParsedLongNote {
            consumed,
            note: Some(note),
            air_points,
        })
    }

    fn position(&self, line: usize, measure: u32, tick: u64) -> Result<Position, UgcError> {
        let ticks_per_measure =
            self.ticks_per_beat
                .checked_mul(4)
                .ok_or_else(|| UgcError::InvalidValue {
                    line,
                    value: self.ticks_per_beat.to_string(),
                })?;
        let position = self
            .timeline
            .position(measure, tick, ticks_per_measure)
            .map_err(|source| UgcError::Chart { line, source })?;
        if self.offset_measure {
            let offset = self
                .timeline
                .position(1, 0, ticks_per_measure)
                .map_err(|source| UgcError::Chart { line, source })?;
            position
                .checked_add(offset)
                .map_err(|source| UgcError::Chart { line, source })
        } else {
            Ok(position)
        }
    }
}

fn ex_long_note(note: Note, direction: ExDirection) -> Result<Note, ChartError> {
    let position = note.position();
    let lane = note.lane();
    let kind = match note.kind().clone() {
        NoteKind::Hold { end } => NoteKind::ExHold { end, direction },
        NoteKind::Slide { points } => NoteKind::ExSlide { points, direction },
        _ => unreachable!("ExLong conversion requires a hold or slide"),
    };
    Note::new(position, lane, kind)
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

fn parse_air_crush_color(line: usize, value: &str) -> Result<AirCrushColor, UgcError> {
    match value {
        "0" => Ok(AirCrushColor::Normal),
        "Z" => Ok(AirCrushColor::Transparent),
        "1" => Ok(AirCrushColor::Red),
        "2" => Ok(AirCrushColor::Orange),
        "3" => Ok(AirCrushColor::Yellow),
        "4" => Ok(AirCrushColor::Lime),
        "5" => Ok(AirCrushColor::Green),
        "6" => Ok(AirCrushColor::Aqua),
        "7" => Ok(AirCrushColor::Cyan),
        "8" => Ok(AirCrushColor::DarkBlue),
        "9" => Ok(AirCrushColor::Blue),
        "A" => Ok(AirCrushColor::Violet),
        "Y" => Ok(AirCrushColor::Purple),
        "B" => Ok(AirCrushColor::Pink),
        "C" => Ok(AirCrushColor::Gray),
        "D" => Ok(AirCrushColor::Black),
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

fn parse_bool(line: usize, value: &str) -> Result<bool, UgcError> {
    match value {
        "TRUE" => Ok(true),
        "FALSE" => Ok(false),
        _ => Err(UgcError::InvalidValue {
            line,
            value: value.to_owned(),
        }),
    }
}

fn parse_u32(line: usize, value: &str) -> Result<u32, UgcError> {
    parse_u64(line, value)?
        .try_into()
        .map_err(|_| UgcError::InvalidValue {
            line,
            value: value.to_owned(),
        })
}
