//! The independent second reading, and the comparison that makes it useful.
//!
//! One class of parser defect has no detector inside a single reading: a **silent structural
//! misread**, where the file is syntactically valid, the reading is internally consistent,
//! and the result is simply not what the file says. The shipped ACOT component file that
//! begins with a stray `3407` is the standing example — one reading finds 336 top-level
//! definitions, another finds 326, and neither reports anything. Only the disagreement
//! surfaces it (`docs/spikes/parser-evaluation.md`, "The cross-check found garbage in
//! shipped source").
//!
//! So this module reads the same bytes a second way, through `jomini::TextTape`, and
//! compares. It walks `tape.tokens()` rather than the `ObjectReader` DOM, whose
//! `field_groups` collapses repeated keys — a normalization that would destroy the very
//! duplicate ordering the parsed model exists to preserve.
//!
//! # Independence is the whole value
//!
//! This reading shares **no** trivia handling and **no** dialect lexing with the production
//! adapter. It does not call `skip_trivia` or `stellaris_construct`, and it re-derives scope
//! tokens and conditional blocks from tape tokens by its own rules. Two readings that shared
//! their lexer would agree about a shared mistake, which is the one thing a cross-check must
//! not do.
//!
//! # What the compared projection carries, and what it deliberately does not
//!
//! [`Shape`] carries item structure, field keys, operator symbols, and exact scalar bytes.
//! It omits two things on purpose:
//!
//! - **Source ranges.** No tape token carries the position of a brace or an operator, so the
//!   tape has nothing to contribute and every comparison would fail for the uninteresting
//!   reason. Ranges are checked far more strictly elsewhere, by re-slicing them from the
//!   source (`super::super::ranges`).
//! - **Scalar kinds.** A kind is a pure function of the raw bytes. Sharing the production
//!   classifier would couple the readings; writing a second classifier would make
//!   classifier disagreement masquerade as a structural finding. Comparing the bytes
//!   answers the question the kind is derived from. Container kind is omitted for the same
//!   reason — it is derived from the item list, which is compared directly.

use super::super::{Container, Item, ParsedFile, Value};
use jomini::text::{Operator as TapeOperator, TextTape, TextToken};

/// A structural projection of one file, derivable from either reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Shape {
    Field {
        key: Vec<u8>,
        operator: &'static str,
        value: Box<Shape>,
    },
    Element(Box<Shape>),
    Conditional {
        parameter: Vec<u8>,
        negated: bool,
        items: Vec<Shape>,
    },
    Scalar(Vec<u8>),
    Container(Vec<Shape>),
    Tagged {
        tag: Vec<u8>,
        container: Vec<Shape>,
    },
}

impl Shape {
    fn label(&self) -> &'static str {
        match self {
            Self::Field { .. } => "field",
            Self::Element(_) => "element",
            Self::Conditional { .. } => "conditional",
            Self::Scalar(_) => "scalar",
            Self::Container(_) => "container",
            Self::Tagged { .. } => "tagged value",
        }
    }
}

/// Whether the tape walk pairs keys with values faithfully, or is deliberately broken.
///
/// The negative control for this whole module. A cross-check that has only ever reported
/// the divergences it was told to expect has not been shown to detect anything;
/// [`Pairing::Perturbed`] seeds exactly the failure the cross-check exists to catch — a
/// mispairing that leaves both readings individually plausible and differing only in
/// structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pairing {
    Faithful,
    Perturbed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TapeRejection {
    pub(super) message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Divergence {
    /// Where in the projection the readings first disagree, e.g.
    /// `<root>[19].value.on_build_complete[1]`.
    pub(super) path: String,
    pub(super) detail: String,
}

/// Reads `data` through the tape and projects it, or reports the tape's refusal.
///
/// A refusal is an ordinary outcome, not a failure of the run: the tape rejects real
/// Stellaris dialect the adapter handles (escaped scripted-constant arithmetic, bare token
/// lists, conditional-compilation blocks). Those files are excluded from comparison and
/// counted, exactly as the spike did.
pub(super) fn read(data: &[u8], pairing: Pairing) -> Result<Vec<Shape>, TapeRejection> {
    let tape = TextTape::from_slice(data).map_err(|error| TapeRejection {
        message: error.to_string(),
    })?;
    let tokens = tape.tokens();
    let mut walker = Walker {
        tokens,
        pairing,
        perturbed_once: false,
    };
    Ok(walker.items(0, tokens.len(), Nesting::Object))
}

/// Projects the production reading into the same shape.
pub(super) fn project(file: &ParsedFile) -> Vec<Shape> {
    file.items
        .iter()
        .map(|parsed| project_item(&parsed.item))
        .collect()
}

fn project_item(item: &Item) -> Shape {
    match item {
        Item::Field(field) => Shape::Field {
            key: field.key.raw.clone(),
            operator: field.operator.symbol(),
            value: Box::new(project_value(&field.value)),
        },
        Item::Element(value) => Shape::Element(Box::new(project_value(value))),
        Item::Conditional(conditional) => Shape::Conditional {
            parameter: conditional.parameter.clone(),
            negated: conditional.negated,
            items: conditional.items.iter().map(project_item).collect(),
        },
    }
}

fn project_value(value: &Value) -> Shape {
    match value {
        Value::Scalar(scalar) => Shape::Scalar(scalar.raw.clone()),
        Value::Container(container) => Shape::Container(project_container(container)),
        Value::Tagged { tag, container, .. } => Shape::Tagged {
            tag: tag.raw.clone(),
            container: project_container(container),
        },
    }
}

fn project_container(container: &Container) -> Vec<Shape> {
    container.items.iter().map(project_item).collect()
}

/// The first place the two readings disagree, or `None` when they agree everywhere.
///
/// First rather than all: a single structural mispairing shifts everything after it, so a
/// full list would report one defect thousands of times and bury the next one.
pub(super) fn first_divergence(tape: &[Shape], adapter: &[Shape]) -> Option<Divergence> {
    diff_items(tape, adapter, "<root>")
}

fn diff_items(tape: &[Shape], adapter: &[Shape], path: &str) -> Option<Divergence> {
    if tape.len() != adapter.len() {
        return Some(Divergence {
            path: path.to_owned(),
            detail: format!("item count {} vs {}", tape.len(), adapter.len()),
        });
    }
    tape.iter()
        .zip(adapter)
        .enumerate()
        .find_map(|(index, (tape, adapter))| diff(tape, adapter, &format!("{path}[{index}]")))
}

fn diff(tape: &Shape, adapter: &Shape, path: &str) -> Option<Divergence> {
    match (tape, adapter) {
        (
            Shape::Field {
                key: tape_key,
                operator: tape_operator,
                value: tape_value,
            },
            Shape::Field {
                key: adapter_key,
                operator: adapter_operator,
                value: adapter_value,
            },
        ) => {
            let path = format!("{path}({})", text(adapter_key));
            if tape_key != adapter_key {
                return Some(Divergence {
                    path,
                    detail: format!("key {} vs {}", text(tape_key), text(adapter_key)),
                });
            }
            if tape_operator != adapter_operator {
                return Some(Divergence {
                    path,
                    detail: format!("operator {tape_operator} vs {adapter_operator}"),
                });
            }
            diff(tape_value, adapter_value, &format!("{path}.value"))
        }
        (Shape::Element(tape), Shape::Element(adapter)) => diff(tape, adapter, path),
        (
            Shape::Conditional {
                parameter: tape_parameter,
                negated: tape_negated,
                items: tape_items,
            },
            Shape::Conditional {
                parameter: adapter_parameter,
                negated: adapter_negated,
                items: adapter_items,
            },
        ) => {
            if tape_parameter != adapter_parameter || tape_negated != adapter_negated {
                return Some(Divergence {
                    path: path.to_owned(),
                    detail: format!(
                        "conditional {}{} vs {}{}",
                        if *tape_negated { "!" } else { "" },
                        text(tape_parameter),
                        if *adapter_negated { "!" } else { "" },
                        text(adapter_parameter)
                    ),
                });
            }
            diff_items(tape_items, adapter_items, path)
        }
        (Shape::Scalar(tape), Shape::Scalar(adapter)) => (tape != adapter).then(|| Divergence {
            path: path.to_owned(),
            detail: format!("scalar {} vs {}", text(tape), text(adapter)),
        }),
        (Shape::Container(tape), Shape::Container(adapter)) => diff_items(tape, adapter, path),
        (
            Shape::Tagged {
                tag: tape_tag,
                container: tape_container,
            },
            Shape::Tagged {
                tag: adapter_tag,
                container: adapter_container,
            },
        ) => {
            if tape_tag != adapter_tag {
                return Some(Divergence {
                    path: path.to_owned(),
                    detail: format!("tag {} vs {}", text(tape_tag), text(adapter_tag)),
                });
            }
            diff_items(tape_container, adapter_container, path)
        }
        (tape, adapter) => Some(Divergence {
            path: path.to_owned(),
            detail: format!("{} vs {}", tape.label(), adapter.label()),
        }),
    }
}

fn text(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).into_owned()
}

/// Whether the region being walked reads as pairs, as bare elements, or as both.
///
/// The tape elides the `=` between a key and its value inside an ordinary object, but emits
/// an explicit `Operator(Equal)` once a container has turned mixed. The same three tokens
/// therefore mean different things depending on where the `MixedContainer` marker fell, so
/// the walk tracks the mode instead of inferring pairs from token shape.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Nesting {
    Object,
    Array,
    Mixed,
}

struct Walker<'a, 'b> {
    tokens: &'b [TextToken<'a>],
    pairing: Pairing,
    /// The seeded misread fires once, at the first field of the file. Once is enough to be
    /// detected and keeps the control's blast radius describable.
    perturbed_once: bool,
}

impl Walker<'_, '_> {
    fn items(&mut self, start: usize, end: usize, mut nesting: Nesting) -> Vec<Shape> {
        let mut items = Vec::new();
        let mut index = start;

        while index < end {
            if matches!(self.tokens[index], TextToken::MixedContainer) {
                nesting = Nesting::Mixed;
                index += 1;
                continue;
            }

            // A conditional block is `Parameter(NAME)` followed by an object holding its
            // body, in every nesting mode. It is lifted before the pair rules run: reading it
            // as an ordinary key and value would claim a definition unconditionally contains
            // fields it only contains when the argument is supplied.
            if let Some((conditional, next)) = self.conditional(index, end) {
                items.push(conditional);
                index = next;
                continue;
            }

            match nesting {
                Nesting::Array | Nesting::Mixed => {
                    let (value, next) = self.value(index);
                    // Pair a scalar with a following operator, or with a following container
                    // when the source elided the `=`. The tape labels a container `Array`
                    // whenever its first item is not an explicit pair, so `{ rgb { 1 2 3 } }`
                    // arrives here even though it is an assignment.
                    let paired = self
                        .operator_at(next, end)
                        .map(|operator| (operator, next + 1))
                        .or_else(|| {
                            let scalar = matches!(value, Shape::Scalar(_));
                            let opens = matches!(
                                self.tokens.get(next),
                                Some(TextToken::Array { .. } | TextToken::Object { .. })
                            );
                            (scalar && opens).then_some(("=", next))
                        })
                        .filter(|_| !self.perturb());

                    match paired {
                        Some((operator, value_at)) => {
                            let (right, after) = self.value(value_at);
                            items.push(Shape::Field {
                                key: into_key(value),
                                operator,
                                value: Box::new(right),
                            });
                            index = after;
                        }
                        None => {
                            items.push(Shape::Element(Box::new(value)));
                            index = next;
                        }
                    }
                }
                Nesting::Object => {
                    let Some((field, next)) = self.field(index, end) else {
                        // A key with no value can only happen at a truncated tape edge.
                        break;
                    };
                    items.push(field);
                    index = next;
                }
            }
        }

        items
    }

    /// Whether this pairing site is the one the seeded misread breaks.
    fn perturb(&mut self) -> bool {
        if self.pairing == Pairing::Faithful || self.perturbed_once {
            return false;
        }
        self.perturbed_once = true;
        true
    }

    /// Re-joins a runtime scope reference the tape split into `[`, its body, and `]`.
    ///
    /// Reconstructed by concatenation rather than sliced from the source, because the tape
    /// offers no position to slice from. Observed scope references contain no whitespace, so
    /// concatenation reproduces them exactly; a file where the two readings disagree about
    /// one shows up as a divergence, which is the point.
    fn scope_token(&self, index: usize, end: usize) -> Option<(Shape, usize)> {
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
                return Some((Shape::Scalar(raw), cursor));
            }
        }
        None
    }

    fn conditional(&mut self, index: usize, end: usize) -> Option<(Shape, usize)> {
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
            Shape::Container(items) => items,
            other => vec![Shape::Element(Box::new(other))],
        };
        Some((
            Shape::Conditional {
                parameter,
                negated,
                items,
            },
            next,
        ))
    }

    fn field(&mut self, index: usize, end: usize) -> Option<(Shape, usize)> {
        let (key, after_key) = self.value(index);
        if after_key >= end {
            return None;
        }
        let (operator, value_at) = match self.operator_at(after_key, end) {
            Some(operator) => (operator, after_key + 1),
            // Inside an ordinary object the tape omits `=` entirely, so its absence is the
            // assignment rather than a missing token.
            None => ("=", after_key),
        };
        if value_at >= end {
            return None;
        }
        if self.perturb() {
            // The seeded misread: emit the key and the value as two bare elements instead of
            // one field. The result is still a plausible reading of a plausible file, and
            // differs from the adapter's only in structure — the exact shape of the defect
            // this cross-check exists to catch.
            return Some((Shape::Element(Box::new(key)), after_key));
        }
        let (value, next) = self.value(value_at);
        Some((
            Shape::Field {
                key: into_key(key),
                operator,
                value: Box::new(value),
            },
            next,
        ))
    }

    fn operator_at(&self, index: usize, end: usize) -> Option<&'static str> {
        if index >= end {
            return None;
        }
        match self.tokens[index] {
            TextToken::Operator(operator) => Some(symbol(operator)),
            _ => None,
        }
    }

    fn value(&mut self, index: usize) -> (Shape, usize) {
        if let Some((scalar, next)) = self.scope_token(index, self.tokens.len()) {
            return (scalar, next);
        }
        match self.tokens[index] {
            TextToken::Array { end, .. } => {
                // The `mixed` flag only says a `MixedContainer` marker appears somewhere
                // inside; the marker itself is what switches the walk, so the entry mode is
                // always Array.
                let items = self.items(index + 1, end, Nesting::Array);
                (Shape::Container(items), end + 1)
            }
            TextToken::Object { end, .. } => {
                let items = self.items(index + 1, end, Nesting::Object);
                (Shape::Container(items), end + 1)
            }
            TextToken::Header(tag) => {
                let (value, next) = self.value(index + 1);
                let container = match value {
                    Shape::Container(items) => items,
                    // A header always precedes a container; anything else is a tape shape
                    // this walk does not model, kept as a lone element rather than dropped.
                    other => vec![Shape::Element(Box::new(other))],
                };
                (
                    Shape::Tagged {
                        tag: tag.as_bytes().to_vec(),
                        container,
                    },
                    next,
                )
            }
            TextToken::Quoted(scalar)
            | TextToken::Unquoted(scalar)
            | TextToken::Parameter(scalar)
            | TextToken::UndefinedParameter(scalar) => {
                (Shape::Scalar(scalar.as_bytes().to_vec()), index + 1)
            }
            // `End`, `Operator`, and `MixedContainer` are consumed by their owners. Reaching
            // one here means the walk lost its place; an empty container keeps it total
            // instead of panicking part-way through a corpus.
            TextToken::End(_) | TextToken::Operator(_) | TextToken::MixedContainer => {
                (Shape::Container(Vec::new()), index + 1)
            }
        }
    }
}

fn into_key(value: Shape) -> Vec<u8> {
    match value {
        Shape::Scalar(raw) => raw,
        // A container in key position is not something Paradox script expresses; an empty
        // key keeps the projection total and diverges visibly if it ever occurs.
        _ => Vec::new(),
    }
}

fn symbol(operator: TapeOperator) -> &'static str {
    // Jomini's own spelling, deliberately: this reading translates through its library's
    // authority rather than borrowing the adapter's `Operator::symbol`.
    operator.symbol()
}
