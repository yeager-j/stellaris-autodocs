//! Resolved registries into the canonical documentation model.
//!
//! Deliberately shallow, and deliberately real.
//!
//! Shallow, because the generator is not what this spike decides. It would happily absorb
//! every remaining hour: unlock effects, event chains, route diagrams, the Enigmalith graph.
//! None of that changes which bundle shape wins, and all of it would arrive by way of
//! Resolution Profile rows that are still unresolved.
//!
//! Real, because the *shape and size* of what it produces is exactly what is being weighed. A
//! generator that emitted plausible-looking filler would produce a bundle whose byte counts
//! were a fact about the filler. So every field here comes from a resolved definition, every
//! name is a localization key that exists, every excerpt is sliced from the original bytes,
//! and every icon slot resolves to a file on disk or to a placeholder with a reason.
//!
//! What that leaves out is stated in the evaluation as a declared scaling envelope, not
//! implied by silence.

use crate::docmodel::{
    AssetSlot, Completeness, Documentation, Entry, EntryKey, Excerpt, Requirement, WeightModifier,
    EXCERPT_LIMIT_BYTES,
};
use crate::localization::{Language, Localization};
use crate::resolve::{self, Impact, IssueScope, Resolved, ResolvedDefinition};
use parser_spike::model::{Container, Item, Value};
use std::collections::{BTreeMap, BTreeSet};

/// How much preserved localization a revision carries.
///
/// The documentation cites a small named set of keys, plus whatever those values reference
/// transitively. Preserving the complete tables was the design's assumption and is measured
/// here as the alternative, not as the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalizationScope {
    /// Every key the documentation cites, plus the closure of its static references, in every
    /// available language.
    CitedClosure,
    /// Every key in every available language.
    AllKeys,
}

/// A static reference closure that cannot terminate is a cycle. The localization module
/// detects those; this bound stops one from becoming an unbounded build.
const MAX_REFERENCE_DEPTH: usize = 16;

pub fn generate(
    resolved: &Resolved,
    sources: &BTreeMap<String, Vec<u8>>,
    scope: LocalizationScope,
) -> Documentation {
    let mut issues = resolved.issues.clone();
    let mut entries = Vec::with_capacity(resolved.technologies.len());

    for definition in &resolved.technologies {
        entries.push(entry(definition, resolved, sources, &mut issues));
    }

    link_unlocks(&mut entries);
    attach_issues(&mut entries, &issues);

    let completeness = completeness(&entries, &issues);
    let localization = match scope {
        LocalizationScope::AllKeys => resolved.localization.clone(),
        LocalizationScope::CitedClosure => cited_closure(&entries, &resolved.localization),
    };

    Documentation {
        case: resolved.case.clone(),
        entries,
        localization,
        issues,
        completeness,
    }
}

/// Every key the documentation cites, plus the closure of its static references.
///
/// The closure is taken across *all* languages at once rather than per language, because a
/// reference present in one translation may be absent from another and a per-language closure
/// would preserve a key for French and drop it for German. That would make language switching
/// lose text, which is the one thing preserving every language exists to prevent.
///
/// Runtime Localization Tokens share the `$NAME$` spelling with static references and are only
/// distinguishable by whether the name resolves. Treating them as references is therefore
/// free: an unresolvable name contributes nothing. The reverse error — missing a real
/// reference — would silently drop text at read time, so the ambiguity is resolved towards
/// over-inclusion deliberately.
fn cited_closure(entries: &[Entry], full: &Localization) -> Localization {
    let mut wanted: BTreeSet<String> = BTreeSet::new();
    for entry in entries {
        wanted.insert(entry.name_key.clone());
        wanted.insert(entry.description_key.clone());
    }

    let mut frontier: Vec<String> = wanted.iter().cloned().collect();
    let mut depth = 0;
    while !frontier.is_empty() && depth < MAX_REFERENCE_DEPTH {
        let mut next = Vec::new();
        for key in frontier.drain(..) {
            for table in full.languages.values() {
                let Some(value) = table.entries.get(&key) else {
                    continue;
                };
                for referenced in static_references(value) {
                    if wanted.insert(referenced.clone()) {
                        next.push(referenced);
                    }
                }
            }
        }
        frontier = next;
        depth += 1;
    }

    let mut pruned = Localization {
        languages: BTreeMap::new(),
        directory_mismatches: full.directory_mismatches.clone(),
        headerless: full.headerless.clone(),
    };
    for (language, table) in &full.languages {
        let entries: BTreeMap<String, String> = wanted
            .iter()
            .filter_map(|key| table.entries.get(key).map(|value| (key.clone(), value.clone())))
            .collect();
        // Every available language is retained even when it contributes no cited key, so the
        // language set a reader offers is still the set the sources supply.
        pruned.languages.insert(
            language.clone(),
            Language {
                entries,
                shadowed: table.shadowed,
            },
        );
    }
    pruned
}

/// `$other_key$` references inside a localization value, including the `$KEY|E$` format
/// suffix Stellaris uses.
pub fn static_references(value: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = value;
    while let Some(open) = rest.find('$') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('$') else { break };
        let name = after[..close].split('|').next().unwrap_or_default();
        if !name.is_empty() && name.len() < 128 {
            found.push(name.to_owned());
        }
        rest = &after[close + 1..];
    }
    found
}

fn entry(
    definition: &ResolvedDefinition,
    resolved: &Resolved,
    sources: &BTreeMap<String, Vec<u8>>,
    issues: &mut Vec<crate::resolve::Issue>,
) -> Entry {
    let container = &definition.container;
    let key = EntryKey::technology(&definition.key);

    let requirements = resolve::field(container, "potential")
        .and_then(|field| match &field.value {
            Value::Container(inner) => Some(requirement_group(inner, false)),
            _ => None,
        });

    let weight_modifiers = resolve::field(container, "weight_modifier")
        .and_then(|field| match &field.value {
            Value::Container(inner) => Some(weight_modifiers(inner)),
            _ => None,
        })
        .unwrap_or_default();

    // An unsupported construct is visibly unsupported and marks its consumer's interpretation
    // incomplete. Counting it here rather than at read time is what lets the completeness
    // state be a property of the published revision.
    let unsupported = requirements.as_ref().map_or(0, Requirement::unsupported)
        + weight_modifiers
            .iter()
            .filter_map(|modifier| modifier.condition.as_ref())
            .map(Requirement::unsupported)
            .sum::<usize>();
    if unsupported > 0 {
        issues.push(crate::resolve::Issue {
            code: "requirement_unsupported".into(),
            scope: IssueScope::Entry(definition.key.clone()),
            impact: Impact::EvidencePresentUnsupported,
            message: format!("{unsupported} condition(s) present in source but not interpreted"),
            source: Some(definition.source.clone()),
        });
    }

    let icon = match resolved.icons.get(&definition.key) {
        Some(slot) => AssetSlot::Resolved {
            contributor: slot.contributor.clone(),
            logical: slot.logical.clone(),
            key: None,
        },
        None => AssetSlot::Placeholder {
            reason: "no icon at the conventional path".into(),
        },
    };

    Entry {
        name_key: definition.key.clone(),
        description_key: format!("{}_desc", definition.key),
        area: text(container, "area"),
        tier: text(container, "tier"),
        cost: text(container, "cost"),
        categories: resolve::field(container, "category")
            .and_then(|field| match &field.value {
                Value::Container(inner) => Some(resolve::elements(inner)),
                _ => None,
            })
            .unwrap_or_default(),
        start_tech: flag(container, "start_tech"),
        rare: flag(container, "is_rare"),
        dangerous: flag(container, "is_dangerous"),
        base_weight: text(container, "weight"),
        weight_modifiers,
        prerequisites: resolve::field(container, "prerequisites")
            .and_then(|field| match &field.value {
                Value::Container(inner) => Some(
                    resolve::elements(inner)
                        .into_iter()
                        .map(EntryKey::technology)
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default(),
        unlocks: Vec::new(),
        requirements,
        icon,
        excerpt: excerpt(definition, sources),
        evidence: definition.evidence,
        source: definition.source.clone(),
        shadowed: definition.shadowed.clone(),
        issues: Vec::new(),
        key,
    }
}

fn text(container: &Container, key: &str) -> Option<String> {
    resolve::field(container, key).and_then(|field| resolve::scalar_text(&field.value))
}

fn flag(container: &Container, key: &str) -> bool {
    text(container, key).is_some_and(|value| value == "yes")
}

/// Project a requirement block into the structured shape the product presents.
///
/// A block's fields are conjunctive unless the block says otherwise, so an ordinary
/// `potential = { a = 1 b = 2 }` becomes an "All of" group. `AND`, `OR`, `NOR`, and `NOT` are
/// the shapes the corpus actually uses; everything else that is not a plain leaf becomes
/// `Unsupported` with its key retained.
///
/// `NOR` is `Not { Any { … } }` rather than its own variant, because that is what it means and
/// a presentation layer that had to special-case it would be re-deciding a fact the generator
/// already knew.
fn requirement_group(container: &Container, negated: bool) -> Requirement {
    let mut children = Vec::new();
    for item in &container.items {
        match item {
            Item::Field(field) => {
                let key = field.key.text().into_owned();
                children.push(match (key.as_str(), &field.value) {
                    ("AND", Value::Container(inner)) => requirement_group(inner, false),
                    ("OR", Value::Container(inner)) => Requirement::Any {
                        of: flatten(requirement_group(inner, false)),
                    },
                    ("NOT", Value::Container(inner)) => Requirement::Not {
                        of: Box::new(requirement_group(inner, false)),
                    },
                    ("NOR", Value::Container(inner)) => Requirement::Not {
                        of: Box::new(Requirement::Any {
                            of: flatten(requirement_group(inner, false)),
                        }),
                    },
                    (_, Value::Scalar(scalar)) => Requirement::Condition {
                        key,
                        operator: field.operator.symbol().to_owned(),
                        value: scalar.text().into_owned(),
                    },
                    (_, Value::Container(inner)) => match arguments(inner) {
                        Some(arguments) => Requirement::Parameterized { key, arguments },
                        None => Requirement::Unsupported {
                            key,
                            reason: "nested block is not a flat argument list".into(),
                        },
                    },
                    (_, Value::Tagged { .. }) => Requirement::Unsupported {
                        key,
                        reason: "tagged literal in requirement position".into(),
                    },
                });
            }
            // A bare element in a requirement block is not a condition. It appears in the
            // corpus and is retained as unsupported rather than guessed at.
            Item::Element(value) => children.push(Requirement::Unsupported {
                key: resolve::scalar_text(value).unwrap_or_else(|| "<element>".into()),
                reason: "bare element in requirement position".into(),
            }),
            // `[[PARAM] … ]` is not unconditionally part of the definition, so its body is
            // not unconditionally part of the requirement.
            Item::Conditional(conditional) => children.push(Requirement::Unsupported {
                key: String::from_utf8_lossy(&conditional.parameter).into_owned(),
                reason: "conditionally compiled block".into(),
            }),
        }
    }

    let group = Requirement::All { of: children };
    if negated {
        Requirement::Not { of: Box::new(group) }
    } else {
        group
    }
}

/// `All { of: [x] }` wrapping is noise inside an `Any`, so unwrap one level.
fn flatten(requirement: Requirement) -> Vec<Requirement> {
    match requirement {
        Requirement::All { of } => of,
        other => vec![other],
    }
}

/// A flat `{ KEY = value … }` argument list, or `None` if it nests.
fn arguments(container: &Container) -> Option<Vec<(String, String)>> {
    let mut arguments = Vec::new();
    for field in container.fields() {
        let value = resolve::scalar_text(&field.value)?;
        arguments.push((field.key.text().into_owned(), value));
    }
    if arguments.len() == container.items.len() {
        Some(arguments)
    } else {
        None
    }
}

fn weight_modifiers(container: &Container) -> Vec<WeightModifier> {
    let mut modifiers = Vec::new();

    // A bare `factor` directly on `weight_modifier` is an unconditional multiplier, not a
    // conditional one, and it is a real shape in the corpus. Recording it as a modifier with
    // no condition keeps it visible; folding it into the base weight would silently change a
    // documented Base Value.
    if let Some(factor) = text(container, "factor") {
        modifiers.push(WeightModifier {
            factor: Some(factor),
            add: None,
            condition: None,
        });
    }

    for field in container.fields() {
        if field.key.text() != "modifier" {
            continue;
        }
        let Value::Container(inner) = &field.value else {
            continue;
        };

        let factor = text(inner, "factor");
        let add = text(inner, "add");
        let condition = Container::from_items(
            inner
                .items
                .iter()
                .filter(|item| match item {
                    Item::Field(field) => {
                        !matches!(field.key.text().as_ref(), "factor" | "add")
                    }
                    _ => true,
                })
                .cloned()
                .collect(),
            inner.span,
        );

        modifiers.push(WeightModifier {
            factor,
            add,
            condition: if condition.items.is_empty() {
                None
            } else {
                Some(requirement_group(&condition, false))
            },
        });
    }

    modifiers
}

/// Slice the definition's own bytes, bounded and line-aligned.
///
/// `docs/technical-design.md:275`: at most 16 KiB, aligned to line boundaries where possible,
/// with omission shown explicitly and undecodable bytes represented visibly rather than
/// dropped. The complete range stays in the record; the text does not grow to match it.
fn excerpt(definition: &ResolvedDefinition, sources: &BTreeMap<String, Vec<u8>>) -> Excerpt {
    let start = definition.source.span_start as usize;
    let end = definition.source.span_end as usize;

    let Some(bytes) = sources.get(&definition.source.logical) else {
        return Excerpt {
            text: String::new(),
            source_start: definition.source.span_start,
            source_end: definition.source.span_end,
            truncated_head: false,
            truncated_tail: true,
            undecodable_bytes: 0,
        };
    };

    let end = end.min(bytes.len());
    let start = start.min(end);
    let mut slice_end = end;
    let mut truncated_tail = false;
    if end - start > EXCERPT_LIMIT_BYTES {
        slice_end = start + EXCERPT_LIMIT_BYTES;
        // Back off to the last line boundary inside the budget, so the excerpt does not end
        // mid-token. If there is no newline at all in 16 KiB, the hard bound wins: the cap is
        // a limit, not a preference.
        if let Some(newline) = bytes[start..slice_end].iter().rposition(|byte| *byte == b'\n') {
            if newline > 0 {
                slice_end = start + newline + 1;
            }
        }
        truncated_tail = true;
    }

    let slice = &bytes[start..slice_end];
    let text = String::from_utf8_lossy(slice).into_owned();
    // `from_utf8_lossy` substitutes U+FFFD, which is the "represented visibly" rule. Counting
    // the substitutions is what makes the fact reportable instead of merely true.
    let undecodable = text.matches('\u{fffd}').count() as u32;

    Excerpt {
        text,
        source_start: definition.source.span_start,
        source_end: definition.source.span_end,
        truncated_head: false,
        truncated_tail,
        undecodable_bytes: undecodable,
    }
}

/// Materialize the reverse prerequisite edge.
///
/// This is denormalization in its purest form — the same fact stored twice, once per
/// direction — and it is here because a technology page has to answer "what does this unlock"
/// without loading every other record. It is also therefore one of the things the duplication
/// measurement should be able to see.
fn link_unlocks(entries: &mut [Entry]) {
    let mut unlocks: BTreeMap<EntryKey, BTreeSet<EntryKey>> = BTreeMap::new();
    for entry in entries.iter() {
        for prerequisite in &entry.prerequisites {
            unlocks
                .entry(prerequisite.clone())
                .or_default()
                .insert(entry.key.clone());
        }
    }
    for entry in entries.iter_mut() {
        if let Some(reverse) = unlocks.remove(&entry.key) {
            entry.unlocks = reverse.into_iter().collect();
        }
    }
}

/// Attach entry-scoped issues to their entries.
///
/// Entry-scoped only. A registry-scoped issue stays at the registry: `docs/technical-design.md:336`
/// requires impact to follow recorded dependency edges, and copying "the technology set is
/// incomplete" onto all 698 technologies would turn one true statement into 698 misleading
/// ones.
fn attach_issues(entries: &mut [Entry], issues: &[crate::resolve::Issue]) {
    let mut by_entry: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, issue) in issues.iter().enumerate() {
        if let IssueScope::Entry(key) = &issue.scope {
            by_entry.entry(key.as_str()).or_default().push(index);
        }
    }
    for entry in entries {
        if let Some(indices) = by_entry.remove(entry.key.identifier.as_str()) {
            entry.issues = indices;
        }
    }
}

fn completeness(entries: &[Entry], issues: &[crate::resolve::Issue]) -> Completeness {
    let incomplete_registries: BTreeSet<String> = issues
        .iter()
        .filter(|issue| issue.impact == Impact::RegistryCompletenessUnknown)
        .filter_map(|issue| match &issue.scope {
            IssueScope::Registry(name) => Some(name.clone()),
            _ => None,
        })
        .collect();

    let unsupported = entries
        .iter()
        .map(|entry| {
            entry.requirements.as_ref().map_or(0, Requirement::unsupported)
                + entry
                    .weight_modifiers
                    .iter()
                    .filter_map(|modifier| modifier.condition.as_ref())
                    .map(Requirement::unsupported)
                    .sum::<usize>()
        })
        .sum();

    Completeness {
        complete: incomplete_registries.is_empty() && issues.is_empty(),
        incomplete_registries: incomplete_registries.into_iter().collect(),
        entries_with_issues: entries.iter().filter(|entry| !entry.issues.is_empty()).count(),
        unsupported_facts: unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(text: &str) -> Container {
        let parsed = parser_spike::lexer::parse(text.as_bytes());
        let value = parsed
            .definitions()
            .next()
            .expect("one definition")
            .value
            .clone();
        match value {
            Value::Container(container) => container,
            _ => panic!("definition is a container"),
        }
    }

    #[test]
    fn a_plain_block_is_a_conjunction_and_or_becomes_a_disjunction() {
        let container = body(
            "t = { potential = { has_ethic = ethic_materialist OR = { a = 1 b = 2 } } }",
        );
        let potential = match &resolve::field(&container, "potential").unwrap().value {
            Value::Container(inner) => requirement_group(inner, false),
            _ => panic!(),
        };

        let Requirement::All { of } = &potential else {
            panic!("a plain block is conjunctive: {potential:?}");
        };
        assert_eq!(of.len(), 2);
        assert!(matches!(of[0], Requirement::Condition { .. }));
        let Requirement::Any { of: alternatives } = &of[1] else {
            panic!("OR becomes Any: {:?}", of[1]);
        };
        assert_eq!(alternatives.len(), 2);
    }

    #[test]
    fn nor_is_a_negated_disjunction_rather_than_its_own_shape() {
        let container = body("t = { potential = { NOR = { a = 1 b = 2 } } }");
        let potential = match &resolve::field(&container, "potential").unwrap().value {
            Value::Container(inner) => requirement_group(inner, false),
            _ => panic!(),
        };

        let Requirement::All { of } = &potential else { panic!() };
        let Requirement::Not { of: inner } = &of[0] else {
            panic!("NOR negates: {:?}", of[0]);
        };
        assert!(matches!(**inner, Requirement::Any { .. }), "{inner:?}");
    }

    #[test]
    fn an_uninterpretable_condition_stays_visible_as_unsupported() {
        let container = body("t = { potential = { weird = { nested = { deep = 1 } } } }");
        let potential = match &resolve::field(&container, "potential").unwrap().value {
            Value::Container(inner) => requirement_group(inner, false),
            _ => panic!(),
        };

        assert_eq!(potential.unsupported(), 1);
        let Requirement::All { of } = &potential else { panic!() };
        let Requirement::Unsupported { key, .. } = &of[0] else {
            panic!("must not be dropped: {:?}", of[0]);
        };
        assert_eq!(key, "weird");
    }

    #[test]
    fn a_bare_factor_is_an_unconditional_modifier_not_part_of_the_base_weight() {
        let container = body(
            "t = { weight = 25 weight_modifier = { factor = 2 modifier = { factor = 1.5 has_tradition = tr_x } } }",
        );
        let modifiers = match &resolve::field(&container, "weight_modifier").unwrap().value {
            Value::Container(inner) => weight_modifiers(inner),
            _ => panic!(),
        };

        assert_eq!(modifiers.len(), 2);
        assert_eq!(modifiers[0].factor.as_deref(), Some("2"));
        assert!(modifiers[0].condition.is_none());
        assert_eq!(modifiers[1].factor.as_deref(), Some("1.5"));
        assert!(modifiers[1].condition.is_some());
        // The base weight is untouched, and it is the exact lexeme rather than a float.
        assert_eq!(text(&container, "weight").as_deref(), Some("25"));
    }

    #[test]
    fn unlock_edges_are_the_reverse_of_prerequisite_edges() {
        let mut entries = vec![
            skeleton("tech_base", &[]),
            skeleton("tech_one", &["tech_base"]),
            skeleton("tech_two", &["tech_base"]),
        ];
        link_unlocks(&mut entries);

        assert_eq!(
            entries[0].unlocks,
            vec![EntryKey::technology("tech_one"), EntryKey::technology("tech_two")]
        );
        assert!(entries[1].unlocks.is_empty());
    }

    fn skeleton(identifier: &str, prerequisites: &[&str]) -> Entry {
        Entry {
            key: EntryKey::technology(identifier),
            name_key: identifier.into(),
            description_key: format!("{identifier}_desc"),
            area: None,
            tier: None,
            cost: None,
            categories: Vec::new(),
            start_tech: false,
            rare: false,
            dangerous: false,
            base_weight: None,
            weight_modifiers: Vec::new(),
            prerequisites: prerequisites.iter().map(|p| EntryKey::technology(*p)).collect(),
            unlocks: Vec::new(),
            requirements: None,
            icon: AssetSlot::Placeholder {
                reason: "test".into(),
            },
            excerpt: Excerpt {
                text: String::new(),
                source_start: 0,
                source_end: 0,
                truncated_head: false,
                truncated_tail: false,
                undecodable_bytes: 0,
            },
            evidence: crate::resolve::Evidence::Clean,
            source: crate::resolve::SourceRef {
                contributor: "test".into(),
                logical: "common/technology/test.txt".into(),
                ordinal: 0,
                span_start: 0,
                span_end: 0,
            },
            shadowed: Vec::new(),
            issues: Vec::new(),
        }
    }
}
