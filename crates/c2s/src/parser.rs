use chart::{
    AirCrushInterval, AirCrushPoint, AirProperties, Chart, ChartError, ChartMode, Lane, Note,
    NoteId, NoteKind, Position, ScrollScope, ScrollSpeedChange, SlidePoint, SlidePointKind,
    TapKind, TempoChange, report_loss,
};

use crate::{
    C2sError,
    syntax::{
        parse_air_color, parse_air_crush_color, parse_air_direction, parse_air_height,
        parse_ex_direction, parse_u8, parse_u32, parse_u64,
    },
    writer,
};

pub(super) fn parse(source: &str, mode: ChartMode) -> Result<Chart, C2sError> {
    Parser::new(mode).parse(source)
}

struct Parser {
    mode: ChartMode,
    resolution: Option<u64>,
    timeline: chart::MeasureTimeline,
    chart: Chart,
    last_ex_parent: Option<NoteId>,
    last_tap_parent: Option<NoteId>,
    last_flick_parent: Option<NoteId>,
    last_mine_parent: Option<NoteId>,
    last_hold_parent: Option<NoteId>,
    last_slide_parent: Option<NoteId>,
    speed_assignments: Vec<(usize, Position, Lane, u64, u32)>,
}

impl Parser {
    fn new(mode: ChartMode) -> Self {
        Self {
            mode,
            resolution: None,
            timeline: chart::MeasureTimeline::new(Position::new(4, 1).unwrap()),
            chart: Chart::new(),
            last_ex_parent: None,
            last_tap_parent: None,
            last_flick_parent: None,
            last_mine_parent: None,
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
                "HLD" => self.parse_hold(line, &fields, None)?,
                "HXD" => self.parse_hold(line, &fields, Some(6))?,
                "SLD" | "SLC" => self.parse_slide(line, &fields, None)?,
                "SXD" | "SXC" => self.parse_slide(line, &fields, Some(8))?,
                "AHD" | "AHX" => self.parse_air_hold(line, &fields)?,
                "ASC" | "ASD" => self.parse_air_slide(line, &fields)?,
                "SLA" => self.parse_speed_assignment(line, &fields)?,
                "SFL" | "SLP" => self.parse_scroll_speed(line, &fields)?,
                "ALD" => self.parse_air_crush(line, &fields)?,
                _ => {}
            }
        }

        if self.resolution.is_none() {
            return Err(C2sError::MissingResolution);
        }
        // C2S uses SLA duration as a lower bound; unmatched auxiliary records are ignorable.
        for index in 0..self.chart.notes().len() {
            let Some((line, group)) = self.speed_assignments.iter().find_map(
                |(line, position, lane, duration, group)| {
                    (self.chart.notes()[index].position() == *position
                        && self.chart.notes()[index].lane() == *lane
                        && writer::note_duration_ticks(&self.chart.notes()[index])
                            .ok()
                            .is_some_and(|note_duration| note_duration <= *duration))
                    .then_some((*line, *group))
                },
            ) else {
                continue;
            };
            self.chart
                .set_note_speed_group(NoteId::new(index as u32), Some(group))
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
        let _measure = parse_u32(
            line,
            fields.get(1).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let _tick = parse_u64(
            line,
            fields.get(2).ok_or(C2sError::MalformedRecord { line })?,
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
        // C2S MET changes display meter only; it does not change chart time.
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
            TapKind::Tap4 | TapKind::Tap5 | TapKind::Tap6 => {}
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
        let id = self.add_note(line, Note::new(position, lane, NoteKind::Mine))?;
        self.last_mine_parent = Some(id);
        Ok(())
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
        let kind = if self.mode == ChartMode::Xlair {
            NoteKind::Tap(TapKind::XTap)
        } else {
            NoteKind::ExTap { direction }
        };
        let id = self.add_note(line, Note::new(position, lane, kind))?;
        self.last_ex_parent = Some(id);
        Ok(())
    }

    fn parse_air(&mut self, line: usize, fields: &[&str]) -> Result<(), C2sError> {
        if self.mode == ChartMode::Xlair {
            report_loss("C2S", "AIR notation in XLAIR mode");
            return Ok(());
        }
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
        let color = parse_air_color(line, fields.get(6).copied().unwrap_or("DEF"))?;
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

    fn parse_hold(
        &mut self,
        line: usize,
        fields: &[&str],
        ex_direction_field: Option<usize>,
    ) -> Result<(), C2sError> {
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
        let kind = if self.mode == ChartMode::Xlair {
            NoteKind::Hold { end }
        } else if let Some(field) = ex_direction_field {
            NoteKind::ExHold {
                end,
                direction: parse_ex_direction(
                    line,
                    fields
                        .get(field)
                        .ok_or(C2sError::MalformedRecord { line })?,
                )?,
            }
        } else {
            NoteKind::Hold { end }
        };
        let id = self.add_note(line, Note::new(start, lane, kind))?;
        self.last_hold_parent = Some(id);
        Ok(())
    }

    fn parse_slide(
        &mut self,
        line: usize,
        fields: &[&str],
        ex_direction_field: Option<usize>,
    ) -> Result<(), C2sError> {
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
        let end_point = SlidePoint::new(end, end_lane).with_kind(if fields[0].ends_with('C') {
            SlidePointKind::Control
        } else {
            SlidePointKind::Visible
        });
        let direction = ex_direction_field
            .filter(|_| self.mode != ChartMode::Xlair)
            .map(|field| {
                parse_ex_direction(
                    line,
                    fields
                        .get(field)
                        .ok_or(C2sError::MalformedRecord { line })?,
                )
            })
            .transpose()?;
        if let Some(note_id) = self
            .chart
            .notes()
            .iter()
            .enumerate()
            .rev()
            .find(|(_, note)| {
                let points = match (direction.is_some(), note.kind()) {
                    (false, NoteKind::Slide { points }) => points,
                    (
                        true,
                        NoteKind::ExSlide {
                            points,
                            direction: existing,
                        },
                    ) if Some(*existing) == direction => points,
                    _ => return false,
                };
                points
                    .last()
                    .is_some_and(|point| point.position() == start && point.lane() == start_lane)
            })
            .map(|(index, _)| NoteId::new(index as u32))
        {
            self.chart
                .append_slide_point(note_id, end_point.clone())
                .map_err(|source| C2sError::Chart { line, source })?;
            self.last_slide_parent = Some(note_id);
            return Ok(());
        }
        let points = vec![SlidePoint::new(start, start_lane), end_point];
        let kind = direction.map_or(
            NoteKind::Slide {
                points: points.clone(),
            },
            |direction| NoteKind::ExSlide { points, direction },
        );
        let id = self.add_note(line, Note::new(start, start_lane, kind))?;
        self.last_slide_parent = Some(id);
        Ok(())
    }

    fn parse_air_hold(&mut self, line: usize, fields: &[&str]) -> Result<(), C2sError> {
        if self.mode == ChartMode::Xlair {
            report_loss("C2S", "AIR notation in XLAIR mode");
            return Ok(());
        }
        let (measure, tick) = self.location(line, fields)?;
        let lane = self.lane(
            line,
            fields.get(3).ok_or(C2sError::MalformedRecord { line })?,
            fields.get(4).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let parent = self.air_parent(line, fields.get(5))?;
        let duration = parse_u64(
            line,
            fields.get(6).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let color = parse_air_color(line, fields.get(7).copied().unwrap_or("DEF"))?;
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
        if self.mode == ChartMode::Xlair {
            report_loss("C2S", "AIR notation in XLAIR mode");
            return Ok(());
        }
        let (measure, tick) = self.location(line, fields)?;
        let start_lane = self.lane(
            line,
            fields.get(3).ok_or(C2sError::MalformedRecord { line })?,
            fields.get(4).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let is_continuation = fields
            .get(5)
            .is_some_and(|value| *value == "ASC" || *value == "ASD");
        let parent = (!is_continuation)
            .then(|| self.air_parent(line, fields.get(5)))
            .transpose()?;
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
        let points = vec![
            chart::AirPoint::new(start, start_lane, height)
                .map_err(|source| C2sError::Chart { line, source })?,
            chart::AirPoint::new(end, end_lane, end_height)
                .map_err(|source| C2sError::Chart { line, source })?,
        ];
        if is_continuation {
            let note_id = self
                .chart
                .notes()
                .iter()
                .enumerate()
                .rev()
                .find(|(_, note)| {
                    let NoteKind::AirSlide {
                        points,
                        color: existing_color,
                        ..
                    } = note.kind()
                    else {
                        return false;
                    };
                    existing_color == &color
                        && points.last().is_some_and(|point| {
                            point.position() == start && point.lane() == start_lane
                        })
                })
                .map(|(index, _)| NoteId::new(index as u32))
                .ok_or(C2sError::InvalidValue {
                    line,
                    value: "AIR Slide continuation without a parent".to_owned(),
                })?;
            self.chart
                .append_air_slide_point(note_id, points[1].clone())
                .map_err(|source| C2sError::Chart { line, source })?;
            return Ok(());
        }
        self.add_note(
            line,
            Note::new(
                start,
                start_lane,
                NoteKind::AirSlide {
                    points,
                    color,
                    parent: parent.ok_or(C2sError::MalformedRecord { line })?,
                },
            ),
        )?;
        Ok(())
    }

    fn parse_air_crush(&mut self, line: usize, fields: &[&str]) -> Result<(), C2sError> {
        if self.mode == ChartMode::Xlair {
            report_loss("C2S", "AIR notation in XLAIR mode");
            return Ok(());
        }
        let (measure, tick) = self.location(line, fields)?;
        let lane = self.lane(
            line,
            fields.get(3).ok_or(C2sError::MalformedRecord { line })?,
            fields.get(4).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let interval = parse_u64(
            line,
            fields.get(5).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let start_height = parse_air_height(
            line,
            fields.get(6).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let duration = parse_u64(
            line,
            fields.get(7).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let end_lane = self.lane(
            line,
            fields.get(8).ok_or(C2sError::MalformedRecord { line })?,
            fields.get(9).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let end_height = parse_air_height(
            line,
            fields.get(10).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let color = parse_air_crush_color(line, fields.get(11).copied().unwrap_or("DEF"))?;
        let start = self.position(line, measure, tick)?;
        let end = self.position(
            line,
            measure,
            tick.checked_add(duration).ok_or(C2sError::InvalidValue {
                line,
                value: duration.to_string(),
            })?,
        )?;
        let parent = self.air_crush_parent(line, start, lane)?;
        let interval = match interval {
            0 => AirCrushInterval::Trace,
            interval if interval < 9600 => AirCrushInterval::Every(
                Position::new(interval, 96).map_err(|source| C2sError::Chart { line, source })?,
            ),
            _ => AirCrushInterval::Start,
        };
        self.add_note(
            line,
            Note::new(
                start,
                lane,
                NoteKind::AirCrush {
                    points: vec![
                        AirCrushPoint::new(start, lane, start_height)
                            .map_err(|source| C2sError::Chart { line, source })?,
                        AirCrushPoint::new(end, end_lane, end_height)
                            .map_err(|source| C2sError::Chart { line, source })?,
                    ],
                    color,
                    interval,
                    parent,
                },
            ),
        )?;
        Ok(())
    }

    fn air_crush_parent(
        &self,
        line: usize,
        start: Position,
        _lane: Lane,
    ) -> Result<NoteId, C2sError> {
        // C2S ALD records do not encode their parent; retain the preceding visible note
        // required by the IR so the AIR Crush survives conversion to formats with parents.
        self.chart
            .notes()
            .iter()
            .enumerate()
            .rev()
            .find(|(_, note)| {
                !matches!(
                    note.kind(),
                    NoteKind::Air { .. }
                        | NoteKind::AirHold { .. }
                        | NoteKind::AirSlide { .. }
                        | NoteKind::AirCrush { .. }
                ) && note.position() <= start
            })
            .map(|(index, _)| NoteId::new(index as u32))
            .ok_or(C2sError::InvalidValue {
                line,
                value: "AIR Crush without a parent note".to_owned(),
            })
    }

    fn air_parent(&self, line: usize, value: Option<&&str>) -> Result<NoteId, C2sError> {
        match value.ok_or(C2sError::MalformedRecord { line })? {
            &"TAP" => self.last_tap_parent,
            &"CHR" => self.last_ex_parent,
            &"FLK" => self.last_flick_parent,
            &"MNE" => self.last_mine_parent,
            &"HLD" => self.last_hold_parent,
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
            value: "AIR long note without a parent".to_owned(),
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
