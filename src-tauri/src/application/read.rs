//! The desktop documentation read: resolve which revision an installation has published, and
//! serve its entries.
//!
//! `revisions` deliberately does not resolve "the current revision" — it never reads the
//! publication pointer, and "choosing which published revision a caller is entitled to belongs
//! to the desktop read use case and to the companion access module" (D-063). This module is
//! the desktop half of that. It takes a Mod Installation identifier, reads the pointer, opens
//! the revision, and decodes one document.
//!
//! It takes a plain [`ModInstallationId`] rather than a
//! [`BuildTarget`](crate::application::BuildTarget): coherence with a Discovery Location is a
//! *write* obligation, owed by whatever stores a publication reference. A read consults a
//! reference that already exists and can neither create nor invalidate one.
//!
//! # Which refusals are expected, decided once
//!
//! The design names persisted mutable state and revision bundles as "genuinely external
//! input" that warrants full runtime parsing. A refusal of external input is therefore an
//! expected outcome, not a defect — somebody edited a file, an upgrade changed a schema, a
//! build never ran. The single exception is a refusal whose *mechanism does not exist in this
//! phase*, which is why [`OpenRevisionError::Retired`] is thrown rather than reported: nothing
//! in Phase 3 grants a retirement claim and no sweep exists, so modelling it would put a
//! variant in this operation's union that nothing can produce (D-081). Phase 9's retention
//! sweep is what promotes it.
//!
//! # What a refusal may not carry
//!
//! [`OpenRevisionError::Damaged`] carries a [`ValidationReport`](crate::revisions::ValidationReport)
//! naming bundle-relative entries — `documents/entry-list.json` and the like. That report stops
//! here. Naming an entry is diagnosis rather than addressing (D-124), and it is legitimate on
//! the `revisions` boundary, but this operation's answer is "rebuild" and its consumer is a
//! transport; Phase 9's Validate Published Revision is the use case that surfaces the findings,
//! reading them straight off the same error. The `detail` strings on
//! [`DocumentUnavailable`] stop here for the stronger reason that they are built from
//! `io::Error` and `serde_json::Error` and carry absolute paths and raw framework text.
//!
//! # Not gated on state recovery
//!
//! An unresolved quarantine does not block this read. A publication reference that exists
//! after a quarantine came from the document that replaced it; what recovery protects is
//! cleanup of artifacts whose references may still be recoverable. Companion access is what
//! the design gates on recovery, and that is Phase 11.

use crate::discovery::identity::ModInstallationId;
use crate::error::{Failure, OpResult};
use crate::revisions::{DocumentUnavailable, OpenRevisionError, RevisionStore};
use crate::state::StateStore;
use std::fmt;

/// Every entry one published revision documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentationEntries {
    pub installation: ModInstallationId,
    pub entries: Vec<DocumentationEntry>,
}

/// One entry, as the application layer describes it.
///
/// **Not a re-export of [`EntrySummary`](crate::revisions::EntrySummary).** That type's serde
/// form *is* the stored bundle document, so using it here would make one type the authority for
/// two contracts — the persisted format and the read model — and a change wanted by either
/// would silently be a change to both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentationEntry {
    pub category: String,
    pub identifier: String,
    pub display_name: Option<String>,
}

/// Why this installation's entries could not be served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadEntryListError {
    /// Nothing has been published for this installation. Named for what is true rather than
    /// for what to do about it: whether the answer is "build it" is the status read's
    /// judgment, and that read reports it as *successful* data (Phase 9).
    NoPublishedRevision,
    /// A revision is published and its documentation cannot be produced.
    DocumentationUnavailable { reason: UnavailableReason },
}

/// What is true of the published revision, at the granularity a person can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailableReason {
    /// The stored publication reference is not a revision identifier. `state` preserves a
    /// token it never parsed, and no schema check can catch it because every string decodes
    /// into a `RevisionId`, so a hand edit or an older build's token lands here.
    ReferenceUnreadable,
    /// No bundle holds the published revision. Expected before a first build completes, and
    /// after a crash that lost an unflushed directory entry.
    RevisionMissing,
    /// The bundle was written by a build with a different bundle schema.
    RevisionFromAnotherBuild { found: u32, supported: u32 },
    /// The bundle disagrees with its own manifest.
    RevisionDamaged,
    /// A complete bundle of a different revision occupies this revision's place. Not damage:
    /// the directory disagrees with nothing, it is simply not what was asked for.
    RevisionDisplaced,
    /// The revision is intact and names no entry-list document at all.
    ///
    /// **Not the same answer as an empty list.** A revision that documents nothing is a
    /// success; this is a revision that carries no such document, which `revisions` reports as
    /// `Ok(None)` precisely so the two stay distinguishable.
    RevisionCarriesNoEntryList,
    /// The entry-list document could not be read now, though the bundle was intact when it was
    /// opened and a pinned bundle is not retired — so something outside this process changed
    /// an artifact the application treats as immutable.
    DocumentUnreadable,
    /// The entry-list document does not hold the bytes its manifest names.
    DocumentChanged,
    /// The entry-list document is intact and this build cannot decode it. The failure manifest
    /// validation structurally cannot catch, because it hashes bytes without parsing them.
    DocumentUndecodable,
}

impl fmt::Display for ReadEntryListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPublishedRevision => {
                f.write_str("no documentation has been published for this mod")
            }
            Self::DocumentationUnavailable { reason } => {
                write!(
                    f,
                    "this mod's published documentation cannot be read: {reason}"
                )
            }
        }
    }
}

impl fmt::Display for UnavailableReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReferenceUnreadable => f.write_str(
                "the published revision recorded for this mod is not a revision identifier",
            ),
            Self::RevisionMissing => f.write_str("no bundle holds the published revision"),
            Self::RevisionFromAnotherBuild { found, supported } => write!(
                f,
                "it was written by a different build (bundle schema version {found} is not \
                 {supported})"
            ),
            Self::RevisionDamaged => f.write_str("its bundle disagrees with its own manifest"),
            Self::RevisionDisplaced => f.write_str("another revision's bundle occupies its place"),
            Self::RevisionCarriesNoEntryList => f.write_str("it carries no entry list to read"),
            Self::DocumentUnreadable => f.write_str("its entry list could not be read"),
            Self::DocumentChanged => f.write_str("its entry list was changed after publication"),
            Self::DocumentUndecodable => {
                f.write_str("its entry list cannot be understood by this build")
            }
        }
    }
}

impl std::error::Error for ReadEntryListError {}
impl std::error::Error for UnavailableReason {}

/// Serves the entries of the revision currently published for `installation`.
pub fn read_entry_list(
    installation: ModInstallationId,
    state: &StateStore,
    revisions: &RevisionStore,
) -> OpResult<DocumentationEntries, ReadEntryListError> {
    let Some(reference) = state
        .snapshot()
        .publication_references
        .get(&installation)
        .cloned()
    else {
        return Err(Failure::Expected(ReadEntryListError::NoPublishedRevision));
    };

    let reader = revisions
        .open_revision(&reference.revision)
        .map_err(classify_open)?;

    let list = reader
        .entry_list()
        .map_err(classify_document)?
        .ok_or(Failure::Expected(unavailable(
            UnavailableReason::RevisionCarriesNoEntryList,
        )))?;

    Ok(DocumentationEntries {
        installation,
        entries: list
            .entries
            .into_iter()
            .map(|entry| DocumentationEntry {
                category: entry.category,
                identifier: entry.identifier,
                display_name: entry.display_name,
            })
            .collect(),
    })
}

fn unavailable(reason: UnavailableReason) -> ReadEntryListError {
    ReadEntryListError::DocumentationUnavailable { reason }
}

/// Total over [`OpenRevisionError`], and pure, so every arm is reachable from a test without
/// reproducing the filesystem state that produces it. That matters more here than style:
/// `IncompatibleSchema` and `Damaged` are not producible from this layer at all, because bundle
/// layout is `revisions`-private.
fn classify_open(error: OpenRevisionError) -> Failure<ReadEntryListError> {
    let reason = match error {
        OpenRevisionError::UnreadableReference(_) => UnavailableReason::ReferenceUnreadable,
        OpenRevisionError::NotFound => UnavailableReason::RevisionMissing,
        OpenRevisionError::IncompatibleSchema(mismatch) => {
            UnavailableReason::RevisionFromAnotherBuild {
                found: mismatch.found,
                supported: mismatch.supported,
            }
        }
        OpenRevisionError::Damaged(_) => UnavailableReason::RevisionDamaged,
        OpenRevisionError::AnotherRevision(_) => UnavailableReason::RevisionDisplaced,
        // Thrown rather than reported: see this module's note. Nothing in Phase 3 grants a
        // retirement claim, so reaching here means the pin registry answered a question
        // nothing asked it.
        OpenRevisionError::Retired => {
            return crate::error::Unexpected::new(
                "a revision was reported retiring before any retention sweep exists",
            )
            .into();
        }
    };
    Failure::Expected(unavailable(reason))
}

/// Total over [`DocumentUnavailable`]. Every `detail` string is dropped here.
fn classify_document(error: DocumentUnavailable) -> Failure<ReadEntryListError> {
    let reason = match error {
        DocumentUnavailable::Unreadable { .. } => UnavailableReason::DocumentUnreadable,
        DocumentUnavailable::Changed { .. } => UnavailableReason::DocumentChanged,
        DocumentUnavailable::Undecodable { .. } => UnavailableReason::DocumentUndecodable,
    };
    Failure::Expected(unavailable(reason))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::revisions::{
        DocumentIdentity, IdentityParseError, SchemaMismatch, ValidationReport,
    };

    fn installation() -> ModInstallationId {
        ModInstallationId::parse(&"ab".repeat(32)).unwrap()
    }

    #[test]
    fn every_open_refusal_that_names_a_persisted_artifact_is_expected() {
        let rows = [
            (
                OpenRevisionError::UnreadableReference(IdentityParseError),
                UnavailableReason::ReferenceUnreadable,
            ),
            (
                OpenRevisionError::NotFound,
                UnavailableReason::RevisionMissing,
            ),
            (
                OpenRevisionError::IncompatibleSchema(SchemaMismatch {
                    found: 7,
                    supported: 1,
                }),
                UnavailableReason::RevisionFromAnotherBuild {
                    found: 7,
                    supported: 1,
                },
            ),
            (
                OpenRevisionError::Damaged(ValidationReport::default()),
                UnavailableReason::RevisionDamaged,
            ),
        ];

        for (error, expected) in rows {
            let Failure::Expected(ReadEntryListError::DocumentationUnavailable { reason }) =
                classify_open(error)
            else {
                panic!("expected an expected refusal");
            };
            assert_eq!(reason, expected);
        }
    }

    #[test]
    fn a_retirement_claim_is_unexpected_because_nothing_in_this_phase_grants_one() {
        assert!(matches!(
            classify_open(OpenRevisionError::Retired),
            Failure::Unexpected(_)
        ));
    }

    #[test]
    fn no_expected_refusal_names_a_bundle_relative_entry() {
        // The D-124 guard. A `ValidationReport` names `documents/entry-list.json`; the refusal
        // built from it must not, and neither must a document refusal built from an
        // `io::Error` that quoted an absolute path.
        let report = ValidationReport {
            unexpected: vec!["documents/entry-list.json".to_owned()],
            ..Default::default()
        };
        let damaged = classify_open(OpenRevisionError::Damaged(report.clone()));
        let unreadable = classify_document(DocumentUnavailable::Unreadable {
            document: DocumentIdentity::EntryList,
            detail: "/Users/x/Library/Application Support/secret".to_owned(),
        });

        // Negative control: the values these refusals were built from do carry the strings, so
        // the assertions below are not vacuous.
        assert!(report.to_string().contains("documents/entry-list.json"));

        for failure in [damaged, unreadable] {
            let Failure::Expected(expected) = failure else {
                panic!("expected an expected refusal");
            };
            for rendering in [expected.to_string(), format!("{expected:?}")] {
                assert!(!rendering.contains("documents/"));
                assert!(!rendering.contains(".json"));
                assert!(!rendering.contains("secret"));
            }
        }
    }

    #[test]
    fn every_document_refusal_is_expected_and_drops_its_detail_string() {
        let rows = [
            (
                DocumentUnavailable::Unreadable {
                    document: DocumentIdentity::EntryList,
                    detail: "detail".to_owned(),
                },
                UnavailableReason::DocumentUnreadable,
            ),
            (
                DocumentUnavailable::Changed {
                    document: DocumentIdentity::EntryList,
                },
                UnavailableReason::DocumentChanged,
            ),
            (
                DocumentUnavailable::Undecodable {
                    document: DocumentIdentity::EntryList,
                    detail: "detail".to_owned(),
                },
                UnavailableReason::DocumentUndecodable,
            ),
        ];

        for (error, expected) in rows {
            let Failure::Expected(ReadEntryListError::DocumentationUnavailable { reason }) =
                classify_document(error)
            else {
                panic!("expected an expected refusal");
            };
            assert_eq!(reason, expected);
        }
    }

    #[test]
    fn read_entry_list_error_displays_and_implements_std_error() {
        let error = unavailable(UnavailableReason::RevisionCarriesNoEntryList);
        assert!(error.to_string().contains("carries no entry list"));
        let _: &dyn std::error::Error = &error;
        let _: &dyn std::error::Error = &UnavailableReason::RevisionMissing;
        let _ = installation();
    }
}
