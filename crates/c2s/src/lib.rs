//! Parser for the note and timing subset of the C2S chart format.

use chart::{
    AirDirection, AirProperties, Chart, ChartError, ExDirection, Lane, Note, NoteId, NoteKind,
    Position, ScrollScope, ScrollSpeedChange, SlidePoint, TapKind, TempoChange,
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
}

/// Parses a C2S document into the shared chart model.
pub fn parse(source: &str) -> Result<Chart, C2sError> {
    Parser::new().parse(source)
}

struct Parser {
    resolution: Option<u64>,
    timeline: chart::MeasureTimeline,
    chart: Chart,
    last_ex_parent: Option<NoteId>,
    last_air_direction: Option<AirDirection>,
}

impl Parser {
    fn new() -> Self {
        Self {
            resolution: None,
            timeline: chart::MeasureTimeline::new(Position::new(4, 1).unwrap()),
            chart: Chart::new(),
            last_ex_parent: None,
            last_air_direction: None,
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
                "FLK" => self.parse_tap(line, &fields, TapKind::Flick)?,
                "MNE" => self.parse_mine(line, &fields)?,
                "CHR" => self.parse_ex_tap(line, &fields)?,
                "AIR" => self.parse_air(line, &fields)?,
                "HLD" => self.parse_hold(line, &fields)?,
                "SLD" => self.parse_slide(line, &fields)?,
                "SFL" | "SLP" => self.parse_scroll_speed(line, &fields)?,
                "AHD" | "SXD" | "SLC" | "SXC" | "ALD" => {
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
        self.add_note(line, Note::new(position, lane, NoteKind::Tap(kind)))
            .map(|_| ())
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
        self.last_air_direction = Some(air_direction_from_ex(line, direction)?);
        Ok(())
    }

    fn parse_air(&mut self, line: usize, fields: &[&str]) -> Result<(), C2sError> {
        let parent_type = fields.get(5).ok_or(C2sError::MalformedRecord { line })?;
        if *parent_type != "CHR" || fields.get(6) != Some(&"DEF") {
            return Err(C2sError::InvalidValue {
                line,
                value: fields[5..].join(" "),
            });
        }
        let parent = self.last_ex_parent.ok_or(C2sError::InvalidValue {
            line,
            value: "AIR without CHR parent".to_owned(),
        })?;
        let (measure, tick) = self.location(line, fields)?;
        let lane = self.lane(
            line,
            fields.get(3).ok_or(C2sError::MalformedRecord { line })?,
            fields.get(4).ok_or(C2sError::MalformedRecord { line })?,
        )?;
        let position = self.position(line, measure, tick)?;
        let direction = self.last_air_direction.ok_or(C2sError::InvalidValue {
            line,
            value: "CHR without an AIR direction".to_owned(),
        })?;
        let properties = AirProperties::new(direction);
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
        self.add_note(line, Note::new(start, lane, NoteKind::Hold { end }))
            .map(|_| ())
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
        let points = vec![
            SlidePoint::new(start, start_lane),
            SlidePoint::new(end, end_lane),
        ];
        self.add_note(
            line,
            Note::new(start, start_lane, NoteKind::Slide { points }),
        )
        .map(|_| ())
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

fn air_direction_from_ex(line: usize, direction: ExDirection) -> Result<AirDirection, C2sError> {
    match direction {
        ExDirection::Up => Ok(AirDirection::Up),
        ExDirection::Down => Ok(AirDirection::Down),
        ExDirection::Left => Ok(AirDirection::UpperLeft),
        ExDirection::Right => Ok(AirDirection::UpperRight),
        _ => Err(C2sError::InvalidValue {
            line,
            value: "unsupported AIR direction".to_owned(),
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
    use chart::{Lane, NoteKind, Position, TapKind};

    use super::parse;

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
    fn rejects_zero_resolution() {
        let error = parse("RESOLUTION\t0").expect_err("invalid resolution");
        assert!(matches!(error, super::C2sError::InvalidValue { .. }));
    }
}
