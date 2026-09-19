//! Deterministic SVG rendering for XLAIR-compatible SUS charts.

use std::fmt::Write as _;

use chart::{Chart, Lane, Note, NoteKind, Position, SideButton, TapKind};
use thiserror::Error;

const WIDTH: f64 = 640.0;
const TOP: f64 = 48.0;
const BEAT_HEIGHT: f64 = 80.0;
const CENTRAL_LEFT: f64 = 64.0;
const LANE_WIDTH: f64 = 32.0;

#[derive(Debug, Error)]
pub enum SvgError {
    #[error("failed to parse SUS chart: {0}")]
    Sus(#[from] sus::SusError),
}

/// Parses an XLAIR-compatible SUS document and renders it as SVG.
pub fn render(source: &str) -> Result<String, SvgError> {
    let chart = sus::parse(source)?;
    Ok(render_chart(&chart))
}

/// Renders a chart using a stable SVG layout.
pub fn render_chart(chart: &Chart) -> String {
    let height = chart_height(chart);
    let mut svg = String::new();
    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {WIDTH:.0} {height:.0}" role="img" aria-label="XLAIR chart">"#
    );
    let _ = write!(
        svg,
        r##"<rect class="background" width="{WIDTH:.0}" height="{height:.0}" fill="#fff"/>"##
    );
    render_grid(&mut svg, height);
    for note in chart.notes() {
        render_note(&mut svg, note);
    }
    svg.push_str("</svg>\n");
    svg
}

fn chart_height(chart: &Chart) -> f64 {
    let max_position = chart
        .notes()
        .iter()
        .map(note_end_position)
        .fold(0.0, f64::max);
    TOP + (max_position.ceil() + 1.0) * BEAT_HEIGHT
}

fn note_end_position(note: &Note) -> f64 {
    match note.kind() {
        NoteKind::Hold { end } | NoteKind::ExHold { end, .. } | NoteKind::AirHold { end, .. } => {
            position_value(*end)
        }
        NoteKind::Slide { points }
        | NoteKind::ExSlide { points, .. }
        | NoteKind::AirSlide { points, .. } => points
            .last()
            .map(|point| position_value(point.position()))
            .unwrap_or_else(|| position_value(note.position())),
        _ => position_value(note.position()),
    }
}

fn render_grid(svg: &mut String, height: f64) {
    for lane in 0..=16 {
        let x = CENTRAL_LEFT + lane as f64 * LANE_WIDTH;
        let _ = write!(
            svg,
            r##"<line class="lane" x1="{x:.2}" y1="{TOP:.2}" x2="{x:.2}" y2="{height:.2}" stroke="#ddd"/>"##
        );
    }
    let beat_count = ((height - TOP) / BEAT_HEIGHT).ceil() as u32;
    for beat in 0..=beat_count {
        let y = TOP + beat as f64 * BEAT_HEIGHT;
        let _ = write!(
            svg,
            r##"<line class="beat" x1="0" y1="{y:.2}" x2="{WIDTH:.2}" y2="{y:.2}" stroke="#eee"/>"##
        );
    }
}

fn render_note(svg: &mut String, note: &Note) {
    let start_y = y_position(note.position());
    match note.kind() {
        NoteKind::Tap(kind) => {
            let class = match kind {
                TapKind::Tap => "tap",
                TapKind::XTap => "xtap",
                TapKind::Flick { .. } => "flick",
            };
            circle(svg, class, note.lane(), start_y, 7.0);
        }
        NoteKind::ExTap { .. } => circle(svg, "extap", note.lane(), start_y, 8.0),
        NoteKind::Mine => circle(svg, "mine", note.lane(), start_y, 7.0),
        NoteKind::Hold { end } | NoteKind::ExHold { end, .. } => {
            line(svg, "hold", note.lane(), start_y, y_position(*end))
        }
        NoteKind::Slide { points } | NoteKind::ExSlide { points, .. } => {
            polyline(svg, "slide", points)
        }
        NoteKind::Air { .. } => circle(svg, "air", note.lane(), start_y, 5.0),
        NoteKind::AirHold { end, .. } => {
            line(svg, "air-hold", note.lane(), start_y, y_position(*end))
        }
        NoteKind::AirSlide { points, .. } => polyline(svg, "air-slide", points),
    }
}

fn circle(svg: &mut String, class: &str, lane: Lane, y: f64, radius: f64) {
    let x = lane_x(lane);
    let _ = write!(
        svg,
        r##"<circle class="note {class}" cx="{x:.2}" cy="{y:.2}" r="{radius:.2}" fill="#4c6fff"/>"##
    );
}

fn line(svg: &mut String, class: &str, lane: Lane, start_y: f64, end_y: f64) {
    let x = lane_x(lane);
    let _ = write!(
        svg,
        r##"<line class="note {class}" x1="{x:.2}" y1="{start_y:.2}" x2="{x:.2}" y2="{end_y:.2}" stroke="#4c6fff" stroke-width="8"/>"##
    );
}

fn polyline(svg: &mut String, class: &str, points: &[chart::SlidePoint]) {
    svg.push_str(&format!(r#"<polyline class="note {class}" points=""#));
    for point in points {
        let _ = write!(
            svg,
            "{:.2},{:.2} ",
            lane_x(point.lane()),
            y_position(point.position())
        );
    }
    svg.push_str("\" fill=\"none\" stroke=\"#4c6fff\" stroke-width=\"6\"/>");
}

fn lane_x(lane: Lane) -> f64 {
    match lane {
        Lane::Slider { start, width } => {
            CENTRAL_LEFT + (start as f64 + width as f64 / 2.0) * LANE_WIDTH
        }
        Lane::Side(SideButton::LeftUpper | SideButton::LeftLower) => 24.0,
        Lane::Side(SideButton::RightUpper | SideButton::RightLower) => WIDTH - 24.0,
    }
}

fn y_position(position: Position) -> f64 {
    TOP + position_value(position) * BEAT_HEIGHT
}

fn position_value(position: Position) -> f64 {
    position.numerator() as f64 / position.denominator() as f64
}

#[cfg(test)]
mod tests {
    use chart::{Chart, Lane, Note, NoteKind, Position, TapKind};

    use super::{render, render_chart};

    #[test]
    fn renders_xlair_sus_notes_as_svg() {
        let source = "#00010: 14\n#00120A: 14\n#00220A: 24\n";
        let svg = render(source).expect("valid SUS");

        assert!(svg.starts_with("<svg "));
        assert!(svg.contains("class=\"lane\""));
        assert!(svg.contains("class=\"note tap\""));
        assert!(svg.contains("class=\"note hold\""));
    }

    #[test]
    fn renders_central_notes_at_their_lane_center() {
        let mut chart = Chart::new();
        chart.add_note(
            Note::new(
                Position::new(0, 1).unwrap(),
                Lane::slider(0, 4).unwrap(),
                NoteKind::Tap(TapKind::Tap),
            )
            .unwrap(),
        );
        let svg = render_chart(&chart);

        assert!(svg.contains("cx=\"128.00\""));
    }

    #[test]
    fn returns_sus_parse_errors() {
        assert!(render("#00010: 9x").is_err());
    }
}
