//! The Rust half of the cross-language contract suite.
//!
//! The committed files under `fixtures/transport/` are the authority; this module compares what
//! [`Envelope`] serializes to against them, and `src/documentation/contract.test.ts` compares what
//! the TypeScript decoder makes of them. Neither side generates the files — see that directory's
//! `README.md` for why.
//!
//! The comparison is on **bytes**, not on a parsed `serde_json::Value`, so the fixtures pin key
//! order as well as names. `envelope.rs` already asserts that `ok` is written before its payload;
//! this is where that guarantee becomes something TypeScript can rely on.
//!
//! # Honest about what this half is
//!
//! The Rust serializer shipped in STE-18, so these tests did not drive it into existence — they
//! are evidence, not a red-green cycle. What makes them worth trusting anyway is
//! [`tests::the_comparison_discriminates_between_adjacent_shapes`]: without a control showing the
//! comparison can fail, a byte equality against a file this repository also owns proves very little.

use super::envelope::Envelope;
use super::tauri::{EntryListView, EntryView, ReadRefusal};
use crate::discovery::identity::ModInstallationId;

const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/transport");

fn vector(relative: &str) -> String {
    let path = format!("{VECTORS}/{relative}");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("reading {path}: {error}"))
}

fn installation() -> ModInstallationId {
    ModInstallationId::parse(&"ab".repeat(32)).expect("a literal installation identifier")
}

/// Every refusal variant paired with the file that fixes its wire form.
///
/// The `match` has no `_` arm, so adding a variant to [`ReadRefusal`] fails to compile until it is
/// given a fixture here — the completeness rule the design asks for, enforced by the type system
/// rather than by remembering.
fn refusal_vector(refusal: &ReadRefusal) -> &'static str {
    match refusal {
        ReadRefusal::NoPublishedRevision => "refusal-noPublishedRevision.json",
        ReadRefusal::ReferenceUnreadable => "refusal-referenceUnreadable.json",
        ReadRefusal::RevisionMissing => "refusal-revisionMissing.json",
        ReadRefusal::RevisionFromAnotherBuild { .. } => "refusal-revisionFromAnotherBuild.json",
        ReadRefusal::RevisionDamaged => "refusal-revisionDamaged.json",
        ReadRefusal::RevisionDisplaced => "refusal-revisionDisplaced.json",
        ReadRefusal::RevisionCarriesNoEntryList => "refusal-revisionCarriesNoEntryList.json",
        ReadRefusal::DocumentUnreadable => "refusal-documentUnreadable.json",
        ReadRefusal::DocumentChanged => "refusal-documentChanged.json",
        ReadRefusal::DocumentUndecodable => "refusal-documentUndecodable.json",
    }
}

fn every_refusal() -> Vec<ReadRefusal> {
    vec![
        ReadRefusal::NoPublishedRevision,
        ReadRefusal::ReferenceUnreadable,
        ReadRefusal::RevisionMissing,
        ReadRefusal::RevisionFromAnotherBuild {
            found: 7,
            supported: 1,
        },
        ReadRefusal::RevisionDamaged,
        ReadRefusal::RevisionDisplaced,
        ReadRefusal::RevisionCarriesNoEntryList,
        ReadRefusal::DocumentUnreadable,
        ReadRefusal::DocumentChanged,
        ReadRefusal::DocumentUndecodable,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Unexpected;
    use crate::transport::envelope::Rejection;

    fn encode<T: serde::Serialize, E: serde::Serialize>(envelope: &Envelope<T, E>) -> String {
        serde_json::to_string(envelope).expect("an envelope serializes")
    }

    #[test]
    fn a_populated_entry_list_matches_its_vector() {
        let envelope: Envelope<_, ReadRefusal> = Envelope::Value(EntryListView {
            installation: installation(),
            entries: vec![
                EntryView {
                    category: "technology".to_owned(),
                    identifier: "tech_skeleton".to_owned(),
                    display_name: Some("Skeleton Technology".to_owned()),
                },
                EntryView {
                    category: "technology".to_owned(),
                    identifier: "tech_unnamed".to_owned(),
                    display_name: None,
                },
            ],
        });

        assert_eq!(
            encode(&envelope),
            vector("get_entry_list/success-two-entries.json")
        );
    }

    #[test]
    fn a_revision_documenting_nothing_matches_its_vector() {
        // `entries: []` is a success. `revisionCarriesNoEntryList` is a refusal. Both have
        // fixtures, because the line between empty and absent is exactly the sort of thing two
        // independent implementations drift across.
        let envelope: Envelope<_, ReadRefusal> = Envelope::Value(EntryListView {
            installation: installation(),
            entries: Vec::new(),
        });

        assert_eq!(
            encode(&envelope),
            vector("get_entry_list/success-empty.json")
        );
    }

    #[test]
    fn every_refusal_matches_its_vector() {
        for refusal in every_refusal() {
            let file = refusal_vector(&refusal);
            let envelope: Envelope<EntryListView, _> = Envelope::Refusal(refusal);

            assert_eq!(
                encode(&envelope),
                vector(&format!("get_entry_list/{file}")),
                "{file}"
            );
        }
    }

    #[test]
    fn the_vector_directory_holds_exactly_the_vectors_that_are_read() {
        // Completeness in the other direction: the exhaustive `match` catches a variant with no
        // fixture, and this catches a fixture no variant reads — a file left behind by a rename,
        // which would otherwise sit there looking like coverage.
        let expected = every_refusal().len() + 2; // the refusals, plus the two success vectors

        let found = std::fs::read_dir(format!("{VECTORS}/get_entry_list"))
            .expect("the vector directory exists")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|kind| kind == "json"))
            .count();

        assert_eq!(found, expected);
    }

    #[test]
    fn a_rejection_matches_its_vector() {
        // A correlation identifier is a fresh UUID, so the bytes cannot be equal outright. The
        // identifier is substituted and everything else — key names, key order, the fixed `kind` —
        // is compared, which is the part a fixture can fix.
        let rejection = Rejection::from(Unexpected::new("a detail that must not reach a wire"));
        let encoded = serde_json::to_string(&rejection).expect("a rejection serializes");

        let normalized = encoded.replace(
            &rejection.correlation().to_string(),
            "0123456789abcdef0123456789abcdef",
        );

        assert_eq!(normalized, vector("rejection/unexpected.json"));
        assert!(
            !encoded.contains("a detail that must not reach a wire"),
            "the message must be unrepresentable in a rejection, not merely unwritten"
        );
    }

    #[test]
    fn the_comparison_discriminates_between_adjacent_shapes() {
        // The negative control. Byte equality against a file this repository also owns proves
        // nothing unless a near-miss is shown to fail: each of these is one plausible drift away
        // from the real vector.
        let absent_name: Envelope<_, ReadRefusal> = Envelope::Value(EntryListView {
            installation: installation(),
            entries: vec![EntryView {
                category: "technology".to_owned(),
                identifier: "tech_skeleton".to_owned(),
                // The drift that matters most: `displayName` resolved to empty rather than absent.
                display_name: Some(String::new()),
            }],
        });
        assert_ne!(
            encode(&absent_name),
            vector("get_entry_list/success-two-entries.json")
        );

        let swapped: Envelope<EntryListView, _> =
            Envelope::Refusal(ReadRefusal::RevisionFromAnotherBuild {
                found: 1,
                supported: 7,
            });
        assert_ne!(
            encode(&swapped),
            vector("get_entry_list/refusal-revisionFromAnotherBuild.json"),
            "`found` and `supported` transposed must not compare equal"
        );

        let wrong_refusal: Envelope<EntryListView, _> =
            Envelope::Refusal(ReadRefusal::RevisionMissing);
        assert_ne!(
            encode(&wrong_refusal),
            vector("get_entry_list/refusal-revisionDamaged.json")
        );
    }
}
