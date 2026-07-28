//! The acceptance thread itself: boot, build, read, and restart.

use stellaris_docs_lib::application::{
    BuildTarget, CandidateUnavailable, DocumentationEntries, DocumentationHost,
    DocumentationPublished, NoAnalysisSource, PublishDocumentationError, ReadEntryListError,
    RevisionCandidateSource,
};
use stellaris_docs_lib::composition::{Stores, open_stores};
use stellaris_docs_lib::discovery::identity::ModInstallationId;
use stellaris_docs_lib::error::OpResult;
use stellaris_docs_lib::revisions::RevisionCandidate;
use stellaris_docs_lib::state::OpenReport;
use stellaris_docs_lib::testsupport::TempAppData;

use crate::corpora::AcceptanceCorpus;

/// The application-data directory, one level below the temporary root.
///
/// Deliberately a directory that does not exist yet, matching `composition::tests::app_data()`:
/// handing `open_stores` an already-created directory would make its `create_dir_all` a no-op
/// and leave the first-launch branch the real app takes unexercised.
const APP_DATA_DIR: &str = "stellaris-docs";

/// One acceptance run over one corpus.
///
/// Field order is drop order, and `host` is declared first on purpose. Today that is
/// defensive rather than load-bearing — neither store holds a `File`, and a Revision Reader
/// holds no operating-system handle at all because every document read is read-and-close
/// (`revisions/read.rs`) — but `TempDir::drop` discards its errors, so a wrong order would
/// fail silently. Stated here so it does not have to be re-derived by whoever adds a field.
pub struct AcceptanceThread {
    host: DocumentationHost,
    target: BuildTarget,
    corpus: AcceptanceCorpus,
    app_data: TempAppData,
}

impl AcceptanceThread {
    /// Boots the application over an isolated application-data directory and points it at
    /// `corpus`.
    ///
    /// The sequence is the composition root's, not a reconstruction of it: `open_stores` owns
    /// the decision that the revisions root sits beside the state document, and a harness that
    /// opened the two stores itself would be verifying a startup path the app does not run.
    ///
    /// **Ordering is fixed by ownership.** `DocumentationHost::new` *moves* the `StateStore`
    /// and the host exposes no accessor back to it, so anything that lives in durable state has
    /// to be configured in the window between opening the stores and constructing the host, or
    /// it is unreachable for the rest of the run. Phase 5's explicit language override is the
    /// next thing that lands in that window.
    pub fn boot(corpus: AcceptanceCorpus) -> Self {
        let app_data = TempAppData::new();
        let stores = open(&app_data, corpus.name());
        assert_eq!(
            stores.report,
            OpenReport::FirstLaunch,
            "the {} thread must boot an empty application-data directory; anything else means \
             it is reading somebody else's state",
            corpus.name(),
        );

        let (location, _commit) = stores
            .state
            .add_discovery_location(corpus.location_path().to_path_buf())
            .expect("a fresh state document accepts a Discovery Location");
        // Derivation rather than assertion: holding the target *is* the evidence that its
        // installation identifier came from this location and this mod root.
        let target = BuildTarget::derive(location, corpus.mod_root().clone());

        let candidates =
            HandAuthoredCandidate::documenting(corpus.candidate(target.installation()));
        let host = DocumentationHost::new(stores.state, stores.revisions, Box::new(candidates));

        Self {
            host,
            target,
            corpus,
            app_data,
        }
    }

    /// The build use case, through the method `transport::tauri::build_documentation` calls.
    pub fn build(&self) -> OpResult<DocumentationPublished, PublishDocumentationError> {
        self.host.publish_documentation(&self.target)
    }

    /// The desktop read, through the method `transport::tauri::get_entry_list` calls.
    ///
    /// Named for the surface rather than for the operation: Phase 11's authorized Companion
    /// read joins as a sibling over the same published revision, and renaming this then would
    /// touch every case.
    pub fn desktop_entries(&self) -> OpResult<DocumentationEntries, ReadEntryListError> {
        self.host.entry_list(self.target.installation())
    }

    /// Restarts the application over the same directory, with the source a shipped binary gets.
    ///
    /// [`NoAnalysisSource`]'s whole job is to refuse, so a read that succeeds afterwards
    /// provably resolved the revision from persisted state and the bundle on disk, with nothing
    /// carried over in memory from the process that published it.
    pub fn reopen(self) -> Self {
        self.restart(None, Box::new(NoAnalysisSource))
    }

    /// Restarts over the same application data pointed at `corpus` — a rebuild after the source
    /// changed.
    ///
    /// **The Discovery Location and the mod root are the original corpus's and do not change**,
    /// which is what a rebuild means: the same Mod Installation, observed again. `corpus`
    /// therefore contributes its snapshots and its documentation, and its own `location_path`
    /// and `mod_root` are unused. Holding the installation fixed is also the only way to compare
    /// two revisions' identifiers and learn something about the corpus, since two separately
    /// booted threads mint two Discovery Location identifiers and would differ regardless.
    pub fn rebuild_over(self, corpus: AcceptanceCorpus) -> Self {
        let candidates =
            HandAuthoredCandidate::documenting(corpus.candidate(self.target.installation()));
        self.restart(Some(corpus), Box::new(candidates))
    }

    /// `Some` replaces the thread's corpus, for a rebuild pointed at new source; `None` keeps
    /// the one it booted with.
    fn restart(
        self,
        corpus: Option<AcceptanceCorpus>,
        candidates: Box<dyn RevisionCandidateSource>,
    ) -> Self {
        let Self {
            host,
            target,
            corpus: booted,
            app_data,
        } = self;
        drop(host);
        let corpus = corpus.unwrap_or(booted);

        let stores = open(&app_data, corpus.name());
        assert_eq!(
            stores.report,
            OpenReport::Loaded,
            "the {} thread must reopen the state document it wrote, not a fresh one",
            corpus.name(),
        );
        let host = DocumentationHost::new(stores.state, stores.revisions, candidates);

        Self {
            host,
            target,
            corpus,
            app_data,
        }
    }

    pub fn installation(&self) -> ModInstallationId {
        self.target.installation()
    }

    pub fn corpus(&self) -> &AcceptanceCorpus {
        &self.corpus
    }
}

fn open(app_data: &TempAppData, corpus: &str) -> Stores {
    open_stores(&app_data.path().join(APP_DATA_DIR)).unwrap_or_else(|refusal| {
        panic!("the {corpus} thread could not open its stores: {refusal}")
    })
}

/// A [`RevisionCandidateSource`] holding exactly one candidate.
///
/// **A Phase 6 deletion target**, alongside the seam it implements (docs/implementation-plan.md,
/// Phase 6 entry conditions). It is deliberately *not*
/// `testsupport::candidates::HandAuthoredCandidates`: that type and `seed_skeleton_thread` are
/// itemised on the same deletion list, and a harness depending on them would turn Phase 6's file
/// removal into a rewrite of the acceptance suite — which is exactly what "widen instead of
/// replacing" forbids.
///
/// One candidate rather than a registry: one thread targets one installation, and a map would be
/// bookkeeping the harness would then be tempted to test.
///
/// **It answers for any installation, and that is not a gap.** Whether a candidate documents the
/// installation it was asked for is `publish_provided_candidate`'s check, which exists precisely
/// because a seam can return somebody else's candidate and throws `Unexpected` when one does.
/// Repeating it here would put the rule in two places and turn a loud defect into a quiet
/// `NothingDocumented` — and since `boot` builds this candidate from the target's own
/// installation on the adjacent line, the guard would be an unreachable branch no case covers.
struct HandAuthoredCandidate {
    candidate: RevisionCandidate,
}

impl HandAuthoredCandidate {
    fn documenting(candidate: RevisionCandidate) -> Self {
        Self { candidate }
    }
}

impl RevisionCandidateSource for HandAuthoredCandidate {
    fn candidate_for(
        &self,
        _installation: ModInstallationId,
    ) -> OpResult<RevisionCandidate, CandidateUnavailable> {
        Ok(self.candidate.clone())
    }
}
