//! Lexical classification of scalar tokens.
//!
//! Both adapters call this. If each classified its own tokens, an A-versus-B divergence
//! could mean "the lexer wrapper is broken" or "the two adapters disagree about what a
//! number looks like", and the cross-check would stop being evidence about the wrapper.
//!
//! Classification is presentation over bytes that are preserved regardless, so the rules
//! stay conservative: anything not recognized stays `Unquoted`.

use crate::model::ScalarKind;

/// Classify an unquoted token by its bytes.
pub fn unquoted(raw: &[u8]) -> ScalarKind {
    match raw.first() {
        // `@[trigger:x]` and `@[ 1 + 2 ]` are expressions; `@name` is a plain reference.
        Some(b'@') if raw.get(1) == Some(&b'[') => ScalarKind::VariableExpr,
        Some(b'@') if raw.len() > 1 => ScalarKind::VariableRef,
        Some(b'$') if raw.len() > 1 && raw.last() == Some(&b'$') => ScalarKind::Parameter,
        _ if is_number(raw) => ScalarKind::Number,
        _ => ScalarKind::Unquoted,
    }
}

/// An integer or finite base-10 decimal, optionally signed.
///
/// Deliberately narrow. Stellaris dates (`2200.1.1`) have two separator dots and must not
/// be classified as numbers, and neither must percentages, hex colors, or identifiers that
/// merely begin with a digit. Anything rejected here keeps its exact bytes as `Unquoted`,
/// so a rejection costs a label and never a value.
fn is_number(raw: &[u8]) -> bool {
    let body = match raw.first() {
        Some(b'-') | Some(b'+') => &raw[1..],
        _ => raw,
    };
    if body.is_empty() {
        return false;
    }

    let mut digits = 0usize;
    let mut dots = 0usize;
    for byte in body {
        match byte {
            b'0'..=b'9' => digits += 1,
            b'.' => dots += 1,
            _ => return false,
        }
    }
    digits > 0 && dots <= 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_keep_their_lexeme_shape() {
        assert_eq!(unquoted(b"0"), ScalarKind::Number);
        assert_eq!(unquoted(b"1000000"), ScalarKind::Number);
        assert_eq!(unquoted(b"0.1"), ScalarKind::Number);
        assert_eq!(unquoted(b"-2.50"), ScalarKind::Number);
        assert_eq!(unquoted(b"+3"), ScalarKind::Number);
    }

    #[test]
    fn near_numbers_stay_unquoted() {
        // A date has two dots. Reading it as a number would invent a value.
        assert_eq!(unquoted(b"2200.1.1"), ScalarKind::Unquoted);
        assert_eq!(unquoted(b"1d"), ScalarKind::Unquoted);
        assert_eq!(unquoted(b"."), ScalarKind::Unquoted);
        assert_eq!(unquoted(b"-"), ScalarKind::Unquoted);
        assert_eq!(unquoted(b"yes"), ScalarKind::Unquoted);
    }

    #[test]
    fn variable_and_parameter_forms_are_distinguished() {
        assert_eq!(unquoted(b"@tier5cost3"), ScalarKind::VariableRef);
        assert_eq!(unquoted(b"@[1-leopard_x]"), ScalarKind::VariableExpr);
        assert_eq!(unquoted(b"$F$"), ScalarKind::Parameter);
        // A bare `@` is not a reference to anything.
        assert_eq!(unquoted(b"@"), ScalarKind::Unquoted);
    }
}
