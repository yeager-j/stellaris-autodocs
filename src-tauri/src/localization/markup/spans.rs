//! The gate that makes "never dropped and never interpreted" checkable rather than eyeballed.
//!
//! Three claims together. The spans **tile** the value — sorted, contiguous, starting at 0,
//! ending at its length — so no character can be dropped without leaving a gap. Each token
//! **accounts for** its own slice, re-derived from its payload, so a token cannot quietly
//! lose a character while its span still tiles. And no two `Text` tokens sit adjacent, so the
//! output is canonical and one input has one tokenization.
//!
//! Test-only, and shared by every surface that tokenizes: the case table, the scanner's own
//! fixtures, and its property test. A second copy would let those disagree about what a
//! correct span is.
//!
//! Modelled on `analysis::parser::ranges`, including the per-node `claim` string — a failure
//! should say what was wrong and not only where — and including the discipline that a gate
//! which has only ever returned an empty vector has not been shown to detect anything. The
//! negative controls at the bottom are that proof, and they build their token streams by hand
//! so they fail independently of whatever the scanner happens to do.

use super::token::{DisplayToken, StyleCode, TextSpan, TokenKind, VerbatimKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SpanFault {
    pub(super) span: TextSpan,
    pub(super) claim: &'static str,
}

pub(super) fn verify_spans(source: &str, tokens: &[DisplayToken]) -> Vec<SpanFault> {
    let mut faults = Vec::new();
    let mut covered = 0usize;

    for (index, token) in tokens.iter().enumerate() {
        let span = token.span;

        if span.start != covered {
            // One claim for both directions: a start behind the cursor overlaps its
            // predecessor and a start ahead of it leaves text unaccounted for, and either way
            // the sequence has stopped tiling at this token.
            faults.push(SpanFault {
                span,
                claim: if span.start < covered {
                    "overlaps the previous token"
                } else {
                    "leaves a gap"
                },
            });
        }
        covered = span.end.max(covered);

        if span.len() == 0 {
            faults.push(SpanFault {
                span,
                claim: "covers nothing",
            });
        }

        match span.slice(source) {
            // `get` returns None for a span that is out of bounds or lands mid-character, so
            // this covers the char-boundary claim as well as the range one.
            None => faults.push(SpanFault {
                span,
                claim: "does not cut the value",
            }),
            Some(slice) if slice != token.source_form() => faults.push(SpanFault {
                span,
                claim: "does not account for its slice",
            }),
            Some(_) => {}
        }

        let follows_text = index
            .checked_sub(1)
            .and_then(|previous| tokens.get(previous))
            .is_some_and(|previous| matches!(previous.kind, TokenKind::Text { .. }));
        if follows_text && matches!(token.kind, TokenKind::Text { .. }) {
            faults.push(SpanFault {
                span,
                claim: "splits a text run",
            });
        }
    }

    if covered != source.len() {
        faults.push(SpanFault {
            span: TextSpan::new(covered, source.len()),
            claim: "leaves the tail of the value uncovered",
        });
    }

    faults
}

/// Panicking form, so a test that only cares about behaviour reads as one line.
pub(super) fn assert_tiles(source: &str, tokens: &[DisplayToken]) {
    let faults = verify_spans(source, tokens);
    assert!(
        faults.is_empty(),
        "tokenizing {source:?} produced {faults:?}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `§Ypay£energy£$K$`, spans and payloads correct. The offsets are byte offsets, so `§`
    /// and `£` cost two apiece.
    fn sound_stream() -> (&'static str, Vec<DisplayToken>) {
        let source = "§Ypay£energy£$K$";
        let tokens = vec![
            DisplayToken {
                span: TextSpan::new(0, 3),
                kind: TokenKind::StyleOn {
                    code: StyleCode::parse(b'Y').unwrap(),
                },
            },
            DisplayToken {
                span: TextSpan::new(3, 6),
                kind: TokenKind::Text {
                    text: "pay".to_owned(),
                },
            },
            DisplayToken {
                span: TextSpan::new(6, 16),
                kind: TokenKind::Icon {
                    name: "energy".to_owned(),
                    variant: None,
                },
            },
            DisplayToken {
                span: TextSpan::new(16, 19),
                kind: TokenKind::Reference {
                    key: "K".to_owned(),
                    format: None,
                },
            },
        ];
        (source, tokens)
    }

    fn claims(source: &str, tokens: &[DisplayToken]) -> Vec<&'static str> {
        verify_spans(source, tokens)
            .into_iter()
            .map(|fault| fault.claim)
            .collect()
    }

    #[test]
    fn a_sound_stream_has_no_faults() {
        let (source, tokens) = sound_stream();
        assert_eq!(verify_spans(source, &tokens), Vec::new());
    }

    #[test]
    fn the_gate_detects_a_dropped_token() {
        let (source, mut tokens) = sound_stream();
        tokens.remove(2);
        assert!(claims(source, &tokens).contains(&"leaves a gap"));
    }

    #[test]
    fn the_gate_detects_a_dropped_final_token() {
        // The tail needs its own control: dropping the last token leaves no successor to
        // notice the gap, so only the closing length check can catch it.
        let (source, mut tokens) = sound_stream();
        tokens.pop();
        assert!(
            claims(source, &tokens).contains(&"leaves the tail of the value uncovered"),
            "dropping the last token went unnoticed"
        );
    }

    #[test]
    fn the_gate_detects_a_shifted_span() {
        let (source, mut tokens) = sound_stream();
        tokens[1].span.start += 1;
        let claims = claims(source, &tokens);
        assert!(
            claims.contains(&"overlaps the previous token") || claims.contains(&"leaves a gap")
        );
        assert!(claims.contains(&"does not account for its slice"));
    }

    #[test]
    fn the_gate_detects_swapped_tokens() {
        let (source, mut tokens) = sound_stream();
        tokens.swap(1, 2);
        assert!(claims(source, &tokens).contains(&"leaves a gap"));
    }

    #[test]
    fn the_gate_detects_a_split_text_run() {
        let source = "paying";
        let tokens = vec![
            DisplayToken {
                span: TextSpan::new(0, 3),
                kind: TokenKind::Text {
                    text: "pay".to_owned(),
                },
            },
            DisplayToken {
                span: TextSpan::new(3, 6),
                kind: TokenKind::Text {
                    text: "ing".to_owned(),
                },
            },
        ];
        assert_eq!(claims(source, &tokens), vec!["splits a text run"]);
    }

    #[test]
    fn the_gate_detects_a_payload_that_lost_a_character() {
        // The tiling half cannot see this one: the spans still cover the value exactly, and
        // only the re-derived source form reveals that the token forgot a character.
        let (source, mut tokens) = sound_stream();
        tokens[2].kind = TokenKind::Icon {
            name: "energ".to_owned(),
            variant: None,
        };
        assert_eq!(
            claims(source, &tokens),
            vec!["does not account for its slice"]
        );
    }

    #[test]
    fn the_gate_detects_a_verbatim_token_that_reinterprets_its_slice() {
        let source = "§こ";
        let tokens = vec![DisplayToken {
            span: TextSpan::new(0, source.len()),
            kind: TokenKind::Verbatim {
                kind: VerbatimKind::UnpairedMarker,
                text: "§".to_owned(),
            },
        }];
        assert_eq!(
            claims(source, &tokens),
            vec!["does not account for its slice"]
        );
    }

    #[test]
    fn the_gate_detects_a_span_that_lands_mid_character() {
        let source = "§!";
        let tokens = vec![DisplayToken {
            span: TextSpan::new(0, 1),
            kind: TokenKind::StyleReset,
        }];
        let claims = claims(source, &tokens);
        assert!(claims.contains(&"does not cut the value"));
        assert!(claims.contains(&"leaves the tail of the value uncovered"));
    }

    #[test]
    fn the_gate_detects_an_empty_token() {
        let source = "x";
        let tokens = vec![
            DisplayToken {
                span: TextSpan::new(0, 0),
                kind: TokenKind::Text {
                    text: String::new(),
                },
            },
            DisplayToken {
                span: TextSpan::new(0, 1),
                kind: TokenKind::Text {
                    text: "x".to_owned(),
                },
            },
        ];
        let claims = claims(source, &tokens);
        assert!(claims.contains(&"covers nothing"));
        assert!(claims.contains(&"splits a text run"));
    }

    #[test]
    fn an_empty_value_tokenizes_to_nothing_without_faulting() {
        assert_eq!(verify_spans("", &[]), Vec::new());
    }

    #[test]
    fn the_gate_detects_an_empty_stream_for_a_non_empty_value() {
        assert_eq!(
            claims("text", &[]),
            vec!["leaves the tail of the value uncovered"]
        );
    }
}
