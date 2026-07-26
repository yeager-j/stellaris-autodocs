//! Counting what a parsed corpus actually contains.
//!
//! Requirements 1, 2, and 5 are claims about coverage, and a coverage claim needs a
//! denominator. "Every file parsed" says nothing if the corpus never exercised a comparison
//! operator or a mixed container, so the runs report which constructs were present as well
//! as whether parsing succeeded.

use crate::model::{Container, ContainerKind, Field, Item, ParsedFile, ScalarKind, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Census {
    pub definitions: usize,
    pub fields: usize,
    pub elements: usize,
    pub max_depth: usize,
    pub operators: BTreeMap<String, usize>,
    pub scalar_kinds: BTreeMap<String, usize>,
    pub container_kinds: BTreeMap<String, usize>,
    pub tagged_values: usize,
    /// `[[NAME] … ]` conditional-compilation blocks, and how many are negated.
    pub conditionals: usize,
    pub negated_conditionals: usize,
    /// Top-level keys defined more than once in one file, counted as extra occurrences.
    pub duplicate_top_level_definitions: usize,
}

impl Census {
    pub fn of(file: &ParsedFile) -> Self {
        let mut census = Census {
            definitions: file.definitions().count(),
            duplicate_top_level_definitions: duplicates(file),
            ..Census::default()
        };
        census.visit_items(&file.items, 0);
        census
    }

    pub fn merge(&mut self, other: &Census) {
        self.definitions += other.definitions;
        self.fields += other.fields;
        self.elements += other.elements;
        self.tagged_values += other.tagged_values;
        self.conditionals += other.conditionals;
        self.negated_conditionals += other.negated_conditionals;
        self.duplicate_top_level_definitions += other.duplicate_top_level_definitions;
        self.max_depth = self.max_depth.max(other.max_depth);
        merge_counts(&mut self.operators, &other.operators);
        merge_counts(&mut self.scalar_kinds, &other.scalar_kinds);
        merge_counts(&mut self.container_kinds, &other.container_kinds);
    }

    fn visit_items(&mut self, items: &[Item], depth: usize) {
        self.max_depth = self.max_depth.max(depth);
        for item in items {
            match item {
                Item::Field(field) => {
                    self.fields += 1;
                    self.visit_field(field, depth);
                }
                Item::Element(value) => {
                    self.elements += 1;
                    self.visit_value(value, depth);
                }
                Item::Conditional(conditional) => {
                    self.conditionals += 1;
                    if conditional.negated {
                        self.negated_conditionals += 1;
                    }
                    self.visit_items(&conditional.items, depth + 1);
                }
            }
        }
    }

    fn visit_field(&mut self, field: &Field, depth: usize) {
        *self
            .operators
            .entry(field.operator.symbol().to_owned())
            .or_default() += 1;
        *self
            .scalar_kinds
            .entry(kind_name(field.key.kind).to_owned())
            .or_default() += 1;
        self.visit_value(&field.value, depth);
    }

    fn visit_value(&mut self, value: &Value, depth: usize) {
        match value {
            Value::Scalar(scalar) => {
                *self
                    .scalar_kinds
                    .entry(kind_name(scalar.kind).to_owned())
                    .or_default() += 1;
            }
            Value::Container(container) => self.visit_container(container, depth),
            Value::Tagged { tag, container, .. } => {
                self.tagged_values += 1;
                *self
                    .scalar_kinds
                    .entry(kind_name(tag.kind).to_owned())
                    .or_default() += 1;
                self.visit_container(container, depth);
            }
        }
    }

    fn visit_container(&mut self, container: &Container, depth: usize) {
        *self
            .container_kinds
            .entry(container_name(container.kind).to_owned())
            .or_default() += 1;
        self.visit_items(&container.items, depth + 1);
    }
}

fn duplicates(file: &ParsedFile) -> usize {
    let mut seen: BTreeMap<&[u8], usize> = BTreeMap::new();
    for definition in file.definitions() {
        *seen.entry(&definition.key.raw).or_default() += 1;
    }
    seen.values().map(|count| count.saturating_sub(1)).sum()
}

fn merge_counts(into: &mut BTreeMap<String, usize>, from: &BTreeMap<String, usize>) {
    for (key, count) in from {
        *into.entry(key.clone()).or_default() += count;
    }
}

fn kind_name(kind: ScalarKind) -> &'static str {
    match kind {
        ScalarKind::Unquoted => "unquoted",
        ScalarKind::Quoted => "quoted",
        ScalarKind::Number => "number",
        ScalarKind::VariableRef => "variable_ref",
        ScalarKind::VariableExpr => "variable_expr",
        ScalarKind::Parameter => "parameter",
        ScalarKind::ScopeToken => "scope_token",
    }
}

fn container_name(kind: ContainerKind) -> &'static str {
    match kind {
        ContainerKind::Empty => "empty",
        ContainerKind::Object => "object",
        ContainerKind::Array => "array",
        ContainerKind::Mixed => "mixed",
    }
}

/// How a file's bytes are encoded, as far as the parser is concerned.
///
/// Recorded because requirement 5 is about not mistaking unfamiliar input for a syntax
/// failure, and a Windows-1252 byte in a mod file is exactly that kind of input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    NotUtf8,
}

impl Encoding {
    pub fn of(data: &[u8]) -> Self {
        if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
            Encoding::Utf8Bom
        } else if std::str::from_utf8(data).is_ok() {
            Encoding::Utf8
        } else {
            Encoding::NotUtf8
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "utf8",
            Encoding::Utf8Bom => "utf8_bom",
            Encoding::NotUtf8 => "not_utf8",
        }
    }
}
