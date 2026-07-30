//! The single-pass scanner.
//!
//! Marker families interleave in both directions in the shipped corpus — a reference inside a
//! bracket run, a bracket run inside a reference, a reference naming an icon, a bracket run
//! inside another — so a pass per family would mis-slice. One left-to-right walk decides each
//! construct where it starts, and pairing is resolved by looking forward for the closer rather
//! than by pattern matching the whole value.
//!
//! Scanning is byte-level over a `&str`, which is sound because of how the markers encode:
//! `§`, `£` and the no-break space are all two-byte sequences led by `0xC2`, and `$`, `[`, `]`
//! and `|` are ASCII. Neither can occur inside another character's encoding, so a byte-by-byte
//! walk cannot mistake part of a character for a marker, and every span this produces begins
//! and ends on a character boundary. `spans` asserts that rather than trusting it.
//!
//! Looking forward for a closer makes the worst case quadratic in the length of one value — a
//! run of unpaired `$` re-scans the tail each time. Values are bounded by a line of a
//! localization file (the longest in the installed corpus is 2,843 characters), so this buys
//! simplicity at a cost nothing here can feel.

use super::token::{DisplayToken, StyleCode, TextSpan, TokenKind, VerbatimKind};

/// The characters that can begin or end a construct, and therefore the ones a key, icon name,
/// or format suffix may never contain — a body holding one is evidence of a mispaired marker
/// rather than of an unusual name.
///
/// A census of the installed `Pegasus v4.4.6 (fdde)` tree settled that these five are the only
/// structural characters, and that several plausible candidates are not: `%` occurs about 2,400
/// times in each of the ten languages and is prose, `@` appears 26 times outside a reference
/// body against 6,047 inside one, `°` is 1,665 French abbreviations out of 1,697, and all 39
/// `¤` are one corrupted-transmission event translated ten times.
///
/// Deliberately the complement of an allowlist. `interface/fonts.gfx` defines style codes as a
/// data table that mods redefine, and mods likewise define their own icon names and
/// localization keys, so a name is anything that could not be a marker rather than anything
/// this build happens to use.
const MARKERS: [char; 5] = ['§', '£', '$', '[', ']'];

pub(super) fn tokenize(value: &str) -> Vec<DisplayToken> {
    let bytes = value.as_bytes();
    let mut tokens = Vec::new();
    let mut text_start = 0usize;
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        let Some((kind, end)) = construct_at(value, cursor) else {
            cursor += 1;
            continue;
        };
        flush_text(value, text_start, cursor, &mut tokens);
        tokens.push(DisplayToken {
            span: TextSpan::new(cursor, end),
            kind,
        });
        cursor = end;
        text_start = end;
    }
    flush_text(value, text_start, bytes.len(), &mut tokens);

    tokens
}

fn flush_text(value: &str, start: usize, end: usize, tokens: &mut Vec<DisplayToken>) {
    if start < end {
        tokens.push(DisplayToken {
            span: TextSpan::new(start, end),
            kind: TokenKind::Text {
                text: value[start..end].to_owned(),
            },
        });
    }
}

/// The construct beginning at `at`, with the offset just past it, or `None` when `at` is
/// ordinary text.
///
/// A marker that begins nothing recognizable yields a one-character `UnpairedMarker` so the
/// scan resumes on the next character; see `markup`'s header for why recovery is that narrow.
fn construct_at(value: &str, at: usize) -> Option<(TokenKind, usize)> {
    let rest = value.get(at..)?;

    if let Some(body) = rest.strip_prefix('§') {
        let after_marker = at + '§'.len_utf8();
        if body.starts_with('!') {
            return Some((TokenKind::StyleReset, after_marker + 1));
        }
        if let Some(code) = body.bytes().next().and_then(StyleCode::parse) {
            return Some((TokenKind::StyleOn { code }, after_marker + 1));
        }
        return Some((unpaired('§'), after_marker));
    }

    if let Some(body) = rest.strip_prefix('£') {
        let body_start = at + '£'.len_utf8();
        if let Some(offset) = body.find('£') {
            let end = body_start + offset + '£'.len_utf8();
            if let Some(kind) = icon(&body[..offset], &value[at..end]) {
                return Some((kind, end));
            }
        }
        return Some((unpaired('£'), body_start));
    }

    if let Some(body) = rest.strip_prefix('$') {
        if let Some(offset) = body.find('$') {
            let end = at + 1 + offset + 1;
            if let Some(kind) = reference(&body[..offset], &value[at..end]) {
                return Some((kind, end));
            }
        }
        return Some((unpaired('$'), at + 1));
    }

    if rest.starts_with('[') {
        if let Some(end) = closing_bracket(value, at) {
            let body = &value[at + 1..end - 1];
            let kind = if body.trim_start().starts_with('\'') {
                VerbatimKind::ConceptLink
            } else {
                VerbatimKind::RuntimeToken
            };
            return Some((
                TokenKind::Verbatim {
                    kind,
                    text: value[at..end].to_owned(),
                },
                end,
            ));
        }
        return Some((unpaired('['), at + 1));
    }

    None
}

fn unpaired(marker: char) -> TokenKind {
    TokenKind::Verbatim {
        kind: VerbatimKind::UnpairedMarker,
        text: marker.to_string(),
    }
}

/// `£energy£`, `£fleet_status|2£`, `£$SHIP_ICON$£`. `None` rejects the pairing entirely, which
/// leaves the opening `£` unpaired and lets everything between it and the next `£` tokenize on
/// its own — the shipped `"in £energy\u{a0}£Energy Credits"` costs a raw `£` that way instead
/// of the rest of the sentence.
fn icon(body: &str, source: &str) -> Option<TokenKind> {
    let (name, variant) = split_at_suffix(body);
    // A variant is opaque — it may itself be a reference, as `£leader_skill|$LEVEL$£` is — so
    // only emptiness and whitespace disqualify it.
    if variant.is_some_and(|variant| variant.is_empty() || has_whitespace(variant)) {
        return None;
    }
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Some(TokenKind::Icon {
            name: name.to_owned(),
            variant: variant.map(str::to_owned),
        });
    }
    // The icon is selected by a reference instead of named. Recognized narrowly — the name
    // must be exactly one closed `$…$` run — so a mispaired `£` that spanned prose falls
    // through to recovery rather than passing for one of these.
    let inner = name.strip_prefix('$')?.strip_suffix('$')?;
    (!inner.is_empty() && is_name_text(inner)).then(|| TokenKind::Verbatim {
        kind: VerbatimKind::DynamicIcon,
        text: source.to_owned(),
    })
}

/// `$KEY$`, `$KEY|0Y$`, `$@scripted_variable$`.
fn reference(body: &str, source: &str) -> Option<TokenKind> {
    let (key, format) = split_at_suffix(body);
    if key.is_empty()
        || !is_name_text(key)
        || format.is_some_and(|format| format.is_empty() || !is_name_text(format))
    {
        return None;
    }
    if key.starts_with('@') {
        return Some(TokenKind::Verbatim {
            kind: VerbatimKind::ScriptVariable,
            text: source.to_owned(),
        });
    }
    Some(TokenKind::Reference {
        key: key.to_owned(),
        format: format.map(str::to_owned),
    })
}

/// Split a body at its first `|` into the part that names something and the opaque suffix.
fn split_at_suffix(body: &str) -> (&str, Option<&str>) {
    match body.split_once('|') {
        Some((name, suffix)) => (name, Some(suffix)),
        None => (body, None),
    }
}

/// Whether a fragment could name something: no whitespace, and no marker character. A body
/// holding either is evidence that the closer this run paired with belongs to another opener.
fn is_name_text(text: &str) -> bool {
    !has_whitespace(text) && !text.chars().any(|c| MARKERS.contains(&c))
}

fn has_whitespace(text: &str) -> bool {
    text.chars().any(char::is_whitespace)
}

/// The offset just past the `]` that closes the run opening at `at`, counting depth so a
/// bracket run nested inside a concept link closes the outer one.
///
/// `None` for a run that never closes. That is what keeps `"[[$INDEX$] $FLEET_NAME$"` honest:
/// the outer `[` is unpaired, the scan resumes one character later on `[$INDEX$]`, and
/// `$FLEET_NAME$` survives as a reference.
fn closing_bracket(value: &str, at: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, byte) in value.as_bytes().get(at..)?.iter().enumerate() {
        match byte {
            b'[' => depth += 1,
            // `depth` cannot already be zero, because the caller enters on a `[`. Written so
            // that a future caller who breaks that precondition gets no answer rather than an
            // arithmetic panic in a tokenizer that is otherwise total.
            b']' => match depth.checked_sub(1)? {
                0 => return Some(at + offset + 1),
                remaining => depth = remaining,
            },
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::spans::verify_spans;
    use super::*;
    use proptest::prelude::*;

    /// The alphabet is every character that carries structural meaning plus the shapes that
    /// have historically confused a marker scanner: the no-break space, a line break, a
    /// quote, and a backslash. Ordinary letters and digits are there to form bodies.
    const ALPHABET: &str = "§£$[]|!@'.,:&_\\\"aZ0\u{a0}\n";

    fn marker_soup() -> impl Strategy<Value = String> {
        let characters: Vec<char> = ALPHABET.chars().collect();
        proptest::collection::vec(proptest::sample::select(characters), 0..48)
            .prop_map(|characters| characters.into_iter().collect())
    }

    proptest! {
        #[test]
        fn every_value_tokenizes_into_spans_that_tile_it(value in marker_soup()) {
            let tokens = tokenize(&value);
            prop_assert_eq!(verify_spans(&value, &tokens), Vec::new());
        }

        #[test]
        fn the_source_forms_reassemble_the_value(value in marker_soup()) {
            // Independent of the span arithmetic: concatenating what the tokens say they were
            // made from has to give the value back, byte for byte.
            let reassembled: String = tokenize(&value)
                .iter()
                .map(DisplayToken::source_form)
                .collect();
            prop_assert_eq!(reassembled, value);
        }

        #[test]
        fn tokenizing_twice_agrees(value in marker_soup()) {
            prop_assert_eq!(tokenize(&value), tokenize(&value));
        }
    }

    #[test]
    fn the_reassembly_property_detects_a_dropped_marker() {
        // The negative control for the property above. A scanner that treated an unpaired
        // marker as nothing would pass every behavioural case that happens not to contain
        // one, so prove the check that forbids it can fail.
        let tokens: Vec<DisplayToken> = tokenize("a£b")
            .into_iter()
            .filter(|token| {
                !matches!(
                    token.kind,
                    TokenKind::Verbatim {
                        kind: VerbatimKind::UnpairedMarker,
                        ..
                    }
                )
            })
            .collect();
        let reassembled: String = tokens.iter().map(DisplayToken::source_form).collect();
        assert_ne!(reassembled, "a£b");
    }
}
