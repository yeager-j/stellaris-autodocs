//! Path A: adapt Jomini's `TextTape` into the application-owned model.
//!
//! This is the strategy `docs/technical-design.md:269` currently assumes, and the baseline
//! the spike measures everything else against. It walks `tape.tokens()` directly rather
//! than the `ObjectReader` DOM, because the DOM's `field_groups` collapses repeated keys
//! into groups — a normalization the resolver must not receive, since duplicate winners run
//! in opposite directions between registries (`docs/spikes/resolver-evaluation.md:113`).
//!
//! Two limits are inherent and are the spike's subject rather than defects to work around:
//!
//! 1. **No structural source ranges.** `TextToken::{Array,Object}.end` is an index into the
//!    token tape, not a byte offset, and no token carries the position of a brace or an
//!    operator. Scalar spans can be recovered by locating a borrowed token's bytes inside
//!    the original buffer, which `scalar_span` does and labels as unofficial.
//! 2. **Whole-file failure.** `TextTape::from_slice` returns one error for a malformed file
//!    and yields no tokens, so every valid definition in that file is lost with it.

use crate::classify;
use crate::model::{
    Conditional, Container, Fault, Field, Item, Operator, ParsedFile, Scalar, ScalarKind, Span,
    Value,
};
use jomini::text::{Operator as JominiOperator, TextTape, TextToken};

pub fn parse(data: &[u8]) -> Result<ParsedFile, ParseError> {
    let tape = TextTape::from_slice(data).map_err(|error| ParseError {
        message: error.to_string(),
        offset: error.offset(),
    })?;
    let tokens = tape.tokens();
    let mut walker = Walker { data, tokens };
    let items = walker.items(0, tokens.len(), Nesting::Object);
    Ok(ParsedFile {
        items,
        faults: Vec::new(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    /// Byte offset of the failure when Jomini reports one.
    pub offset: Option<usize>,
}

/// How many of each raw tape token a file contains.
///
/// Counted separately from the model because the model deliberately does not carry every
/// distinction the tape makes. Requirement 5 asks whether unknown constructs survive, and
/// the honest way to answer is to show which constructs the corpus actually contains —
/// including `Parameter` and `UndefinedParameter`, which Jomini documents as EU4 syntax and
/// which may not occur in Stellaris at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenCensus {
    pub arrays: usize,
    pub objects: usize,
    pub mixed_containers: usize,
    pub headers: usize,
    pub parameters: usize,
    pub undefined_parameters: usize,
    pub operators: usize,
    pub quoted: usize,
    pub unquoted: usize,
    pub utf8_bom: bool,
}

pub fn token_census(data: &[u8]) -> Result<TokenCensus, ParseError> {
    let tape = TextTape::from_slice(data).map_err(|error| ParseError {
        message: error.to_string(),
        offset: error.offset(),
    })?;

    let mut census = TokenCensus {
        utf8_bom: tape.utf8_bom(),
        ..TokenCensus::default()
    };
    for token in tape.tokens() {
        match token {
            TextToken::Array { .. } => census.arrays += 1,
            TextToken::Object { .. } => census.objects += 1,
            TextToken::MixedContainer => census.mixed_containers += 1,
            TextToken::Header(_) => census.headers += 1,
            TextToken::Parameter(_) => census.parameters += 1,
            TextToken::UndefinedParameter(_) => census.undefined_parameters += 1,
            TextToken::Operator(_) => census.operators += 1,
            TextToken::Quoted(_) => census.quoted += 1,
            TextToken::Unquoted(_) => census.unquoted += 1,
            TextToken::End(_) => {}
        }
    }
    Ok(census)
}

impl From<ParseError> for Fault {
    fn from(error: ParseError) -> Self {
        Fault {
            message: error.message,
            offset: error.offset.unwrap_or(0) as u32,
            resumed_at: None,
            // The tape yields no tokens at all when it fails, so the entire file is
            // abandoned. The count is unknown here and is measured against a clean parse of
            // the same file by the blast-radius run.
            abandoned: 0,
        }
    }
}

/// Whether the region being walked reads as pairs, as bare elements, or as both.
///
/// The tape elides the `=` between a key and its value inside an ordinary object, but emits
/// an explicit `Operator(Equal)` once a container has turned mixed. So the same three
/// tokens mean different things depending on where the `MixedContainer` marker fell, and
/// the walker has to track that rather than inferring pairs from token shape.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Nesting {
    Object,
    Array,
    Mixed,
}

struct Walker<'a, 'b> {
    data: &'a [u8],
    tokens: &'b [TextToken<'a>],
}

impl<'a> Walker<'a, '_> {
    fn items(&mut self, start: usize, end: usize, mut nesting: Nesting) -> Vec<Item> {
        let mut items = Vec::new();
        let mut index = start;

        while index < end {
            if matches!(self.tokens[index], TextToken::MixedContainer) {
                nesting = Nesting::Mixed;
                index += 1;
                continue;
            }

            // A parameter block is `Parameter(NAME)` followed by an object holding its body,
            // in every nesting mode. It is lifted before the pair rules run, because reading
            // it as an ordinary key and value would state that a definition unconditionally
            // contains fields it only contains when the argument is supplied.
            if let Some((conditional, next)) = self.conditional(index, end) {
                items.push(Item::Conditional(conditional));
                index = next;
                continue;
            }

            match nesting {
                Nesting::Array | Nesting::Mixed => {
                    let (value, next) = self.value(index);
                    // Pair a scalar with a following operator, or with a following container
                    // when the source elided the `=`. The tape labels a container Array
                    // whenever its first item is not an explicit pair, so `{ rgb { 1 2 3 } }`
                    // and `{ rotation {0 0 0} }` arrive here even though both are
                    // assignments.
                    let paired = self
                        .operator_at(next, end)
                        .map(|operator| (operator, next + 1))
                        .or_else(|| {
                            let scalar = matches!(value, Value::Scalar(_));
                            let opens = matches!(
                                self.tokens.get(next),
                                Some(TextToken::Array { .. } | TextToken::Object { .. })
                            );
                            (scalar && opens).then_some((Operator::Equal, next))
                        });

                    match paired {
                        Some((operator, value_at)) => {
                            let (right, after) = self.value(value_at);
                            items.push(Item::Field(Field {
                                key: into_key(value),
                                operator,
                                value: right,
                                span: None,
                            }));
                            index = after;
                        }
                        None => {
                            items.push(Item::Element(value));
                            index = next;
                        }
                    }
                }
                Nesting::Object => {
                    let Some((field, next)) = self.field(index, end) else {
                        // A key with no value can only happen at a truncated tape edge.
                        break;
                    };
                    items.push(Item::Field(field));
                    index = next;
                }
            }
        }

        items
    }

    /// Re-join a runtime scope reference the tape split into `[`, its body, and `]`.
    ///
    /// Reconstructed rather than sliced from the source, because the tape offers no
    /// position to slice from. Observed scope references contain no whitespace, so
    /// concatenation reproduces them exactly; the lexer adapter takes its copy straight
    /// from the bytes, and any file where the two disagree surfaces in the divergence list.
    fn scope_token(&mut self, index: usize, end: usize) -> Option<(Scalar, usize)> {
        if self.tokens[index].as_scalar()?.as_bytes() != b"[" {
            return None;
        }
        let mut raw = b"[".to_vec();
        let mut cursor = index + 1;
        while cursor < end {
            let bytes = self.tokens[cursor].as_scalar()?.as_bytes();
            raw.extend_from_slice(bytes);
            cursor += 1;
            if bytes == b"]" {
                return Some((
                    Scalar {
                        kind: ScalarKind::ScopeToken,
                        raw,
                        span: None,
                    },
                    cursor,
                ));
            }
        }
        None
    }

    fn conditional(&mut self, index: usize, end: usize) -> Option<(Conditional, usize)> {
        let (parameter, negated) = match self.tokens[index] {
            TextToken::Parameter(scalar) => (scalar.as_bytes().to_vec(), false),
            TextToken::UndefinedParameter(scalar) => (scalar.as_bytes().to_vec(), true),
            _ => return None,
        };
        if index + 1 >= end {
            return None;
        }
        let (value, next) = self.value(index + 1);
        let items = match value {
            Value::Container(container) => container.items,
            other => vec![Item::Element(other)],
        };
        Some((
            Conditional {
                parameter,
                negated,
                items,
                span: None,
            },
            next,
        ))
    }

    fn field(&mut self, index: usize, end: usize) -> Option<(Field, usize)> {
        let (key, after_key) = self.value(index);
        if after_key >= end {
            return None;
        }
        let (operator, value_at) = match self.operator_at(after_key, end) {
            Some(operator) => (operator, after_key + 1),
            // Inside an ordinary object the tape omits `=` entirely, so its absence is the
            // assignment rather than a missing token.
            None => (Operator::Equal, after_key),
        };
        if value_at >= end {
            return None;
        }
        let (value, next) = self.value(value_at);
        Some((
            Field {
                key: into_key(key),
                operator,
                value,
                span: None,
            },
            next,
        ))
    }

    fn operator_at(&self, index: usize, end: usize) -> Option<Operator> {
        if index >= end {
            return None;
        }
        match self.tokens[index] {
            TextToken::Operator(operator) => Some(convert_operator(operator)),
            _ => None,
        }
    }

    fn value(&mut self, index: usize) -> (Value, usize) {
        if let Some((scalar, next)) = self.scope_token(index, self.tokens.len()) {
            return (Value::Scalar(scalar), next);
        }
        match self.tokens[index] {
            TextToken::Array { end, .. } => {
                // The `mixed` flag only says a `MixedContainer` marker appears somewhere
                // inside; the marker itself is what switches the walk, so the entry mode is
                // always Array.
                let items = self.items(index + 1, end, Nesting::Array);
                (
                    Value::Container(Container::from_items(items, None)),
                    end + 1,
                )
            }
            TextToken::Object { end, .. } => {
                let items = self.items(index + 1, end, Nesting::Object);
                (
                    Value::Container(Container::from_items(items, None)),
                    end + 1,
                )
            }
            TextToken::Header(tag) => {
                let scalar = self.scalar(tag, ScalarKind::Unquoted);
                let (value, next) = self.value(index + 1);
                let container = match value {
                    Value::Container(container) => container,
                    // A header always precedes a container; anything else would be a tape
                    // shape this walker does not model, so it is kept as a lone element
                    // rather than silently discarded.
                    other => Container::from_items(vec![Item::Element(other)], None),
                };
                (
                    Value::Tagged {
                        tag: scalar,
                        container,
                        span: None,
                    },
                    next,
                )
            }
            TextToken::Quoted(scalar) => (
                Value::Scalar(self.scalar(scalar, ScalarKind::Quoted)),
                index + 1,
            ),
            TextToken::Unquoted(scalar) => {
                let kind = classify::unquoted(scalar.as_bytes());
                (Value::Scalar(self.scalar(scalar, kind)), index + 1)
            }
            TextToken::Parameter(scalar) | TextToken::UndefinedParameter(scalar) => (
                Value::Scalar(self.scalar(scalar, ScalarKind::Parameter)),
                index + 1,
            ),
            // `End`, `Operator`, and `MixedContainer` are consumed by their owners. Reaching
            // one here means the walker lost its place; representing it as an empty
            // container keeps the walk total instead of panicking on a corpus file.
            _ => (
                Value::Container(Container::from_items(Vec::new(), None)),
                index + 1,
            ),
        }
    }

    fn scalar(&self, scalar: jomini::Scalar<'a>, kind: ScalarKind) -> Scalar {
        Scalar {
            kind,
            raw: scalar.as_bytes().to_vec(),
            span: scalar_span(self.data, scalar, kind),
        }
    }
}

fn into_key(value: Value) -> Scalar {
    match value {
        Value::Scalar(scalar) => scalar,
        // A container in key position is not something Paradox script expresses; recording
        // an empty unquoted key keeps the model total and shows up in the census.
        _ => Scalar {
            kind: ScalarKind::Unquoted,
            raw: Vec::new(),
            span: None,
        },
    }
}

fn convert_operator(operator: JominiOperator) -> Operator {
    match operator {
        JominiOperator::Equal => Operator::Equal,
        JominiOperator::Exact => Operator::Exact,
        JominiOperator::NotEqual => Operator::NotEqual,
        JominiOperator::LessThan => Operator::LessThan,
        JominiOperator::LessThanEqual => Operator::LessThanEqual,
        JominiOperator::GreaterThan => Operator::GreaterThan,
        JominiOperator::GreaterThanEqual => Operator::GreaterThanEqual,
        JominiOperator::Exists => Operator::Exists,
    }
}

/// Recover a scalar's byte range by locating its borrowed bytes inside the original buffer.
///
/// **This is an unofficial technique.** Jomini documents no relationship between a token's
/// `Scalar` and a position in the input; it happens to borrow, so the address arithmetic
/// happens to work. The spike measures what it can and cannot reach precisely so the
/// finding can say how far the supported API alone would have gone: it locates scalars and
/// nothing else — not braces, not operators, and therefore not a definition's extent.
///
/// A quoted scalar's borrowed bytes exclude its quotes, so the span is widened by one on
/// each side to name the text the source actually contains.
pub fn scalar_span(data: &[u8], scalar: jomini::Scalar<'_>, kind: ScalarKind) -> Option<Span> {
    let bytes = scalar.as_bytes();
    let base = data.as_ptr() as usize;
    let start = bytes.as_ptr() as usize;
    if start < base || start.checked_add(bytes.len())? > base + data.len() {
        return None;
    }

    let offset = start - base;
    let (from, to) = match kind {
        ScalarKind::Quoted if offset > 0 && offset + bytes.len() < data.len() => {
            (offset - 1, offset + bytes.len() + 1)
        }
        _ => (offset, offset + bytes.len()),
    };
    Some(Span::new(from, to))
}
