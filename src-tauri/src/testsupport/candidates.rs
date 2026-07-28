//! The Phase 3 stand-in for analysis: a [`RevisionCandidateSource`] that answers from
//! candidates a test wrote by hand.
//!
//! **This is the Phase 6 deletion target.** Deleting the Phase 3 candidate provider from the
//! build path is a named entry condition of that phase, and the checklist of what goes with it
//! — this file, the [`RevisionCandidateSource`] seam, and the wiring that chooses between
//! implementations — is recorded there rather than here, because a note that lives only in the
//! file being deleted disappears at the moment it is needed
//! (docs/implementation-plan.md, Phase 6 entry conditions).
//!
//! It lives in `testsupport`, behind the `test-support` feature, so it structurally cannot
//! reach a shipped binary (D-107). That is what makes "a production build cannot publish a
//! revision documenting nothing real" a property of the type system rather than of a wiring
//! line: the only source a release build can construct is
//! [`NoAnalysisSource`](crate::application::NoAnalysisSource), because this one does not exist
//! there to be chosen.
//!
//! It performs no analysis and does not pretend to. A candidate is registered whole, so what a
//! test asserts about publication is never confused with an assertion about how a candidate was
//! derived — and no false analysis behaviour enters
//! [`analysis`](crate::analysis), which stays empty until Phase 4.

use crate::analysis::version::AnalysisVersionVector;
use crate::application::candidates::{CandidateUnavailable, RevisionCandidateSource};
use crate::application::target::BuildTarget;
use crate::canonical::path::LogicalPath;
use crate::discovery::identity::ModInstallationId;
use crate::error::{Failure, OpResult};
use crate::revisions::{
    EntryList, EntrySummary, RevisionCandidate, RevisionDocument, RevisionInputs,
};
use crate::source::fixture::FixtureCorpus;
use crate::source::snapshot::SourceKind;
use crate::state::StateStore;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The marker path and mod root the seeded thread uses. Constants because
/// [`seed_skeleton_thread`] both writes and recognizes them, and two spellings would make the
/// seed stop being idempotent without failing anything.
const SKELETON_LOCATION_PATH: &str = "/skeleton/workshop";
const SKELETON_MOD_ROOT: &str = "skeleton_mod";

/// Answers with the candidate registered for the installation asked for.
#[derive(Default)]
pub struct HandAuthoredCandidates {
    /// Keyed by the candidate's own `installation` rather than by a separately supplied key,
    /// so a registration cannot disagree with what it registers — the coordinator refuses that
    /// disagreement as a defect, and a test that could construct one would be exercising the
    /// registry's bookkeeping instead of the coordinator.
    by_installation: BTreeMap<ModInstallationId, RevisionCandidate>,
}

impl HandAuthoredCandidates {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `candidate`, replacing any earlier one for the same installation — which is
    /// how a test arranges the second of two publications.
    #[must_use]
    pub fn with(mut self, candidate: RevisionCandidate) -> Self {
        self.by_installation
            .insert(candidate.installation, candidate);
        self
    }
}

impl RevisionCandidateSource for HandAuthoredCandidates {
    fn candidate_for(
        &self,
        installation: ModInstallationId,
    ) -> OpResult<RevisionCandidate, CandidateUnavailable> {
        self.by_installation
            .get(&installation)
            .cloned()
            .ok_or(Failure::Expected(CandidateUnavailable::NothingDocumented))
    }
}

/// Configures one Discovery Location on `state` and registers a candidate for one mod under
/// it, so a `test-support` build has a thread that can actually be run.
///
/// Without this, enabling the feature would swap in a source with nothing in it and every
/// build would refuse — the walking skeleton would be wired and unexercisable. It is
/// deliberately the *whole* of the fake: the location is real state, the fingerprints are
/// derived by [`FixtureCorpus`] through the same construction path a live snapshot uses
/// (D-113), and everything downstream of here is production code.
///
/// Returns the target the build command must be given, because the identifier it derives is a
/// digest that nothing else can reproduce without the pair.
///
/// The seeded location's path is a marker, not a directory: nothing here reads the filesystem,
/// and Phase 3 has no discovery scan to disagree with it.
pub fn seed_skeleton_thread(state: &StateStore) -> (BuildTarget, HandAuthoredCandidates) {
    // Idempotent, because a development build runs this on **every** launch against a state
    // document that persists between them. Adding unconditionally would mint a fresh Discovery
    // Location identifier each time — a growing pile of locations, and a new Mod Installation
    // identifier per launch, so yesterday's published revision would be unreachable today.
    let seeded = state
        .snapshot()
        .discovery_locations
        .into_iter()
        .find(|configured| configured.path == Path::new(SKELETON_LOCATION_PATH))
        .map(|configured| configured.id);
    let location = match seeded {
        Some(location) => location,
        None => {
            state
                .add_discovery_location(PathBuf::from(SKELETON_LOCATION_PATH))
                .expect("the state document accepts a Discovery Location")
                .0
        }
    };
    let target = BuildTarget::derive(
        location,
        LogicalPath::parse(SKELETON_MOD_ROOT).expect("a literal logical path"),
    );

    let target_mod = FixtureCorpus::new(SourceKind::TargetMod)
        .with_file(
            "descriptor.mod",
            b"name=\"Skeleton Mod\"\nsupported_version=\"4.4\"\n",
        )
        .with_file(
            "common/technology/00_tech.txt",
            b"tech_skeleton = { cost = 1 }\n",
        )
        .build()
        .expect("the skeleton fixture corpus is well formed");
    let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
        .with_file(
            "common/technology/00_tech.txt",
            b"tech_skeleton = { cost = 2 }\n",
        )
        .build()
        .expect("the skeleton vanilla corpus is well formed");

    let candidate = RevisionCandidate::new(
        target.installation(),
        RevisionInputs {
            target_mod: target_mod.fingerprint(),
            vanilla_content: vanilla.fingerprint(),
        },
        AnalysisVersionVector::current(),
        true,
        vec![RevisionDocument::EntryList(EntryList {
            entries: vec![EntrySummary {
                category: "technology".to_owned(),
                identifier: "tech_skeleton".to_owned(),
                display_name: Some("Skeleton Technology".to_owned()),
            }],
        })],
    )
    .expect("the skeleton candidate carries one document");

    (
        target.clone(),
        HandAuthoredCandidates::new().with(candidate),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::OpenOutcome;
    use tempfile::TempDir;

    fn opened(dir: &TempDir) -> StateStore {
        let OpenOutcome::Ready { store, .. } = StateStore::open(dir.path()).unwrap() else {
            panic!("state store blocked");
        };
        store
    }

    #[test]
    fn a_hand_authored_source_answers_only_for_the_installation_it_registered() {
        let dir = TempDir::new().unwrap();
        let store = opened(&dir);

        let (target, source) = seed_skeleton_thread(&store);

        let candidate = source.candidate_for(target.installation()).unwrap();
        assert_eq!(candidate.installation, target.installation());

        let stranger = ModInstallationId::parse(&"cd".repeat(32)).unwrap();
        assert!(matches!(
            source.candidate_for(stranger),
            Err(Failure::Expected(CandidateUnavailable::NothingDocumented))
        ));
    }

    #[test]
    fn seeding_twice_reuses_the_location_and_therefore_the_installation() {
        // A development build seeds on every launch against state that persists between them.
        // If this minted a new location each time, yesterday's published revision would belong
        // to an installation identifier nothing asks for again.
        let dir = TempDir::new().unwrap();
        let store = opened(&dir);

        let (first, _) = seed_skeleton_thread(&store);
        let (second, _) = seed_skeleton_thread(&store);

        assert_eq!(first.location(), second.location());
        assert_eq!(first.installation(), second.installation());
        assert_eq!(store.snapshot().discovery_locations.len(), 1);
    }

    #[test]
    fn the_seeded_location_is_configured_so_a_publication_can_reference_it() {
        // The half of the seed that is real state rather than a fixture. Without the Discovery
        // Location the coordinator refuses with `InstallationUnavailable` and the seeded
        // candidate is unreachable — the failure a developer would meet on launch, which is
        // exactly the kind no test would otherwise catch.
        let dir = TempDir::new().unwrap();
        let store = opened(&dir);

        let (target, _) = seed_skeleton_thread(&store);

        assert!(
            store
                .snapshot()
                .discovery_locations
                .iter()
                .any(|configured| configured.id == target.location())
        );
    }
}
