//! Parser and writer for the general SUS chart format and its XLAIR mode.

use std::collections::{BTreeMap, HashMap};

use chart::{
    AirDirection, AirProperties, Chart, ChartError, ChartMode, ExDirection, Lane, MeasureTimeline,
    Note, NoteAttributes, NoteId, NoteKind, Position, ScrollScope, ScrollSpeedChange, SideButton,
    SlidePoint, SlidePointKind, TapKind, TempoChange, report_loss,
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

/// Parses a SUS document into the shared chart model.
pub fn parse(source: &str) -> Result<Chart, SusError> {
    parse_with_mode(source, ChartMode::Normal)
}

/// Parses a SUS document using the selected interpretation mode.
pub fn parse_with_mode(source: &str, mode: ChartMode) -> Result<Chart, SusError> {
    Parser::new(mode).parse(source)
}

/// Writes a chart as a general SUS document.
///
/// Unsupported or unrepresentable chart information is omitted and reported to stdout.
pub fn write(chart: &Chart) -> Result<String, SusError> {
    write_with_mode(chart, ChartMode::Normal)
}

/// Writes a SUS document using the selected interpretation mode.
pub fn write_with_mode(chart: &Chart, mode: ChartMode) -> Result<String, SusError> {
    if mode == ChartMode::Normal {
        return write_standard(chart);
    }
    write_xlair(chart)
}

fn write_xlair(chart: &Chart) -> Result<String, SusError> {
    let timeline = output_timeline(chart);
    let mut records = Vec::new();
    let mut bpm_definitions = Vec::new();
    let mut speed_definitions = BTreeMap::<u32, Vec<(u32, u64, f64)>>::new();
    for change in chart.scroll_speed_changes() {
        let definition = (|| {
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
            let (measure, tick) = output_position(change.position(), &timeline)?;
            Ok((group, measure, tick, change.speed()))
        })();
        match definition {
            Ok((group, measure, tick, speed)) => speed_definitions
                .entry(group)
                .or_default()
                .push((measure, tick, speed)),
            Err(error) if is_loss(&error) => report_loss("SUS", error),
            Err(error) => return Err(error),
        }
    }
    for definitions in speed_definitions.values_mut() {
        definitions.sort_by_key(|(measure, tick, _)| (*measure, *tick));
    }
    for (index, tempo) in chart.tempo_changes().iter().enumerate() {
        match output_position(tempo.position(), &timeline) {
            Ok((measure, tick)) => {
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
            Err(error) if is_loss(&error) => report_loss("SUS", error),
            Err(error) => return Err(error),
        }
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
            report_loss("SUS", "AIR notation");
            continue;
        }
        let channel = match channel(channel_index) {
            Ok(channel) => channel,
            Err(error) if is_loss(&error) => {
                report_loss("SUS", error);
                continue;
            }
            Err(error) => return Err(error),
        };
        let record_start = records.len();
        let result = (|| {
            match note.kind() {
                NoteKind::Tap(kind) => {
                    let (measure, tick) = output_position(note.position(), &timeline)?;
                    let token = match kind {
                        TapKind::Tap => '1',
                        TapKind::XTap => '2',
                        TapKind::Flick { .. } => '3',
                        TapKind::Tap4 | TapKind::Tap5 | TapKind::Tap6 => {
                            return Err(SusError::UnsupportedNote {
                                note: "SUS tap variant outside XLAIR".to_owned(),
                            });
                        }
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
                    let (measure, tick) = output_position(note.position(), &timeline)?;
                    records.push(Record {
                        measure,
                        tick,
                        key: format!("1{}", base36_digit(central_lane(note.lane(), "ExTap")?)),
                        token: format!("2{}", base36_digit(lane_width(note.lane())?)),
                        speed_group: None,
                    });
                }
                NoteKind::Hold { end } => match note.lane() {
                    Lane::Slider { start, width } => add_hold_records(
                        &mut records,
                        note.position(),
                        *end,
                        start,
                        width,
                        channel,
                        &timeline,
                    )?,
                    Lane::Side(button) => add_side_hold_records(
                        &mut records,
                        note.position(),
                        *end,
                        button,
                        channel,
                        &timeline,
                    )?,
                },
                NoteKind::ExHold { end, .. } => match note.lane() {
                    Lane::Slider { start, width } => add_hold_records(
                        &mut records,
                        note.position(),
                        *end,
                        start,
                        width,
                        channel,
                        &timeline,
                    )?,
                    Lane::Side(button) => add_side_hold_records(
                        &mut records,
                        note.position(),
                        *end,
                        button,
                        channel,
                        &timeline,
                    )?,
                },
                NoteKind::ExSlide { points, .. } => {
                    add_slide_records(&mut records, points, channel, &timeline)?
                }
                NoteKind::Slide { points } => {
                    add_slide_records(&mut records, points, channel, &timeline)?;
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
            let speed_group = match speed_group {
                Some(group) if !speed_definitions.contains_key(&group) => {
                    report_loss("SUS", format!("note speed group {group}"));
                    None
                }
                speed_group => speed_group,
            };
            for record in &mut records[record_start..] {
                record.speed_group = speed_group;
            }
            Ok::<(), SusError>(())
        })();
        match result {
            Ok(()) => {}
            Err(error) if is_loss(&error) => {
                records.truncate(record_start);
                report_loss("SUS", error);
            }
            Err(error) => return Err(error),
        }
        if records.len() > record_start {
            channel_index += 1;
        }
    }

    records.sort_by_key(|record| (record.measure, record.tick, record.key.clone()));
    let mut output = String::from("#REQUEST \"ticks_per_beat 384\"\n");
    for &(measure, length) in chart.measure_lengths() {
        output.push_str(&format!("#{measure:03}02: {}\n", format_position(length)?));
    }
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
        let text = match format_record(&record, &timeline) {
            Ok(text) => text,
            Err(error) if is_loss(&error) => {
                report_loss("SUS", error);
                continue;
            }
            Err(error) => return Err(error),
        };
        if record.speed_group != current_speed_group {
            match record.speed_group {
                Some(group) => output.push_str(&format!("#HISPEED {}\n", speed_group_text(group))),
                None => output.push_str("#NOSPEED\n"),
            }
            current_speed_group = record.speed_group;
        }
        output.push_str(&text);
    }
    Ok(output)
}

fn write_standard(chart: &Chart) -> Result<String, SusError> {
    let timeline = output_timeline(chart);
    let ticks_per_beat = standard_ticks_per_beat(chart)?;
    let ticks_per_measure = ticks_per_beat
        .checked_mul(4)
        .ok_or(SusError::UnrepresentablePosition)?;
    let mut output = format!("#REQUEST \"ticks_per_beat {ticks_per_beat}\"\n");
    for &(measure, length) in chart.measure_lengths() {
        output.push_str(&format!("#{measure:03}02: {}\n", format_position(length)?));
    }
    let mut speed_definitions = BTreeMap::<u32, Vec<(u32, u64, f64)>>::new();
    for change in chart.scroll_speed_changes() {
        let definition = (|| {
            if change.duration().is_some() {
                return Err(SusError::UnsupportedNote {
                    note: "scroll speed duration".to_owned(),
                });
            }
            let ScrollScope::Group(group) = change.scope() else {
                return Err(SusError::UnsupportedNote {
                    note: "non-group scroll speed change".to_owned(),
                });
            };
            let (measure, tick) = standard_position(change.position(), ticks_per_beat, &timeline)?;
            let group = speed_group(group)?;
            Ok((group, measure, tick, change.speed()))
        })();
        match definition {
            Ok((group, measure, tick, speed)) => speed_definitions
                .entry(group)
                .or_default()
                .push((measure, tick, speed)),
            Err(error) if is_loss(&error) => report_loss("SUS", error),
            Err(error) => return Err(error),
        }
    }
    for definitions in speed_definitions.values_mut() {
        definitions.sort_by_key(|(measure, tick, _)| (*measure, *tick));
    }
    for (index, tempo) in chart.tempo_changes().iter().enumerate() {
        let id = format!("{:02}", index + 1);
        output.push_str(&format!("#BPM{id}: {}\n", tempo.bpm()));
        let (measure, tick) = standard_position(tempo.position(), ticks_per_beat, &timeline)?;
        output.push_str(&format!("#{measure:02X}{tick:03X}: 08{id}\n"));
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
    let mut current_attributes = NoteAttributes::default();
    let mut attribute_index = 0u32;
    for note_id in standard_note_order(chart) {
        let speed_group = chart
            .note_speed_group(note_id)
            .map_err(|_| SusError::UnrepresentablePosition)?
            .map(speed_group)
            .transpose()?;
        let speed_group = match speed_group {
            Some(group) if chart.scroll_speed_changes().iter().any(|change| {
                matches!(change.scope(), ScrollScope::Group(candidate) if candidate == group)
            }) => Some(group),
            Some(group) => {
                report_loss("SUS", format!("note speed group {group}"));
                None
            }
            None => None,
        };
        if speed_group != current_speed_group {
            if let Some(group) = speed_group {
                output.push_str(&format!("#HISPEED {}\n", speed_group_text(group)));
            } else if current_speed_group.is_some() {
                output.push_str("#NOSPEED\n");
            }
            current_speed_group = speed_group;
        }
        let note = &chart.notes()[note_id.value() as usize];
        let attributes = note.attributes();
        if attributes != current_attributes {
            if attributes.is_empty() {
                output.push_str("#NOATTRIBUTE\n");
            } else {
                let id = speed_group_text(attribute_index);
                attribute_index = attribute_index
                    .checked_add(1)
                    .ok_or(SusError::UnrepresentablePosition)?;
                output.push_str(&format!(
                    "#ATR{id}: \"{}\"\n#ATTRIBUTE {id}\n",
                    format_attributes(attributes)
                ));
            }
            current_attributes = attributes;
        }
        match write_standard_note(
            chart,
            note_id,
            note,
            ticks_per_measure,
            ticks_per_beat,
            &timeline,
        ) {
            Ok(lines) => {
                for line in lines {
                    output.push_str(&line);
                    output.push('\n');
                }
            }
            Err(error) if is_loss(&error) => report_loss("SUS", error),
            Err(error) => return Err(error),
        }
    }
    Ok(output)
}

fn standard_note_order(chart: &Chart) -> Vec<NoteId> {
    fn visit(chart: &Chart, index: usize, visited: &mut [bool], output: &mut Vec<NoteId>) {
        if visited[index] {
            return;
        }
        visited[index] = true;
        let parent = match chart.notes()[index].kind() {
            NoteKind::Air { parent, .. }
            | NoteKind::AirHold { parent, .. }
            | NoteKind::AirSlide { parent, .. }
            | NoteKind::AirCrush { parent, .. } => Some(*parent),
            _ => None,
        };
        if let Some(parent) = parent {
            let parent_index = parent.value() as usize;
            if parent_index < chart.notes().len() {
                visit(chart, parent_index, visited, output);
            }
        }
        output.push(NoteId::new(index as u32));
    }

    let mut visited = vec![false; chart.notes().len()];
    let mut output = Vec::with_capacity(chart.notes().len());
    for index in 0..chart.notes().len() {
        visit(chart, index, &mut visited, &mut output);
    }
    output
}

fn standard_parent_code(note: &Note) -> &'static str {
    match note.kind() {
        NoteKind::Tap(TapKind::Flick { .. }) => "FLK",
        NoteKind::Tap(_) => "TAP",
        NoteKind::ExTap { .. } => "CHR",
        NoteKind::Mine => "MNE",
        NoteKind::Hold { .. } | NoteKind::ExHold { .. } | NoteKind::AirHold { .. } => "HLD",
        NoteKind::Slide { .. } | NoteKind::ExSlide { .. } => "SLD",
        NoteKind::Air { .. } | NoteKind::AirSlide { .. } | NoteKind::AirCrush { .. } => "TAP",
    }
}

fn note_end_position(note: &Note) -> Position {
    match note.kind() {
        NoteKind::Hold { end } | NoteKind::ExHold { end, .. } | NoteKind::AirHold { end, .. } => {
            *end
        }
        NoteKind::Slide { points } | NoteKind::ExSlide { points, .. } => points
            .last()
            .map(SlidePoint::position)
            .unwrap_or(note.position()),
        NoteKind::AirSlide { points, .. } | NoteKind::AirCrush { points, .. } => points
            .last()
            .map(chart::AirPoint::position)
            .unwrap_or(note.position()),
        _ => note.position(),
    }
}

fn standard_ticks_per_beat(chart: &Chart) -> Result<u64, SusError> {
    let mut ticks_per_beat = 1;
    let mut add_position = |position: Position| -> Result<(), SusError> {
        ticks_per_beat =
            lcm(ticks_per_beat, position.denominator()).ok_or(SusError::UnrepresentablePosition)?;
        Ok(())
    };
    for tempo in chart.tempo_changes() {
        add_position(tempo.position())?;
    }
    for change in chart.scroll_speed_changes() {
        add_position(change.position())?;
        if let Some(duration) = change.duration() {
            add_position(duration)?;
        }
    }
    for note in chart.notes() {
        add_position(note.position())?;
        match note.kind() {
            NoteKind::Hold { end }
            | NoteKind::ExHold { end, .. }
            | NoteKind::AirHold { end, .. } => add_position(*end)?,
            NoteKind::Slide { points } | NoteKind::ExSlide { points, .. } => {
                for point in points {
                    add_position(point.position())?;
                }
            }
            NoteKind::AirSlide { points, .. } | NoteKind::AirCrush { points, .. } => {
                for point in points {
                    add_position(point.position())?;
                }
            }
            NoteKind::Tap(_) | NoteKind::ExTap { .. } | NoteKind::Mine | NoteKind::Air { .. } => {}
        }
    }
    let ticks_per_measure = ticks_per_beat
        .checked_mul(4)
        .ok_or(SusError::UnrepresentablePosition)?;
    if ticks_per_measure > 0x0fff {
        return Err(SusError::UnrepresentablePosition);
    }
    Ok(ticks_per_beat)
}

fn lcm(left: u64, right: u64) -> Option<u64> {
    left.checked_div(gcd(left, right))?.checked_mul(right)
}

fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

fn output_timeline(chart: &Chart) -> MeasureTimeline {
    let mut timeline = MeasureTimeline::new(Position::new(4, 1).expect("valid position"));
    for &(measure, length) in chart.measure_lengths() {
        timeline.set_length(measure, length);
    }
    timeline
}

fn format_position(position: Position) -> Result<String, SusError> {
    let integer = position.numerator() / position.denominator();
    let mut remainder = position.numerator() % position.denominator();
    if remainder == 0 {
        return Ok(integer.to_string());
    }
    let mut fraction = String::new();
    for _ in 0..18 {
        remainder = remainder
            .checked_mul(10)
            .ok_or(SusError::UnrepresentablePosition)?;
        fraction.push(char::from(
            b'0' + (remainder / position.denominator()) as u8,
        ));
        remainder %= position.denominator();
        if remainder == 0 {
            while fraction.ends_with('0') {
                fraction.pop();
            }
            return Ok(format!("{integer}.{fraction}"));
        }
    }
    Err(SusError::UnrepresentablePosition)
}

fn format_attributes(attributes: NoteAttributes) -> String {
    let mut fields = Vec::new();
    if let Some(roll_speed) = attributes.roll_speed() {
        fields.push(format!("rh: {roll_speed}"));
    }
    if let Some(height) = attributes.height() {
        fields.push(format!("h: {height}"));
    }
    if let Some(priority) = attributes.priority() {
        fields.push(format!("pr: {priority}"));
    }
    fields.join(", ")
}

fn write_standard_note(
    chart: &Chart,
    note_id: NoteId,
    note: &Note,
    ticks_per_measure: u64,
    ticks_per_beat: u64,
    timeline: &MeasureTimeline,
) -> Result<Vec<String>, SusError> {
    let (measure, tick) = standard_position(note.position(), ticks_per_beat, timeline)?;
    let Lane::Slider { start, width } = note.lane() else {
        return Err(SusError::UnsupportedNote {
            note: "side lane in standard SUS".to_owned(),
        });
    };
    let lane_width = format!("{:02X}{:02X}", start * 2, width * 2);
    let prefix = format!("#{measure:02X}{tick:03X}");
    let mut lines = Vec::new();
    match note.kind() {
        NoteKind::Tap(kind) => match kind {
            TapKind::Tap => lines.push(format!("{prefix}: 01{lane_width}")),
            TapKind::XTap => lines.push(format!("{prefix}: 02{lane_width}")),
            TapKind::Flick { .. } => lines.push(format!("{prefix}: 03{lane_width}")),
            TapKind::Tap4 => lines.push(standard_channel_line(
                note.position(),
                '1',
                start,
                '4',
                width,
                ticks_per_beat,
                timeline,
            )?),
            TapKind::Tap5 => lines.push(standard_channel_line(
                note.position(),
                '1',
                start,
                '5',
                width,
                ticks_per_beat,
                timeline,
            )?),
            TapKind::Tap6 => lines.push(standard_channel_line(
                note.position(),
                '1',
                start,
                '6',
                width,
                ticks_per_beat,
                timeline,
            )?),
        },
        NoteKind::ExTap { direction } => {
            if let Ok(direction) = standard_ex_direction(*direction) {
                lines.push(standard_channel_line(
                    note.position(),
                    '5',
                    start,
                    direction,
                    width,
                    ticks_per_beat,
                    timeline,
                )?);
            } else {
                lines.push(format!("{prefix}: 02{lane_width}"));
            }
        }
        NoteKind::Mine => lines.push(format!("{prefix}: 10{lane_width}")),
        NoteKind::Hold { end } | NoteKind::ExHold { end, .. } => {
            let duration = duration_ticks(note.position(), *end, ticks_per_measure)?;
            lines.push(format!("{prefix}: 05{lane_width}{duration:04X}"));
        }
        NoteKind::Slide { points } | NoteKind::ExSlide { points, .. } => {
            for segment in points.windows(2) {
                let start = &segment[0];
                let end = &segment[1];
                let Lane::Slider {
                    start: start_lane,
                    width: start_width,
                } = start.lane()
                else {
                    return Err(SusError::UnsupportedNote {
                        note: "side slide lane in standard SUS".to_owned(),
                    });
                };
                let Lane::Slider {
                    start: end_start,
                    width: end_width,
                } = end.lane()
                else {
                    return Err(SusError::UnsupportedNote {
                        note: "side slide endpoint in standard SUS".to_owned(),
                    });
                };
                let (segment_measure, segment_tick) =
                    standard_position(start.position(), ticks_per_beat, timeline)?;
                let duration = duration_ticks(start.position(), end.position(), ticks_per_measure)?;
                lines.push(format!(
                    "#{segment_measure:02X}{segment_tick:03X}: 06{:02X}{:02X}{duration:04X}{:02X}{:02X}",
                    start_lane * 2,
                    start_width * 2,
                    end_start * 2,
                    end_width * 2,
                ));
            }
        }
        NoteKind::Air { properties, parent } => {
            let code = match properties.direction() {
                Some(AirDirection::Up) => "07",
                Some(AirDirection::Down) => "09",
                _ => {
                    return Err(SusError::UnsupportedNote {
                        note: "standard SUS AIR direction".to_owned(),
                    });
                }
            };
            let target =
                standard_parent_code(chart.note(*parent).map_err(|_| SusError::InvalidValue {
                    line: 0,
                    value: format!("invalid AIR parent for note {}", note_id.value()),
                })?);
            lines.push(format!("{prefix}: {code}{lane_width}{target}"));
        }
        NoteKind::AirHold { end, parent, .. } => {
            let duration = duration_ticks(note.position(), *end, ticks_per_measure)?;
            let target =
                standard_parent_code(chart.note(*parent).map_err(|_| SusError::InvalidValue {
                    line: 0,
                    value: format!("invalid AIR parent for note {}", note_id.value()),
                })?);
            lines.push(format!("{prefix}: 08{lane_width}{duration:04X}{target}"));
        }
        NoteKind::AirSlide { .. } | NoteKind::AirCrush { .. } => {
            return Err(SusError::UnsupportedNote {
                note: "standard SUS AIR slide or crush".to_owned(),
            });
        }
    }
    Ok(lines)
}

fn standard_position(
    position: Position,
    ticks_per_beat: u64,
    timeline: &MeasureTimeline,
) -> Result<(u32, u64), SusError> {
    timeline
        .locate(position, ticks_per_beat)
        .map_err(|_| SusError::UnrepresentablePosition)
}

fn duration_ticks(start: Position, end: Position, ticks_per_measure: u64) -> Result<u64, SusError> {
    if end <= start {
        return Err(SusError::UnrepresentablePosition);
    }
    let duration = Position::new(
        end.numerator()
            .checked_mul(start.denominator())
            .and_then(|right| right.checked_sub(start.numerator().checked_mul(end.denominator())?))
            .ok_or(SusError::UnrepresentablePosition)?,
        end.denominator()
            .checked_mul(start.denominator())
            .ok_or(SusError::UnrepresentablePosition)?,
    )
    .map_err(|_| SusError::UnrepresentablePosition)?;
    let numerator = duration
        .numerator()
        .checked_mul(ticks_per_measure / 4)
        .ok_or(SusError::UnrepresentablePosition)?;
    if numerator % duration.denominator() != 0 {
        return Err(SusError::UnrepresentablePosition);
    }
    let ticks = numerator / duration.denominator();
    u16::try_from(ticks)
        .map(u64::from)
        .map_err(|_| SusError::UnrepresentablePosition)
}

fn standard_channel_line(
    position: Position,
    kind: char,
    lane: u8,
    token_kind: char,
    width: u8,
    ticks_per_beat: u64,
    timeline: &MeasureTimeline,
) -> Result<String, SusError> {
    let (measure, tick) = standard_position(position, ticks_per_beat, timeline)?;
    let tick = usize::try_from(tick).map_err(|_| SusError::UnrepresentablePosition)?;
    let slots = usize::try_from(
        timeline
            .ticks(measure, ticks_per_beat)
            .map_err(|_| SusError::UnrepresentablePosition)?,
    )
    .map_err(|_| SusError::UnrepresentablePosition)?;
    let trailing = slots
        .checked_sub(tick + 1)
        .ok_or(SusError::UnrepresentablePosition)?;
    Ok(format!(
        "#{measure:03}{kind}{}: {}{}{}{}",
        base36_digit(lane),
        "00".repeat(tick),
        token_kind,
        base36_digit(width),
        "00".repeat(trailing),
    ))
}

fn standard_ex_direction(direction: ExDirection) -> Result<char, SusError> {
    match direction {
        ExDirection::Up => Ok('1'),
        ExDirection::Down => Ok('2'),
        ExDirection::UpperLeft => Ok('3'),
        ExDirection::UpperRight => Ok('4'),
        ExDirection::LowerLeft => Ok('5'),
        ExDirection::LowerRight => Ok('6'),
        _ => Err(SusError::UnsupportedNote {
            note: "standard SUS ExTap direction".to_owned(),
        }),
    }
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
    timeline: &MeasureTimeline,
) -> Result<(), SusError> {
    let (start_measure, start_tick) = output_position(start, timeline)?;
    let (end_measure, end_tick) = output_position(end, timeline)?;
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
    timeline: &MeasureTimeline,
) -> Result<(), SusError> {
    let (start_measure, start_tick) = output_position(start, timeline)?;
    let (end_measure, end_tick) = output_position(end, timeline)?;
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
    timeline: &MeasureTimeline,
) -> Result<(), SusError> {
    for (index, point) in points.iter().enumerate() {
        let lane = central_lane(point.lane(), "slide")?;
        let (measure, tick) = output_position(point.position(), timeline)?;
        let kind = if index == 0 {
            '1'
        } else if index + 1 == points.len() {
            '2'
        } else {
            match point.kind() {
                SlidePointKind::Visible => '3',
                SlidePointKind::Control => '4',
                SlidePointKind::Invisible => '5',
            }
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

fn format_record(record: &Record, timeline: &MeasureTimeline) -> Result<String, SusError> {
    let padding = usize::try_from(record.tick).map_err(|_| SusError::UnrepresentablePosition)?;
    let slots = usize::try_from(
        timeline
            .ticks(record.measure, 96)
            .map_err(|_| SusError::UnrepresentablePosition)?,
    )
    .map_err(|_| SusError::UnrepresentablePosition)?;
    let trailing = slots
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

fn output_position(position: Position, timeline: &MeasureTimeline) -> Result<(u32, u64), SusError> {
    timeline
        .locate(position, 96)
        .map_err(|_| SusError::UnrepresentablePosition)
}

fn channel(index: usize) -> Result<char, SusError> {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    DIGITS
        .get(index)
        .copied()
        .map(char::from)
        .ok_or(SusError::UnrepresentablePosition)
}

fn is_loss(error: &SusError) -> bool {
    matches!(
        error,
        SusError::UnsupportedNote { .. } | SusError::UnrepresentablePosition
    )
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
    mode: ChartMode,
    timeline: chart::MeasureTimeline,
    bpm_definitions: HashMap<String, f64>,
    pending_side_longs: HashMap<char, PendingSlide>,
    pending_holds: HashMap<char, PendingSlide>,
    pending_sliders: HashMap<char, PendingSlide>,
    ticks_per_beat: u64,
    measure_base: u32,
    current_speed_group: Option<u32>,
    attribute_definitions: HashMap<String, NoteAttributes>,
    current_attributes: Option<NoteAttributes>,
    chart: Chart,
    pending_standard_air: Vec<PendingStandardAir>,
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
            ticks_per_beat: 480,
            measure_base: 0,
            current_speed_group: None,
            attribute_definitions: HashMap::new(),
            current_attributes: None,
            chart: Chart::new(),
            pending_standard_air: Vec::new(),
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
            if matches!(fields[1], "0" | "1" | "true" | "false") {
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
            return Ok(());
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
                    let point_kind = match token[0] {
                        b'4' => SlidePointKind::Control,
                        b'5' => SlidePointKind::Invisible,
                        _ => SlidePointKind::Visible,
                    };
                    pending
                        .points
                        .push(SlidePoint::new(position, lane).with_kind(point_kind));
                }
                _ => return Err(invalid_token(line, token)),
            }
        }
        Ok(())
    }

    fn add_note(&mut self, line: usize, note: Note) -> Result<(), SusError> {
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
        Chart, Lane, Note, NoteId, NoteKind, Position, ScrollScope, ScrollSpeedChange, SideButton,
        SlidePoint, SlidePointKind, TapKind, TempoChange,
    };

    use super::{parse, parse_with_mode, write, write_with_mode};

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
    fn preserves_variable_measure_lengths_when_writing_standard_sus() {
        let source = "#00102: 3\n#00210: 14";
        let chart = parse(source).expect("valid SUS");
        let written = write(&chart).expect("variable measure length is representable");
        let reparsed = parse(&written).expect("written SUS is valid");

        assert!(written.contains("#00102: 3"));
        assert_eq!(reparsed.measure_lengths(), chart.measure_lengths());
        assert_eq!(reparsed.notes(), chart.notes());
    }

    #[test]
    fn preserves_variable_measure_lengths_when_writing_xlair_sus() {
        let chart = parse("#00102: 3\n#00210: 14").expect("valid SUS");
        let written = write_with_mode(&chart, chart::ChartMode::Xlair).expect("valid XLAIR SUS");
        let reparsed = parse_with_mode(&written, chart::ChartMode::Xlair).expect("valid XLAIR SUS");

        assert!(written.contains("#00102: 3"));
        assert_eq!(reparsed.measure_lengths(), chart.measure_lengths());
        assert_eq!(reparsed.notes(), chart.notes());
    }

    #[test]
    fn parses_bpm_changes_and_side_longs() {
        let source = "#BPM01: 120\n#00008: 01\n#00120A: 14\n#00220A: 24\n";
        let chart =
            super::parse_with_mode(source, chart::ChartMode::Xlair).expect("valid XLAIR SUS");

        assert_eq!(chart.tempo_changes().len(), 1);
        assert_eq!(chart.notes().len(), 1);
        assert_eq!(chart.notes()[0].lane(), Lane::Side(SideButton::LeftUpper));
        assert!(matches!(chart.notes()[0].kind(), NoteKind::Hold { .. }));
    }

    #[test]
    fn parses_xlair_side_taps_and_relayed_holds() {
        let source = "#00150: 34\n#0015c: 64\n#00120A: 14\n#00220A: 34\n#00320A: 24";
        let chart =
            super::parse_with_mode(source, chart::ChartMode::Xlair).expect("valid XLAIR SUS");

        assert_eq!(chart.notes().len(), 3);
        assert_eq!(chart.notes()[0].lane(), Lane::Side(SideButton::LeftUpper));
        assert_eq!(chart.notes()[1].lane(), Lane::Side(SideButton::RightLower));
        assert!(matches!(chart.notes()[2].kind(), NoteKind::Hold { .. }));
    }

    #[test]
    fn ignores_standard_metadata() {
        let source = r#"
#TITLE "title"
#SUBTITLE "subtitle"
#ARTIST "artist"
#GENRE "genre"
#DESIGNER "designer"
#DIFFICULTY 3
#PLAYLEVEL 12
#SONGID "song"
#WAVE "song.wav"
#WAVEOFFSET 0
#JACKET "jacket.jpg"
#BACKGROUND "background.jpg"
#MOVIE "movie.mp4"
#MOVIEOFFSET 0
#BASEBPM 154
#00110: 14
"#;

        let chart = parse(source).expect("standard SUS metadata is not chart data");
        assert_eq!(chart.notes().len(), 1);
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
        let chart = parse("#00130A: 144g\n#0023cA: 24").expect("valid SUS");

        assert_eq!(chart.notes().len(), 1);
        let NoteKind::Slide { points } = chart.notes()[0].kind() else {
            panic!("expected a slide");
        };
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].lane(), Lane::slider(0, 4).unwrap());
        assert_eq!(points[1].lane(), Lane::slider(0, 16).unwrap());
        assert_eq!(points[2].lane(), Lane::slider(12, 4).unwrap());
        assert_eq!(points[1].kind(), &SlidePointKind::Control);
    }

    #[test]
    fn parses_slide_two_channels() {
        let chart = parse("#00140A: 144g\n#0024cA: 24").expect("valid SUS");

        assert_eq!(chart.notes().len(), 1);
        assert!(matches!(chart.notes()[0].kind(), NoteKind::Slide { points } if points.len() == 3));
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
    fn ignores_measure_line_speed_commands() {
        let chart = parse("#MEASUREHS 00\n#REQUEST \"enable_priority true\"\n#00110: 14")
            .expect("valid SUS display commands");
        assert_eq!(chart.notes().len(), 1);
    }

    #[test]
    fn preserves_sus_note_attributes() {
        let source = concat!(
            "#ATR01: \"rh: 1.5, h: 2.0, pr: 100\"\n",
            "#ATTRIBUTE 01\n",
            "#00010: 14\n",
            "#NOATTRIBUTE\n",
            "#00110: 14\n",
        );
        let chart = parse(source).expect("valid SUS attributes");

        assert_eq!(chart.notes()[0].attributes().roll_speed(), Some(1.5));
        assert_eq!(chart.notes()[0].attributes().height(), Some(2.0));
        assert_eq!(chart.notes()[0].attributes().priority(), Some(100));
        assert!(chart.notes()[1].attributes().is_empty());

        let written = write(&chart).expect("attributes are representable in SUS");
        let reparsed = parse(&written).expect("written SUS attributes");
        assert_eq!(reparsed.notes(), chart.notes());
    }

    #[test]
    fn applies_measure_base_to_note_positions() {
        let chart = parse("#MEASUREBS 10\n#00010: 14").expect("valid SUS");

        assert_eq!(chart.notes()[0].position(), Position::new(40, 1).unwrap());
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
        assert!(sus.contains("#00001: 02140C"));
        let parsed = parse(&sus).unwrap();
        assert_eq!(
            parsed.notes()[0].kind(),
            &NoteKind::ExTap {
                direction: chart::ExDirection::Up
            }
        );
    }

    #[test]
    fn parses_standard_sus_note_codes() {
        let source = concat!(
            "#REQUEST \"ticks_per_beat 480\"\n",
            "#00000: 010008\n",
            "#000A0: 020008\n",
            "#00140: 05000801E0\n",
            "#00280: 06000801E00008\n",
            "#003C0: 100008\n",
        );
        let chart = parse(source).expect("valid standard SUS");
        assert_eq!(chart.notes().len(), 5);
        assert!(matches!(chart.notes()[1].kind(), NoteKind::ExTap { .. }));
        assert!(matches!(chart.notes()[2].kind(), NoteKind::Hold { .. }));
        assert!(matches!(chart.notes()[3].kind(), NoteKind::Slide { .. }));
        assert_eq!(chart.notes()[4].kind(), &NoteKind::Mine);
    }

    #[test]
    fn preserves_generic_tap_two_as_xtap() {
        let chart = parse("#00010: 22").expect("valid generic SUS");

        assert_eq!(chart.notes()[0].kind(), &NoteKind::Tap(TapKind::XTap));
    }

    #[test]
    fn resolves_standard_sus_air_to_its_parent() {
        let source = concat!(
            "#REQUEST \"ticks_per_beat 480\"\n",
            "#00000: 010008\n",
            "#00000: 070008\n",
            "#00000: 08000801E0\n",
        );
        let chart = parse(source).expect("valid standard SUS AIR");
        assert_eq!(chart.notes().len(), 3);
        assert!(matches!(
            chart.notes()[1].kind(),
            NoteKind::Air { parent, .. } if *parent == chart::NoteId::new(0)
        ));
        assert!(matches!(
            chart.notes()[2].kind(),
            NoteKind::AirHold { parent, .. } if *parent == chart::NoteId::new(0)
        ));
    }

    #[test]
    fn preserves_speed_groups_on_standard_air_notes() {
        let source = concat!(
            "#TIL00: \"0'0:1.0\"\n",
            "#HISPEED 00\n",
            "#00000: 010008\n",
            "#00000: 070008\n",
        );
        let chart = parse(source).expect("valid standard SUS AIR");

        assert_eq!(
            chart.note_speed_group(chart::NoteId::new(1)).unwrap(),
            Some(0)
        );
    }

    #[test]
    fn writes_standard_sus_air_without_losing_the_parent_relation() {
        let position = Position::new(1, 1).unwrap();
        let lane = Lane::slider(4, 4).unwrap();
        let mut chart = Chart::new();
        let parent = chart.add_note(
            Note::new(position, lane, NoteKind::Tap(TapKind::Tap)).expect("valid parent"),
        );
        chart.add_note(
            Note::new(
                position,
                lane,
                NoteKind::Air {
                    properties: chart::AirProperties::new(chart::AirDirection::Up),
                    parent,
                },
            )
            .expect("valid AIR"),
        );
        let source = write(&chart).expect("valid standard SUS output");
        let parsed = parse(&source).expect("valid standard SUS round trip");
        assert!(matches!(
            parsed.notes()[1].kind(),
            NoteKind::Air { parent, .. } if *parent == chart::NoteId::new(0)
        ));
    }

    #[test]
    fn preserves_standard_sus_scroll_speed_groups() {
        let position = Position::new(1, 1).unwrap();
        let mut chart = Chart::new();
        chart.add_scroll_speed_change(
            ScrollSpeedChange::with_scope(position, 1.5, ScrollScope::Group(2))
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
            .set_note_speed_group(note_id, Some(2))
            .expect("valid speed group");
        let source = write(&chart).expect("valid standard SUS output");
        let parsed = parse(&source).expect("valid standard SUS round trip");
        assert_eq!(parsed.scroll_speed_changes(), chart.scroll_speed_changes());
        assert_eq!(parsed.note_speed_group(NoteId::new(0)).unwrap(), Some(2));
    }

    #[test]
    fn preserves_generic_sus_tap_variants_and_directions() {
        let mut chart = Chart::new();
        for (index, kind) in [TapKind::Tap4, TapKind::Tap5, TapKind::Tap6]
            .into_iter()
            .enumerate()
        {
            chart.add_note(
                Note::new(
                    Position::new(index as u64, 1).unwrap(),
                    Lane::slider(index as u8, 2).unwrap(),
                    NoteKind::Tap(kind),
                )
                .unwrap(),
            );
        }
        chart.add_note(
            Note::new(
                Position::new(3, 1).unwrap(),
                Lane::slider(6, 2).unwrap(),
                NoteKind::ExTap {
                    direction: chart::ExDirection::LowerRight,
                },
            )
            .unwrap(),
        );
        let source = write(&chart).expect("valid generic SUS output");
        let parsed = parse(&source).expect("valid generic SUS round trip");
        assert_eq!(parsed.notes(), chart.notes());
    }

    #[test]
    fn writes_slide_point_kinds() {
        let start = Position::new(0, 1).unwrap();
        let middle = Position::new(1, 4).unwrap();
        let invisible = Position::new(1, 2).unwrap();
        let end = Position::new(1, 1).unwrap();
        let mut chart = Chart::new();
        chart.add_note(
            Note::new(
                start,
                Lane::slider(0, 4).unwrap(),
                NoteKind::Slide {
                    points: vec![
                        SlidePoint::new(start, Lane::slider(0, 4).unwrap()),
                        SlidePoint::new(middle, Lane::slider(4, 4).unwrap())
                            .with_kind(SlidePointKind::Control),
                        SlidePoint::new(invisible, Lane::slider(8, 4).unwrap())
                            .with_kind(SlidePointKind::Invisible),
                        SlidePoint::new(end, Lane::slider(12, 4).unwrap()),
                    ],
                },
            )
            .unwrap(),
        );

        let sus =
            super::write_with_mode(&chart, chart::ChartMode::Xlair).expect("valid SUS output");
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

        let sus =
            super::write_with_mode(&chart, chart::ChartMode::Xlair).expect("valid speed output");
        assert!(sus.contains("#TIL0z: \"0'96:1.5\""));
        assert!(sus.contains("#HISPEED 0z"));
        let parsed = super::parse_with_mode(&sus, chart::ChartMode::Xlair)
            .expect("round-tripped speed output");
        assert_eq!(parsed.scroll_speed_changes(), chart.scroll_speed_changes());
        assert_eq!(parsed.note_speed_group(note_id).unwrap(), Some(35));
    }

    #[test]
    fn omits_speed_changes_that_sus_cannot_represent() {
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

        let sus = write(&chart).expect("unsupported speed is omitted");
        assert_eq!(sus, "#REQUEST \"ticks_per_beat 4\"\n");
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

        let sus =
            super::write_with_mode(&chart, chart::ChartMode::Xlair).expect("valid XLAIR output");
        let parsed = super::parse_with_mode(&sus, chart::ChartMode::Xlair)
            .expect("round-tripped XLAIR output");
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

        let sus = super::write_with_mode(&chart, chart::ChartMode::Xlair)
            .expect("AIR is intentionally omitted in XLAIR output");
        let parsed =
            super::parse_with_mode(&sus, chart::ChartMode::Xlair).expect("valid XLAIR output");
        assert_eq!(parsed.notes().len(), 1);
        assert_eq!(parsed.notes()[0].kind(), &NoteKind::Tap(TapKind::Tap));
    }
}
