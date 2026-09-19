#[cfg(test)]
mod tests {
    use super::Beat;

    #[test]
    fn reduces_fraction_and_orders_by_measure_and_fraction() {
        let first = Beat::new(2, 2, 4).expect("valid beat");
        let second = Beat::new(2, 3, 4).expect("valid beat");

        assert_eq!(first, Beat::new(2, 1, 2).expect("valid beat"));
        assert!(first < second);
    }

    #[test]
    fn rejects_zero_denominator() {
        assert!(Beat::new(0, 0, 0).is_err());
    }
}
use std::cmp::Ordering;

use crate::ChartError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Beat {
    measure: u32,
    numerator: u32,
    denominator: u32,
}

impl Beat {
    pub fn new(measure: u32, numerator: u32, denominator: u32) -> Result<Self, ChartError> {
        if denominator == 0 {
            return Err(ChartError::InvalidBeatDenominator);
        }
        if numerator >= denominator {
            return Err(ChartError::InvalidBeatFraction);
        }

        let divisor = gcd(numerator, denominator);
        Ok(Self {
            measure,
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }

    pub fn measure(self) -> u32 {
        self.measure
    }

    pub fn numerator(self) -> u32 {
        self.numerator
    }

    pub fn denominator(self) -> u32 {
        self.denominator
    }
}

impl Ord for Beat {
    fn cmp(&self, other: &Self) -> Ordering {
        self.measure.cmp(&other.measure).then_with(|| {
            (self.numerator as u128 * other.denominator as u128)
                .cmp(&(other.numerator as u128 * self.denominator as u128))
        })
    }
}

impl PartialOrd for Beat {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

const fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    if left == 0 { 1 } else { left }
}
