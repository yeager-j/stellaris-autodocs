//! Exact numeric representation for source values (docs/technical-design.md,
//! "Canonicalization and numeric representation").
//!
//! A parsed number preserves its original lexeme and, when the lexeme is an integer or
//! finite base-10 decimal, an exact rational value. Deterministic static arithmetic
//! operates on that exact form. Binary floating point never participates in equality,
//! hashing, identity, or displayed exact values. An operation without a proven exact
//! result — division by zero here; unproven Stellaris rounding semantics later — yields
//! `None` and stays visibly unresolved, never approximated.

use crate::canonical::encode::CanonicalDigest;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Signed, Zero};

/// A numeric scalar as it appeared in source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceNumber {
    lexeme: String,
    value: Option<ExactValue>,
}

impl SourceNumber {
    /// The lexeme is always preserved verbatim; a value is present only when the lexeme
    /// is a supported exact form.
    pub fn parse(lexeme: &str) -> Self {
        Self {
            lexeme: lexeme.to_owned(),
            value: parse_exact(lexeme).map(ExactValue),
        }
    }

    pub fn lexeme(&self) -> &str {
        &self.lexeme
    }

    pub fn value(&self) -> Option<&ExactValue> {
        self.value.as_ref()
    }
}

/// Supported forms: `[+-]?digits`, `[+-]?digits.digits`, `[+-]?.digits`.
/// A trailing dot (`5.`) is unproven in source and stays symbolic.
fn parse_exact(lexeme: &str) -> Option<BigRational> {
    let unsigned = lexeme.strip_prefix(['+', '-']).unwrap_or(lexeme);
    let (integer, fraction) = match unsigned.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (unsigned, ""),
    };
    if integer.is_empty() && fraction.is_empty() {
        return None;
    }
    if unsigned.contains('.') && fraction.is_empty() {
        return None;
    }
    if !integer.bytes().all(|b| b.is_ascii_digit()) || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{integer}{fraction}");
    let numerator: BigInt = digits.parse().ok()?;
    let denominator = num_traits::pow(BigInt::from(10), fraction.len());
    let magnitude = BigRational::new(numerator, denominator);
    Some(if lexeme.starts_with('-') {
        -magnitude
    } else {
        magnitude
    })
}

/// An exact rational produced by parsing or deterministic static arithmetic.
/// `BigRational` keeps a reduced canonical form, so `Eq`, `Ord`, and `Hash` agree on
/// mathematically equal values.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExactValue(BigRational);

impl ExactValue {
    pub fn add(&self, other: &Self) -> Self {
        Self(&self.0 + &other.0)
    }

    pub fn sub(&self, other: &Self) -> Self {
        Self(&self.0 - &other.0)
    }

    pub fn mul(&self, other: &Self) -> Self {
        Self(&self.0 * &other.0)
    }

    /// `None` when `other` is zero: unresolved, never an approximation or panic.
    pub fn div(&self, other: &Self) -> Option<Self> {
        if other.0.is_zero() {
            None
        } else {
            Some(Self(&self.0 / &other.0))
        }
    }

    /// Canonical identity contribution: sign, numerator magnitude, denominator, all in
    /// reduced form, as decimal digit strings.
    pub fn encode(&self, digest: &mut CanonicalDigest) {
        digest
            .bool(self.0.is_negative())
            .text(&self.0.numer().magnitude().to_string())
            .text(&self.0.denom().to_string());
    }

    /// Decimal rendering when the reduced denominator is `2^a * 5^b`; `None` otherwise.
    /// Trailing fraction zeros are trimmed for display; identity uses [`Self::encode`].
    pub fn to_decimal_string(&self) -> Option<String> {
        let two = BigInt::from(2);
        let five = BigInt::from(5);
        let mut reduced = self.0.denom().clone();
        let mut twos = 0usize;
        let mut fives = 0usize;
        while (&reduced % &two).is_zero() {
            reduced = &reduced / &two;
            twos += 1;
        }
        while (&reduced % &five).is_zero() {
            reduced = &reduced / &five;
            fives += 1;
        }
        if reduced != BigInt::from(1) {
            return None;
        }
        let scale = twos.max(fives);
        let scaled = (self.0.numer() * num_traits::pow(BigInt::from(10), scale)) / self.0.denom();
        let digits = scaled.magnitude().to_string();
        let sign = if self.0.is_negative() { "-" } else { "" };
        if scale == 0 {
            return Some(format!("{sign}{digits}"));
        }
        let padded = format!("{digits:0>width$}", width = scale + 1);
        let (integer, fraction) = padded.split_at(padded.len() - scale);
        let fraction = fraction.trim_end_matches('0');
        Some(if fraction.is_empty() {
            format!("{sign}{integer}")
        } else {
            format!("{sign}{integer}.{fraction}")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn value_of(lexeme: &str) -> ExactValue {
        SourceNumber::parse(lexeme).value().cloned().unwrap()
    }

    #[test]
    fn preserves_the_lexeme_verbatim() {
        let number = SourceNumber::parse("007.500");
        assert_eq!(number.lexeme(), "007.500");
        assert_eq!(number.value(), SourceNumber::parse("7.5").value());
    }

    #[test]
    fn one_tenth_plus_two_tenths_is_exactly_three_tenths() {
        let sum = value_of("0.1").add(&value_of("0.2"));
        assert_eq!(sum, value_of("0.3"));
    }

    #[test]
    fn unsupported_lexemes_stay_symbolic_with_lexeme_preserved() {
        for lexeme in ["1e5", "5.", "1.2.3", "0x10", "--1", "+", "@base_cost"] {
            let number = SourceNumber::parse(lexeme);
            assert_eq!(number.lexeme(), lexeme, "lexeme {lexeme}");
            assert!(number.value().is_none(), "lexeme {lexeme}");
        }
    }

    #[test]
    fn division_by_zero_is_unresolved_not_a_panic_or_approximation() {
        assert!(value_of("1").div(&value_of("0")).is_none());
    }

    #[test]
    fn decimal_rendering_terminates_or_declines() {
        assert_eq!(value_of("2.50").to_decimal_string().as_deref(), Some("2.5"));
        assert_eq!(
            value_of("-0.125").to_decimal_string().as_deref(),
            Some("-0.125")
        );
        assert_eq!(value_of("40").to_decimal_string().as_deref(), Some("40"));
        let third = value_of("1").div(&value_of("3")).unwrap();
        assert_eq!(third.to_decimal_string(), None);
    }

    #[test]
    fn encoding_distinguishes_close_values() {
        use crate::canonical::encode::CanonicalDigest;
        let digest_for = |value: &ExactValue| {
            let mut digest = CanonicalDigest::new("stellaris-docs/numeric-test/v1");
            value.encode(&mut digest);
            digest.finish()
        };
        assert_ne!(digest_for(&value_of("0.1")), digest_for(&value_of("0.10001")));
        assert_eq!(digest_for(&value_of("0.10")), digest_for(&value_of("0.1")));
    }

    proptest! {
        #[test]
        fn integer_arithmetic_matches_i64(
            a in -1_000_000i64..1_000_000,
            b in -1_000_000i64..1_000_000,
        ) {
            let left = value_of(&a.to_string());
            let right = value_of(&b.to_string());
            prop_assert_eq!(left.add(&right), value_of(&(a + b).to_string()));
            prop_assert_eq!(left.sub(&right), value_of(&(a - b).to_string()));
            prop_assert_eq!(left.mul(&right), value_of(&(a * b).to_string()));
        }

        #[test]
        fn decimal_rendering_round_trips(
            int in 0u32..100_000,
            frac in 0u32..10_000,
        ) {
            let lexeme = format!("{int}.{frac:04}");
            let value = value_of(&lexeme);
            let rendered = value.to_decimal_string().unwrap();
            prop_assert_eq!(&value_of(&rendered), &value);
        }

        #[test]
        fn multiply_then_divide_round_trips(
            a in -1_000_000i64..1_000_000,
            b in 1i64..1_000_000,
        ) {
            let left = value_of(&a.to_string());
            let right = value_of(&b.to_string());
            let round_tripped = left.mul(&right).div(&right).unwrap();
            prop_assert_eq!(round_tripped, left);
        }
    }
}
