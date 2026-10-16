//! Read-only C bindings for parsed chart notes, paths, and timing.
//!
//! The result owns all returned memory. Views are immutable and remain valid until
//! `cc_result_free`; callers must not free individual strings, arrays, or views.
//! Format-specific metadata, note attributes, and AIR height/color are not exposed.

use std::{
    ffi::{CString, c_char},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr, slice,
};

use chart::{
    AirDirection, Chart, ChartMode, Lane, NoteKind, Position, ScrollScope, SlidePointKind, TapKind,
};
use chart_converter::Format;

#[repr(u32)]
pub enum CcFormat {
    Sus = 0,
    C2s = 1,
    Ugc = 2,
}
#[repr(u32)]
pub enum CcMode {
    Normal = 0,
    Xlair = 1,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CcPosition {
    pub numerator: u64,
    pub denominator: u64,
}
impl From<Position> for CcPosition {
    fn from(value: Position) -> Self {
        Self {
            numerator: value.numerator(),
            denominator: value.denominator(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub enum CcSideButton {
    LeftUpper,
    LeftLower,
    RightLower,
    RightUpper,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub enum CcLaneKind {
    Slider,
    Side,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CcLane {
    pub kind: CcLaneKind,
    pub start: u8,
    pub width: u8,
    pub button: CcSideButton,
}
impl From<Lane> for CcLane {
    fn from(value: Lane) -> Self {
        match value {
            Lane::Slider { start, width } => Self {
                kind: CcLaneKind::Slider,
                start,
                width,
                button: CcSideButton::LeftUpper,
            },
            Lane::Side(button) => Self {
                kind: CcLaneKind::Side,
                start: 0,
                width: 0,
                button: match button {
                    chart::SideButton::LeftUpper => CcSideButton::LeftUpper,
                    chart::SideButton::LeftLower => CcSideButton::LeftLower,
                    chart::SideButton::RightLower => CcSideButton::RightLower,
                    chart::SideButton::RightUpper => CcSideButton::RightUpper,
                },
            },
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub enum CcPointKind {
    Visible,
    Invisible,
    Control,
}
#[repr(C)]
pub struct CcPoint {
    pub position: CcPosition,
    pub lane: CcLane,
    pub kind: CcPointKind,
}
impl From<&SlidePointKind> for CcPointKind {
    fn from(value: &SlidePointKind) -> Self {
        match value {
            SlidePointKind::Visible => Self::Visible,
            SlidePointKind::Invisible => Self::Invisible,
            SlidePointKind::Control => Self::Control,
        }
    }
}
#[repr(C)]
pub enum CcNoteKind {
    Tap,
    XTap,
    Flick,
    Tap4,
    Tap5,
    Tap6,
    ExTap,
    Mine,
    Hold,
    ExHold,
    Slide,
    ExSlide,
    Air,
    AirHold,
    AirSlide,
    AirCrush,
}
#[repr(C)]
pub enum CcAirDirection {
    None,
    Up,
    UpperLeft,
    UpperRight,
    Down,
    LowerLeft,
    LowerRight,
}
#[repr(C)]
pub struct CcNote {
    /// Stable insertion index, also used by parent and note-scoped speed references.
    pub id: u32,
    pub position: CcPosition,
    pub lane: CcLane,
    pub kind: CcNoteKind,
    /// -1 when no speed group is assigned.
    pub speed_group: i64,
    /// UINT32_MAX when this note has no parent.
    pub parent: u32,
    pub air_direction: CcAirDirection,
    /// Path points for slides; start, checkpoints, and end for holds. Empty for short notes.
    pub points: *const CcPoint,
    pub point_count: usize,
}
#[repr(C)]
pub struct CcTempo {
    pub position: CcPosition,
    pub bpm: f64,
}
#[repr(C)]
pub enum CcScrollScope {
    Global,
    Lane,
    Note,
    Group,
}
#[repr(C)]
pub struct CcScroll {
    pub position: CcPosition,
    pub speed: f64,
    pub scope: CcScrollScope,
    /// Note ID or group ID, depending on scope.
    pub target: u32,
    pub lane: CcLane,
    /// A zero denominator means no duration was specified.
    pub duration: CcPosition,
}

/// Borrowed arrays owned by a successful CcResult. Empty arrays may have null pointers.
#[repr(C)]
#[derive(Default)]
pub struct CcChartView {
    pub notes: *const CcNote,
    pub note_count: usize,
    pub tempos: *const CcTempo,
    pub tempo_count: usize,
    pub scrolls: *const CcScroll,
    pub scroll_count: usize,
    /// Positive values start chart playback before audio; zero when absent.
    pub audio_offset_seconds: f64,
    /// Zero when the source specifies no base BPM.
    pub base_bpm: f64,
}

struct OwnedChart {
    notes: Vec<CcNote>,
    _points: Vec<Vec<CcPoint>>,
    tempos: Vec<CcTempo>,
    scrolls: Vec<CcScroll>,
    audio_offset_seconds: f64,
    base_bpm: f64,
}

impl From<Chart> for OwnedChart {
    fn from(chart: Chart) -> Self {
        let mut result = Self {
            notes: Vec::new(),
            _points: Vec::new(),
            tempos: Vec::new(),
            scrolls: Vec::new(),
            audio_offset_seconds: chart.audio_offset_seconds().unwrap_or(0.0),
            base_bpm: chart.base_bpm().unwrap_or(0.0),
        };
        for (index, note) in chart.notes().iter().enumerate() {
            let mut points = Vec::new();
            let mut parent = u32::MAX;
            let mut direction = CcAirDirection::None;
            let kind = match note.kind() {
                NoteKind::Tap(kind) => match kind {
                    TapKind::Tap => CcNoteKind::Tap,
                    TapKind::XTap => CcNoteKind::XTap,
                    TapKind::Flick { .. } => CcNoteKind::Flick,
                    TapKind::Tap4 => CcNoteKind::Tap4,
                    TapKind::Tap5 => CcNoteKind::Tap5,
                    TapKind::Tap6 => CcNoteKind::Tap6,
                },
                NoteKind::ExTap { .. } => CcNoteKind::ExTap,
                NoteKind::Mine => CcNoteKind::Mine,
                NoteKind::Hold { .. } => CcNoteKind::Hold,
                NoteKind::ExHold { .. } => CcNoteKind::ExHold,
                NoteKind::Slide { points: path } | NoteKind::ExSlide { points: path, .. } => {
                    points.extend(path.iter().map(|point| CcPoint {
                        position: point.position().into(),
                        lane: point.lane().into(),
                        kind: point.kind().into(),
                    }));
                    if matches!(note.kind(), NoteKind::Slide { .. }) {
                        CcNoteKind::Slide
                    } else {
                        CcNoteKind::ExSlide
                    }
                }
                NoteKind::Air {
                    properties,
                    parent: id,
                }
                | NoteKind::AirHold {
                    properties,
                    parent: id,
                    ..
                } => {
                    parent = id.value();
                    direction = match properties.direction() {
                        None => CcAirDirection::None,
                        Some(AirDirection::Up) => CcAirDirection::Up,
                        Some(AirDirection::Down) => CcAirDirection::Down,
                        Some(AirDirection::UpperLeft) => CcAirDirection::UpperLeft,
                        Some(AirDirection::UpperRight) => CcAirDirection::UpperRight,
                        Some(AirDirection::LowerLeft) => CcAirDirection::LowerLeft,
                        Some(AirDirection::LowerRight) => CcAirDirection::LowerRight,
                    };
                    if matches!(note.kind(), NoteKind::Air { .. }) {
                        CcNoteKind::Air
                    } else {
                        CcNoteKind::AirHold
                    }
                }
                NoteKind::AirSlide {
                    points: path,
                    parent: id,
                    ..
                }
                | NoteKind::AirCrush {
                    points: path,
                    parent: id,
                    ..
                } => {
                    parent = id.value();
                    points.extend(path.iter().map(|point| CcPoint {
                        position: point.position().into(),
                        lane: point.lane().into(),
                        kind: point.kind().into(),
                    }));
                    if matches!(note.kind(), NoteKind::AirSlide { .. }) {
                        CcNoteKind::AirSlide
                    } else {
                        CcNoteKind::AirCrush
                    }
                }
            };
            match note.kind() {
                NoteKind::Hold { end, checkpoints }
                | NoteKind::ExHold {
                    end, checkpoints, ..
                }
                | NoteKind::AirHold {
                    end, checkpoints, ..
                } => {
                    points.push(CcPoint {
                        position: note.position().into(),
                        lane: note.lane().into(),
                        kind: CcPointKind::Visible,
                    });
                    points.extend(
                        checkpoints
                            .iter()
                            .chain(std::iter::once(end))
                            .map(|position| CcPoint {
                                position: (*position).into(),
                                lane: note.lane().into(),
                                kind: CcPointKind::Visible,
                            }),
                    );
                }
                _ => {}
            }
            result.notes.push(CcNote {
                id: index as u32,
                position: note.position().into(),
                lane: note.lane().into(),
                kind,
                speed_group: chart
                    .note_speed_group(chart::NoteId::new(index as u32))
                    .expect("existing note")
                    .map(i64::from)
                    .unwrap_or(-1),
                parent,
                air_direction: direction,
                points: points.as_ptr(),
                point_count: points.len(),
            });
            result._points.push(points);
        }
        result
            .tempos
            .extend(chart.tempo_changes().iter().map(|tempo| CcTempo {
                position: tempo.position().into(),
                bpm: tempo.bpm(),
            }));
        result
            .scrolls
            .extend(chart.scroll_speed_changes().iter().map(|scroll| {
                let (scope, target, lane) = match scroll.scope() {
                    ScrollScope::Global => (
                        CcScrollScope::Global,
                        0,
                        Lane::Side(chart::SideButton::LeftUpper),
                    ),
                    ScrollScope::Lane(lane) => (CcScrollScope::Lane, 0, lane),
                    ScrollScope::Note(id) => (
                        CcScrollScope::Note,
                        id.value(),
                        Lane::Side(chart::SideButton::LeftUpper),
                    ),
                    ScrollScope::Group(id) => (
                        CcScrollScope::Group,
                        id,
                        Lane::Side(chart::SideButton::LeftUpper),
                    ),
                };
                CcScroll {
                    position: scroll.position().into(),
                    speed: scroll.speed(),
                    scope,
                    target,
                    lane: lane.into(),
                    duration: scroll.duration().map(Into::into).unwrap_or_default(),
                }
            }));
        result
    }
}

/// Opaque owner of a parsed chart or a UTF-8 error message.
pub struct CcResult {
    chart: Option<OwnedChart>,
    error: Option<CString>,
}

/// Parses bytes as SUS (0), C2S (1), or UGC (2), in Normal (0) or XLAIR (1) mode.
/// Accepts UTF-8 and BOM-marked UTF-16, matching the Rust conversion API.
/// Always returns an owned result, including on invalid arguments or a caught panic.
///
/// # Safety
/// `source` must point to `length` readable bytes and remain valid for this call.
/// A null source is allowed only when length is zero. Free the result exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cc_parse(
    format: u32,
    mode: u32,
    source: *const u8,
    length: usize,
) -> *mut CcResult {
    let parsed = catch_unwind(AssertUnwindSafe(|| -> Result<OwnedChart, String> {
        let format = match format {
            0 => Format::Sus,
            1 => Format::C2s,
            2 => Format::Ugc,
            _ => return Err("unknown chart format".into()),
        };
        let mode = match mode {
            0 => ChartMode::Normal,
            1 => ChartMode::Xlair,
            _ => return Err("unknown chart mode".into()),
        };
        if length > isize::MAX as usize || (source.is_null() && length != 0) {
            return Err("invalid source buffer".into());
        }
        let source = if length == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(source, length) }
        };
        chart_converter::parse_bytes_with_mode(format, mode, source)
            .map(Into::into)
            .map_err(|error| error.to_string())
    }))
    .unwrap_or_else(|_| Err("chart parser panicked".into()));
    let result = match parsed {
        Ok(chart) => CcResult {
            chart: Some(chart),
            error: None,
        },
        Err(error) => CcResult {
            chart: None,
            error: Some(CString::new(error.replace('\0', "\\0")).expect("no embedded NUL")),
        },
    };
    Box::into_raw(Box::new(result))
}

/// Returns a borrowed chart view, or an empty view on failure or a null result.
/// # Safety
/// A non-null result must be a live pointer returned by cc_parse.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cc_result_chart(result: *const CcResult) -> CcChartView {
    let Some(chart) = (unsafe { result.as_ref() }).and_then(|result| result.chart.as_ref()) else {
        return CcChartView::default();
    };
    CcChartView {
        notes: chart.notes.as_ptr(),
        note_count: chart.notes.len(),
        tempos: chart.tempos.as_ptr(),
        tempo_count: chart.tempos.len(),
        scrolls: chart.scrolls.as_ptr(),
        scroll_count: chart.scrolls.len(),
        audio_offset_seconds: chart.audio_offset_seconds,
        base_bpm: chart.base_bpm,
    }
}

/// Returns a borrowed NUL-terminated UTF-8 error, or null on success or a null result.
/// # Safety
/// A non-null result must be a live pointer returned by cc_parse.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cc_result_error(result: *const CcResult) -> *const c_char {
    (unsafe { result.as_ref() })
        .and_then(|result| result.error.as_ref())
        .map_or(ptr::null(), |error| error.as_ptr())
}

/// Releases the result and every view obtained from it. Null is a no-op.
/// # Safety
/// A non-null result must be a live pointer returned by cc_parse, freed exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cc_result_free(result: *mut CcResult) {
    if !result.is_null() {
        drop(unsafe { Box::from_raw(result) });
    }
}
