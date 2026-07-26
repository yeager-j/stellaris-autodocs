//! Index construction, the encoded representation, and query.
//!
//! One module owns both sides of the persisted contract, which is what
//! `docs/technical-design.md:644` requires: construction during analysis, the versioned
//! representation, its encoding and decoding, and query normalization, matching, ranking, and
//! bounded selection. A build-time indexer and a read-time matcher that were separate modules
//! would be two authorities for what a match is.
//!
//! Ranking is fixed by the product specification: exact and prefix localized-name matches
//! precede fuzzy and identifier matches, and stable content identity breaks otherwise equal
//! ties (`docs/technical-design.md:657`). Ties are broken by Entry Key rather than by
//! insertion order so the same query returns the same order in a later process.
//!
//! The measurement this exists for is not ranking quality. It is retained index memory and
//! cold and warm query latency at the maximum result limit.

use crate::docmodel::{normalize, Documentation, EntryKey, SearchEntry};
use serde::{Deserialize, Serialize};

/// Bumped by any change to the encoded representation or to matching and ranking.
///
/// Part of the analysis version vector: a revision built under one version is not readable
/// under another (`docs/technical-design.md:332`).
pub const INDEX_SCHEMA_VERSION: u32 = 1;

/// The maximum result limit the Companion HTTP surface accepts
/// (`docs/technical-design.md:981`). Warm search is budgeted at this limit, not at a
/// convenient smaller one.
pub const MAX_RESULT_LIMIT: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    pub schema: u32,
    pub language: String,
    pub entries: Vec<SearchEntry>,
}

impl Index {
    pub fn build(documentation: &Documentation, language: &str) -> Index {
        let entries = documentation
            .search_material()
            .remove(language)
            .unwrap_or_default();
        Index {
            schema: INDEX_SCHEMA_VERSION,
            language: language.to_owned(),
            entries,
        }
    }

    pub fn encode(&self) -> String {
        crate::record::to_compact_json(self)
    }

    pub fn decode(text: &str) -> Result<Index, String> {
        let index: Index = serde_json::from_str(text).map_err(|error| error.to_string())?;
        if index.schema != INDEX_SCHEMA_VERSION {
            return Err(format!(
                "search index schema {} is not {INDEX_SCHEMA_VERSION}",
                index.schema
            ));
        }
        Ok(index)
    }

    /// Bytes this index holds after decoding, counted by walking it.
    ///
    /// The first of two readings of retained memory. It knows exactly what was retained and
    /// nothing about allocator overhead; [`crate::timing::max_rss_bytes`] knows the opposite.
    /// A budget met by one and missed by the other is a finding rather than a rounding
    /// decision.
    pub fn retained_bytes(&self) -> u64 {
        let per_entry = std::mem::size_of::<SearchEntry>() as u64;
        self.entries
            .iter()
            .map(|entry| {
                per_entry
                    + (entry.name.len()
                        + entry.normalized.len()
                        + entry.identifier.len()
                        + entry.category.len()
                        + entry.key.category.len()
                        + entry.key.identifier.len()) as u64
            })
            .sum()
    }

    pub fn query(&self, text: &str, categories: &[String], limit: usize) -> Vec<Hit> {
        let needle = normalize(text);
        if needle.is_empty() {
            return Vec::new();
        }

        let mut hits: Vec<Hit> = self
            .entries
            .iter()
            .filter(|entry| categories.is_empty() || categories.contains(&entry.category))
            .filter_map(|entry| {
                rank(&needle, entry).map(|rank| Hit {
                    key: entry.key.clone(),
                    name: entry.name.clone(),
                    category: entry.category.clone(),
                    rank,
                })
            })
            .collect();

        // Rank first, then Entry Key. Sorting by key alone within a rank is what makes the
        // result order reproducible across processes; a stable sort over insertion order
        // would tie to whatever the builder happened to emit.
        hits.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.key.cmp(&b.key)));
        hits.truncate(limit.min(MAX_RESULT_LIMIT));
        hits
    }
}

/// Lower is better. Explicit discriminants because this ordering is the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum Rank {
    ExactName = 0,
    PrefixName = 1,
    /// The MVP's "fuzzy": the query appears somewhere in the localized name.
    ///
    /// Named for what it does rather than for what a fuzzier matcher might do later. An
    /// edit-distance matcher would slot in at this rank without moving the two above it.
    SubstringName = 2,
    ExactIdentifier = 3,
    PrefixIdentifier = 4,
    SubstringIdentifier = 5,
}

fn rank(needle: &str, entry: &SearchEntry) -> Option<Rank> {
    if entry.normalized == needle {
        return Some(Rank::ExactName);
    }
    if entry.normalized.starts_with(needle) {
        return Some(Rank::PrefixName);
    }
    if entry.normalized.contains(needle) {
        return Some(Rank::SubstringName);
    }
    if entry.identifier == needle {
        return Some(Rank::ExactIdentifier);
    }
    if entry.identifier.starts_with(needle) {
        return Some(Rank::PrefixIdentifier);
    }
    if entry.identifier.contains(needle) {
        return Some(Rank::SubstringIdentifier);
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hit {
    pub key: EntryKey,
    pub name: String,
    pub category: String,
    pub rank: Rank,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(identifier: &str, name: &str) -> SearchEntry {
        SearchEntry {
            key: EntryKey::technology(identifier),
            name: name.to_owned(),
            normalized: normalize(name),
            identifier: identifier.to_owned(),
            category: "technology".to_owned(),
        }
    }

    fn index(entries: Vec<SearchEntry>) -> Index {
        Index {
            schema: INDEX_SCHEMA_VERSION,
            language: "english".into(),
            entries,
        }
    }

    #[test]
    fn a_name_match_of_any_kind_outranks_every_identifier_match() {
        let index = index(vec![
            // Every real Stellaris identifier is `tech_…`, so a query that matches the name
            // matches the identifier only as a substring. The `enigmalith_relic` entry exists
            // to reach the prefix-identifier rank at all.
            entry("tech_enigmalith_core", "Unrelated Thing"),
            entry("enigmalith_relic", "Another Unrelated Thing"),
            entry("tech_z", "Enigmalith Reactor"),
            entry("tech_y", "Enigmalith"),
            entry("tech_x", "The Enigmalith Array"),
        ]);

        let hits = index.query("enigmalith", &[], 10);
        let ranks: Vec<Rank> = hits.iter().map(|hit| hit.rank).collect();

        assert_eq!(
            ranks,
            vec![
                Rank::ExactName,
                Rank::PrefixName,
                Rank::SubstringName,
                Rank::PrefixIdentifier,
                Rank::SubstringIdentifier,
            ]
        );
    }

    #[test]
    fn ties_break_on_entry_key_so_the_order_survives_a_restart() {
        let index = index(vec![
            entry("tech_c", "Same Name"),
            entry("tech_a", "Same Name"),
            entry("tech_b", "Same Name"),
        ]);

        let first = index.query("same name", &[], 10);
        let reversed = Index {
            entries: index.entries.iter().rev().cloned().collect(),
            ..index.clone()
        };
        let second = reversed.query("same name", &[], 10);

        assert_eq!(first, second);
        assert_eq!(
            first.iter().map(|hit| hit.key.identifier.as_str()).collect::<Vec<_>>(),
            vec!["tech_a", "tech_b", "tech_c"]
        );
    }

    #[test]
    fn the_result_limit_is_capped_by_the_transport_maximum() {
        let entries = (0..200)
            .map(|index| entry(&format!("tech_{index:03}"), &format!("Thing {index:03}")))
            .collect();
        let index = index(entries);

        assert_eq!(index.query("thing", &[], 10).len(), 10);
        assert_eq!(index.query("thing", &[], 10_000).len(), MAX_RESULT_LIMIT);
    }

    #[test]
    fn an_empty_query_matches_nothing_rather_than_everything() {
        let index = index(vec![entry("tech_a", "Alpha")]);
        assert!(index.query("", &[], 10).is_empty());
        assert!(index.query("   ", &[], 10).is_empty());
    }

    #[test]
    fn an_index_encoded_under_another_schema_is_refused_rather_than_read() {
        let index = index(vec![entry("tech_a", "Alpha")]);
        // Rewritten through the value rather than by string replacement, so the test does not
        // silently stop testing anything when the encoder's spacing changes.
        let mut value: serde_json::Value =
            serde_json::from_str(&index.encode()).expect("the encoding is JSON");
        value["schema"] = serde_json::Value::from(INDEX_SCHEMA_VERSION + 998);
        let text = value.to_string();
        assert!(Index::decode(&text).is_err());
        assert!(Index::decode(&index.encode()).is_ok());
    }
}
