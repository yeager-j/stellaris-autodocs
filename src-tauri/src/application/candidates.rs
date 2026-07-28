//! Where a Revision Candidate comes from, as a seam the composition root chooses an
//! implementation of (docs/implementation-plan.md, Phase 3 task 4).
//!
//! # Why this is a seam and not a call into `analysis`
//!
//! Phase 3 has no analysis. The walking skeleton has to exercise application coordination and
//! the real publication protocol without pretending to read Stellaris source, and the one
//! thing it must not do is put a fake analysis inside the module that will hold the real one.
//! So the coordinator asks *something* for a candidate, and the composition root decides what.
//!
//! **The evidence for the seam is in this ticket, not in a promise.** AGENTS.md asks for two
//! implementations rather than one hypothetical, and there are two here:
//! [`NoAnalysisSource`], which is what a shipped binary gets, and
//! [`HandAuthoredCandidates`](crate::testsupport::candidates::HandAuthoredCandidates), which is
//! what a test gets. That variation is not decoration — it is the whole mechanism by which "a
//! production build cannot publish a revision that documents nothing real" is true by
//! construction rather than by care. Phase 6 does not add a third implementation; it *deletes*
//! the seam, which is a named entry condition of that phase, and the plan carries the checklist
//! of what that deletion covers (docs/implementation-plan.md, Phase 6 entry conditions).
//!
//! # Why the candidate is assembled here rather than produced by `analysis`
//!
//! `revisions` owns the [`RevisionCandidate`] type and `analysis` never names one (D-120), so
//! the application layer is what maps analysis output into a candidate. That keeps
//! `analysis -> revisions` from existing at all; both edges run downward from here. A source
//! implemented over the real pipeline in Phase 6 therefore still lives at this layer.
//!
//! # What a source may not decide
//!
//! **Which Discovery Location a revision is published against.** A location is editable
//! configuration rather than analysis output (docs/technical-design.md, "Installation
//! identity"), the coordinator already holds it inside a
//! [`BuildTarget`](crate::application::BuildTarget), and letting a source return one would
//! reopen exactly the coherence hole that type exists to close.
//!
//! **Which failures the operation reports.** [`CandidateUnavailable`] is this seam's own
//! error and travels nested inside [`Failure`](crate::error::Failure), so a source cannot
//! name `StorageUnavailable` or any other outcome that belongs to a step it does not perform.

use crate::discovery::identity::ModInstallationId;
use crate::error::{Failure, OpResult};
use crate::revisions::RevisionCandidate;
use std::fmt;

/// Supplies the Revision Candidate a publication publishes.
///
/// `Send + Sync` because one implementation is held for the process lifetime in shared state
/// and is called from a blocking worker, not from the thread that constructed it.
pub trait RevisionCandidateSource: Send + Sync {
    /// The candidate documenting `installation`, or why none can be produced.
    ///
    /// Takes the installation identifier alone. Everything else a publication needs — the
    /// Discovery Location, the prior published revision — is the coordinator's to hold, and a
    /// parameter here would invite a source to have an opinion about it.
    fn candidate_for(
        &self,
        installation: ModInstallationId,
    ) -> OpResult<RevisionCandidate, CandidateUnavailable>;
}

/// Why a source produced no candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateUnavailable {
    /// This build performs no documentation analysis at all, so no candidate exists for any
    /// installation. Not a fact about the installation that was asked for.
    NoAnalysis,
    /// The source analyzes, and has nothing for this installation. Kept distinct from
    /// [`NoAnalysis`](CandidateUnavailable::NoAnalysis) because the two send a person to
    /// different places — one is a property of the build, the other of the request — and
    /// folding them would make the production source's refusal indistinguishable from a real
    /// analyzer's.
    NothingDocumented,
}

impl fmt::Display for CandidateUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAnalysis => f.write_str(
                "this build performs no documentation analysis, so it can produce no \
                 documentation to publish",
            ),
            Self::NothingDocumented => {
                f.write_str("no documentation was produced for this mod installation")
            }
        }
    }
}

impl std::error::Error for CandidateUnavailable {}

/// The source a production build gets: it refuses, always.
///
/// **This is application behaviour, which is why it lives here and not in `composition`.**
/// Homing it in the application layer makes refusal the default that a build *starts* from,
/// and `test-support` a thing that replaces it. Homing it beside the wiring would make "a
/// production build cannot publish a fake revision" a property of one line in `run`, where the
/// next person to edit the builder could quietly lose it.
///
/// It is fallible for the same reason it exists: refusing is its entire job, and an infallible
/// seam would leave it only a panic or a fabrication to return.
pub struct NoAnalysisSource;

impl RevisionCandidateSource for NoAnalysisSource {
    fn candidate_for(
        &self,
        _installation: ModInstallationId,
    ) -> OpResult<RevisionCandidate, CandidateUnavailable> {
        Err(Failure::Expected(CandidateUnavailable::NoAnalysis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_production_candidate_source_refuses_because_this_build_performs_no_analysis() {
        let refusal = NoAnalysisSource
            .candidate_for(ModInstallationId::parse(&"ab".repeat(32)).unwrap())
            .unwrap_err();

        assert!(matches!(
            refusal,
            Failure::Expected(CandidateUnavailable::NoAnalysis)
        ));
    }

    #[test]
    fn candidate_unavailable_displays_and_implements_std_error() {
        let error = CandidateUnavailable::NoAnalysis;
        assert!(error.to_string().contains("no documentation analysis"));
        let _: &dyn std::error::Error = &error;
    }
}
