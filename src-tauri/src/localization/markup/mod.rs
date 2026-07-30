//! Private markup-tokenization seam: Stellaris localization text in, application-owned
//! display tokens out.
//!
//! The rest of `localization` consumes only the vocabulary re-exported here, so the marker
//! grammar lives in one place rather than in every consumer that needs display text.
//!
//! # What the caller must hand over
//!
//! One localization value, already decoded by ingestion: valid UTF-8, byte order mark
//! removed, the delimiting quotes stripped, and the four escapes the corpus actually uses
//! (`\n`, `\"`, `\t`, `\\`) resolved to the characters they stand for. This module never
//! opens a file, consults a table, or resolves a reference. Decoding here would give a line
//! break two representations — an escape token and a literal U+000A — and every consumer
//! would have to know which layer produced the value it holds.
//!
//! # Style runs are flat, not nested
//!
//! `§!` returns to the default style; it does not pop an enclosing one. The installed corpus
//! proves the difference matters: 6,037 values re-open the outer colour by hand after an inner
//! reset, 94 open a run and never reset it, 58 reset with nothing open, and 550 contain a
//! doubled `§!§!`. A renderer therefore keeps a single **current-style register, not a stack**
//! — an unterminated run extends to the end of the value and a stray reset is a no-op, which
//! is the game's own behaviour and needs no error path. Modelling nested spans would require
//! inventing a repair for each of those four shapes.
//!
//! # Nothing is dropped, and nothing unmeasured is interpreted
//!
//! One rule covers every malformed input: **a marker character that begins no construct this
//! module recognizes becomes a one-character [`VerbatimKind::UnpairedMarker`], and scanning
//! resumes at the very next character.** So `"[[$INDEX$] $FLEET_NAME$"` yields a verbatim
//! `[`, then the raw runtime token `[$INDEX$]`, then text — and `$FLEET_NAME$` stays a
//! resolvable reference instead of being swallowed. `[[` is *not* modelled as an escape for a
//! literal `[`: that reading is widely repeated and nowhere measured, and D-131 and D-132 set
//! the standard that detection may precede a record while handling may not. A capture showing
//! what the game displays for one of the 41 vanilla values containing `[[` is what would
//! settle it.
//!
//! Recovery never consumes more than the one character it could not interpret, which is why
//! the mispaired `£` in a shipped value like `"in £energy\u{a0}£Energy Credits"` costs a raw
//! `£` rather than the rest of the sentence.

mod token;

#[cfg(test)]
mod cases;
mod scan;
#[cfg(test)]
mod spans;

pub(super) use token::{DisplayToken, StyleCode, TextSpan, TokenKind, VerbatimKind};

/// Tokenize one already-decoded localization value.
///
/// Total and deterministic: every `&str` yields a token sequence whose spans tile the input
/// exactly, so there is no failure to report and no fault list to carry. An anomaly is a
/// displayable token in position, which is the evidence a parallel fault vector would
/// duplicate.
pub(super) fn tokenize(value: &str) -> Vec<DisplayToken> {
    scan::tokenize(value)
}
