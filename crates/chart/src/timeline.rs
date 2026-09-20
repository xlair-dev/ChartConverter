use crate::{ChartError, Position};

#[derive(Clone, Debug, PartialEq)]
pub struct MeasureTimeline {
    default_length: Position,
    length_changes: Vec<(u32, Position)>,
}

impl MeasureTimeline {
    pub fn new(default_length: Position) -> Self {
        Self {
            default_length,
            length_changes: Vec::new(),
        }
    }

    pub fn set_length(&mut self, measure: u32, length: Position) {
        if let Some((_, current)) = self
            .length_changes
            .iter_mut()
            .find(|(current_measure, _)| *current_measure == measure)
        {
            *current = length;
        } else {
            self.length_changes.push((measure, length));
            self.length_changes
                .sort_by_key(|(current_measure, _)| *current_measure);
        }
    }

    pub fn position(
        &self,
        measure: u32,
        numerator: u64,
        denominator: u64,
    ) -> Result<Position, ChartError> {
        let offset = Position::new(numerator, denominator)?;
        if offset > Position::new(1, 1).expect("valid position") {
            return Err(ChartError::InvalidMeasureOffset);
        }

        let mut current = Position::new(0, 1).expect("valid position");
        for current_measure in 0..measure {
            current = current.checked_add(self.length_at(current_measure))?;
        }
        current.checked_add(
            self.length_at(measure)
                .checked_scale(offset.numerator(), offset.denominator())?,
        )
    }

    pub fn locate(
        &self,
        position: Position,
        ticks_per_beat: u64,
    ) -> Result<(u32, u64), ChartError> {
        let mut measure = 0u32;
        let mut start = Position::new(0, 1)?;
        loop {
            let length = self.length_at(measure);
            let end = start.checked_add(length)?;
            if position < end {
                let offset = position.checked_sub(start)?;
                let subdivisions = self.ticks(measure, ticks_per_beat)?;
                let numerator = offset
                    .numerator()
                    .checked_mul(length.denominator())
                    .and_then(|value| value.checked_mul(subdivisions))
                    .ok_or(ChartError::PositionOverflow)?;
                let denominator = offset
                    .denominator()
                    .checked_mul(length.numerator())
                    .ok_or(ChartError::PositionOverflow)?;
                if numerator % denominator != 0 {
                    return Err(ChartError::PositionOverflow);
                }
                return Ok((measure, numerator / denominator));
            }
            start = end;
            measure = measure.checked_add(1).ok_or(ChartError::PositionOverflow)?;
        }
    }

    pub fn ticks(&self, measure: u32, ticks_per_beat: u64) -> Result<u64, ChartError> {
        let length = self.length_at(measure);
        let numerator = length
            .numerator()
            .checked_mul(ticks_per_beat)
            .ok_or(ChartError::PositionOverflow)?;
        if numerator % length.denominator() != 0 {
            return Err(ChartError::PositionOverflow);
        }
        Ok(numerator / length.denominator())
    }

    fn length_at(&self, measure: u32) -> Position {
        self.length_changes
            .iter()
            .rev()
            .find(|(changed_measure, _)| *changed_measure <= measure)
            .map(|(_, length)| *length)
            .unwrap_or(self.default_length)
    }
}

#[cfg(test)]
mod tests {
    use super::MeasureTimeline;
    use crate::Position;

    #[test]
    fn maps_variable_length_measures_to_absolute_positions() {
        let mut timeline = MeasureTimeline::new(Position::new(4, 1).expect("valid position"));
        timeline.set_length(1, Position::new(3, 1).expect("valid position"));

        assert_eq!(
            timeline.position(2, 1, 2).expect("valid position"),
            Position::new(17, 2).expect("valid position")
        );
    }
}
