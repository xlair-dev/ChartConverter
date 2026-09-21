use std::collections::BTreeMap;

use chart::{
    AirDirection, Chart, ChartMode, ExDirection, Lane, MeasureTimeline, Note, NoteAttributes,
    NoteId, NoteKind, Position, ScrollScope, SideButton, SlidePoint, SlidePointKind, TapKind,
    report_loss,
};

use crate::{
    SusError,
    syntax::{note_end_position, side_direction, side_lane, standard_parent_code},
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
                let id = format!("{:02}", index + 1);
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
                NoteKind::Hold { end } => match note.lane() {
                    Lane::Slider { start, width } => add_hold_records(
                        &mut records,
                        note.position(),
                        *end,
                        start,
                        width,
                        channel,
                        &timing,
                    )?,
                    Lane::Side(button) => add_side_hold_records(
                        &mut records,
                        note.position(),
                        *end,
                        button,
                        channel,
                        &timing,
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
                        &timing,
                    )?,
                    Lane::Side(button) => add_side_hold_records(
                        &mut records,
                        note.position(),
                        *end,
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
                record.attributes = note.attributes();
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
    let header_ticks = ticks_per_beat
        .checked_mul(4)
        .ok_or(SusError::UnrepresentablePosition)?;
    let mut output = format!("#REQUEST \"ticks_per_beat {header_ticks}\"\n");
    if let Some(enabled) = chart.priority_enabled() {
        output.push_str(&format!(
            "#REQUEST \"enable_priority {}\"\n",
            if enabled { "true" } else { "false" }
        ));
    }
    if let Some(base_bpm) = chart.base_bpm() {
        output.push_str(&format!("#BASEBPM {base_bpm}\n"));
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
            speed_group_text(group),
            definitions.join(",")
        ));
    }
    let mut current_speed_group = None;
    let mut current_attributes = NoteAttributes::default();
    let mut attribute_index = 0u32;
    for record in records {
        let text = match format_record(&record, &timing) {
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
        if record.attributes != current_attributes {
            if record.attributes.is_empty() {
                output.push_str("#NOATTRIBUTE\n");
            } else {
                let id = speed_group_text(attribute_index);
                attribute_index = attribute_index
                    .checked_add(1)
                    .ok_or(SusError::UnrepresentablePosition)?;
                output.push_str(&format!(
                    "#ATR{id}: \"{}\"\n#ATTRIBUTE {id}\n",
                    format_attributes(record.attributes)
                ));
            }
            current_attributes = record.attributes;
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
    if let Some(enabled) = chart.priority_enabled() {
        output.push_str(&format!(
            "#REQUEST \"enable_priority {}\"\n",
            if enabled { "true" } else { "false" }
        ));
    }
    if let Some(base_bpm) = chart.base_bpm() {
        output.push_str(&format!("#BASEBPM {base_bpm}\n"));
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
                let ratio = Position::new(
                    offset
                        .numerator()
                        .checked_mul(length.denominator())
                        .ok_or(SusError::UnrepresentablePosition)?,
                    offset
                        .denominator()
                        .checked_mul(length.numerator())
                        .ok_or(SusError::UnrepresentablePosition)?,
                )
                .map_err(|_| SusError::UnrepresentablePosition)?;
                let required = ratio.denominator() / gcd(ratio.denominator(), 4);
                ticks = lcm(ticks, required).ok_or(SusError::UnrepresentablePosition)?;
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
    attributes: NoteAttributes,
}

struct XlairTiming<'a> {
    timeline: &'a MeasureTimeline,
    ticks_per_beat: u64,
}

fn add_hold_records(
    records: &mut Vec<Record>,
    start: Position,
    end: Position,
    lane: u8,
    width: u8,
    channel: char,
    timing: &XlairTiming<'_>,
) -> Result<(), SusError> {
    let (start_measure, start_tick) = output_position(start, timing)?;
    let (end_measure, end_tick) = output_position(end, timing)?;
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
    start: Position,
    end: Position,
    button: SideButton,
    channel: char,
    timing: &XlairTiming<'_>,
) -> Result<(), SusError> {
    let (start_measure, start_tick) = output_position(start, timing)?;
    let (end_measure, end_tick) = output_position(end, timing)?;
    let lane = side_lane(button);
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
    let padding = usize::try_from(record.tick).map_err(|_| SusError::UnrepresentablePosition)?;
    let slots = usize::try_from(
        timing
            .timeline
            .ticks(record.measure, timing.ticks_per_beat)
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

fn speed_group_text(group: u32) -> String {
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let high = digits[(group / 36) as usize] as char;
    let low = digits[(group % 36) as usize] as char;
    format!("{high}{low}")
}
