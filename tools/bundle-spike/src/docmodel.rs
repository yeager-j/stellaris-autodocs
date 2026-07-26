//! The canonical in-memory documentation model, and every view derived from it.
//!
//! This is the load-bearing rule of the whole spike. `docs/technical-design.md:630` requires
//! that every denormalized representation be derived from one in-memory documentation model
//! during the same build, and the evaluation restates it. The reason is measurement, not
//! tidiness: if a browse summary and a full record were produced by two generators, a
//! comparison between bundle shapes would be measuring how far those two generators had
//! drifted apart, and no amount of care in the timing harness would recover the intended
//! number.
//!
//! So [`Documentation`] is the only authority here, and [`browse_index`], [`search_material`],
//! [`full_records`], and [`unsharded_payload`] are functions of it. A writer chooses file
//! layout; it never chooses content.
//!
//! ## What is language-independent and what is not
//!
//! Records and browse summaries hold localization *keys*, not localized text. Search material
//! holds localized text, because matching and ranking depend on it
//! (`docs/technical-design.md:636`). That asymmetry is the design's, and it is the single
//! biggest lever on bundle size — a bundle that materialized every record per language would
//! multiply the largest thing in it by ten.

use crate::localization::Localization;
use crate::resolve::{Evidence, Issue, SourceRef};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Content category plus raw Stellaris script identifier (`docs/technical-design.md:593`).
///
/// The category is present because unrelated registries may reuse an identifier. The Mod
/// Installation and the Documentation Revision supply the enclosing namespace and are
/// deliberately *not* repeated inside every key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntryKey {
    pub category: String,
    pub identifier: String,
}

impl EntryKey {
    pub fn technology(identifier: impl Into<String>) -> Self {
        EntryKey {
            category: "technology".into(),
            identifier: identifier.into(),
        }
    }

    /// A stable, filesystem-safe address for this entry.
    ///
    /// Not the identifier itself. Stellaris identifiers are conventionally
    /// `[a-z0-9_]`, but nothing enforces that, and a bundle writer that turned an
    /// identifier straight into a filename would be one hostile mod away from a path
    /// traversal. Any byte outside the allowed set is escaped, so the mapping stays
    /// injective and two different identifiers cannot collide on one file.
    pub fn slug(&self) -> String {
        let mut slug = String::with_capacity(self.category.len() + self.identifier.len() + 1);
        for source in [self.category.as_str(), self.identifier.as_str()] {
            if !slug.is_empty() {
                slug.push('.');
            }
            for byte in source.bytes() {
                match byte {
                    b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' => slug.push(byte as char),
                    b'A'..=b'Z' => {
                        // Escaped rather than lowercased: a case-insensitive filesystem
                        // would otherwise merge `Tech_A` and `tech_a` into one file.
                        slug.push('~');
                        slug.push_str(&format!("{byte:02x}"));
                    }
                    other => {
                        slug.push('~');
                        slug.push_str(&format!("{other:02x}"));
                    }
                }
            }
        }
        slug
    }
}

/// A structured requirement, preserving the logical shape rather than flattening it to prose
/// (`docs/decision-log.md` D-006).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Requirement {
    /// `AND`, and the implicit conjunction of a block's fields.
    All { of: Vec<Requirement> },
    /// `OR`.
    Any { of: Vec<Requirement> },
    /// `NOT` and `NOR`. Presented as a blocker rather than as a negated sentence.
    Not { of: Box<Requirement> },
    /// A leaf condition: `has_ethic = ethic_materialist`.
    Condition {
        key: String,
        operator: String,
        value: String,
    },
    /// A leaf whose value is itself a block: `has_trait_in_council = { TRAIT = x }`.
    ///
    /// Retained as a distinct shape rather than rendered into a `Condition` with a
    /// stringified value, so a consumer can tell "a condition with arguments" from "a
    /// condition whose value happens to contain braces".
    Parameterized {
        key: String,
        arguments: Vec<(String, String)>,
    },
    /// Present in source, not interpretable here.
    ///
    /// `docs/technical-design.md:336` forbids the alternative: an unsupported construct is
    /// shown as unsupported and marks its consumer's interpretation incomplete. It is never
    /// silently dropped, because a dropped requirement reads to a player as a requirement
    /// that does not exist.
    Unsupported { key: String, reason: String },
}

impl Requirement {
    pub fn leaves(&self) -> usize {
        match self {
            Requirement::All { of } | Requirement::Any { of } => {
                of.iter().map(Requirement::leaves).sum()
            }
            Requirement::Not { of } => of.leaves(),
            _ => 1,
        }
    }

    pub fn unsupported(&self) -> usize {
        match self {
            Requirement::All { of } | Requirement::Any { of } => {
                of.iter().map(Requirement::unsupported).sum()
            }
            Requirement::Not { of } => of.unsupported(),
            Requirement::Unsupported { .. } => 1,
            _ => 0,
        }
    }
}

/// One conditional adjustment to draw weight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeightModifier {
    /// Exact source lexeme. Never a parsed float — see [`crate::resolve::scalar_text`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub add: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<Requirement>,
}

/// A logical reference to a browser-safe asset, or the reason there isn't one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AssetSlot {
    /// Resolved to a source asset. The Asset Store key is filled in at materialization.
    Resolved {
        contributor: String,
        logical: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
    },
    /// No source asset for this slot. `analysis::finalize` substitutes a deterministic
    /// placeholder rather than a required key (`docs/technical-design.md:263`).
    Placeholder { reason: String },
}

/// A bounded excerpt of original source bytes.
///
/// `docs/technical-design.md:275` caps this at 16 KiB, aligned to line boundaries where
/// possible, with leading or trailing omission shown explicitly. Provenance retains the
/// complete source range; the excerpt is not a window a reader can widen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Excerpt {
    pub text: String,
    /// Byte range of the *complete* definition, which may be larger than `text` covers.
    pub source_start: u32,
    pub source_end: u32,
    pub truncated_head: bool,
    pub truncated_tail: bool,
    /// Bytes that could not be decoded and are represented visibly rather than dropped.
    pub undecodable_bytes: u32,
}

pub const EXCERPT_LIMIT_BYTES: usize = 16 * 1024;

/// Everything the product knows about one documented entry.
/// Absent optional fields and empty collections are omitted on write and defaulted on read.
///
/// Both halves are required and neither implies the other. `skip_serializing_if` alone
/// produces a bundle the reader cannot parse — which is exactly what the first capture of
/// `b3-read` and `b4-checks` hit, on the very first entry with no `categories`. A write-side
/// size optimization is a read-side contract change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub key: EntryKey,
    /// Localization key, not localized text. Resolved at read time against preserved
    /// localization, which is what lets language change without a rebuild.
    pub name_key: String,
    pub description_key: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    pub start_tech: bool,
    pub rare: bool,
    pub dangerous: bool,

    /// Base Draw Weight, as the exact source lexeme.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_weight: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weight_modifiers: Vec<WeightModifier>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prerequisites: Vec<EntryKey>,
    /// The reverse edge, materialized. Derived from `prerequisites` across the whole model,
    /// which is exactly the kind of denormalization this spike exists to weigh.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unlocks: Vec<EntryKey>,

    /// Eligibility, from `potential`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requirements: Option<Requirement>,

    pub icon: AssetSlot,
    pub excerpt: Excerpt,
    pub evidence: Evidence,
    pub source: SourceRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shadowed: Vec<SourceRef>,
    /// Indices into [`Documentation::issues`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<usize>,
}

/// Whether a revision is complete, and if not, at what scope.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Completeness {
    pub complete: bool,
    /// Registries whose complete entry set could not be established.
    pub incomplete_registries: Vec<String>,
    pub entries_with_issues: usize,
    pub unsupported_facts: usize,
}

/// The one canonical model. Every view below is a function of this value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Documentation {
    pub case: String,
    pub entries: Vec<Entry>,
    pub localization: Localization,
    pub issues: Vec<Issue>,
    pub completeness: Completeness,
}

impl Documentation {
    /// The 2.0x denominator: the complete model serialized once, as a single document.
    ///
    /// Declared in the evaluation before capture, and produced from the same value every
    /// other shape is produced from so the ratio measures materialization overhead rather
    /// than two generators disagreeing.
    pub fn unsharded_payload(&self) -> String {
        crate::record::to_compact_json(self)
    }

    /// Per-category browse summaries. Language-independent by construction.
    pub fn browse_index(&self) -> BTreeMap<String, Vec<BrowseSummary>> {
        let mut index: BTreeMap<String, Vec<BrowseSummary>> = BTreeMap::new();
        for entry in &self.entries {
            index
                .entry(entry.key.category.clone())
                .or_default()
                .push(BrowseSummary {
                    key: entry.key.clone(),
                    name_key: entry.name_key.clone(),
                    area: entry.area.clone(),
                    tier: entry.tier.clone(),
                    cost: entry.cost.clone(),
                    categories: entry.categories.clone(),
                    has_issues: !entry.issues.is_empty(),
                    icon: entry.icon.clone(),
                });
        }
        index
    }

    /// Per-language search material.
    ///
    /// Localized text is materialized here and nowhere else. Ranking needs the localized
    /// name — an exact or prefix match on it outranks a fuzzy or identifier match
    /// (`docs/technical-design.md:657`) — and a matcher that resolved localization per query
    /// would pay that cost on every keystroke instead of once per build.
    pub fn search_material(&self) -> BTreeMap<String, Vec<SearchEntry>> {
        let mut material = BTreeMap::new();
        for language in self.localization.languages.keys() {
            let entries = self
                .entries
                .iter()
                .map(|entry| {
                    let name = self.localization.resolve(language, &entry.name_key);
                    SearchEntry {
                        key: entry.key.clone(),
                        name: name.text.to_owned(),
                        normalized: normalize(name.text),
                        identifier: entry.key.identifier.clone(),
                        category: entry.key.category.clone(),
                    }
                })
                .collect();
            material.insert(language.clone(), entries);
        }
        material
    }

    /// Full records, addressable by stable content identity.
    pub fn full_records(&self) -> impl Iterator<Item = (String, &Entry)> {
        self.entries.iter().map(|entry| (entry.key.slug(), entry))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowseSummary {
    pub key: EntryKey,
    pub name_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    pub has_issues: bool,
    pub icon: AssetSlot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchEntry {
    pub key: EntryKey,
    /// As displayed.
    pub name: String,
    /// As matched. Kept beside `name` rather than instead of it, because a result list shows
    /// the display form and re-deriving it at read time would put the normalizer on the
    /// response path.
    pub normalized: String,
    pub identifier: String,
    pub category: String,
}

/// Query and index normalization.
///
/// `docs/technical-design.md:327` pins this to NFKC, locale-independent case folding, and
/// canonical whitespace collapse against a pinned Unicode-data version. This harness
/// implements the case folding and whitespace rules and **not** NFKC: Rust's standard library
/// has no normalizer, and pulling one in would add a dependency whose version this spike
/// would then have to pin and drift-check for a measurement that is about bytes and latency.
/// The narrowing is disclosed rather than left implicit — it makes search material slightly
/// smaller than production's and cannot change which bundle shape wins.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for character in text.chars() {
        if character.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        for lowered in character.to_lowercase() {
            out.push(lowered);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slug_is_injective_over_identifiers_a_filesystem_would_merge() {
        let lower = EntryKey::technology("tech_a").slug();
        let upper = EntryKey::technology("Tech_A").slug();
        assert_ne!(lower, upper);
        assert_eq!(lower, "technology.tech_a");
    }

    #[test]
    fn a_slug_escapes_every_byte_a_path_could_act_on() {
        let hostile = EntryKey::technology("../../etc/passwd").slug();
        assert!(!hostile.contains('/'), "{hostile}");
        assert!(!hostile.contains('.') || hostile.starts_with("technology."), "{hostile}");
        assert_eq!(hostile.matches('.').count(), 1, "{hostile}");
    }

    #[test]
    fn normalization_folds_case_and_collapses_whitespace_without_trailing_space() {
        assert_eq!(normalize("  Enigmalith   Reactor \n"), "enigmalith reactor");
        assert_eq!(normalize("ÉCLAT"), "éclat");
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("   "), "");
    }

    #[test]
    fn requirement_counts_reach_through_every_group_shape() {
        let requirement = Requirement::All {
            of: vec![
                Requirement::Condition {
                    key: "has_ethic".into(),
                    operator: "=".into(),
                    value: "ethic_materialist".into(),
                },
                Requirement::Not {
                    of: Box::new(Requirement::Unsupported {
                        key: "custom_trigger_tooltip".into(),
                        reason: "unmodelled".into(),
                    }),
                },
                Requirement::Any {
                    of: vec![Requirement::Unsupported {
                        key: "hidden_trigger".into(),
                        reason: "unmodelled".into(),
                    }],
                },
            ],
        };

        assert_eq!(requirement.leaves(), 3);
        assert_eq!(requirement.unsupported(), 2);
    }
}

#[cfg(test)]
mod round_trip {
    use super::*;
    use crate::resolve::{Evidence, SourceRef};

    /// An entry with every omittable field empty must survive the encoder and the decoder.
    ///
    /// This is the test that was missing. `skip_serializing_if` without `default` produced a
    /// bundle whose very first entry could not be read back, and nothing caught it until a
    /// measurement run panicked on the real corpus — after the bundle had already been
    /// written, validated, and published. Validation cannot catch it, because the manifest
    /// hashes bytes rather than parsing them.
    #[test]
    fn an_entry_with_every_optional_field_absent_round_trips() {
        let sparse = Entry {
            key: EntryKey::technology("tech_sparse"),
            name_key: "tech_sparse".into(),
            description_key: "tech_sparse_desc".into(),
            area: None,
            tier: None,
            cost: None,
            categories: Vec::new(),
            start_tech: false,
            rare: false,
            dangerous: false,
            base_weight: None,
            weight_modifiers: Vec::new(),
            prerequisites: Vec::new(),
            unlocks: Vec::new(),
            requirements: None,
            icon: AssetSlot::Placeholder {
                reason: "none".into(),
            },
            excerpt: Excerpt {
                text: String::new(),
                source_start: 0,
                source_end: 0,
                truncated_head: false,
                truncated_tail: false,
                undecodable_bytes: 0,
            },
            evidence: Evidence::Clean,
            source: SourceRef {
                contributor: "vanilla".into(),
                logical: "common/technology/a.txt".into(),
                ordinal: 0,
                span_start: 0,
                span_end: 0,
            },
            shadowed: Vec::new(),
            issues: Vec::new(),
        };

        let encoded = crate::record::to_compact_json(&sparse);
        assert!(
            !encoded.contains("categories"),
            "the writer is expected to omit it — that is the condition under test"
        );

        let decoded: Entry = serde_json::from_str(&encoded).expect("a written entry is readable");
        assert_eq!(decoded, sparse);
    }

    #[test]
    fn a_browse_summary_with_every_optional_field_absent_round_trips() {
        let sparse = BrowseSummary {
            key: EntryKey::technology("tech_sparse"),
            name_key: "tech_sparse".into(),
            area: None,
            tier: None,
            cost: None,
            categories: Vec::new(),
            has_issues: false,
            icon: AssetSlot::Placeholder {
                reason: "none".into(),
            },
        };

        let encoded = crate::record::to_compact_json(&sparse);
        let decoded: BrowseSummary =
            serde_json::from_str(&encoded).expect("a written summary is readable");
        assert_eq!(decoded, sparse);
    }
}
