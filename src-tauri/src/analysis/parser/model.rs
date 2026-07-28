//! Application-owned representation of parsed Stellaris source.
//!
//! Scalars retain exact bytes instead of decoding to text or floating point. Containers
//! retain one ordered item sequence instead of grouping fields, so duplicates and
//! field/element interleaving remain evidence for the resolver.

use crate::canonical::path::LogicalPath;
use crate::source::SourceKind;
use std::borrow::Cow;
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::analysis) struct SourceIdentity {
    pub source: SourceKind,
    pub logical: LogicalPath,
}

impl SourceIdentity {
    pub fn new(source: SourceKind, logical: LogicalPath) -> Self {
        Self { source, logical }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::analysis) struct SourceRange {
    pub start: u64,
    pub end: u64,
}

impl SourceRange {
    pub fn new(start: usize, end: usize) -> Self {
        debug_assert!(start <= end);
        Self {
            start: start as u64,
            end: end as u64,
        }
    }

    pub fn as_usize(self) -> Option<Range<usize>> {
        Some(usize::try_from(self.start).ok()?..usize::try_from(self.end).ok()?)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(in crate::analysis) enum EvidenceQuality {
    Clean = 1,
    Recovered = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(in crate::analysis) enum ScalarKind {
    Unquoted = 1,
    Quoted = 2,
    Number = 3,
    VariableRef = 4,
    VariableExpr = 5,
    Parameter = 6,
    ScopeToken = 7,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct Scalar {
    pub kind: ScalarKind,
    /// Exact token bytes. Quotes are excluded while `range` includes them.
    pub raw: Vec<u8>,
    pub range: SourceRange,
}

impl Scalar {
    pub fn text(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.raw)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(in crate::analysis) enum Operator {
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
            Self::Equal => "=",
            Self::Exact => "==",
            Self::NotEqual => "!=",
            Self::LessThan => "<",
            Self::LessThanEqual => "<=",
            Self::GreaterThan => ">",
            Self::GreaterThanEqual => ">=",
            Self::Exists => "?=",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct Field {
    pub key: Scalar,
    pub operator: Operator,
    pub value: Value,
    pub range: SourceRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) enum Item {
    Field(Field),
    Element(Value),
    Conditional(Conditional),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct Conditional {
    pub parameter: Vec<u8>,
    pub negated: bool,
    pub items: Vec<Item>,
    pub range: SourceRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(in crate::analysis) enum ContainerKind {
    Empty = 1,
    Object = 2,
    Array = 3,
    Mixed = 4,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct Container {
    pub kind: ContainerKind,
    pub items: Vec<Item>,
    pub range: SourceRange,
}

impl Container {
    pub fn from_items(items: Vec<Item>, range: SourceRange) -> Self {
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
        Self { kind, items, range }
    }

    pub fn fields(&self) -> impl Iterator<Item = &Field> {
        self.items.iter().filter_map(|item| match item {
            Item::Field(field) => Some(field),
            Item::Element(_) | Item::Conditional(_) => None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) enum Value {
    Scalar(Scalar),
    Container(Container),
    Tagged {
        tag: Scalar,
        container: Container,
        range: SourceRange,
    },
}

impl Value {
    pub fn range(&self) -> SourceRange {
        match self {
            Self::Scalar(scalar) => scalar.range,
            Self::Container(container) => container.range,
            Self::Tagged { range, .. } => *range,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct ParsedItem {
    pub evidence: EvidenceQuality,
    pub item: Item,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(in crate::analysis) enum ParseFaultKind {
    UnexpectedCloseBrace = 1,
    OperatorWithoutKey = 2,
    ConditionalDelimiterWithoutBlock = 3,
    ValueExpected = 4,
    ContainerUnclosed = 5,
    ConditionalUnclosed = 6,
    NestingLimitExceeded = 7,
    TokenReaderFailure = 8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct ParseFault {
    pub kind: ParseFaultKind,
    pub position: u64,
    /// Always `None` until STE-23 adds layout-heuristic resynchronization.
    pub recovery_boundary: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct ParsedFile {
    pub source: SourceIdentity,
    pub items: Vec<ParsedItem>,
    pub faults: Vec<ParseFault>,
}

impl ParsedFile {
    pub fn definitions(&self) -> impl Iterator<Item = (&Field, EvidenceQuality)> {
        self.items.iter().filter_map(|parsed| match &parsed.item {
            Item::Field(field) => Some((field, parsed.evidence)),
            Item::Element(_) | Item::Conditional(_) => None,
        })
    }
}
