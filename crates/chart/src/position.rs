use std::cmp::Ordering;

use crate::ChartError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Position {
    numerator: u64,
    denominator: u64,
}

impl Position {
    pub fn new(numerator: u64, denominator: u64) -> Result<Self, ChartError> {
        if denominator == 0 {
            return Err(ChartError::InvalidPositionDenominator);
        }
        let divisor = gcd(numerator, denominator);
        Ok(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }

    pub fn numerator(self) -> u64 {
        self.numerator
    }

    pub fn denominator(self) -> u64 {
        self.denominator
    }

    /// Adds two absolute beat positions without losing rational precision.
    pub fn checked_add(self, other: Self) -> Result<Self, ChartError> {
        let left = self
            .numerator
            .checked_mul(other.denominator)
            .ok_or(ChartError::PositionOverflow)?;
        let right = other
            .numerator
            .checked_mul(self.denominator)
            .ok_or(ChartError::PositionOverflow)?;
        let numerator = left
            .checked_add(right)
            .ok_or(ChartError::PositionOverflow)?;
        let denominator = self
            .denominator
            .checked_mul(other.denominator)
            .ok_or(ChartError::PositionOverflow)?;
        Self::new(numerator, denominator)
    }

    pub(crate) fn checked_scale(
        self,
        numerator: u64,
        denominator: u64,
    ) -> Result<Self, ChartError> {
        let numerator = self
            .numerator
            .checked_mul(numerator)
            .ok_or(ChartError::PositionOverflow)?;
        let denominator = self
            .denominator
            .checked_mul(denominator)
            .ok_or(ChartError::PositionOverflow)?;
        Self::new(numerator, denominator)
    }
}

impl Ord for Position {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.numerator as u128 * other.denominator as u128)
            .cmp(&(other.numerator as u128 * self.denominator as u128))
    }
}

impl PartialOrd for Position {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

const fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    if left == 0 { 1 } else { left }
}

#[cfg(test)]
mod tests {
    use super::Position;

    #[test]
    fn reduces_and_orders_absolute_positions() {
        let first = Position::new(2, 4).expect("valid position");
        let second = Position::new(3, 4).expect("valid position");

        assert_eq!(first, Position::new(1, 2).expect("valid position"));
        assert!(first < second);
    }

    #[test]
    fn rejects_zero_denominator() {
        assert!(Position::new(0, 0).is_err());
    }
}
