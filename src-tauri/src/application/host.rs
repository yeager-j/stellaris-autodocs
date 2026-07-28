//! The whole of the application a transport is given.
//!
//! It lives here rather than in `composition` for a dependency reason: `transport` must be
//! able to name the type it holds, and `transport -> composition` would invert the direction
//! the design fixes (transports → application → deep modules).
//!
//! **Its value is what it withholds, not the forwarding.** A `&StateStore` handed to a command
//! would also hand it [`add_discovery_location`], [`rebind_discovery_location`],
//! [`remove_discovery_location`], and `confirm_discard_unrecovered_references` — the mutable
//! state document entire, reachable from a Tauri command that has no business with any of it.
//! Code holding a [`DocumentationHost`] has no expressible path to those. That is the argument
//! [`PublicationCapability`](crate::state::PublicationCapability) makes for `revisions`, made
//! once more one layer up, and it is why this type has two methods rather than accessors.
//!
//! The stores are owned rather than borrowed: their lifetime is the process, and the
//! composition root constructs them once and shares this value with every transport.
//!
//! [`add_discovery_location`]: crate::state::StateStore::add_discovery_location
//! [`rebind_discovery_location`]: crate::state::StateStore::rebind_discovery_location
//! [`remove_discovery_location`]: crate::state::StateStore::remove_discovery_location

use crate::application::candidates::RevisionCandidateSource;
use crate::application::publish::{
    BuildGuard, DocumentationPublished, PublishDocumentationError, publish_provided_candidate,
};
use crate::application::read::{DocumentationEntries, ReadEntryListError, read_entry_list};
use crate::application::target::BuildTarget;
use crate::discovery::identity::ModInstallationId;
use crate::error::OpResult;
use crate::revisions::RevisionStore;
use crate::state::StateStore;

pub struct DocumentationHost {
    state: StateStore,
    revisions: RevisionStore,
    /// Which source a build gets is the composition root's one decision here, and it is the
    /// whole of "a production build cannot publish documentation it did not analyze".
    candidates: Box<dyn RevisionCandidateSource>,
    guard: BuildGuard,
}

impl DocumentationHost {
    pub fn new(
        state: StateStore,
        revisions: RevisionStore,
        candidates: Box<dyn RevisionCandidateSource>,
    ) -> Self {
        Self {
            state,
            revisions,
            candidates,
            guard: BuildGuard::new(),
        }
    }

    pub fn publish_documentation(
        &self,
        target: &BuildTarget,
    ) -> OpResult<DocumentationPublished, PublishDocumentationError> {
        publish_provided_candidate(
            target,
            self.candidates.as_ref(),
            &self.guard,
            &self.state,
            &self.revisions,
        )
    }

    pub fn entry_list(
        &self,
        installation: ModInstallationId,
    ) -> OpResult<DocumentationEntries, ReadEntryListError> {
        read_entry_list(installation, &self.state, &self.revisions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host is shared for the process lifetime and its methods run on a blocking worker, so
    /// this is a precondition of the transport shape rather than an observation about it. The
    /// precedent is `publish.rs::a_revision_reader_is_send_and_sync_and_owns_no_borrow`.
    #[test]
    fn a_documentation_host_is_send_and_sync_and_owns_no_borrow() {
        fn assert_send_sync<T: Send + Sync + 'static>() {}
        assert_send_sync::<DocumentationHost>();
    }

    /// The walking skeleton's own thread, through the two methods a transport can call and
    /// nothing else. Everything below the host is production code: a real bundle, a real
    /// atomic publication, and a read that resolves the revision from the state pointer alone.
    #[cfg(feature = "test-support")]
    mod thread {
        use super::*;
        use crate::analysis::version::AnalysisVersionVector;
        use crate::application::read::ReadEntryListError;
        use crate::application::{DocumentationEntry, UnavailableReason};
        use crate::canonical::path::LogicalPath;
        use crate::error::Failure;
        use crate::revisions::{
            EntryList, EntrySummary, RevisionCandidate, RevisionDocument, RevisionInputs,
        };
        use crate::source::fixture::FixtureCorpus;
        use crate::source::snapshot::SourceKind;
        use crate::state::OpenOutcome;
        use crate::testsupport::candidates::HandAuthoredCandidates;
        use std::path::PathBuf;
        use tempfile::TempDir;

        /// A candidate documenting `installation`, with fingerprints derived by the same
        /// construction path a live Source Snapshot uses (D-113) rather than hand-made digests.
        fn candidate(
            installation: ModInstallationId,
            documents: Vec<RevisionDocument>,
        ) -> RevisionCandidate {
            let target_mod = FixtureCorpus::new(SourceKind::TargetMod)
                .with_file("descriptor.mod", b"name=\"Fixture Mod\"\n")
                .build()
                .unwrap();
            let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
                .with_file("common/technology/00_tech.txt", b"tech_a = { cost = 2 }\n")
                .build()
                .unwrap();
            RevisionCandidate::new(
                installation,
                RevisionInputs {
                    target_mod: target_mod.fingerprint(),
                    vanilla_content: vanilla.fingerprint(),
                },
                AnalysisVersionVector::current(),
                true,
                documents,
            )
            .unwrap()
        }

        fn entry_list(entries: Vec<EntrySummary>) -> Vec<RevisionDocument> {
            vec![RevisionDocument::EntryList(EntryList { entries })]
        }

        /// Opens both stores under one directory, configures a Discovery Location, and builds
        /// the host around a source registering `documents` for the target.
        fn host_for(documents: Vec<RevisionDocument>) -> (TempDir, DocumentationHost, BuildTarget) {
            let dir = TempDir::new().unwrap();
            let OpenOutcome::Ready { store, .. } = StateStore::open(dir.path()).unwrap() else {
                panic!("state store blocked");
            };
            let (location, _) = store
                .add_discovery_location(PathBuf::from("/tmp/workshop"))
                .unwrap();
            let target = BuildTarget::derive(location, LogicalPath::parse("ugc_1").unwrap());
            let revisions = RevisionStore::open(&dir.path().join("revisions")).unwrap();
            let source =
                HandAuthoredCandidates::new().with(candidate(target.installation(), documents));
            (
                dir,
                DocumentationHost::new(store, revisions, Box::new(source)),
                target,
            )
        }

        #[test]
        fn a_build_publishes_entries_that_the_read_serves_back() {
            let (_dir, host, target) = host_for(entry_list(vec![EntrySummary {
                category: "technology".to_owned(),
                identifier: "tech_a".to_owned(),
                display_name: Some("Fixture Technology".to_owned()),
            }]));

            host.publish_documentation(&target).unwrap();
            let served = host.entry_list(target.installation()).unwrap();

            assert_eq!(served.installation, target.installation());
            assert_eq!(
                served.entries,
                vec![DocumentationEntry {
                    category: "technology".to_owned(),
                    identifier: "tech_a".to_owned(),
                    display_name: Some("Fixture Technology".to_owned()),
                }]
            );
        }

        #[test]
        fn reading_before_any_build_reports_that_nothing_is_published() {
            let (_dir, host, target) = host_for(entry_list(Vec::new()));

            let refusal = host.entry_list(target.installation()).unwrap_err();

            assert!(matches!(
                refusal,
                Failure::Expected(ReadEntryListError::NoPublishedRevision)
            ));
        }

        #[test]
        fn a_revision_that_documents_nothing_is_a_success_with_no_entries() {
            let (_dir, host, target) = host_for(entry_list(Vec::new()));

            host.publish_documentation(&target).unwrap();
            let served = host.entry_list(target.installation()).unwrap();

            assert!(served.entries.is_empty());
        }

        #[test]
        fn a_revision_carrying_no_entry_list_is_a_different_answer_from_one_that_documents_nothing()
        {
            // The empty-versus-absent line, end to end. A candidate with no documents publishes
            // cleanly — `revisions` has no rule against it — and its manifest then names no
            // entry-list entry, which the reader reports as `Ok(None)` rather than as an empty
            // list. Conflating the two here would erase a distinction two layers below.
            let (_dir, host, target) = host_for(Vec::new());

            host.publish_documentation(&target).unwrap();
            let refusal = host.entry_list(target.installation()).unwrap_err();

            assert!(matches!(
                refusal,
                Failure::Expected(ReadEntryListError::DocumentationUnavailable {
                    reason: UnavailableReason::RevisionCarriesNoEntryList
                })
            ));
        }
    }
}
