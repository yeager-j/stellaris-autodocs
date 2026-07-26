//! The application-owned parsed representation.
//!
//! This module must not name a parser-library type. `docs/technical-design.md:267` requires
//! that parser-library values stop at the adapter boundary, and evidence requirement 6 of
//! [the spike](../../../docs/spikes/parser-evaluation.md) asks for proof rather than
//! intent, so `tests/boundary.rs` reads this file and fails if it mentions `jomini`.
//!
//! Two decisions here are load-bearing rather than stylistic.
//!
//! Scalars keep their **exact source bytes**. A numeric value is never converted to `f64`
//! at parse time: `docs/decision-log.md` D-099 forbids binary floating point from reaching
//! source equality, identity, or displayed Base Values, and the resolver oracle observed the
//! game comparing `0.1 + 0.2` against `0.3` as exactly equal
//! (`docs/spikes/resolver-evaluation.md:240`). Bytes also survive files that are not valid
//! UTF-8, which a `String` would either reject or silently corrupt.
//!
//! Containers keep **one ordered item list** rather than separate field and element lists.
//! Paradox script permits `{ a b key = value }`, and splitting that into two collections
//! would discard the interleaving that the source actually states.

use std::ops::Range;

/// A file-relative byte range.
///
/// File-relative rather than absolute so the representation cannot depend on where the
/// corpus is mounted, which is what evidence requirement 12 asks the spike to establish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span {
            start: start as u32,
            end: end as u32,
        }
    }

    pub fn range(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }

    pub fn len(self) -> usize {
        self.end.saturating_sub(self.start) as usize
    }
}

/// How a scalar was written.
///
/// `Number` and `VariableRef` exist because evidence requirement 9 asks for them by name.
/// Classification is lexical and conservative; `Scalar::raw` always holds the original
/// bytes, so a misclassification loses presentation, never content.
///
/// Discriminants are explicit because `digest.rs` hashes them: they are part of a canonical
/// encoding, not a consequence of declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ScalarKind {
    /// A bare token: `yes`, `tech_combat_computers_3`, `industry`.
    Unquoted = 1,
    /// A double-quoted token. `raw` excludes the quotes; `span` includes them.
    Quoted = 2,
    /// An integer or finite base-10 decimal, possibly signed. The lexeme is preserved
    /// exactly and no numeric conversion happens here.
    Number = 3,
    /// `@name` — a scripted-constant reference in value or key position.
    ///
    /// Kept distinct from a block so an unresolved reference stays visible as a reference.
    /// The game itself does not do this: an unresolved `@name` makes it reinterpret the
    /// consuming field as a script-value block and swallow the following lines
    /// (`docs/spikes/resolver-evaluation.md:244`). The resolver has to detect the condition
    /// itself, which it can only do if the parser has not already imitated the game's
    /// misreading.
    VariableRef = 4,
    /// `@[expr]` — an inline arithmetic expression over scripted constants.
    VariableExpr = 5,
    /// `$NAME$` — an inline-script parameter placeholder.
    ///
    /// Inline-script callees are fragments, not whole definitions: the body of
    /// `common/inline_scripts/oracle/param_factor.txt` is `modifier = { factor = $F$ }`.
    /// Requirement 10 forbids the parser from expanding or discarding these.
    Parameter = 6,
    /// `[From.GetName]` — a runtime scope reference, resolvable only against live game
    /// state.
    ///
    /// `CONTEXT.md` names this a Runtime Localization Token and requires it to be preserved
    /// rather than resolved. It appears in script as well as in localization, for instance
    /// `variable_string = [from.from.System.GetName]`.
    ScopeToken = 7,
}

/// A leaf value, with its bytes preserved verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scalar {
    pub kind: ScalarKind,
    /// Exact source bytes of the token, excluding surrounding quotes.
    pub raw: Vec<u8>,
    /// Byte range in the source file, including surrounding quotes. `None` when the
    /// producing adapter cannot supply one.
    pub span: Option<Span>,
}

impl Scalar {
    pub fn text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.raw)
    }
}

/// The operators Paradox script admits between a key and its value.
///
/// The full set is carried because evidence requirement 2 asks for operator preservation
/// and because a comparison is not a synonym for an assignment: `count > 2` and `count = 2`
/// are different requirements, and collapsing them would silently rewrite documented
/// mechanics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Operator {
    Equal = 1,
    Exact = 2,
    NotEqual = 3,
    LessThan = 4,
    LessThanEqual = 5,
    GreaterThan = 6,
    GreaterThanEqual = 7,
    Exists = 8,
}

impl Operator {
    pub fn symbol(self) -> &'static str {
        match self {
            Operator::Equal => "=",
            Operator::Exact => "==",
            Operator::NotEqual => "!=",
            Operator::LessThan => "<",
            Operator::LessThanEqual => "<=",
            Operator::GreaterThan => ">",
            Operator::GreaterThanEqual => ">=",
            Operator::Exists => "?=",
        }
    }
}

/// A `key <op> value` pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub key: Scalar,
    pub operator: Operator,
    pub value: Value,
    /// Key start through value end.
    pub span: Option<Span>,
}

/// One position in a container, in source order.
///
/// Duplicate keys are ordinary items rather than a merged map. The resolver oracle found
/// that duplicate resolution runs in opposite directions between registries — technologies
/// keep the last definition, scripted constants and events keep the first
/// (`docs/spikes/resolver-evaluation.md:113`) — so a parser that deduplicated would have
/// destroyed the evidence the resolver needs before the resolver ever saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Field(Field),
    /// A bare element, as in `category = { industry }` or a mixed container's array half.
    Element(Value),
    /// `[[NAME] … ]` — content compiled in only when an inline-script argument is defined.
    Conditional(Conditional),
}

/// Conditional compilation on an inline-script parameter.
///
/// ```text
/// giga_job_weight = {
///     [[IMPORTANT] base = @specialist_job_weight ]
///     [[!IMPORTANT] base = @worker_job_weight ]
/// }
/// ```
///
/// Vanilla uses this in `common/script_values` and `common/scripted_effects`, and
/// Gigastructural Engineering uses it heavily. It is modelled as its own item rather than
/// flattened into the enclosing container because the body is not unconditionally part of
/// the definition: flattening would state that a definition contains fields it only
/// sometimes contains, and both branches of a `[[X]` / `[[!X]` pair would appear at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conditional {
    /// The parameter name, without brackets or the negation mark.
    pub parameter: Vec<u8>,
    /// `[[!NAME] … ]`: the body applies when the parameter is *absent*.
    pub negated: bool,
    pub items: Vec<Item>,
    pub span: Option<Span>,
}

/// What shape a container turned out to have. Derived from its items, never declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ContainerKind {
    Empty = 1,
    /// Only `key <op> value` items.
    Object = 2,
    /// Only bare elements.
    Array = 3,
    /// Both, in one container.
    Mixed = 4,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    pub kind: ContainerKind,
    pub items: Vec<Item>,
    /// Opening brace through closing brace, inclusive.
    pub span: Option<Span>,
}

impl Container {
    /// Classify a container by what it holds.
    ///
    /// Conditional blocks do not vote. Their body may contain fields, elements, or both,
    /// and letting that decide the enclosing container's shape would make a definition's
    /// kind depend on an argument that is not supplied at parse time.
    pub fn from_items(items: Vec<Item>, span: Option<Span>) -> Self {
        let mut fields = 0usize;
        let mut elements = 0usize;
        for item in &items {
            match item {
                Item::Field(_) => fields += 1,
                Item::Element(_) => elements += 1,
                Item::Conditional(_) => {}
            }
        }
        let kind = match (fields, elements) {
            (0, 0) => ContainerKind::Empty,
            (_, 0) => ContainerKind::Object,
            (0, _) => ContainerKind::Array,
            _ => ContainerKind::Mixed,
        };
        Container { kind, items, span }
    }

    pub fn fields(&self) -> impl Iterator<Item = &Field> {
        self.items.iter().filter_map(|item| match item {
            Item::Field(field) => Some(field),
            _ => None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Scalar(Scalar),
    Container(Container),
    /// A typed literal such as `color = rgb { 100 200 50 }`, where a scalar precedes a
    /// container. Retained as its own shape so the tag is not mistaken for a sibling value.
    Tagged {
        tag: Scalar,
        container: Container,
        span: Option<Span>,
    },
}

/// One parsed file.
///
/// `recovered` records definitions that survived a syntax fault later in the file. It is
/// separate from `items` only in the sense that a reader can ask; the items themselves are
/// in source order regardless of whether recovery was involved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFile {
    pub items: Vec<Item>,
    /// Faults encountered and resynchronized past. Empty for a clean parse.
    pub faults: Vec<Fault>,
}

/// A syntax fault the adapter recovered from, with where it happened and what it cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub message: String,
    /// Byte offset in the source file the fault is attributed to.
    pub offset: u32,
    /// Byte offset the parser resumed from, when it resumed at all.
    pub resumed_at: Option<u32>,
    /// Items that parsed inside the incomplete construct and were then discarded.
    ///
    /// Recorded because "recovered" and "recovered everything" are different claims, and a
    /// blast-radius number that counted only surviving definitions would quietly report the
    /// second while measuring the first.
    pub abandoned: u32,
}

impl ParsedFile {
    /// Top-level `key <op> value` definitions, which is the unit the blast-radius
    /// measurement counts.
    pub fn definitions(&self) -> impl Iterator<Item = &Field> {
        self.items.iter().filter_map(|item| match item {
            Item::Field(field) => Some(field),
            _ => None,
        })
    }
}
