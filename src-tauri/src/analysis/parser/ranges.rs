//! Re-slicing every derived byte range from the source it claims to cover.
//!
//! The adapter derives ranges rather than reading them off the token stream
//! (`jomini.rs`'s header explains why the reader's position cannot serve as an end
//! boundary), so a derived range is a claim about the source and this is what checks it.
//! A container's range must cut a slice that opens `{` and closes `}`; a scalar's must cut
//! exactly its `raw` bytes, or those bytes inside quotes; a field's must span its key
//! through its value.
//!
//! Test-only, and shared by two surfaces on purpose: the fixture tests here and in
//! `jomini.rs`, and the whole-corpus run in `conformance`. A second copy for the corpus
//! would let the two disagree about what a correct range is, and the corpus is where the
//! answer actually matters — fixtures are 16 files, the corpus is nearly eight thousand.

use super::{
    Container, Field, Item, ParsedFile, Scalar, ScalarKind, SourceIdentity, SourceRange, Value,
};
use crate::canonical::path::LogicalPath;
use crate::source::SourceKind;

/// One range that does not cut what it claims to. `claim` names the node kind so a failure
/// report says what was wrong rather than only where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RangeFault {
    pub(super) range: SourceRange,
    pub(super) claim: &'static str,
}

pub(super) fn verify_ranges(data: &[u8], file: &ParsedFile) -> Vec<RangeFault> {
    let mut faults = Vec::new();
    for parsed in &file.items {
        check_item(data, &parsed.item, &mut faults);
    }
    faults
}

fn source_slice(data: &[u8], range: SourceRange) -> Option<&[u8]> {
    data.get(range.as_usize()?)
}

fn check_item(data: &[u8], item: &Item, faults: &mut Vec<RangeFault>) {
    match item {
        Item::Field(field) => check_field(data, field, faults),
        Item::Element(value) => check_value(data, value, faults),
        Item::Conditional(conditional) => {
            if !source_slice(data, conditional.range)
                .is_some_and(|source| source.starts_with(b"[[") && source.ends_with(b"]"))
            {
                faults.push(RangeFault {
                    range: conditional.range,
                    claim: "conditional",
                });
            }
            for item in &conditional.items {
                check_item(data, item, faults);
            }
        }
    }
}

fn check_field(data: &[u8], field: &Field, faults: &mut Vec<RangeFault>) {
    check_scalar(data, &field.key, faults);
    check_value(data, &field.value, faults);
    if field.range.start != field.key.range.start
        || field.range.end != field.value.range().end
        || source_slice(data, field.range).is_none()
    {
        faults.push(RangeFault {
            range: field.range,
            claim: "field",
        });
    }
}

fn check_value(data: &[u8], value: &Value, faults: &mut Vec<RangeFault>) {
    match value {
        Value::Scalar(scalar) => check_scalar(data, scalar, faults),
        Value::Container(container) => check_container(data, container, faults),
        Value::Tagged {
            tag,
            container,
            range,
        } => {
            check_scalar(data, tag, faults);
            check_container(data, container, faults);
            if range.start != tag.range.start
                || range.end != container.range.end
                || source_slice(data, *range).is_none()
            {
                faults.push(RangeFault {
                    range: *range,
                    claim: "tagged value",
                });
            }
        }
    }
}

fn check_container(data: &[u8], container: &Container, faults: &mut Vec<RangeFault>) {
    if !source_slice(data, container.range)
        .is_some_and(|source| source.starts_with(b"{") && source.ends_with(b"}"))
    {
        faults.push(RangeFault {
            range: container.range,
            claim: "container",
        });
    }
    for item in &container.items {
        check_item(data, item, faults);
    }
}

fn check_scalar(data: &[u8], scalar: &Scalar, faults: &mut Vec<RangeFault>) {
    let source = source_slice(data, scalar.range);
    // A quoted scalar is the one asymmetry in the model: `raw` excludes the quotes and
    // `range` includes them.
    let matches = match scalar.kind {
        ScalarKind::Quoted => source.is_some_and(|source| {
            source.len() == scalar.raw.len() + 2
                && source.starts_with(b"\"")
                && source.ends_with(b"\"")
                && source[1..source.len() - 1] == scalar.raw
        }),
        _ => source == Some(scalar.raw.as_slice()),
    };
    if !matches {
        faults.push(RangeFault {
            range: scalar.range,
            claim: "scalar",
        });
    }
}

#[test]
fn the_range_check_detects_a_shifted_span() {
    // The negative control for every use of this module, corpus run included: a check that
    // has only ever returned an empty vector has not been shown to detect anything.
    let source = b"tech_x = { cost = 100 }\n";
    let identity = SourceIdentity::new(
        SourceKind::TargetMod,
        LogicalPath::parse("shift.txt").unwrap(),
    );
    let mut file = super::parse(identity, source);
    assert!(verify_ranges(source, &file).is_empty());

    let Item::Field(field) = &mut file.items[0].item else {
        panic!("fixture begins with a field");
    };
    field.key.range.start += 1;

    let faults = verify_ranges(source, &file);
    assert!(faults.iter().any(|fault| fault.claim == "scalar"));
    assert!(faults.iter().any(|fault| fault.claim == "field"));
}
