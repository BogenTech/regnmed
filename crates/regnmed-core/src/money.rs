use std::fmt;

/// An amount of money in øre (1/100 of a Norwegian krone).
///
/// All ledger arithmetic is integer arithmetic — floating point is never
/// used for money anywhere in regnmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Ore(pub i64);

impl Ore {
    pub const ZERO: Ore = Ore(0);

    pub fn checked_add(self, other: Ore) -> Option<Ore> {
        self.0.checked_add(other.0).map(Ore)
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for Ore {
    /// Kroner with two decimals and Norwegian decimal comma, e.g. `1234,56`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        write!(f, "{sign}{},{:02}", abs / 100, abs % 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_formats_kroner_and_ore() {
        assert_eq!(Ore(123_456).to_string(), "1234,56");
        assert_eq!(Ore(-50).to_string(), "-0,50");
        assert_eq!(Ore::ZERO.to_string(), "0,00");
    }

    #[test]
    fn checked_add_detects_overflow() {
        assert_eq!(Ore(1).checked_add(Ore(2)), Some(Ore(3)));
        assert_eq!(Ore(i64::MAX).checked_add(Ore(1)), None);
    }
}
