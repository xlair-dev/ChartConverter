//! Lower SUS-only Bezier handles to the IR's ordinary on-path control points.

use chart::{Lane, PointMetadata, Position, SlidePoint, SlidePointKind, TempoChange};

use crate::SusError;

const SEGMENTS_PER_SECOND: f64 = 20.0;
// Bound generated denominators independently of SUS's TIL tick setting. This
// retains the previous default precision and is exactly writable as UGC ticks.
const GENERATED_TICKS_PER_BEAT: u64 = 480;
const INVERSION_STEPS: usize = 64;
const MAXIMUM_POINT_COUNT: usize = 1_000_000;
const MAXIMUM_EVALUATION_WORK: usize = 100_000_000;
const LANE_EDGE_EPSILON: f64 = 1e-9;

#[derive(Clone, Copy)]
struct Coordinate {
    seconds: f64,
    center_lane: f64,
}

struct TempoSegment {
    beat: f64,
    seconds: f64,
    seconds_per_beat: f64,
}

pub(super) struct Timing {
    segments: Vec<TempoSegment>,
}

impl Timing {
    pub(super) fn new(tempos: &[TempoChange], line: usize) -> Result<Self, SusError> {
        let mut tempos = tempos.to_vec();
        tempos.sort_by_key(TempoChange::position);
        let mut segments = vec![TempoSegment {
            beat: 0.0,
            seconds: 0.0,
            seconds_per_beat: 0.5, // The same default BPM 120 as XLAIR.
        }];
        for tempo in tempos {
            let beat = beat_value(tempo.position());
            let previous = segments.last().expect("initial segment exists");
            let seconds = previous.seconds + (beat - previous.beat) * previous.seconds_per_beat;
            let seconds_per_beat = 60.0 / tempo.bpm();
            if !seconds.is_finite() || !seconds_per_beat.is_finite() {
                return Err(invalid_curve(
                    line,
                    "tempo exceeds the supported timing range",
                ));
            }
            segments.push(TempoSegment {
                beat,
                seconds,
                seconds_per_beat,
            });
        }
        Ok(Self { segments })
    }

    fn seconds_at(&self, position: Position) -> f64 {
        let beat = beat_value(position);
        let index = self
            .segments
            .partition_point(|segment| segment.beat <= beat);
        let active = &self.segments[index - 1];
        active.seconds + (beat - active.beat) * active.seconds_per_beat
    }

    fn position_at(&self, seconds: f64, line: usize) -> Result<Position, SusError> {
        let index = self
            .segments
            .partition_point(|segment| segment.seconds <= seconds);
        let active = &self.segments[index.saturating_sub(1)];
        let beat = active.beat + (seconds - active.seconds) / active.seconds_per_beat;
        // Source tick resolution does not constrain SUS note subdivisions and
        // must not change the curve. Only generated points use this fixed grid.
        let ticks = (beat * GENERATED_TICKS_PER_BEAT as f64).round();
        if !ticks.is_finite() || ticks < 0.0 || ticks >= u64::MAX as f64 {
            return Err(invalid_curve(
                line,
                "generated position exceeds the supported tick range",
            ));
        }
        Position::new(ticks as u64, GENERATED_TICKS_PER_BEAT)
            .map_err(|source| SusError::Chart { line, source })
    }
}

pub(super) fn expand(
    points: Vec<SlidePoint>,
    metadata: Vec<PointMetadata>,
    timing: &Timing,
    line: usize,
) -> Result<(Vec<SlidePoint>, Vec<PointMetadata>), SusError> {
    if !points
        .iter()
        .any(|point| point.kind() == &SlidePointKind::Control)
    {
        return Ok((points, metadata));
    }
    let mut expanded = vec![points[0].clone()];
    let mut contexts = vec![metadata[0]];
    let mut anchor = 0;
    let mut work_remaining = MAXIMUM_EVALUATION_WORK;
    for index in 1..points.len() {
        if points[index].kind() == &SlidePointKind::Control {
            continue;
        }
        if index > anchor + 1 {
            let coordinates: Vec<_> = points[anchor..=index]
                .iter()
                .map(|point| {
                    let (start, width) = slider_lane(point.lane());
                    Coordinate {
                        seconds: timing.seconds_at(point.position()),
                        center_lane: f64::from(start) + f64::from(width) * 0.5,
                    }
                })
                .collect();
            let first = coordinates.first().expect("segment start exists");
            let last = coordinates.last().expect("segment end exists");
            let duration = last.seconds - first.seconds;
            if !duration.is_finite() || duration <= 0.0 {
                return Err(invalid_curve(
                    line,
                    "anchors must have distinct increasing times",
                ));
            }
            // Match XLAIR's time-based subdivision, De Casteljau evaluation,
            // time-linear width interpolation and outward integer lane rounding.
            let subdivisions = (duration * SEGMENTS_PER_SECOND).ceil().max(2.0);
            if !subdivisions.is_finite() || subdivisions > MAXIMUM_POINT_COUNT as f64 {
                return Err(invalid_curve(line, "too many generated points"));
            }
            let subdivisions = subdivisions as usize;
            let work = subdivisions
                .saturating_mul(coordinates.len())
                .saturating_mul(coordinates.len())
                .saturating_mul(INVERSION_STEPS + 2);
            work_remaining = work_remaining
                .checked_sub(work)
                .ok_or_else(|| invalid_curve(line, "curve evaluation budget exceeded"))?;
            let (_, first_width) = slider_lane(points[anchor].lane());
            let (_, last_width) = slider_lane(points[index].lane());
            let mut workspace = coordinates.clone();
            for subdivision in 1..subdivisions {
                let t = subdivision as f64 / subdivisions as f64;
                let coordinate = evaluate(&coordinates, t, &mut workspace);
                let position = timing.position_at(coordinate.seconds, line)?;
                if position <= expanded.last().expect("start exists").position()
                    || position >= points[index].position()
                {
                    continue;
                }
                // Rounding moves time slightly. Evaluate the lane and width at
                // the emitted time, not at the old parameter value. Inversion
                // is safe because source points are sorted by chart time.
                let seconds = timing.seconds_at(position);
                let coordinate = evaluate_at_seconds(&coordinates, seconds, &mut workspace);
                let time_ratio = (seconds - first.seconds) / duration;
                let width = f64::from(first_width)
                    + (f64::from(last_width) - f64::from(first_width)) * time_ratio;
                expanded.push(
                    SlidePoint::new(position, quantize_lane(coordinate.center_lane, width))
                        .with_kind(SlidePointKind::Control),
                );
                // Handles affect geometry only. Until the next authored anchor,
                // generated points use the preceding anchor's rendering context.
                contexts.push(metadata[anchor]);
                if expanded.len() >= MAXIMUM_POINT_COUNT {
                    return Err(invalid_curve(line, "too many generated points"));
                }
            }
        }
        expanded.push(points[index].clone());
        contexts.push(metadata[index]);
        if expanded.len() > MAXIMUM_POINT_COUNT {
            return Err(invalid_curve(line, "too many generated points"));
        }
        anchor = index;
    }
    Ok((expanded, contexts))
}

fn evaluate(points: &[Coordinate], t: f64, work: &mut [Coordinate]) -> Coordinate {
    work.copy_from_slice(points);
    for remaining in (2..=work.len()).rev() {
        for index in 0..remaining - 1 {
            work[index].seconds += (work[index + 1].seconds - work[index].seconds) * t;
            work[index].center_lane += (work[index + 1].center_lane - work[index].center_lane) * t;
        }
    }
    work[0]
}

fn evaluate_at_seconds(points: &[Coordinate], seconds: f64, work: &mut [Coordinate]) -> Coordinate {
    let mut lower = 0.0;
    let mut upper = 1.0;
    for _ in 0..INVERSION_STEPS {
        let middle = (lower + upper) * 0.5;
        if evaluate(points, middle, work).seconds < seconds {
            lower = middle;
        } else {
            upper = middle;
        }
    }
    evaluate(points, (lower + upper) * 0.5, work)
}

fn quantize_lane(center: f64, width: f64) -> Lane {
    let left = (center - width * 0.5 + LANE_EDGE_EPSILON)
        .floor()
        .clamp(0.0, 15.0) as u8;
    let right = (center + width * 0.5 - LANE_EDGE_EPSILON)
        .ceil()
        .clamp(f64::from(left) + 1.0, 16.0) as u8;
    Lane::slider(left, right - left).expect("quantized lane fits the sixteen slider lanes")
}

fn slider_lane(lane: Lane) -> (u8, u8) {
    match lane {
        Lane::Slider { start, width } => (start, width),
        Lane::Side(_) => unreachable!("SUS slide points are central slider lanes"),
    }
}

fn beat_value(position: Position) -> f64 {
    position.numerator() as f64 / position.denominator() as f64
}

fn invalid_curve(line: usize, reason: &'static str) -> SusError {
    SusError::InvalidCurve { line, reason }
}

#[cfg(test)]
mod tests {
    use super::Timing;

    #[test]
    fn rejects_generated_positions_outside_the_supported_range() {
        let timing = Timing::new(&[], 1).unwrap();
        assert!(timing.position_at(u64::MAX as f64, 1).is_err());
        assert!(timing.position_at(f64::INFINITY, 1).is_err());
    }
}
