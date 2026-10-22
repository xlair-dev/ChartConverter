use std::collections::BTreeMap;

use chart::{
    AirColor, AirDirection, Chart, ChartMode, ExDirection, Lane, MeasureTimeline, Note,
    NoteAttributes, NoteId, NoteKind, Position, ScrollScope, SideButton, SlidePoint,
    SlidePointKind, TapKind, report_loss,
};

use crate::{
    SusError,
    syntax::{note_end_position, side_direction, side_lane},
};

pub(super) fn write_with_mode(chart: &Chart, mode: ChartMode) -> Result<String, SusError> {
    if mode == ChartMode::Normal {
        return write_standard(chart);
    }
    write_xlair(chart)
}

fn write_xlair(chart: &Chart) -> Result<String, SusError> {
    let timeline = output_timeline(chart);
    let ticks_per_beat = xlair_ticks_per_beat(chart, &timeline)?;
    let timing = XlairTiming {
        timeline: &timeline,
        ticks_per_beat,
    };
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
            let (measure, tick) = output_position(change.position(), &timing)?;
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
        match output_position(tempo.position(), &timing) {
            Ok((measure, tick)) => {
                let id = definition_id(index)?;
                bpm_definitions.push(format!("#BPM{id}: {}\n", tempo.bpm()));
                records.push(Record {
                    measure,
                    tick,
                    key: "08".to_owned(),
                    token: id,
                    speed_group: None,
                    attributes: NoteAttributes::default(),
                });
            }
            Err(error) if is_loss(&error) => report_loss("SUS", error),
            Err(error) => return Err(error),
        }
    }

    let mut long_note_channels = vec![Vec::<(Position, Position)>::new(); 36];
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
        let channel = match note_channel(note, &mut long_note_channels) {
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
                    let (measure, tick) = output_position(note.position(), &timing)?;
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
                            attributes: NoteAttributes::default(),
                        }),
                        Lane::Side(button) if *kind == TapKind::Tap => records.push(Record {
                            measure,
                            tick,
                            key: format!("5{}", base36_digit(side_lane(button))),
                            token: format!("{}1", side_direction(button)),
                            speed_group: None,
                            attributes: NoteAttributes::default(),
                        }),
                        Lane::Side(_) => {
                            return Err(SusError::UnsupportedNote {
                                note: "side ExTap/Flick".to_owned(),
                            });
                        }
                    }
                }
                NoteKind::ExTap { .. } => {
                    let (measure, tick) = output_position(note.position(), &timing)?;
                    records.push(Record {
                        measure,
                        tick,
                        key: format!("1{}", base36_digit(central_lane(note.lane(), "ExTap")?)),
                        token: format!("2{}", base36_digit(lane_width(note.lane())?)),
                        speed_group: None,
                        attributes: NoteAttributes::default(),
                    });
                }
                NoteKind::Hold { end, checkpoints } => match note.lane() {
                    Lane::Slider { start, width } => add_hold_records(
                        &mut records,
                        HoldSpan {
                            start: note.position(),
                            end: *end,
                            checkpoints,
                        },
                        start,
                        width,
                        channel,
                        &timing,
                    )?,
                    Lane::Side(button) => add_side_hold_records(
                        &mut records,
                        HoldSpan {
                            start: note.position(),
                            end: *end,
                            checkpoints,
                        },
                        button,
                        channel,
                        &timing,
                    )?,
                },
                NoteKind::ExHold {
                    end, checkpoints, ..
                } => match note.lane() {
                    Lane::Slider { start, width } => add_hold_records(
                        &mut records,
                        HoldSpan {
                            start: note.position(),
                            end: *end,
                            checkpoints,
                        },
                        start,
                        width,
                        channel,
                        &timing,
                    )?,
                    Lane::Side(button) => add_side_hold_records(
                        &mut records,
                        HoldSpan {
                            start: note.position(),
                            end: *end,
                            checkpoints,
                        },
                        button,
                        channel,
                        &timing,
                    )?,
                },
                NoteKind::ExSlide { points, .. } => {
                    add_slide_records(&mut records, points, channel, &timing)?
                }
                NoteKind::Slide { points } => {
                    add_slide_records(&mut records, points, channel, &timing)?;
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
            let note_id = NoteId::new(note_index as u32);
            for (record_index, record) in records[record_start..].iter_mut().enumerate() {
                // Hold helpers emit checkpoints first, then start and end.
                let count = note.point_metadata().len();
                let point_index =
                    if matches!(note.kind(), NoteKind::Hold { .. } | NoteKind::ExHold { .. }) {
                        if record_index < count - 2 {
                            record_index + 1
                        } else if record_index == count - 2 {
                            0
                        } else {
                            count - 1
                        }
                    } else {
                        record_index
                    };
                let metadata = if count == 0 {
                    chart::PointMetadata {
                        speed_group: chart
                            .note_speed_group(note_id)
                            .map_err(|_| SusError::UnrepresentablePosition)?,
                        attributes: note.attributes(),
                    }
                } else {
                    chart
                        .point_metadata(note_id, point_index)
                        .map_err(|_| SusError::UnrepresentablePosition)?
                };
                record.speed_group = output_speed_group(chart, metadata.speed_group)?;
                record.attributes = metadata.attributes;
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
    }

    records.sort_by_key(|record| {
        (
            record.measure,
            record.tick,
            record.key.chars().last().unwrap_or_default(),
            record_kind_order(record),
            record.key.clone(),
        )
    });
    let mut output = format!("#REQUEST \"ticks_per_beat {ticks_per_beat}\"\n");
    // These are already resolved IR notes, not editor carrier records. Protect any
    // separate tap/side pair from being merged by the reader's legacy default.
    if chart.notes().iter().any(|side| {
        let Lane::Side(button) = side.lane() else {
            return false;
        };
        let lane = Lane::slider(side_lane(button), 1).expect("valid side lane");
        matches!(side.kind(), NoteKind::Tap(TapKind::Tap))
            && chart.notes().iter().any(|tap| {
                tap.position() == side.position()
                    && lane.overlaps(tap.lane())
                    && matches!(
                        tap.kind(),
                        NoteKind::Tap(TapKind::Tap | TapKind::Flick { .. })
                    )
            })
    }) {
        output.push_str("#REQUEST \"xlair_directional_mode independent\"\n");
    }
    if let Some(enabled) = chart.priority_enabled() {
        output.push_str(&format!(
            "#REQUEST \"enable_priority {}\"\n",
            if enabled { "true" } else { "false" }
        ));
    }
    if let Some(base_bpm) = chart.base_bpm() {
        output.push_str(&format!("#BASEBPM {base_bpm}\n"));
    }
    if let Some(seconds) = chart.audio_offset_seconds() {
        output.push_str(&format!("#WAVEOFFSET {seconds}\n"));
    }
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
            definition_id(group as usize)?,
            definitions.join(",")
        ));
    }
    let mut context = ContextWriter::default();
    for record in records {
        let text = match format_record(&record, &timing) {
            Ok(text) => text,
            Err(error) if is_loss(&error) => {
                report_loss("SUS", error);
                continue;
            }
            Err(error) => return Err(error),
        };
        context.write(&mut output, record.speed_group, record.attributes)?;
        output.push_str(&text);
    }
    Ok(output)
}

fn write_standard(chart: &Chart) -> Result<String, SusError> {
    let timeline = output_timeline(chart);
    let ticks_per_beat = standard_ticks_per_beat(chart)?;
    let mut output = format!("#REQUEST \"ticks_per_beat {ticks_per_beat}\"\n");
    if let Some(enabled) = chart.priority_enabled() {
        output.push_str(&format!(
            "#REQUEST \"enable_priority {}\"\n",
            if enabled { "true" } else { "false" }
        ));
    }
    if let Some(base_bpm) = chart.base_bpm() {
        output.push_str(&format!("#BASEBPM {base_bpm}\n"));
    }
    if let Some(seconds) = chart.audio_offset_seconds() {
        output.push_str(&format!("#WAVEOFFSET {seconds}\n"));
    }
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
        let id = definition_id(index)?;
        output.push_str(&format!("#BPM{id}: {}\n", tempo.bpm()));
        let (measure, tick) = standard_position(tempo.position(), ticks_per_beat, &timeline)?;
        output.push_str(&standard_timing_channel_line(
            measure,
            tick,
            &id,
            ticks_per_beat,
            &timeline,
        )?);
    }
    for (group, definitions) in speed_definitions {
        let definitions = definitions
            .into_iter()
            .map(|(measure, tick, speed)| format!("{measure}'{tick}:{speed}"))
            .collect::<Vec<_>>();
        output.push_str(&format!(
            "#TIL{}: \"{}\"\n",
            definition_id(group as usize)?,
            definitions.join(",")
        ));
    }
    let mut context = ContextWriter::default();
    let mut long_note_channels = vec![Vec::<(Position, Position)>::new(); 36];
    for note_id in standard_note_order(chart) {
        let note = chart
            .note(note_id)
            .map_err(|_| SusError::UnrepresentablePosition)?;
        let channel = note_channel(note, &mut long_note_channels)?;
        match write_standard_note(note, ticks_per_beat, &timeline, channel) {
            Ok(lines) => {
                for (point_index, line) in lines.into_iter().enumerate() {
                    let metadata = if note.point_metadata().is_empty() {
                        chart::PointMetadata {
                            speed_group: chart
                                .note_speed_group(note_id)
                                .map_err(|_| SusError::UnrepresentablePosition)?,
                            attributes: note.attributes(),
                        }
                    } else {
                        chart
                            .point_metadata(note_id, point_index)
                            .map_err(|_| SusError::UnrepresentablePosition)?
                    };
                    context.write(
                        &mut output,
                        output_speed_group(chart, metadata.speed_group)?,
                        metadata.attributes,
                    )?;
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

#[derive(Default)]
struct ContextWriter {
    speed_group: Option<u32>,
    attributes: NoteAttributes,
    next_attribute: usize,
}

impl ContextWriter {
    fn write(
        &mut self,
        output: &mut String,
        group: Option<u32>,
        attributes: NoteAttributes,
    ) -> Result<(), SusError> {
        if group != self.speed_group {
            match group {
                Some(group) => {
                    output.push_str(&format!("#HISPEED {}\n", definition_id(group as usize)?))
                }
                None => output.push_str("#NOSPEED\n"),
            }
            self.speed_group = group;
        }
        if attributes != self.attributes {
            if attributes.is_empty() {
                output.push_str("#NOATTRIBUTE\n");
            } else {
                let id = definition_id(self.next_attribute)?;
                self.next_attribute += 1;
                output.push_str(&format!(
                    "#ATR{id}: \"{}\"\n#ATTRIBUTE {id}\n",
                    format_attributes(attributes)
                ));
            }
            self.attributes = attributes;
        }
        Ok(())
    }
}

fn output_speed_group(chart: &Chart, group: Option<u32>) -> Result<Option<u32>, SusError> {
    let Some(group) = group else {
        return Ok(None);
    };
    let group = speed_group(group)?;
    if chart
        .scroll_speed_changes()
        .iter()
        .any(|change| change.scope() == ScrollScope::Group(group) && change.duration().is_none())
    {
        Ok(Some(group))
    } else {
        report_loss("SUS", format!("note speed group {group}"));
        Ok(None)
    }
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
            NoteKind::Hold { end, checkpoints }
            | NoteKind::ExHold {
                end, checkpoints, ..
            } => {
                add_position(*end)?;
                for checkpoint in checkpoints {
                    add_position(*checkpoint)?;
                }
            }
            NoteKind::AirHold {
                end, checkpoints, ..
            } => {
                add_position(*end)?;
                for checkpoint in checkpoints {
                    add_position(*checkpoint)?;
                }
            }
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

fn xlair_ticks_per_beat(chart: &Chart, timeline: &MeasureTimeline) -> Result<u64, SusError> {
    let mut ticks = 96;
    for &(_, length) in chart.measure_lengths() {
        ticks = lcm(ticks, length.denominator()).ok_or(SusError::UnrepresentablePosition)?;
    }
    let mut add_position = |position: Position| -> Result<(), SusError> {
        let mut start = Position::new(0, 1).expect("valid position");
        let mut measure = 0;
        loop {
            let length = measure_length(chart, measure);
            let end = start
                .checked_add(length)
                .map_err(|_| SusError::UnrepresentablePosition)?;
            if position < end {
                let offset = Position::new(
                    position
                        .numerator()
                        .checked_mul(start.denominator())
                        .and_then(|left| {
                            start
                                .numerator()
                                .checked_mul(position.denominator())
                                .and_then(|right| left.checked_sub(right))
                        })
                        .ok_or(SusError::UnrepresentablePosition)?,
                    position
                        .denominator()
                        .checked_mul(start.denominator())
                        .ok_or(SusError::UnrepresentablePosition)?,
                )
                .map_err(|_| SusError::UnrepresentablePosition)?;
                // The output tick is an offset in beats, not a fraction of a measure.
                ticks =
                    lcm(ticks, offset.denominator()).ok_or(SusError::UnrepresentablePosition)?;
                return Ok(());
            }
            start = end;
            measure = measure
                .checked_add(1)
                .ok_or(SusError::UnrepresentablePosition)?;
        }
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
            NoteKind::Hold { end, checkpoints }
            | NoteKind::ExHold {
                end, checkpoints, ..
            } => {
                add_position(*end)?;
                for checkpoint in checkpoints {
                    add_position(*checkpoint)?;
                }
            }
            NoteKind::AirHold {
                end, checkpoints, ..
            } => {
                add_position(*end)?;
                for checkpoint in checkpoints {
                    add_position(*checkpoint)?;
                }
            }
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
    timeline
        .ticks(0, ticks)
        .map_err(|_| SusError::UnrepresentablePosition)?;
    Ok(ticks)
}

fn measure_length(chart: &Chart, measure: u32) -> Position {
    chart
        .measure_lengths()
        .iter()
        .rev()
        .find(|(changed_measure, _)| *changed_measure <= measure)
        .map(|(_, length)| *length)
        .unwrap_or_else(|| Position::new(4, 1).expect("valid position"))
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
    note: &Note,
    ticks_per_beat: u64,
    timeline: &MeasureTimeline,
    channel: char,
) -> Result<Vec<String>, SusError> {
    let Lane::Slider { start, width } = note.lane() else {
        return Err(SusError::UnsupportedNote {
            note: "side lane in standard SUS".to_owned(),
        });
    };
    let mut lines = Vec::new();
    match note.kind() {
        NoteKind::Tap(kind) => match kind {
            TapKind::Tap => lines.push(standard_channel_line(
                note.position(),
                '1',
                start,
                '1',
                width,
                ticks_per_beat,
                timeline,
            )?),
            TapKind::XTap => lines.push(standard_channel_line(
                note.position(),
                '1',
                start,
                '2',
                width,
                ticks_per_beat,
                timeline,
            )?),
            TapKind::Flick { .. } => lines.push(standard_channel_line(
                note.position(),
                '1',
                start,
                '3',
                width,
                ticks_per_beat,
                timeline,
            )?),
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
            let direction = standard_ex_direction(*direction)?;
            lines.push(standard_channel_line(
                note.position(),
                '5',
                start,
                direction,
                width,
                ticks_per_beat,
                timeline,
            )?);
        }
        NoteKind::Mine => {
            return Err(SusError::UnsupportedNote {
                note: "mine in standard SUS".to_owned(),
            });
        }
        NoteKind::Hold { end, checkpoints } => {
            if !checkpoints.is_empty() {
                report_loss("SUS", "hold checkpoints in standard SUS");
            }
            let mut events = vec![(note.position(), '1', start, width)];
            events.extend(
                checkpoints
                    .iter()
                    .map(|position| (*position, '3', start, width)),
            );
            events.push((*end, '2', start, width));
            for (position, token, lane, width) in events {
                lines.push(standard_event_channel_line(
                    StandardChannelEvent {
                        position,
                        kind: '2',
                        lane,
                        channel,
                        token,
                        width,
                    },
                    ticks_per_beat,
                    timeline,
                )?);
            }
        }
        NoteKind::ExHold {
            end, checkpoints, ..
        } => {
            report_loss("SUS", "extended hold direction in standard SUS");
            if !checkpoints.is_empty() {
                report_loss("SUS", "hold checkpoints in standard SUS");
            }
            let mut events = vec![(note.position(), '1', start, width)];
            events.extend(
                checkpoints
                    .iter()
                    .map(|position| (*position, '3', start, width)),
            );
            events.push((*end, '2', start, width));
            for (position, token, lane, width) in events {
                lines.push(standard_event_channel_line(
                    StandardChannelEvent {
                        position,
                        kind: '2',
                        lane,
                        channel,
                        token,
                        width,
                    },
                    ticks_per_beat,
                    timeline,
                )?);
            }
        }
        NoteKind::Slide { points } | NoteKind::ExSlide { points, .. } => {
            if matches!(note.kind(), NoteKind::ExSlide { .. }) {
                report_loss("SUS", "extended slide direction in standard SUS");
            }
            for (index, point) in points.iter().enumerate() {
                let Lane::Slider {
                    start: point_lane,
                    width: point_width,
                } = point.lane()
                else {
                    return Err(SusError::UnsupportedNote {
                        note: "side slide lane in standard SUS".to_owned(),
                    });
                };
                let token = if index == 0 {
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
                lines.push(standard_event_channel_line(
                    StandardChannelEvent {
                        position: point.position(),
                        kind: '3',
                        lane: point_lane,
                        channel,
                        token,
                        width: point_width,
                    },
                    ticks_per_beat,
                    timeline,
                )?);
            }
        }
        NoteKind::Air { properties, .. } => {
            if properties.height().is_some() || properties.color() != AirColor::Normal {
                report_loss("SUS", "AIR height or color");
            }
            let direction = match properties.direction() {
                Some(AirDirection::Up) => '1',
                Some(AirDirection::Down) => '2',
                Some(AirDirection::UpperLeft) => '3',
                Some(AirDirection::UpperRight) => '4',
                Some(AirDirection::LowerLeft) => '5',
                Some(AirDirection::LowerRight) => '6',
                None => {
                    return Err(SusError::UnsupportedNote {
                        note: "directionless AIR in standard SUS".to_owned(),
                    });
                }
            };
            lines.push(standard_channel_line(
                note.position(),
                '5',
                start,
                direction,
                width,
                ticks_per_beat,
                timeline,
            )?);
        }
        NoteKind::AirHold { .. } => {
            return Err(SusError::UnsupportedNote {
                note: "AIR hold notation in standard SUS".to_owned(),
            });
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
    let slots = timeline
        .ticks(measure, ticks_per_beat)
        .map_err(|_| SusError::UnrepresentablePosition)?;
    let (tick, slots) = compact_data_position(tick, slots)?;
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

fn standard_timing_channel_line(
    measure: u32,
    tick: u64,
    definition: &str,
    ticks_per_beat: u64,
    timeline: &MeasureTimeline,
) -> Result<String, SusError> {
    let slots = timeline
        .ticks(measure, ticks_per_beat)
        .map_err(|_| SusError::UnrepresentablePosition)?;
    let (tick, slots) = compact_data_position(tick, slots)?;
    let trailing = slots
        .checked_sub(tick + 1)
        .ok_or(SusError::UnrepresentablePosition)?;
    Ok(format!(
        "#{measure:03}08: {}{definition}{}\n",
        "00".repeat(tick),
        "00".repeat(trailing),
    ))
}

struct StandardChannelEvent {
    position: Position,
    kind: char,
    lane: u8,
    channel: char,
    token: char,
    width: u8,
}

fn standard_event_channel_line(
    event: StandardChannelEvent,
    ticks_per_beat: u64,
    timeline: &MeasureTimeline,
) -> Result<String, SusError> {
    let (measure, tick) = standard_position(event.position, ticks_per_beat, timeline)?;
    let slots = timeline
        .ticks(measure, ticks_per_beat)
        .map_err(|_| SusError::UnrepresentablePosition)?;
    let (tick, slots) = compact_data_position(tick, slots)?;
    let trailing = slots
        .checked_sub(tick + 1)
        .ok_or(SusError::UnrepresentablePosition)?;
    Ok(format!(
        "#{measure:03}{}{}{}: {}{}{}{}",
        event.kind,
        base36_digit(event.lane),
        event.channel,
        "00".repeat(tick),
        event.token,
        base36_digit(event.width),
        "00".repeat(trailing),
    ))
}

/// Reduces a tick grid to the fewest SUS divisions that preserve the exact position.
fn compact_data_position(tick: u64, slots: u64) -> Result<(usize, usize), SusError> {
    if slots == 0 || tick >= slots {
        return Err(SusError::UnrepresentablePosition);
    }
    let divisor = gcd(tick, slots);
    let tick = usize::try_from(tick / divisor).map_err(|_| SusError::UnrepresentablePosition)?;
    let slots = usize::try_from(slots / divisor).map_err(|_| SusError::UnrepresentablePosition)?;
    Ok((tick, slots))
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
    attributes: NoteAttributes,
}

struct XlairTiming<'a> {
    timeline: &'a MeasureTimeline,
    ticks_per_beat: u64,
}

struct HoldSpan<'a> {
    start: Position,
    end: Position,
    checkpoints: &'a [Position],
}

fn add_hold_records(
    records: &mut Vec<Record>,
    hold: HoldSpan<'_>,
    lane: u8,
    width: u8,
    channel: char,
    timing: &XlairTiming<'_>,
) -> Result<(), SusError> {
    let (start_measure, start_tick) = output_position(hold.start, timing)?;
    let (end_measure, end_tick) = output_position(hold.end, timing)?;
    for checkpoint in hold.checkpoints {
        let (measure, tick) = output_position(*checkpoint, timing)?;
        records.push(Record {
            measure,
            tick,
            key: format!("3{}{channel}", base36_digit(lane)),
            token: format!("3{}", base36_digit(width)),
            speed_group: None,
            attributes: NoteAttributes::default(),
        });
    }
    records.push(Record {
        measure: start_measure,
        tick: start_tick,
        key: format!("3{}{channel}", base36_digit(lane)),
        token: format!("1{}", base36_digit(width)),
        speed_group: None,
        attributes: NoteAttributes::default(),
    });
    records.push(Record {
        measure: end_measure,
        tick: end_tick,
        key: format!("3{}{channel}", base36_digit(lane)),
        token: format!("2{}", base36_digit(width)),
        speed_group: None,
        attributes: NoteAttributes::default(),
    });
    Ok(())
}

fn add_side_hold_records(
    records: &mut Vec<Record>,
    hold: HoldSpan<'_>,
    button: SideButton,
    channel: char,
    timing: &XlairTiming<'_>,
) -> Result<(), SusError> {
    let (start_measure, start_tick) = output_position(hold.start, timing)?;
    let (end_measure, end_tick) = output_position(hold.end, timing)?;
    let lane = side_lane(button);
    for checkpoint in hold.checkpoints {
        let (measure, tick) = output_position(*checkpoint, timing)?;
        records.push(Record {
            measure,
            tick,
            key: format!("2{}{channel}", base36_digit(lane)),
            token: "31".to_owned(),
            speed_group: None,
            attributes: NoteAttributes::default(),
        });
    }
    records.push(Record {
        measure: start_measure,
        tick: start_tick,
        key: format!("2{}{channel}", base36_digit(lane)),
        token: "11".to_owned(),
        speed_group: None,
        attributes: NoteAttributes::default(),
    });
    records.push(Record {
        measure: end_measure,
        tick: end_tick,
        key: format!("2{}{channel}", base36_digit(lane)),
        token: "21".to_owned(),
        speed_group: None,
        attributes: NoteAttributes::default(),
    });
    Ok(())
}

fn add_slide_records(
    records: &mut Vec<Record>,
    points: &[SlidePoint],
    channel: char,
    timing: &XlairTiming<'_>,
) -> Result<(), SusError> {
    for (index, point) in points.iter().enumerate() {
        let lane = central_lane(point.lane(), "slide")?;
        let (measure, tick) = output_position(point.position(), timing)?;
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
            attributes: NoteAttributes::default(),
        });
    }
    Ok(())
}

fn format_record(record: &Record, timing: &XlairTiming<'_>) -> Result<String, SusError> {
    let slots = timing
        .timeline
        .ticks(record.measure, timing.ticks_per_beat)
        .map_err(|_| SusError::UnrepresentablePosition)?;
    let (tick, slots) = compact_data_position(record.tick, slots)?;
    let trailing = slots
        .checked_sub(tick + 1)
        .ok_or(SusError::UnrepresentablePosition)?;
    Ok(format!(
        "#{:03}{}: {}{}{}\n",
        record.measure,
        record.key,
        "00".repeat(tick),
        record.token,
        "00".repeat(trailing),
    ))
}

fn output_position(position: Position, timing: &XlairTiming<'_>) -> Result<(u32, u64), SusError> {
    timing
        .timeline
        .locate(position, timing.ticks_per_beat)
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

fn note_channel(
    note: &Note,
    long_note_channels: &mut [Vec<(Position, Position)>],
) -> Result<char, SusError> {
    let is_long = matches!(
        note.kind(),
        NoteKind::Hold { .. }
            | NoteKind::ExHold { .. }
            | NoteKind::Slide { .. }
            | NoteKind::ExSlide { .. }
    );
    if !is_long {
        return Ok('0');
    }

    let start = note.position();
    let end = note_end_position(note);
    let channel_index = long_note_channels
        .iter()
        .position(|intervals| {
            intervals
                .iter()
                .all(|&(other_start, other_end)| end < other_start || other_end < start)
        })
        .ok_or(SusError::UnrepresentablePosition)?;
    long_note_channels[channel_index].push((start, end));
    channel(channel_index)
}

fn record_kind_order(record: &Record) -> u8 {
    match record.token.as_bytes().first().copied() {
        Some(b'1') => 0,
        Some(b'3'..=b'5') => 1,
        Some(b'2') => 2,
        _ => 0,
    }
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

fn definition_id(index: usize) -> Result<String, SusError> {
    let id = u32::try_from(index)
        .ok()
        .and_then(|index| index.checked_add(1))
        .filter(|id| *id < 36 * 36)
        .ok_or(SusError::UnrepresentablePosition)?;
    Ok(speed_group_text(id))
}

fn speed_group_text(group: u32) -> String {
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let high = digits[(group / 36) as usize] as char;
    let low = digits[(group % 36) as usize] as char;
    format!("{high}{low}")
}
