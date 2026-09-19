#[cfg(test)]
mod tests {
    use super::{ScrollSpeedChange, TempoChange};
    use crate::Beat;

    #[test]
    fn rejects_non_positive_tempo() {
        let position = Beat::new(0, 0, 1).expect("valid beat");

        assert!(TempoChange::new(position, 0.0).is_err());
    }

    #[test]
    fn rejects_non_finite_scroll_speed() {
        let position = Beat::new(0, 0, 1).expect("valid beat");

        assert!(ScrollSpeedChange::new(position, f64::NAN).is_err());
    }
}
use crate::{Beat, ChartError};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoChange {
    position: Beat,
    bpm: f64,
}

impl TempoChange {
    pub fn new(position: Beat, bpm: f64) -> Result<Self, ChartError> {
        if !bpm.is_finite() || bpm <= 0.0 {
            return Err(ChartError::InvalidTempo);
        }
        Ok(Self { position, bpm })
    }

    pub fn position(&self) -> Beat {
        self.position
    }

    pub fn bpm(&self) -> f64 {
        self.bpm
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollSpeedChange {
    position: Beat,
    speed: f64,
}

impl ScrollSpeedChange {
    pub fn new(position: Beat, speed: f64) -> Result<Self, ChartError> {
        if !speed.is_finite() {
            return Err(ChartError::InvalidScrollSpeed);
        }
        Ok(Self { position, speed })
    }

    pub fn position(&self) -> Beat {
        self.position
    }

    pub fn speed(&self) -> f64 {
        self.speed
    }
}
