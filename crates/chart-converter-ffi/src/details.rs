use chart::{
    AirColor, AirCrushColor, AirCrushInterval, Chart, ExDirection, NoteId, NoteKind, TapKind,
};

use crate::CcPosition;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CcOptionalDouble {
    pub has_value: bool,
    pub value: f64,
}
impl From<Option<f64>> for CcOptionalDouble {
    fn from(value: Option<f64>) -> Self {
        Self {
            has_value: value.is_some(),
            value: value.unwrap_or_default(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CcOptionalInt {
    pub has_value: bool,
    pub value: i32,
}
impl From<Option<i32>> for CcOptionalInt {
    fn from(value: Option<i32>) -> Self {
        Self {
            has_value: value.is_some(),
            value: value.unwrap_or_default(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CcOptionalBool {
    pub has_value: bool,
    pub value: bool,
}
impl From<Option<bool>> for CcOptionalBool {
    fn from(value: Option<bool>) -> Self {
        Self {
            has_value: value.is_some(),
            value: value.unwrap_or_default(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CcNoteAttributes {
    pub roll_speed: CcOptionalDouble,
    pub height: CcOptionalDouble,
    pub priority: CcOptionalInt,
}
impl From<chart::NoteAttributes> for CcNoteAttributes {
    fn from(value: chart::NoteAttributes) -> Self {
        Self {
            roll_speed: value.roll_speed().into(),
            height: value.height().into(),
            priority: value.priority().into(),
        }
    }
}

#[repr(C)]
pub struct CcMeasureLength {
    pub measure: u32,
    /// Length in quarter-note beats, applying from this measure onward.
    pub length: CcPosition,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CcChartMetadata {
    /// Positive values delay audio relative to chart time.
    pub audio_offset_seconds: CcOptionalDouble,
    pub base_bpm: CcOptionalDouble,
    pub priority_enabled: CcOptionalBool,
    /// -1 if the source does not specify a base timeline.
    pub main_speed_group: i64,
    pub measure_lengths: *const CcMeasureLength,
    pub measure_length_count: usize,
}
impl Default for CcChartMetadata {
    fn default() -> Self {
        Self {
            audio_offset_seconds: CcOptionalDouble::default(),
            base_bpm: CcOptionalDouble::default(),
            priority_enabled: CcOptionalBool::default(),
            main_speed_group: -1,
            measure_lengths: std::ptr::null(),
            measure_length_count: 0,
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Default)]
pub enum CcExDirection {
    #[default]
    None,
    Up,
    Down,
    Center,
    All,
    Wide,
    Left,
    Right,
    Inward,
    UpperLeft,
    UpperRight,
    LowerLeft,
    LowerRight,
}
impl From<ExDirection> for CcExDirection {
    fn from(value: ExDirection) -> Self {
        match value {
            ExDirection::Up => Self::Up,
            ExDirection::Down => Self::Down,
            ExDirection::Center => Self::Center,
            ExDirection::All => Self::All,
            ExDirection::Wide => Self::Wide,
            ExDirection::Left => Self::Left,
            ExDirection::Right => Self::Right,
            ExDirection::Inward => Self::Inward,
            ExDirection::UpperLeft => Self::UpperLeft,
            ExDirection::UpperRight => Self::UpperRight,
            ExDirection::LowerLeft => Self::LowerLeft,
            ExDirection::LowerRight => Self::LowerRight,
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Default)]
pub enum CcAirColor {
    #[default]
    None,
    Normal,
    Inverted,
}
impl From<AirColor> for CcAirColor {
    fn from(value: AirColor) -> Self {
        match value {
            AirColor::Normal => Self::Normal,
            AirColor::Inverted => Self::Inverted,
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Default)]
pub enum CcAirCrushColor {
    #[default]
    None,
    Normal,
    Transparent,
    Red,
    Orange,
    Yellow,
    Lime,
    Green,
    Aqua,
    Cyan,
    DarkBlue,
    Blue,
    Violet,
    Purple,
    Pink,
    Gray,
    Black,
}
impl From<AirCrushColor> for CcAirCrushColor {
    fn from(value: AirCrushColor) -> Self {
        match value {
            AirCrushColor::Normal => Self::Normal,
            AirCrushColor::Transparent => Self::Transparent,
            AirCrushColor::Red => Self::Red,
            AirCrushColor::Orange => Self::Orange,
            AirCrushColor::Yellow => Self::Yellow,
            AirCrushColor::Lime => Self::Lime,
            AirCrushColor::Green => Self::Green,
            AirCrushColor::Aqua => Self::Aqua,
            AirCrushColor::Cyan => Self::Cyan,
            AirCrushColor::DarkBlue => Self::DarkBlue,
            AirCrushColor::Blue => Self::Blue,
            AirCrushColor::Violet => Self::Violet,
            AirCrushColor::Purple => Self::Purple,
            AirCrushColor::Pink => Self::Pink,
            AirCrushColor::Gray => Self::Gray,
            AirCrushColor::Black => Self::Black,
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Default)]
pub enum CcAirCrushIntervalKind {
    #[default]
    None,
    Trace,
    Start,
    Every,
}

#[repr(C)]
#[derive(Default)]
pub struct CcAirCrushInterval {
    pub kind: CcAirCrushIntervalKind,
    /// Absolute beat distance; only meaningful for Every.
    pub interval: CcPosition,
}
impl From<AirCrushInterval> for CcAirCrushInterval {
    fn from(value: AirCrushInterval) -> Self {
        match value {
            AirCrushInterval::Trace => Self {
                kind: CcAirCrushIntervalKind::Trace,
                ..Self::default()
            },
            AirCrushInterval::Start => Self {
                kind: CcAirCrushIntervalKind::Start,
                ..Self::default()
            },
            AirCrushInterval::Every(interval) => Self {
                kind: CcAirCrushIntervalKind::Every,
                interval: interval.into(),
            },
        }
    }
}

/// One entry per legacy CcPoint, in exactly the same order. Context is already resolved.
#[repr(C)]
pub struct CcPointDetails {
    /// -1 explicitly means no group speed at this point, not inheritance.
    pub speed_group: i64,
    pub attributes: CcNoteAttributes,
    pub air_height: CcOptionalDouble,
}

/// Extension keyed by the stable CcNote.id; does not change CcNote's layout.
#[repr(C)]
pub struct CcNoteDetails {
    pub id: u32,
    pub attributes: CcNoteAttributes,
    pub ex_direction: CcExDirection,
    pub air_height: CcOptionalDouble,
    pub air_color: CcAirColor,
    pub air_crush_color: CcAirCrushColor,
    pub air_crush_interval: CcAirCrushInterval,
    pub points: *const CcPointDetails,
    pub point_count: usize,
}

pub(super) struct OwnedDetails {
    metadata: CcChartMetadata,
    _measures: Vec<CcMeasureLength>,
    pub notes: Vec<CcNoteDetails>,
    _points: Vec<Vec<CcPointDetails>>,
}

impl OwnedDetails {
    pub fn new(chart: &Chart) -> Self {
        let measures: Vec<_> = chart
            .measure_lengths()
            .iter()
            .map(|&(measure, length)| CcMeasureLength {
                measure,
                length: length.into(),
            })
            .collect();
        let metadata = CcChartMetadata {
            audio_offset_seconds: chart.audio_offset_seconds().into(),
            base_bpm: chart.base_bpm().into(),
            priority_enabled: chart.priority_enabled().into(),
            main_speed_group: chart.main_speed_group().map(i64::from).unwrap_or(-1),
            measure_lengths: measures.as_ptr(),
            measure_length_count: measures.len(),
        };
        let mut result = Self {
            metadata,
            _measures: measures,
            notes: Vec::new(),
            _points: Vec::new(),
        };
        for (index, note) in chart.notes().iter().enumerate() {
            let mut details = CcNoteDetails {
                id: index as u32,
                attributes: note.attributes().into(),
                ex_direction: CcExDirection::None,
                air_height: CcOptionalDouble::default(),
                air_color: CcAirColor::None,
                air_crush_color: CcAirCrushColor::None,
                air_crush_interval: CcAirCrushInterval::default(),
                points: std::ptr::null(),
                point_count: 0,
            };
            match note.kind() {
                NoteKind::Tap(TapKind::Flick {
                    direction: Some(direction),
                })
                | NoteKind::ExTap { direction }
                | NoteKind::ExHold { direction, .. }
                | NoteKind::ExSlide { direction, .. } => details.ex_direction = (*direction).into(),
                NoteKind::Air { properties, .. } | NoteKind::AirHold { properties, .. } => {
                    details.air_height = properties.height().into();
                    details.air_color = properties.color().into();
                }
                NoteKind::AirSlide { color, .. } => details.air_color = (*color).into(),
                NoteKind::AirCrush {
                    color, interval, ..
                } => {
                    details.air_crush_color = (*color).into();
                    details.air_crush_interval = (*interval).into();
                }
                _ => {}
            }
            let points: Vec<_> = (0..note.point_metadata().len())
                .map(|point_index| {
                    let context = chart
                        .point_metadata(NoteId::new(index as u32), point_index)
                        .expect("valid path point");
                    let height = match note.kind() {
                        NoteKind::AirSlide { points, .. } | NoteKind::AirCrush { points, .. } => {
                            Some(points[point_index].height())
                        }
                        NoteKind::AirHold { properties, .. } => properties.height(),
                        _ => None,
                    };
                    CcPointDetails {
                        speed_group: context.speed_group.map(i64::from).unwrap_or(-1),
                        attributes: context.attributes.into(),
                        air_height: height.into(),
                    }
                })
                .collect();
            details.points = points.as_ptr();
            details.point_count = points.len();
            result.notes.push(details);
            result._points.push(points);
        }
        result
    }

    pub fn metadata(&self) -> CcChartMetadata {
        self.metadata
    }
}
