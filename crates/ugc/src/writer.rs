use std::collections::BTreeSet;

use chart::{
    AirColor, AirCrushColor, AirCrushInterval, AirDirection, AirProperties, Chart, ChartMode,
    ExDirection, Lane, MeasureTimeline, Note, NoteId, NoteKind, Position, ScrollScope,
    SlidePointKind, TapKind, report_loss,
};

use crate::{UgcError, syntax::is_xlair_side_air_direction};

pub(super) fn write_with_mode(chart: &Chart, mode: ChartMode) -> Result<String, UgcError> {
    if chart.priority_enabled().is_some() {
        report_loss("UGC", "SUS enable_priority request");
    }
    let base_bpm = chart.base_bpm();
    let timeline = output_timeline(chart);
    let mut tempo_records = Vec::new();
    for tempo in chart.tempo_changes() {
        match output_position(tempo.position(), &timeline) {
            Ok((measure, tick)) => tempo_records.push((
                measure,
                tick,
                format!("@BPM\t{measure}'{tick}\t{:.6}\n", tempo.bpm()),
            )),
            Err(error) if is_loss(&error) => report_loss("UGC", error),
            Err(error) => return Err(error),
        }
    }

    let mut speed_records = Vec::new();
    let mut speed_groups = BTreeSet::new();
    for change in chart.scroll_speed_changes() {
        let record = (|| {
            if change.duration().is_some() {
                return Err(UgcError::UnsupportedNote {
                    note: "scroll speed duration".to_owned(),
                });
            }
            let (measure, tick) = output_position(change.position(), &timeline)?;
            let speed = format!("{:.6}", change.speed());
            let text = match change.scope() {
                ScrollScope::Global => format!("@SPDMOD\t{measure}'{tick}\t{speed}\n"),
                ScrollScope::Group(group) => {
                    format!("@TIL\t{group}\t{measure}'{tick}\t{speed}\n")
                }
                _ => {
                    return Err(UgcError::UnsupportedNote {
                        note: "unsupported scroll speed scope".to_owned(),
                    });
                }
            };
            Ok((measure, tick, text))
        })();
        match record {
            Ok(record) => {
                if let ScrollScope::Group(group) = change.scope() {
                    speed_groups.insert(group);
                }
                speed_records.push(record);
            }
            Err(error) if is_loss(&error) => report_loss("UGC", error),
            Err(error) => return Err(error),
        }
    }

    let mut note_records = Vec::new();
    for (index, note) in chart.notes().iter().enumerate() {
        let note_id = NoteId::new(index as u32);
        match write_note(chart, note_id, note, mode, &timeline) {
            Ok(mut record) => {
                if let Some(group) = record.speed_group
                    && !speed_groups.contains(&group)
                {
                    report_loss("UGC", format!("note speed group {group}"));
                    record.speed_group = None;
                }
                note_records.push(record);
            }
            Err(error) if is_loss(&error) => report_loss("UGC", error),
            Err(error) => return Err(error),
        }
    }
    tempo_records.sort_by_key(|(measure, tick, _)| (*measure, *tick));
    speed_records.sort_by_key(|(measure, tick, _)| (*measure, *tick));
    note_records.sort_by_key(|record| {
        (
            record.measure,
            record.tick,
            record.parent_order,
            record.is_air,
            record.order,
        )
    });

    let mut output = String::from("@VER\t8\n@EXVER\t1\n@TICKS\t480\n");
    if let Some(base_bpm) = base_bpm {
        output.push_str(&format!("@MAINBPM\t{base_bpm}\n"));
    }
    if chart.measure_lengths().is_empty() {
        output.push_str("@BEAT\t0\t4\t4\n");
    } else {
        for &(measure, length) in chart.measure_lengths() {
            let denominator = length
                .denominator()
                .checked_mul(4)
                .ok_or(UgcError::UnrepresentablePosition)?;
            output.push_str(&format!(
                "@BEAT\t{measure}\t{}\t{denominator}\n",
                length.numerator()
            ));
        }
    }
    for (_, _, text) in tempo_records {
        output.push_str(&text);
    }
    for (_, _, text) in speed_records {
        output.push_str(&text);
    }
    output.push_str("@ENDHEAD\n");
    let mut current_speed_group = None;
    for record in note_records {
        if record.speed_group != current_speed_group {
            if let Some(group) = record.speed_group {
                output.push_str(&format!("@USETIL\t{group}\n"));
            } else if current_speed_group.is_some() {
                output.push_str("@USETIL\t0\n");
            }
            current_speed_group = record.speed_group;
        }
        output.push_str(&record.text);
    }
    Ok(output)
}

struct OutputRecord {
    measure: u32,
    tick: u64,
    order: u32,
    parent_order: u32,
    is_air: bool,
    speed_group: Option<u32>,
    text: String,
}

fn write_note(
    chart: &Chart,
    note_id: NoteId,
    note: &Note,
    mode: ChartMode,
    timeline: &MeasureTimeline,
) -> Result<OutputRecord, UgcError> {
    // XLAIR reuses UGC's diagonal AIR records for its side-device actions.
    if !note.attributes().is_empty() {
        report_loss("UGC", "SUS note attributes");
    }
    if mode == ChartMode::Xlair
        && matches!(
            note.kind(),
            NoteKind::Air { properties, .. }
                if !properties
                    .direction()
                    .is_some_and(is_xlair_side_air_direction)
        )
    {
        return Err(UgcError::UnsupportedNote {
            note: "non-side AIR notation in XLAIR mode".to_owned(),
        });
    }
    if mode == ChartMode::Xlair
        && matches!(
            note.kind(),
            NoteKind::AirHold { .. } | NoteKind::AirSlide { .. } | NoteKind::AirCrush { .. }
        )
    {
        return Err(UgcError::UnsupportedNote {
            note: "AIR long notation in XLAIR mode".to_owned(),
        });
    }
    if mode == ChartMode::Xlair
        && let (Lane::Side(button), NoteKind::Tap(TapKind::Tap)) = (note.lane(), note.kind())
    {
        return write_xlair_side_tap(chart, note_id, note, button, mode, timeline);
    }
    let (measure, tick) = output_position(note.position(), timeline)?;
    let (lane, width) = match (mode, note.lane(), note.kind()) {
        (ChartMode::Xlair, Lane::Side(button), NoteKind::Hold { .. }) => {
            (button.xlair_lane_start(), 2)
        }
        (_, lane, _) => central_lane(lane)?,
    };
    let prefix = format!("#{measure}'{tick}:");
    let is_air = matches!(
        note.kind(),
        NoteKind::Air { .. }
            | NoteKind::AirHold { .. }
            | NoteKind::AirSlide { .. }
            | NoteKind::AirCrush { .. }
    );
    let (parent, text) = match note.kind() {
        NoteKind::Air { parent, .. }
        | NoteKind::AirHold { parent, .. }
        | NoteKind::AirSlide { parent, .. }
        | NoteKind::AirCrush { parent, .. } => (Some(*parent), write_air_note(note, &prefix)?),
        _ => {
            let ex_direction = match note.kind() {
                NoteKind::ExHold { direction, .. } | NoteKind::ExSlide { direction, .. } => {
                    Some(*direction)
                }
                _ => None,
            };
            let include_ex_carrier = mode != ChartMode::Xlair
                && ex_direction.is_some_and(|direction| {
                    !chart.notes().iter().enumerate().any(|(index, candidate)| {
                        index != note_id.value() as usize
                            && candidate.position() == note.position()
                            && candidate.lane() == note.lane()
                            && matches!(
                                candidate.kind(),
                                NoteKind::ExTap {
                                    direction: candidate_direction
                                } if *candidate_direction == direction
                            )
                    })
                });
            (
                None,
                write_non_air_note(note, &prefix, lane, width, include_ex_carrier, mode)?,
            )
        }
    };
    if let Some(parent) = parent {
        let parent_note = chart.note(parent).map_err(|_| UgcError::InvalidValue {
            line: 0,
            value: "invalid AIR parent".to_owned(),
        })?;
        if parent_note.position() > note.position() {
            return Err(UgcError::InvalidValue {
                line: 0,
                value: "AIR parent appears after its child".to_owned(),
            });
        }
    }
    let speed_group = chart
        .note_speed_group(note_id)
        .map_err(|_| UgcError::InvalidValue {
            line: 0,
            value: "invalid note id".to_owned(),
        })?;
    Ok(OutputRecord {
        measure,
        tick,
        order: note_id.value(),
        parent_order: parent.map_or(note_id.value(), NoteId::value),
        is_air,
        speed_group,
        text,
    })
}

/// Encodes a side tap using a non-overlapping carrier tap and its diagonal AIR child.
fn write_xlair_side_tap(
    chart: &Chart,
    note_id: NoteId,
    note: &Note,
    button: chart::SideButton,
    mode: ChartMode,
    timeline: &MeasureTimeline,
) -> Result<OutputRecord, UgcError> {
    let width = 1;
    let lane = (0..16)
        .find(|start| {
            let candidate = Lane::slider(*start, width).expect("one-lane slider is valid");
            !chart.notes().iter().any(|other| {
                other.position() == note.position()
                    && candidate.overlaps(other.lane())
                    && matches!(other.kind(), NoteKind::Tap(_))
            })
        })
        .ok_or_else(|| UgcError::UnsupportedNote {
            note: "XLAIR side tap has no available UGC carrier lane".to_owned(),
        })?;
    let (measure, tick) = output_position(note.position(), timeline)?;
    let prefix = format!("#{measure}'{tick}:");
    let mut text = write_non_air_note(note, &prefix, lane, width, false, mode)?;
    let direction = match button {
        chart::SideButton::LeftUpper => "UL",
        chart::SideButton::RightUpper => "UR",
        chart::SideButton::LeftLower => "DL",
        chart::SideButton::RightLower => "DR",
    };
    text.push_str(&format!(
        "{prefix}a{}{}{}N\n",
        encode_base36(lane),
        encode_base36(width),
        direction
    ));
    let speed_group = chart
        .note_speed_group(note_id)
        .map_err(|_| UgcError::InvalidValue {
            line: 0,
            value: "invalid note id".to_owned(),
        })?;
    Ok(OutputRecord {
        measure,
        tick,
        order: note_id.value(),
        parent_order: note_id.value(),
        is_air: false,
        speed_group,
        text,
    })
}

fn write_non_air_note(
    note: &Note,
    prefix: &str,
    lane: u8,
    width: u8,
    include_ex_carrier: bool,
    mode: ChartMode,
) -> Result<String, UgcError> {
    let text = match note.kind() {
        NoteKind::Tap(kind) => {
            let type_code = match kind {
                TapKind::Tap => 't',
                TapKind::XTap => 'x',
                TapKind::Flick { .. } => 'f',
                TapKind::Tap4 | TapKind::Tap5 | TapKind::Tap6 => {
                    return Err(UgcError::UnsupportedNote {
                        note: "SUS tap variant".to_owned(),
                    });
                }
            };
            let suffix = match kind {
                TapKind::Tap => "".to_owned(),
                TapKind::XTap => "".to_owned(),
                TapKind::Flick { direction } => encode_flick_direction(*direction)?,
                TapKind::Tap4 | TapKind::Tap5 | TapKind::Tap6 => unreachable!(),
            };
            format!(
                "{prefix}{type_code}{}{}{suffix}\n",
                encode_base36(lane),
                encode_base36(width)
            )
        }
        NoteKind::ExTap { .. } if mode == ChartMode::Xlair => {
            format!("{prefix}x{}{}\n", encode_base36(lane), encode_base36(width),)
        }
        NoteKind::ExTap { direction } => {
            let direction = encode_ex_direction(*direction)?;
            format!(
                "{prefix}x{}{}{}\n",
                encode_base36(lane),
                encode_base36(width),
                direction
            )
        }
        NoteKind::Mine => format!("{prefix}d{}{}\n", encode_base36(lane), encode_base36(width)),
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
        NoteKind::ExHold { end, direction } => {
            let offset = relative_tick(note.position(), *end)?;
            let direction = encode_ex_direction(*direction)?;
            let carrier = if include_ex_carrier {
                format!(
                    "{prefix}x{}{}{}\n",
                    encode_base36(lane),
                    encode_base36(width),
                    direction
                )
            } else {
                String::new()
            };
            format!(
                "{carrier}{prefix}h{}{}\n#{}>s{}{}\n",
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
                    "#{}>{}{}{}\n",
                    offset,
                    encode_slide_point_marker(point.kind())?,
                    encode_base36(point_lane),
                    encode_base36(point_width),
                ));
            }
            text
        }
        NoteKind::ExSlide { points, direction } => {
            let direction = encode_ex_direction(*direction)?;
            let carrier = if include_ex_carrier {
                format!(
                    "{prefix}x{}{}{}\n",
                    encode_base36(lane),
                    encode_base36(width),
                    direction
                )
            } else {
                String::new()
            };
            let mut text = format!(
                "{carrier}{prefix}s{}{}\n",
                encode_base36(lane),
                encode_base36(width),
            );
            for point in points.iter().skip(1) {
                let (point_lane, point_width) = central_lane(point.lane())?;
                let offset = relative_tick(note.position(), point.position())?;
                text.push_str(&format!(
                    "#{}>{}{}{}\n",
                    offset,
                    encode_slide_point_marker(point.kind())?,
                    encode_base36(point_lane),
                    encode_base36(point_width),
                ));
            }
            text
        }
        NoteKind::Air { .. } | NoteKind::AirHold { .. } | NoteKind::AirSlide { .. } => {
            unreachable!("AIR notes are written by write_air_note")
        }
        NoteKind::AirCrush { .. } => {
            return Err(UgcError::UnsupportedNote {
                note: "AIR Crush".to_owned(),
            });
        }
    };
    Ok(text)
}

fn write_air_note(note: &Note, prefix: &str) -> Result<String, UgcError> {
    match note.kind() {
        NoteKind::Air { properties, .. } => {
            let (lane, width) = central_lane(note.lane())?;
            properties.direction().ok_or(UgcError::UnsupportedNote {
                note: "AIR direction".to_owned(),
            })?;
            let attributes = encode_air_attributes(*properties)?;
            Ok(format!(
                "{prefix}a{}{}{}\n",
                encode_base36(lane),
                encode_base36(width),
                attributes
            ))
        }
        NoteKind::AirHold {
            end, properties, ..
        } => {
            let (lane, width) = central_lane(note.lane())?;
            let attributes = encode_air_attributes(*properties)?;
            Ok(format!(
                "{prefix}H{}{}{}\n#{}>s{}{}\n",
                encode_base36(lane),
                encode_base36(width),
                attributes,
                relative_tick(note.position(), *end)?,
                encode_base36(lane),
                encode_base36(width)
            ))
        }
        NoteKind::AirSlide { points, color, .. } => {
            if points.len() < 2 {
                return Err(UgcError::UnsupportedNote {
                    note: "AIR Slide without an end point".to_owned(),
                });
            }
            let (lane, width) = central_lane(note.lane())?;
            let attributes = encode_air_attributes(
                AirProperties::without_direction()
                    .with_height(points[0].height())
                    .map_err(|source| UgcError::Chart { line: 0, source })?
                    .with_color(*color),
            )?;
            let mut text = format!(
                "{prefix}S{}{}{}\n",
                encode_base36(lane),
                encode_base36(width),
                attributes
            );
            for point in points.iter().skip(1) {
                let (point_lane, point_width) = central_lane(point.lane())?;
                let marker = encode_slide_point_marker(point.kind())?;
                text.push_str(&format!(
                    "#{}>{marker}{}{}{}\n",
                    relative_tick(note.position(), point.position())?,
                    encode_base36(point_lane),
                    encode_base36(point_width),
                    encode_air_height(point.height())?
                ));
            }
            Ok(text)
        }
        NoteKind::AirCrush {
            points,
            color,
            interval,
            ..
        } => {
            let (lane, width) = central_lane(note.lane())?;
            let start = &points[0];
            let interval = match interval {
                AirCrushInterval::Trace => "0".to_owned(),
                AirCrushInterval::Start => "$".to_owned(),
                AirCrushInterval::Every(interval) => {
                    relative_tick(Position::new(0, 1).unwrap(), *interval)?.to_string()
                }
            };
            let mut text = format!(
                "{prefix}C{}{}{}{},{}\n",
                encode_base36(lane),
                encode_base36(width),
                encode_air_height(start.height())?,
                encode_air_crush_color(*color),
                interval
            );
            for point in points.iter().skip(1) {
                let (point_lane, point_width) = central_lane(point.lane())?;
                let offset = relative_tick(note.position(), point.position())?;
                text.push_str(&format!(
                    "#{}>c{}{}{}\n",
                    offset,
                    encode_base36(point_lane),
                    encode_base36(point_width),
                    encode_air_height(point.height())?
                ));
            }
            Ok(text)
        }
        _ => unreachable!("non-AIR note passed to write_air_note"),
    }
}

fn encode_slide_point_marker(kind: &SlidePointKind) -> Result<char, UgcError> {
    match kind {
        SlidePointKind::Visible => Ok('s'),
        SlidePointKind::Control => Ok('c'),
        SlidePointKind::Invisible => Err(UgcError::UnsupportedNote {
            note: "invisible slide point".to_owned(),
        }),
    }
}

fn central_lane(lane: Lane) -> Result<(u8, u8), UgcError> {
    match lane {
        Lane::Slider { start, width } => Ok((start, width)),
        Lane::Side(_) => Err(UgcError::UnsupportedNote {
            note: "side lane".to_owned(),
        }),
    }
}

fn output_position(position: Position, timeline: &MeasureTimeline) -> Result<(u32, u64), UgcError> {
    timeline
        .locate(position, 480)
        .map_err(|_| UgcError::UnrepresentablePosition)
}

fn output_timeline(chart: &Chart) -> MeasureTimeline {
    let mut timeline = MeasureTimeline::new(Position::new(4, 1).expect("valid position"));
    for &(measure, length) in chart.measure_lengths() {
        timeline.set_length(measure, length);
    }
    timeline
}

fn is_loss(error: &UgcError) -> bool {
    matches!(
        error,
        UgcError::UnsupportedNote { .. } | UgcError::UnrepresentablePosition
    )
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

fn encode_ex_direction(direction: ExDirection) -> Result<&'static str, UgcError> {
    match direction {
        ExDirection::Up => Ok("U"),
        ExDirection::Down => Ok("D"),
        ExDirection::Center => Ok("C"),
        ExDirection::All => Ok("A"),
        ExDirection::Wide => Ok("W"),
        ExDirection::Left => Ok("L"),
        ExDirection::Right => Ok("R"),
        ExDirection::Inward => Ok("I"),
        ExDirection::UpperLeft
        | ExDirection::UpperRight
        | ExDirection::LowerLeft
        | ExDirection::LowerRight => Err(UgcError::UnsupportedNote {
            note: "diagonal Ex direction".to_owned(),
        }),
    }
}

fn encode_flick_direction(direction: Option<ExDirection>) -> Result<String, UgcError> {
    match direction {
        None => Ok(String::new()),
        Some(ExDirection::All) => Ok("A".to_owned()),
        Some(ExDirection::Left) => Ok("L".to_owned()),
        Some(ExDirection::Right) => Ok("R".to_owned()),
        Some(_) => Err(UgcError::UnsupportedNote {
            note: "unsupported Flick direction".to_owned(),
        }),
    }
}

fn encode_air_attributes(properties: AirProperties) -> Result<String, UgcError> {
    let direction = properties.direction().map(|direction| match direction {
        AirDirection::Up => "UC",
        AirDirection::UpperLeft => "UL",
        AirDirection::UpperRight => "UR",
        AirDirection::Down => "DC",
        AirDirection::LowerLeft => "DL",
        AirDirection::LowerRight => "DR",
    });
    let height = properties
        .height()
        .map(encode_air_height)
        .transpose()?
        .unwrap_or_default();
    let color = match properties.color() {
        AirColor::Normal => 'N',
        AirColor::Inverted => 'I',
    };
    Ok(format!("{}{}{}", direction.unwrap_or(""), height, color))
}

fn encode_air_crush_color(color: AirCrushColor) -> char {
    match color {
        AirCrushColor::Normal => '0',
        AirCrushColor::Transparent => 'Z',
        AirCrushColor::Red => '1',
        AirCrushColor::Orange => '2',
        AirCrushColor::Yellow => '3',
        AirCrushColor::Lime => '4',
        AirCrushColor::Green => '5',
        AirCrushColor::Aqua => '6',
        AirCrushColor::Cyan => '7',
        AirCrushColor::DarkBlue => '8',
        AirCrushColor::Blue => '9',
        AirCrushColor::Violet => 'A',
        AirCrushColor::Purple => 'Y',
        AirCrushColor::Pink => 'B',
        AirCrushColor::Gray => 'C',
        AirCrushColor::Black => 'D',
    }
}

fn encode_air_height(height: f64) -> Result<String, UgcError> {
    if !height.is_finite() || height < 1.0 {
        return Err(UgcError::UnsupportedNote {
            note: "AIR height cannot be represented by UGC".to_owned(),
        });
    }
    let raw = (height - 1.0) * 20.0;
    let rounded = raw.round();
    if (raw - rounded).abs() > 1e-9 || rounded > 1295.0 {
        return Err(UgcError::UnsupportedNote {
            note: "AIR height cannot be represented by UGC".to_owned(),
        });
    }
    let raw = rounded as u32;
    Ok(format!(
        "{}{}",
        encode_base36_u32(raw / 36),
        encode_base36_u32(raw % 36)
    ))
}

fn encode_base36_u32(value: u32) -> char {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    char::from(DIGITS[value as usize])
}
