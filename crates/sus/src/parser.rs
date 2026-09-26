use std::collections::HashMap;

use chart::{
    AirDirection, AirProperties, Chart, ChartMode, ExDirection, Lane, Note, NoteAttributes, NoteId,
    NoteKind, Position, ScrollScope, ScrollSpeedChange, SideButton, SlidePoint, SlidePointKind,
    TapKind, TempoChange, report_loss,
};

use crate::{
    SusError,
    syntax::{note_end_position, standard_parent_code},
};

pub(super) fn parse(source: &str, mode: ChartMode) -> Result<Chart, SusError> {
    Parser::new(mode).parse(source)
}

struct PendingSlide {
    points: Vec<SlidePoint>,
    side_button: Option<SideButton>,
}

struct PendingSlider {
    points: Vec<SlidePoint>,
}

struct SliderEvent {
    line: usize,
    channel: char,
    position: Position,
    lane: Lane,
    kind: u8,
    attributes: NoteAttributes,
    speed_group: Option<u32>,
}

enum PendingStandardAir {
    Tap {
        position: Position,
        lane: Lane,
        direction: AirDirection,
        target: Option<String>,
        speed_group: Option<u32>,
        attributes: NoteAttributes,
    },
    Hold {
        position: Position,
        lane: Lane,
        end: Position,
        target: Option<String>,
        speed_group: Option<u32>,
        attributes: NoteAttributes,
    },
}

pub(super) struct Parser {
    mode: ChartMode,
    timeline: chart::MeasureTimeline,
    bpm_definitions: HashMap<String, f64>,
    pending_side_longs: HashMap<char, PendingSlide>,
    pending_holds: HashMap<char, PendingSlide>,
    pending_sliders: HashMap<char, PendingSlider>,
    slider_events: Vec<SliderEvent>,
    ticks_per_beat: u64,
    measure_base: u32,
    current_speed_group: Option<u32>,
    attribute_definitions: HashMap<String, NoteAttributes>,
    current_attributes: Option<NoteAttributes>,
    chart: Chart,
    pending_standard_air: Vec<PendingStandardAir>,
    /// Geometry retained so later central taps are suppressed regardless of record order.
    side_tap_regions: Vec<(Position, Lane)>,
}

impl Parser {
    fn new(mode: ChartMode) -> Self {
        Self {
            mode,
            timeline: chart::MeasureTimeline::new(Position::new(4, 1).unwrap()),
            bpm_definitions: HashMap::new(),
            pending_side_longs: HashMap::new(),
            pending_holds: HashMap::new(),
            pending_sliders: HashMap::new(),
            slider_events: Vec::new(),
            ticks_per_beat: 480,
            measure_base: 0,
            current_speed_group: None,
            attribute_definitions: HashMap::new(),
            current_attributes: None,
            chart: Chart::new(),
            pending_standard_air: Vec::new(),
            side_tap_regions: Vec::new(),
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
            if command.starts_with("BPM_DEF") {
                self.parse_bpm_default(line, command)?;
                continue;
            }
            if command.starts_with("BASEBPM") {
                let value = command
                    .split_whitespace()
                    .nth(1)
                    .ok_or(SusError::MalformedCommand { line })?;
                let bpm = value.parse::<f64>().map_err(|_| SusError::InvalidValue {
                    line,
                    value: value.to_owned(),
                })?;
                self.chart
                    .set_base_bpm(bpm)
                    .map_err(|source| SusError::Chart { line, source })?;
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
            if command.starts_with("MEASUREBS") {
                let value = command
                    .split_whitespace()
                    .nth(1)
                    .ok_or(SusError::MalformedCommand { line })?;
                self.measure_base = value.parse().map_err(|_| SusError::InvalidValue {
                    line,
                    value: value.to_owned(),
                })?;
                continue;
            }
            if command.starts_with("MEASUREHS") {
                // The IR has no barline rendering state, so this display-only command is ignored.
                let value = command
                    .split_whitespace()
                    .nth(1)
                    .ok_or(SusError::MalformedCommand { line })?;
                parse_group(line, value)?;
                continue;
            }
            if command.starts_with("ATR") {
                self.parse_attribute_definition(line, command)?;
                continue;
            }
            if command.starts_with("ATTRIBUTE") {
                self.apply_attribute(line, command)?;
                continue;
            }
            if command == "NOATTRIBUTE" {
                self.current_attributes = None;
                continue;
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
            } else if header.len() == 5
                && header.chars().all(|c| c.is_ascii_hexdigit())
                && !header.ends_with("08")
                && (data.len() >= 6 || (data.len() == 4 && &data[..2] == "08"))
                && u8::from_str_radix(&data[..2], 16).is_ok_and(|code| {
                    matches!(
                        code,
                        0x01 | 0x02 | 0x03 | 0x05 | 0x06 | 0x07 | 0x08 | 0x09 | 0x10
                    )
                })
            {
                self.parse_standard_note(line, header, &data)?;
            } else if header.len() >= 3 && header[..3].chars().all(|c| c.is_ascii_digit()) {
                self.parse_data_line(line, header, &data)?;
            }
        }

        if let Some(channel) = self.pending_side_longs.keys().next().copied() {
            return Err(SusError::MissingEnd { channel });
        }
        if let Some(channel) = self.pending_holds.keys().next().copied() {
            return Err(SusError::MissingEnd { channel });
        }
        self.resolve_sliders()?;
        if let Some(channel) = self.pending_sliders.keys().next().copied() {
            return Err(SusError::MissingEnd { channel });
        }
        self.resolve_standard_air()?;
        Ok(self.chart)
    }

    fn resolve_standard_air(&mut self) -> Result<(), SusError> {
        for pending in self.pending_standard_air.drain(..) {
            let (position, lane, target, kind, speed_group, attributes) = match pending {
                PendingStandardAir::Tap {
                    position,
                    lane,
                    direction,
                    target,
                    speed_group,
                    attributes,
                } => (
                    position,
                    lane,
                    target,
                    NoteKind::Air {
                        properties: AirProperties::new(direction),
                        parent: NoteId::new(0),
                    },
                    speed_group,
                    attributes,
                ),
                PendingStandardAir::Hold {
                    position,
                    lane,
                    end,
                    target,
                    speed_group,
                    attributes,
                } => (
                    position,
                    lane,
                    target,
                    NoteKind::AirHold {
                        end,
                        properties: AirProperties::without_direction(),
                        parent: NoteId::new(0),
                    },
                    speed_group,
                    attributes,
                ),
            };
            let parent = self
                .chart
                .notes()
                .iter()
                .enumerate()
                .rev()
                .find(|(_, note)| {
                    (note_end_position(note) == position
                        || (note.position() == position && note.lane() == lane))
                        && target
                            .as_deref()
                            .is_none_or(|target| standard_parent_code(note) == target)
                        && !matches!(
                            note.kind(),
                            NoteKind::Air { .. }
                                | NoteKind::AirHold { .. }
                                | NoteKind::AirSlide { .. }
                                | NoteKind::AirCrush { .. }
                        )
                })
                .map(|(index, _)| NoteId::new(index as u32))
                .ok_or_else(|| SusError::InvalidValue {
                    line: 0,
                    value: format!(
                        "standard SUS AIR without a parent at {position:?}, lane {lane:?}, target {target:?}"
                    ),
                })?;
            let kind = match kind {
                NoteKind::Air { properties, .. } => NoteKind::Air { properties, parent },
                NoteKind::AirHold {
                    end, properties, ..
                } => NoteKind::AirHold {
                    end,
                    properties,
                    parent,
                },
                _ => unreachable!(),
            };
            let note_id = self.chart.add_note(
                Note::new(position, lane, kind)
                    .map_err(|source| SusError::Chart { line: 0, source })?
                    .with_attributes(attributes),
            );
            self.chart
                .set_note_speed_group(note_id, speed_group)
                .map_err(|source| SusError::Chart { line: 0, source })?;
        }
        Ok(())
    }

    fn parse_standard_note(
        &mut self,
        line: usize,
        header: &str,
        data: &str,
    ) -> Result<(), SusError> {
        if data.len() == 4 && &data[..2] == "08" {
            let bpm =
                *self
                    .bpm_definitions
                    .get(&data[2..])
                    .ok_or_else(|| SusError::InvalidValue {
                        line,
                        value: data.to_owned(),
                    })?;
            let position = self
                .timeline
                .position(
                    self.measure_base
                        .checked_add(
                            u32::from_str_radix(&header[..2], 16)
                                .map_err(|_| SusError::MalformedCommand { line })?,
                        )
                        .ok_or(SusError::InvalidValue {
                            line,
                            value: header[..2].to_owned(),
                        })?,
                    u64::from_str_radix(&header[2..], 16)
                        .map_err(|_| SusError::MalformedCommand { line })?,
                    self.ticks_per_beat
                        .checked_mul(4)
                        .ok_or(SusError::InvalidValue {
                            line,
                            value: self.ticks_per_beat.to_string(),
                        })?,
                )
                .map_err(|source| SusError::Chart { line, source })?;
            self.chart.add_tempo_change(
                TempoChange::new(position, bpm)
                    .map_err(|source| SusError::Chart { line, source })?,
            );
            return Ok(());
        }
        let measure = self
            .measure_base
            .checked_add(
                u32::from_str_radix(&header[..2], 16)
                    .map_err(|_| SusError::MalformedCommand { line })?,
            )
            .ok_or(SusError::InvalidValue {
                line,
                value: header[..2].to_owned(),
            })?;
        let tick = u64::from_str_radix(&header[2..], 16)
            .map_err(|_| SusError::MalformedCommand { line })?;
        let ticks_per_measure =
            self.ticks_per_beat
                .checked_mul(4)
                .ok_or(SusError::InvalidValue {
                    line,
                    value: self.ticks_per_beat.to_string(),
                })?;
        let position = self
            .timeline
            .position(measure, tick, ticks_per_measure)
            .map_err(|source| SusError::Chart { line, source })?;
        let type_code = u8::from_str_radix(&data[..2], 16).map_err(|_| SusError::InvalidValue {
            line,
            value: data.to_owned(),
        })?;
        let lane = u8::from_str_radix(&data[2..4], 16).map_err(|_| SusError::InvalidValue {
            line,
            value: data.to_owned(),
        })? / 2;
        let width = (u8::from_str_radix(&data[4..6], 16).map_err(|_| SusError::InvalidValue {
            line,
            value: data.to_owned(),
        })? / 2)
            .max(1);
        let lane = Lane::slider(lane, width).map_err(|source| SusError::Chart { line, source })?;
        match type_code {
            0x01 => self.add_note(
                line,
                Note::new(position, lane, NoteKind::Tap(TapKind::Tap))
                    .map_err(|source| SusError::Chart { line, source })?,
            ),
            0x02 => {
                let kind = if self.mode == ChartMode::Xlair {
                    NoteKind::Tap(TapKind::XTap)
                } else {
                    NoteKind::ExTap {
                        direction: ExDirection::Up,
                    }
                };
                self.add_note(
                    line,
                    Note::new(position, lane, kind)
                        .map_err(|source| SusError::Chart { line, source })?,
                )
            }
            0x03 => self.add_note(
                line,
                Note::new(
                    position,
                    lane,
                    NoteKind::Tap(TapKind::Flick { direction: None }),
                )
                .map_err(|source| SusError::Chart { line, source })?,
            ),
            0x10 if self.mode == ChartMode::Xlair => {
                report_loss("SUS", "damage note in XLAIR mode");
                Ok(())
            }
            0x10 => self.add_note(
                line,
                Note::new(position, lane, NoteKind::Mine)
                    .map_err(|source| SusError::Chart { line, source })?,
            ),
            0x05 => self.parse_standard_hold(line, position, lane, data),
            0x06 => self.parse_standard_slide(line, position, lane, data),
            0x07 | 0x09 if self.mode == ChartMode::Xlair => {
                report_loss("SUS", "AIR notation in XLAIR mode");
                Ok(())
            }
            0x07 | 0x09 => {
                self.pending_standard_air.push(PendingStandardAir::Tap {
                    position,
                    lane,
                    direction: if type_code == 0x07 {
                        AirDirection::Up
                    } else {
                        AirDirection::Down
                    },
                    target: data
                        .get(6..)
                        .filter(|target| !target.is_empty())
                        .map(str::to_owned),
                    speed_group: self.current_speed_group,
                    attributes: self.current_attributes.unwrap_or_default(),
                });
                Ok(())
            }
            0x08 if self.mode == ChartMode::Xlair => {
                report_loss("SUS", "AIR notation in XLAIR mode");
                Ok(())
            }
            0x08 => self.parse_standard_air_hold(line, position, lane, data),
            _ => Err(SusError::InvalidValue {
                line,
                value: data.to_owned(),
            }),
        }
    }

    fn parse_standard_hold(
        &mut self,
        line: usize,
        position: Position,
        lane: Lane,
        data: &str,
    ) -> Result<(), SusError> {
        if data.len() < 10 {
            return Err(SusError::InvalidValue {
                line,
                value: data.to_owned(),
            });
        }
        let duration =
            u64::from_str_radix(&data[6..10], 16).map_err(|_| SusError::InvalidValue {
                line,
                value: data.to_owned(),
            })?;
        let ticks_per_measure =
            self.ticks_per_beat
                .checked_mul(4)
                .ok_or(SusError::InvalidValue {
                    line,
                    value: self.ticks_per_beat.to_string(),
                })?;
        let end = position
            .checked_add(
                Position::new(duration, ticks_per_measure / 4)
                    .map_err(|source| SusError::Chart { line, source })?,
            )
            .map_err(|source| SusError::Chart { line, source })?;
        self.add_note(
            line,
            Note::new(position, lane, NoteKind::Hold { end })
                .map_err(|source| SusError::Chart { line, source })?,
        )
    }

    fn parse_standard_slide(
        &mut self,
        line: usize,
        position: Position,
        lane: Lane,
        data: &str,
    ) -> Result<(), SusError> {
        if data.len() < 14 {
            return Err(SusError::InvalidValue {
                line,
                value: data.to_owned(),
            });
        }
        let ticks_per_measure =
            self.ticks_per_beat
                .checked_mul(4)
                .ok_or(SusError::InvalidValue {
                    line,
                    value: self.ticks_per_beat.to_string(),
                })?;
        let duration =
            u64::from_str_radix(&data[6..10], 16).map_err(|_| SusError::InvalidValue {
                line,
                value: data.to_owned(),
            })?;
        let end = position
            .checked_add(
                Position::new(duration, ticks_per_measure / 4)
                    .map_err(|source| SusError::Chart { line, source })?,
            )
            .map_err(|source| SusError::Chart { line, source })?;
        let end_lane =
            u8::from_str_radix(&data[10..12], 16).map_err(|_| SusError::InvalidValue {
                line,
                value: data.to_owned(),
            })? / 2;
        let end_width =
            (u8::from_str_radix(&data[12..14], 16).map_err(|_| SusError::InvalidValue {
                line,
                value: data.to_owned(),
            })? / 2)
                .max(1);
        let end_lane =
            Lane::slider(end_lane, end_width).map_err(|source| SusError::Chart { line, source })?;
        if let Some((note_id, _)) = self
            .chart
            .notes()
            .iter()
            .enumerate()
            .rev()
            .find(|(_, note)| {
                matches!(
                    note.kind(),
                    NoteKind::Slide { points }
                        if points.last().is_some_and(|point| {
                            point.position() == position && point.lane() == lane
                        })
                )
            })
        {
            self.chart
                .append_slide_point(NoteId::new(note_id as u32), SlidePoint::new(end, end_lane))
                .map_err(|source| SusError::Chart { line, source })?;
            return Ok(());
        }
        self.add_note(
            line,
            Note::new(
                position,
                lane,
                NoteKind::Slide {
                    points: vec![
                        SlidePoint::new(position, lane),
                        SlidePoint::new(end, end_lane),
                    ],
                },
            )
            .map_err(|source| SusError::Chart { line, source })?,
        )
    }

    fn parse_standard_air_hold(
        &mut self,
        line: usize,
        position: Position,
        lane: Lane,
        data: &str,
    ) -> Result<(), SusError> {
        if data.len() < 10 {
            return Err(SusError::InvalidValue {
                line,
                value: data.to_owned(),
            });
        }
        let ticks_per_measure =
            self.ticks_per_beat
                .checked_mul(4)
                .ok_or(SusError::InvalidValue {
                    line,
                    value: self.ticks_per_beat.to_string(),
                })?;
        let duration =
            u64::from_str_radix(&data[6..10], 16).map_err(|_| SusError::InvalidValue {
                line,
                value: data.to_owned(),
            })?;
        let end = position
            .checked_add(
                Position::new(duration, ticks_per_measure / 4)
                    .map_err(|source| SusError::Chart { line, source })?,
            )
            .map_err(|source| SusError::Chart { line, source })?;
        self.pending_standard_air.push(PendingStandardAir::Hold {
            position,
            lane,
            end,
            target: data
                .get(10..)
                .filter(|target| !target.is_empty())
                .map(str::to_owned),
            speed_group: self.current_speed_group,
            attributes: self.current_attributes.unwrap_or_default(),
        });
        Ok(())
    }

    fn parse_request(&mut self, line: usize, command: &str) -> Result<(), SusError> {
        let value = command.trim_start_matches("REQUEST").trim();
        let fields: Vec<_> = value.trim_matches('"').split_whitespace().collect();
        if fields.len() == 2 && fields[0] == "enable_priority" {
            if let Some(enabled) = match fields[1] {
                "0" | "false" => Some(false),
                "1" | "true" => Some(true),
                _ => None,
            } {
                self.chart.set_priority_enabled(enabled);
                return Ok(());
            }
            return Err(SusError::InvalidValue {
                line,
                value: fields[1].to_owned(),
            });
        }
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

    fn parse_attribute_definition(&mut self, line: usize, command: &str) -> Result<(), SusError> {
        let (header, value) = command
            .split_once(':')
            .ok_or(SusError::MalformedCommand { line })?;
        if header.len() != 5 || !header.starts_with("ATR") {
            return Err(SusError::MalformedCommand { line });
        }
        let attributes = parse_attributes(line, value.trim())?;
        self.attribute_definitions
            .insert(header[3..].to_owned(), attributes);
        Ok(())
    }

    fn apply_attribute(&mut self, line: usize, command: &str) -> Result<(), SusError> {
        let fields: Vec<_> = command.split_whitespace().collect();
        if fields.len() != 2 {
            return Err(SusError::MalformedCommand { line });
        }
        let attributes = self
            .attribute_definitions
            .get(fields[1])
            .copied()
            .ok_or_else(|| SusError::InvalidValue {
                line,
                value: fields[1].to_owned(),
            })?;
        self.current_attributes = Some(attributes);
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

    fn parse_bpm_default(&mut self, line: usize, command: &str) -> Result<(), SusError> {
        let value = command
            .trim_start_matches("BPM_DEF")
            .trim()
            .trim_matches('"');
        let bpm = value.parse::<f64>().map_err(|_| SusError::InvalidValue {
            line,
            value: value.to_owned(),
        })?;
        self.chart.add_tempo_change(
            TempoChange::new(Position::new(0, 1).expect("valid position"), bpm)
                .map_err(|source| SusError::Chart { line, source })?,
        );
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
            let denominator = if self.mode == ChartMode::Normal {
                self.ticks_per_beat
                    .checked_mul(4)
                    .ok_or(SusError::InvalidValue {
                        line,
                        value: self.ticks_per_beat.to_string(),
                    })?
            } else {
                self.ticks_per_beat
            };
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
        let measure = self
            .measure_base
            .checked_add(
                header[..3]
                    .parse::<u32>()
                    .map_err(|_| SusError::MalformedCommand { line })?,
            )
            .ok_or(SusError::InvalidValue {
                line,
                value: header[..3].to_owned(),
            })?;
        let kind = &header[3..];
        if kind == "02" {
            let length = parse_position(data).map_err(|_| SusError::InvalidValue {
                line,
                value: data.to_owned(),
            })?;
            self.timeline.set_length(measure, length);
            self.chart
                .set_measure_length(measure, length)
                .map_err(|source| SusError::Chart { line, source })?;
            return Ok(());
        }
        if kind == "08" {
            return self.parse_bpm_change(line, measure, data);
        }
        if kind.len() != 2 && kind.len() != 3 {
            return Err(SusError::UnsupportedCommand {
                line,
                command: format!("#{header}: {data}"),
            });
        }

        let channel = kind.chars().nth(2);
        let note_kind = kind.chars().next().unwrap();
        let lane = base36(kind.chars().nth(1).unwrap(), line)?;
        let positions = self.positions(line, measure, data)?;
        match note_kind {
            '1' => self.parse_short_notes(line, lane, data, positions),
            '2' if self.mode == ChartMode::Xlair => {
                self.parse_side_long(line, lane, channel.unwrap(), data, positions)
            }
            '2' => self.parse_central_hold(line, lane, channel.unwrap(), data, positions),
            '3' => self.parse_slider(line, lane, channel.unwrap(), data, positions),
            '4' => self.parse_slider(line, lane, channel.unwrap(), data, positions),
            '5' => self.parse_directional_notes(line, lane, data, positions),
            _ => Err(SusError::UnsupportedCommand {
                line,
                command: format!("#{header}: {data}"),
            }),
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
            if self.mode == ChartMode::Xlair && matches!(token[0], b'4'..=b'6') {
                report_loss("SUS", "unsupported tap variant in XLAIR mode");
                continue;
            }
            let kind = match token[0] {
                b'1' => TapKind::Tap,
                b'2' => TapKind::XTap,
                b'3' => TapKind::Flick { direction: None },
                b'4' => TapKind::Tap4,
                b'5' => TapKind::Tap5,
                b'6' => TapKind::Tap6,
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
        lane: u8,
        data: &str,
        positions: Vec<Position>,
    ) -> Result<(), SusError> {
        for (token, position) in data.as_bytes().chunks(2).zip(positions) {
            if token[0] == b'0' {
                continue;
            }
            if self.mode == ChartMode::Normal {
                let direction = match token[0] {
                    b'1' => ExDirection::Up,
                    b'2' => ExDirection::Down,
                    b'3' => ExDirection::UpperLeft,
                    b'4' => ExDirection::UpperRight,
                    b'5' => ExDirection::LowerLeft,
                    b'6' => ExDirection::LowerRight,
                    _ => return Err(invalid_token(line, token)),
                };
                self.add_note(
                    line,
                    Note::new(
                        position,
                        Lane::slider(lane, base36_byte(token[1], line)?)
                            .map_err(|source| SusError::Chart { line, source })?,
                        NoteKind::ExTap { direction },
                    )
                    .map_err(|source| SusError::Chart { line, source })?,
                )?;
                continue;
            }
            if matches!(token[0], b'1' | b'2') {
                report_loss("SUS", "non-side AIR direction in XLAIR mode");
                continue;
            }
            let button = match token[0] {
                b'3' => SideButton::LeftUpper,
                b'4' => SideButton::RightUpper,
                b'5' => SideButton::LeftLower,
                b'6' => SideButton::RightLower,
                _ => return Err(invalid_token(line, token)),
            };
            let width = base36_byte(token[1], line)?;
            let source_lane =
                Lane::slider(lane, width).map_err(|source| SusError::Chart { line, source })?;
            self.side_tap_regions.push((position, source_lane));
            let side_note = Note::new(position, Lane::Side(button), NoteKind::Tap(TapKind::Tap))
                .map_err(|source| SusError::Chart { line, source })?
                .with_attributes(self.current_attributes.unwrap_or_default());
            let overlapping_tap =
                self.chart
                    .notes()
                    .iter()
                    .enumerate()
                    .find_map(|(index, note)| {
                        (note.position() == position
                            && source_lane.overlaps(note.lane())
                            && matches!(note.kind(), NoteKind::Tap(_)))
                        .then_some(NoteId::new(index as u32))
                    });
            if let Some(note_id) = overlapping_tap {
                self.chart
                    .replace_note(note_id, side_note)
                    .map_err(|source| SusError::Chart { line, source })?;
                self.chart
                    .set_note_speed_group(note_id, self.current_speed_group)
                    .map_err(|source| SusError::Chart { line, source })?;
            } else {
                self.add_note(line, side_note)?;
            }
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

    fn parse_central_hold(
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
            let note_lane =
                Lane::slider(lane, width).map_err(|source| SusError::Chart { line, source })?;
            match token[0] {
                b'1' => {
                    if self.pending_holds.contains_key(&channel) {
                        return Err(SusError::InvalidValue {
                            line,
                            value: channel.to_string(),
                        });
                    }
                    self.pending_holds.insert(
                        channel,
                        PendingSlide {
                            points: vec![SlidePoint::new(position, note_lane)],
                            side_button: None,
                        },
                    );
                }
                b'2' => {
                    let pending = self
                        .pending_holds
                        .remove(&channel)
                        .ok_or(SusError::MissingStart { line, channel })?;
                    self.add_note(
                        line,
                        Note::new(
                            pending.points[0].position(),
                            pending.points[0].lane(),
                            NoteKind::Hold { end: position },
                        )
                        .map_err(|source| SusError::Chart { line, source })?,
                    )?;
                }
                b'3' => {
                    let pending = self
                        .pending_holds
                        .get_mut(&channel)
                        .ok_or(SusError::MissingStart { line, channel })?;
                    pending.points.push(SlidePoint::new(position, note_lane));
                }
                _ => return Err(invalid_token(line, token)),
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
            if !matches!(token[0], b'1'..=b'5') {
                return Err(invalid_token(line, token));
            }
            self.slider_events.push(SliderEvent {
                line,
                channel,
                position,
                lane,
                kind: token[0],
                attributes: self.current_attributes.unwrap_or_default(),
                speed_group: self.current_speed_group,
            });
        }
        Ok(())
    }

    fn resolve_sliders(&mut self) -> Result<(), SusError> {
        self.slider_events
            .sort_by_key(|event| (event.position, event.line));
        for event in std::mem::take(&mut self.slider_events) {
            match event.kind {
                b'1' => {
                    if self.pending_sliders.contains_key(&event.channel) {
                        return Err(SusError::InvalidValue {
                            line: event.line,
                            value: event.channel.to_string(),
                        });
                    }
                    self.pending_sliders.insert(
                        event.channel,
                        PendingSlider {
                            points: vec![SlidePoint::new(event.position, event.lane)],
                        },
                    );
                }
                b'2' => {
                    let pending = self.pending_sliders.remove(&event.channel).ok_or(
                        SusError::MissingStart {
                            line: event.line,
                            channel: event.channel,
                        },
                    )?;
                    let mut points = pending.points;
                    points.push(SlidePoint::new(event.position, event.lane));
                    let start = points[0].clone();
                    let note =
                        Note::new(start.position(), start.lane(), NoteKind::Slide { points })
                            .map_err(|source| SusError::Chart {
                                line: event.line,
                                source,
                            })?
                            .with_attributes(event.attributes);
                    let note_id = self.chart.add_note(note);
                    self.chart
                        .set_note_speed_group(note_id, event.speed_group)
                        .map_err(|source| SusError::Chart {
                            line: event.line,
                            source,
                        })?;
                }
                b'3'..=b'5' => {
                    let pending = self.pending_sliders.get_mut(&event.channel).ok_or(
                        SusError::MissingStart {
                            line: event.line,
                            channel: event.channel,
                        },
                    )?;
                    let point_kind = match event.kind {
                        b'4' => SlidePointKind::Control,
                        b'5' => SlidePointKind::Invisible,
                        _ => SlidePointKind::Visible,
                    };
                    pending
                        .points
                        .push(SlidePoint::new(event.position, event.lane).with_kind(point_kind));
                }
                _ => unreachable!(),
            }
        }
        Ok(())
    }

    fn add_note(&mut self, line: usize, note: Note) -> Result<(), SusError> {
        if self.mode == ChartMode::Xlair
            && matches!(note.kind(), NoteKind::Tap(_))
            && self
                .side_tap_regions
                .iter()
                .any(|(position, lane)| *position == note.position() && lane.overlaps(note.lane()))
        {
            return Ok(());
        }
        let note_id = self
            .chart
            .add_note(note.with_attributes(self.current_attributes.unwrap_or_default()));
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
                | "SUBTITLE"
                | "ARTIST"
                | "GENRE"
                | "DESIGNER"
                | "DIFFICULTY"
                | "PLAYLEVEL"
                | "SONGID"
                | "WAVE"
                | "WAVEOFFSET"
                | "JACKET"
                | "BACKGROUND"
                | "MOVIE"
                | "MOVIEOFFSET"
                | "BASEBPM"
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

fn parse_attributes(line: usize, value: &str) -> Result<NoteAttributes, SusError> {
    let mut attributes = NoteAttributes::new();
    let value = value.trim().trim_matches('"');
    if value.is_empty() {
        return Ok(attributes);
    }
    for field in value.split(',') {
        let (key, raw_value) = field
            .trim()
            .split_once(':')
            .ok_or(SusError::MalformedCommand { line })?;
        let raw_value = raw_value.trim();
        attributes = match key.trim() {
            "rh" => attributes
                .with_roll_speed(raw_value.parse().map_err(|_| SusError::InvalidValue {
                    line,
                    value: raw_value.to_owned(),
                })?)
                .map_err(|_| SusError::InvalidValue {
                    line,
                    value: raw_value.to_owned(),
                })?,
            "h" => attributes
                .with_height(raw_value.parse().map_err(|_| SusError::InvalidValue {
                    line,
                    value: raw_value.to_owned(),
                })?)
                .map_err(|_| SusError::InvalidValue {
                    line,
                    value: raw_value.to_owned(),
                })?,
            "pr" => {
                attributes.with_priority(raw_value.parse().map_err(|_| SusError::InvalidValue {
                    line,
                    value: raw_value.to_owned(),
                })?)
            }
            _ => {
                return Err(SusError::InvalidValue {
                    line,
                    value: key.trim().to_owned(),
                });
            }
        };
    }
    Ok(attributes)
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
    SideButton::from_xlair_lane_start(lane)
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
