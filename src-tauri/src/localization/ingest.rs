//! The fold: an ordered stream of localization files becomes per-language effective tables.
//!
//! # Two passes, and why the order of them is load-bearing
//!
//! 1. **Surviving files**, in stream order, lines in file order. Each statement either creates
//!    an entry or demotes the incumbent winner and takes its place. Last-wins is therefore
//!    structural — there is no comparison to get backwards, and no position to compare.
//! 2. **Removed files.** Each statement either joins an existing key's shadowed list or, if
//!    nothing surviving states that key, becomes a raw-key casualty.
//!
//! Running the removed files second is what makes "a removed file can never win" true by
//! construction rather than by a guard, and it is what makes casualty status decidable at all:
//! a key is a casualty relative to the *complete* effective table, not relative to the file it
//! was lost from.
//!
//! # The rule the two passes implement
//!
//! The game reads localization as one ordered stream — surviving Vanilla files, then ordinary
//! mod files in enabled-mod order, then every `replace/` file — and the last statement of a
//! key wins. Exact-path collision is a separate, earlier mechanism: the losing file is removed
//! whole, so every key the winning file does not restate disappears with it and renders as its
//! own identifier. Both were measured (`r13-loc-methods`, `r14-loc-samepath`,
//! `r15-loc-modvmod`); the resolver owns constructing the order and deciding which files were
//! removed, and this module owns what that means key by key.
//!
//! # Totality
//!
//! [`ingest`] returns tables unconditionally. Every failure is scoped to a file, a section, or
//! a line and recorded as a typed condition, because refusing a build over one malformed line
//! in one mod file is the fatal-or-silent dichotomy the condition vocabulary exists to break.
//!
//! Casualty status is relative to the Source Snapshot: if enumeration of a contributor was
//! incomplete, keys can look like casualties because the file that states them was never read.
//! The snapshot already records that gap; this module does not re-check it.

use super::syntax::{self, Fault};
use super::table::{
    Condition, EffectiveTables, FileIndex, FileOrigin, IngestedFile, KeyOccurrence,
    LocalizationInput, RemovedFile, StreamedFile, file_index,
};

/// Build the per-language effective tables for one build's localization.
pub fn ingest(input: LocalizationInput) -> EffectiveTables {
    let LocalizationInput {
        streamed,
        mut removed,
    } = input;

    // The resolver hands removed files over in whatever order its projection produced. Sorting
    // here rather than trusting the caller keeps determinism a property of this module: the
    // path is unique within a contributor, so `(logical, source)` is a total order.
    removed
        .sort_by(|left, right| (&left.logical, left.source).cmp(&(&right.logical, right.source)));

    let files = streamed
        .iter()
        .enumerate()
        .map(|(order, file)| IngestedFile {
            source: file.source,
            logical: file.logical.clone(),
            origin: FileOrigin::Streamed {
                order: order as u32,
            },
        })
        .chain(removed.iter().map(|file| IngestedFile {
            source: file.source,
            logical: file.logical.clone(),
            origin: FileOrigin::Removed {
                loss: file.loss.clone(),
            },
        }))
        .collect();

    let mut tables = EffectiveTables::new(files);
    let mut position = 0;

    for StreamedFile { bytes, .. } in &streamed {
        let index = file_index(position);
        let document = syntax::parse(bytes.as_slice());
        for entry in document.entries {
            tables.table_mut(entry.language).record_streamed(
                entry.key.into(),
                KeyOccurrence {
                    value: entry.value.into(),
                    file: index,
                    line: entry.line,
                },
            );
        }
        record(&mut tables, index, document.faults);
        position += 1;
    }

    for RemovedFile { bytes, .. } in &removed {
        let index = file_index(position);
        let document = syntax::parse(bytes.as_slice());
        for entry in document.entries {
            tables.table_mut(entry.language).record_removed(
                entry.key.into(),
                KeyOccurrence {
                    value: entry.value.into(),
                    file: index,
                    line: entry.line,
                },
            );
        }
        record(&mut tables, index, document.faults);
        position += 1;
    }

    tables
}

/// Conditions from a removed file are recorded too: a malformed shadowed file would otherwise
/// be doubly silent — its keys are gone *and* nothing said its bytes were unreadable.
fn record(tables: &mut EffectiveTables, file: FileIndex, faults: Vec<Fault>) {
    for fault in faults {
        tables.record_condition(Condition {
            file,
            line: fault.line,
            kind: fault.kind,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::canonical::path::LogicalPath;
    use crate::localization::table::FileLoss;
    use crate::localization::{ConditionKind, LanguageTag};
    use crate::source::snapshot::SourceKind;
    use proptest::prelude::*;

    fn tag(name: &str) -> LanguageTag {
        LanguageTag::parse(name).expect("a well-formed language token")
    }

    fn path(raw: &str) -> LogicalPath {
        LogicalPath::parse(raw).expect("a fixture path")
    }

    fn streamed(logical: &str, source: SourceKind, body: &str) -> StreamedFile {
        StreamedFile {
            source,
            logical: path(logical),
            bytes: body.as_bytes().into(),
        }
    }

    fn shadowed(logical: &str, body: &str) -> RemovedFile {
        RemovedFile {
            source: SourceKind::VanillaContent,
            logical: path(logical),
            bytes: body.as_bytes().into(),
            loss: FileLoss::ShadowedByPathCollision {
                winner: SourceKind::TargetMod,
            },
        }
    }

    fn english(tables: &EffectiveTables, key: &str) -> String {
        tables
            .get(&tag("l_english"), key)
            .unwrap_or_else(|| panic!("{key} has no effective value"))
            .winner()
            .value
            .as_str()
            .to_owned()
    }

    #[test]
    fn a_later_file_in_the_stream_wins_and_the_earlier_value_is_retained() {
        let tables = ingest(LocalizationInput {
            streamed: vec![
                streamed(
                    "localisation/english/a_l_english.yml",
                    SourceKind::VanillaContent,
                    "l_english:\n k:0 \"first\"\n",
                ),
                streamed(
                    "localisation/english/b_l_english.yml",
                    SourceKind::TargetMod,
                    "l_english:\n k:0 \"second\"\n",
                ),
            ],
            removed: Vec::new(),
        });

        let entry = tables.get(&tag("l_english"), "k").expect("an entry");
        assert_eq!(entry.winner().value.as_str(), "second");
        assert_eq!(entry.shadowed().len(), 1);
        assert_eq!(entry.shadowed()[0].value.as_str(), "first");
        assert!(entry.is_contested());
        assert_eq!(
            tables.file(entry.winner().file).logical.as_str(),
            "localisation/english/b_l_english.yml"
        );
    }

    #[test]
    fn a_later_occurrence_within_one_file_wins_and_the_earlier_value_is_retained() {
        let tables = ingest(LocalizationInput {
            streamed: vec![streamed(
                "localisation/english/a_l_english.yml",
                SourceKind::VanillaContent,
                "l_english:\n k:0 \"first\"\n k:0 \"second\"\n",
            )],
            removed: Vec::new(),
        });

        let entry = tables.get(&tag("l_english"), "k").expect("an entry");
        assert_eq!(entry.winner().value.as_str(), "second");
        assert_eq!(entry.winner().line, 3);
        assert_eq!(entry.shadowed()[0].line, 2);
    }

    #[test]
    fn keys_lost_with_a_removed_file_are_casualties_scoped_to_that_file() {
        let tables = ingest(LocalizationInput {
            streamed: vec![
                streamed(
                    "localisation/english/other_l_english.yml",
                    SourceKind::VanillaContent,
                    "l_english:\n untouched:0 \"Untouched\"\n",
                ),
                streamed(
                    "localisation/english/collided_l_english.yml",
                    SourceKind::TargetMod,
                    "l_english:\n restated:0 \"Mod value\"\n",
                ),
            ],
            removed: vec![shadowed(
                "localisation/english/collided_l_english.yml",
                "l_english:\n restated:0 \"Vanilla value\"\n lost_a:0 \"Lost A\"\n lost_b:0 \"Lost B\"\n",
            )],
        });

        let table = tables.table(&tag("l_english")).expect("a table");
        assert_eq!(
            table
                .casualties()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
            ["lost_a", "lost_b"]
        );
        assert_eq!(
            table.casualty("lost_a").unwrap()[0].value.as_str(),
            "Lost A"
        );
        assert!(table.get("lost_a").is_none(), "a casualty has no winner");
        assert!(
            table.casualty("untouched").is_none(),
            "a key in a file nothing collided with is untouched — the natural control that \
             separates 'this file was shadowed' from 'localization broke'"
        );

        // Scoped: every casualty is attributed to the one file that took it away.
        let grouped = tables.casualties_by_file();
        assert_eq!(grouped.len(), 1);
        let (index, by_language) = grouped.into_iter().next().expect("one shadowed file");
        assert_eq!(
            tables.file(index).logical.as_str(),
            "localisation/english/collided_l_english.yml"
        );
        assert_eq!(
            by_language[&tag("l_english")],
            BTreeSet::from(["lost_a", "lost_b"])
        );
        assert!(matches!(
            tables.file(index).origin,
            FileOrigin::Removed {
                loss: FileLoss::ShadowedByPathCollision {
                    winner: SourceKind::TargetMod
                }
            }
        ));
    }

    #[test]
    fn a_key_the_winner_also_states_is_not_a_casualty_but_keeps_the_lost_statement() {
        let tables = ingest(LocalizationInput {
            streamed: vec![streamed(
                "localisation/english/collided_l_english.yml",
                SourceKind::TargetMod,
                "l_english:\n restated:0 \"Mod value\"\n",
            )],
            removed: vec![shadowed(
                "localisation/english/collided_l_english.yml",
                "l_english:\n restated:0 \"Vanilla value\"\n lost:0 \"Lost\"\n",
            )],
        });

        let entry = tables.get(&tag("l_english"), "restated").expect("an entry");
        assert_eq!(entry.winner().value.as_str(), "Mod value");
        assert_eq!(entry.shadowed().len(), 1);
        assert_eq!(entry.shadowed()[0].value.as_str(), "Vanilla value");
        assert!(
            tables
                .table(&tag("l_english"))
                .unwrap()
                .casualty("restated")
                .is_none()
        );
    }

    #[test]
    fn an_overridden_key_is_a_shadowed_value_and_not_a_casualty() {
        // Negative control for the rule above. The naive reading — every key in a removed file
        // is a casualty — is computed here and asserted to disagree. It is the reading that
        // turns a mod which renamed one technology into a report of one casualty too many, and
        // it is indistinguishable from the shipped rule on any corpus where the winning file
        // restates nothing.
        let input = LocalizationInput {
            streamed: vec![streamed(
                "localisation/english/collided_l_english.yml",
                SourceKind::TargetMod,
                "l_english:\n restated:0 \"Mod value\"\n",
            )],
            removed: vec![shadowed(
                "localisation/english/collided_l_english.yml",
                "l_english:\n restated:0 \"Vanilla value\"\n lost:0 \"Lost\"\n",
            )],
        };
        let naive: Vec<&str> = ["restated", "lost"].into();
        let shipped: Vec<_> = ingest(input)
            .table(&tag("l_english"))
            .unwrap()
            .casualties()
            .map(|(key, _)| key.as_str().to_owned())
            .collect();
        assert_eq!(shipped, ["lost"]);
        assert_ne!(shipped, naive);
    }

    #[test]
    fn dropping_the_removed_files_removes_every_casualty() {
        // The other half of the same control: casualties come from the removed pass and
        // nowhere else, so a fold that forgot the shadowed bytes would report a clean build
        // over a corpus that lost hundreds of names.
        let streamed_only = LocalizationInput {
            streamed: vec![streamed(
                "localisation/english/collided_l_english.yml",
                SourceKind::TargetMod,
                "l_english:\n restated:0 \"Mod value\"\n",
            )],
            removed: Vec::new(),
        };
        let tables = ingest(streamed_only);
        assert_eq!(tables.casualties_by_file().len(), 0);
        assert_eq!(
            tables
                .get(&tag("l_english"), "restated")
                .unwrap()
                .shadowed(),
            []
        );
    }

    #[test]
    fn the_header_decides_the_language_and_the_path_does_not() {
        // Workshop mod 3039370479 ships `localisation/braz_por/PreSelect_l_braz_por.yml` whose
        // header is `l_english:`. Attributing by path would file its keys under a language the
        // player would never see them in.
        let tables = ingest(LocalizationInput {
            streamed: vec![streamed(
                "localisation/braz_por/mislabelled_l_braz_por.yml",
                SourceKind::TargetMod,
                "l_english:\n k:0 \"English after all\"\n",
            )],
            removed: Vec::new(),
        });
        assert_eq!(english(&tables, "k"), "English after all");
        assert!(tables.table(&tag("l_braz_por")).is_none());
        assert_eq!(tables.available().collect::<Vec<_>>(), [&tag("l_english")]);
    }

    #[test]
    fn one_multi_section_file_feeds_two_language_tables() {
        // Vanilla's `localisation/languages.yml` holds ten sections in one file.
        let tables = ingest(LocalizationInput {
            streamed: vec![streamed(
                "localisation/languages.yml",
                SourceKind::VanillaContent,
                "l_english:\n l_english:0 \"English\"\nl_korean:\n l_english:0 \"영어\"\n",
            )],
            removed: Vec::new(),
        });
        assert_eq!(english(&tables, "l_english"), "English");
        assert_eq!(
            tables
                .get(&tag("l_korean"), "l_english")
                .unwrap()
                .winner()
                .value
                .as_str(),
            "영어"
        );
        assert_eq!(
            tables.across_languages("l_english").count(),
            2,
            "the closure's per-key walk sees both languages from the one file"
        );
    }

    #[test]
    fn a_condition_in_a_removed_file_is_still_recorded() {
        let tables = ingest(LocalizationInput {
            streamed: vec![streamed(
                "localisation/english/a_l_english.yml",
                SourceKind::TargetMod,
                "l_english:\n k:0 \"v\"\n",
            )],
            removed: vec![shadowed(
                "localisation/english/a_l_english.yml",
                "l_english:\n broken:0 \"unterminated\n",
            )],
        });
        assert_eq!(tables.conditions().len(), 1);
        assert_eq!(
            tables.conditions()[0].kind,
            ConditionKind::UnterminatedValue {
                key: "broken".to_owned()
            },
            "the condition names the definition that was lost, not only where it sat"
        );
        assert_eq!(
            tables.file(tables.conditions()[0].file).logical.as_str(),
            "localisation/english/a_l_english.yml"
        );
        assert!(matches!(
            tables.file(tables.conditions()[0].file).origin,
            FileOrigin::Removed { .. }
        ));
    }

    #[test]
    fn conditions_are_ordered_by_file_then_line() {
        let tables = ingest(LocalizationInput {
            streamed: vec![
                streamed(
                    "localisation/english/a_l_english.yml",
                    SourceKind::VanillaContent,
                    "l_english:\n broken\n also broken\n",
                ),
                streamed(
                    "localisation/english/b_l_english.yml",
                    SourceKind::TargetMod,
                    "l_english:\n broken\n",
                ),
            ],
            removed: Vec::new(),
        });
        let sites: Vec<_> = tables
            .conditions()
            .iter()
            .map(|condition| (condition.file, condition.line))
            .collect();
        let mut sorted = sites.clone();
        sorted.sort();
        assert_eq!(sites, sorted);
        assert_eq!(sites.len(), 3);
    }

    #[test]
    fn removed_files_are_ordered_deterministically_whatever_order_they_arrive_in() {
        let build = |flip: bool| {
            let mut removed = vec![
                shadowed(
                    "localisation/english/a_l_english.yml",
                    "l_english:\n a:0 \"A\"\n",
                ),
                shadowed(
                    "localisation/english/b_l_english.yml",
                    "l_english:\n b:0 \"B\"\n",
                ),
            ];
            if flip {
                removed.reverse();
            }
            ingest(LocalizationInput {
                streamed: Vec::new(),
                removed,
            })
        };
        assert_eq!(build(false), build(true));
    }

    proptest! {
        #[test]
        fn every_parsed_occurrence_is_exactly_one_of_winner_shadowed_or_casualty(
            bodies in prop::collection::vec(
                prop::collection::vec(("[a-c]", "[xyz]"), 0..4),
                0..4,
            ),
        ) {
            // Conservation: the fold may reorder statements and may decide which one wins, but
            // it may never lose one or count one twice.
            let stated: usize = bodies.iter().map(Vec::len).sum();
            let input = LocalizationInput {
                streamed: bodies
                    .iter()
                    .enumerate()
                    .map(|(index, lines)| {
                        let body = lines.iter().fold("l_english:\n".to_owned(), |mut acc, (k, v)| {
                            acc.push_str(&format!(" {k}:0 \"{v}\"\n"));
                            acc
                        });
                        streamed(
                            &format!("localisation/english/f{index}_l_english.yml"),
                            SourceKind::TargetMod,
                            &body,
                        )
                    })
                    .collect(),
                removed: Vec::new(),
            };
            let tables = ingest(input);
            let held: usize = tables
                .tables()
                .map(|(_, table)| {
                    table.entries().map(|(_, e)| 1 + e.shadowed().len()).sum::<usize>()
                        + table.casualties().map(|(_, lost)| lost.len()).sum::<usize>()
                })
                .sum();
            prop_assert_eq!(held, stated);
            prop_assert!(tables.conditions().is_empty());
        }

        #[test]
        fn the_winner_is_the_last_streamed_occurrence_by_file_then_line(
            values in prop::collection::vec("[a-z]{1,3}", 1..6),
        ) {
            let input = LocalizationInput {
                streamed: values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        streamed(
                            &format!("localisation/english/f{index}_l_english.yml"),
                            SourceKind::TargetMod,
                            &format!("l_english:\n k:0 \"{value}\"\n"),
                        )
                    })
                    .collect(),
                removed: Vec::new(),
            };
            let tables = ingest(input);
            let entry = tables.get(&tag("l_english"), "k").expect("an entry");
            prop_assert_eq!(entry.winner().value.as_str(), values.last().unwrap().as_str());
            prop_assert_eq!(entry.shadowed().len(), values.len() - 1);
            let lines: Vec<_> = entry.shadowed().iter().map(|o| (o.file, o.line)).collect();
            let mut sorted = lines.clone();
            sorted.sort();
            prop_assert_eq!(lines, sorted);
        }

        #[test]
        fn languages_do_not_interact(left in "[a-z]{1,3}", right in "[a-z]{1,3}") {
            let tables = ingest(LocalizationInput {
                streamed: vec![streamed(
                    "localisation/english/f_l_english.yml",
                    SourceKind::TargetMod,
                    &format!("l_english:\n k:0 \"{left}\"\nl_german:\n k:0 \"{right}\"\n"),
                )],
                removed: Vec::new(),
            });
            prop_assert_eq!(
                tables.get(&tag("l_english"), "k").unwrap().winner().value.as_str(),
                left.as_str()
            );
            prop_assert_eq!(
                tables.get(&tag("l_german"), "k").unwrap().winner().value.as_str(),
                right.as_str()
            );
        }

        #[test]
        fn ingest_is_a_pure_function(body in "(l_english:\n)?( ?[a-z]{1,3}:0 \"[a-z ]{0,5}\"\n)*") {
            let input = || LocalizationInput {
                streamed: vec![streamed(
                    "localisation/english/f_l_english.yml",
                    SourceKind::TargetMod,
                    &body,
                )],
                removed: Vec::new(),
            };
            prop_assert_eq!(ingest(input()), ingest(input()));
        }
    }
}
