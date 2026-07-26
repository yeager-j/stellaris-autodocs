//! Canonical structural digest, and a locator for the first structural difference.
//!
//! The digest deliberately **excludes spans**. Two questions need separating: "do the two
//! adapters agree about what the file says?" and "are the byte ranges right?". Folding
//! spans into one hash would answer neither, because the tape adapter has no spans to
//! contribute and every comparison would fail for the uninteresting reason.
//!
//! Encoding is tagged and length-prefixed so that no two distinct trees can produce the
//! same byte stream by concatenation — `{ "a" }` and `{ a }` differ in their tag, and
//! `{ ab }` and `{ a b }` differ in their lengths.

use crate::model::{Container, Field, Item, ParsedFile, Scalar, Value};
use sha2::{Digest, Sha256};

pub fn digest(file: &ParsedFile) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"stellaris-docs/parsed-file/v1\0");
    write_items(&mut hasher, &file.items);
    hex(hasher.finalize().as_slice())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn write_len(hasher: &mut Sha256, len: usize) {
    hasher.update((len as u64).to_be_bytes());
}

fn write_items(hasher: &mut Sha256, items: &[Item]) {
    write_len(hasher, items.len());
    for item in items {
        match item {
            Item::Field(field) => {
                hasher.update([0x01]);
                write_scalar(hasher, &field.key);
                hasher.update([field.operator as u8]);
                write_value(hasher, &field.value);
            }
            Item::Element(value) => {
                hasher.update([0x02]);
                write_value(hasher, value);
            }
            Item::Conditional(conditional) => {
                hasher.update([0x03, u8::from(conditional.negated)]);
                write_len(hasher, conditional.parameter.len());
                hasher.update(&conditional.parameter);
                write_items(hasher, &conditional.items);
            }
        }
    }
}

fn write_scalar(hasher: &mut Sha256, scalar: &Scalar) {
    hasher.update([scalar.kind as u8 | 0x10]);
    write_len(hasher, scalar.raw.len());
    hasher.update(&scalar.raw);
}

fn write_container(hasher: &mut Sha256, container: &Container) {
    hasher.update([container.kind as u8 | 0x20]);
    write_items(hasher, &container.items);
}

fn write_value(hasher: &mut Sha256, value: &Value) {
    match value {
        Value::Scalar(scalar) => {
            hasher.update([0x30]);
            write_scalar(hasher, scalar);
        }
        Value::Container(container) => {
            hasher.update([0x31]);
            write_container(hasher, container);
        }
        Value::Tagged { tag, container, .. } => {
            hasher.update([0x32]);
            write_scalar(hasher, tag);
            write_container(hasher, container);
        }
    }
}

/// Where two parsed files first disagree, described well enough to debug from a report.
///
/// A count of mismatched files answers "is the lexer adapter wrong?" but not "how is it
/// wrong?", and the second question is the one that decides whether a hand-built wrapper is
/// maintainable. So the comparison reports a path such as
/// `technology[3].value.potential.items[0]` and what differed there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divergence {
    pub path: String,
    pub detail: String,
}

pub fn first_divergence(left: &ParsedFile, right: &ParsedFile) -> Option<Divergence> {
    let mut path = String::new();
    diff_items(&mut path, &left.items, &right.items)
}

fn report(path: &str, detail: String) -> Option<Divergence> {
    Some(Divergence {
        path: if path.is_empty() { "<root>".into() } else { path.into() },
        detail,
    })
}

fn scoped<T>(path: &mut String, segment: &str, body: impl FnOnce(&mut String) -> T) -> T {
    let restore = path.len();
    path.push_str(segment);
    let out = body(path);
    path.truncate(restore);
    out
}

fn diff_items(path: &mut String, left: &[Item], right: &[Item]) -> Option<Divergence> {
    if left.len() != right.len() {
        return report(
            path,
            format!("item count {} vs {}", left.len(), right.len()),
        );
    }
    for (index, (l, r)) in left.iter().zip(right).enumerate() {
        let found = scoped(path, &format!("[{index}]"), |path| match (l, r) {
            (Item::Field(l), Item::Field(r)) => diff_field(path, l, r),
            (Item::Element(l), Item::Element(r)) => diff_value(path, l, r),
            (Item::Conditional(l), Item::Conditional(r)) => {
                if l.parameter != r.parameter || l.negated != r.negated {
                    return report(
                        path,
                        format!(
                            "conditional [[{}{}] vs [[{}{}]",
                            if l.negated { "!" } else { "" },
                            String::from_utf8_lossy(&l.parameter),
                            if r.negated { "!" } else { "" },
                            String::from_utf8_lossy(&r.parameter),
                        ),
                    );
                }
                diff_items(path, &l.items, &r.items)
            }
            _ => report(
                path,
                format!("item kind {} vs {}", item_kind(l), item_kind(r)),
            ),
        });
        if found.is_some() {
            return found;
        }
    }
    None
}

fn diff_field(path: &mut String, left: &Field, right: &Field) -> Option<Divergence> {
    if left.key.raw != right.key.raw || left.key.kind != right.key.kind {
        return report(
            path,
            format!(
                "key {:?} `{}` vs {:?} `{}`",
                left.key.kind,
                left.key.text(),
                right.key.kind,
                right.key.text()
            ),
        );
    }
    if left.operator != right.operator {
        return report(
            path,
            format!(
                "operator `{}` vs `{}`",
                left.operator.symbol(),
                right.operator.symbol()
            ),
        );
    }
    scoped(path, &format!(".{}", left.key.text()), |path| {
        diff_value(path, &left.value, &right.value)
    })
}

fn diff_value(path: &mut String, left: &Value, right: &Value) -> Option<Divergence> {
    match (left, right) {
        (Value::Scalar(l), Value::Scalar(r)) => diff_scalar(path, l, r),
        (Value::Container(l), Value::Container(r)) => diff_container(path, l, r),
        (
            Value::Tagged {
                tag: lt,
                container: lc,
                ..
            },
            Value::Tagged {
                tag: rt,
                container: rc,
                ..
            },
        ) => diff_scalar(path, lt, rt).or_else(|| diff_container(path, lc, rc)),
        _ => report(
            path,
            format!("value shape {} vs {}", shape(left), shape(right)),
        ),
    }
}

fn diff_container(path: &mut String, left: &Container, right: &Container) -> Option<Divergence> {
    if left.kind != right.kind {
        return report(
            path,
            format!("container kind {:?} vs {:?}", left.kind, right.kind),
        );
    }
    diff_items(path, &left.items, &right.items)
}

fn diff_scalar(path: &mut String, left: &Scalar, right: &Scalar) -> Option<Divergence> {
    if left.raw != right.raw || left.kind != right.kind {
        return report(
            path,
            format!(
                "scalar {:?} `{}` vs {:?} `{}`",
                left.kind,
                left.text(),
                right.kind,
                right.text()
            ),
        );
    }
    None
}

fn item_kind(item: &Item) -> String {
    match item {
        Item::Field(field) => format!("field `{}`", field.key.text()),
        Item::Element(value) => format!("element ({})", shape(value)),
        Item::Conditional(conditional) => format!(
            "conditional [[{}]",
            String::from_utf8_lossy(&conditional.parameter)
        ),
    }
}

fn shape(value: &Value) -> &'static str {
    match value {
        Value::Scalar(_) => "scalar",
        Value::Container(_) => "container",
        Value::Tagged { .. } => "tagged",
    }
}
