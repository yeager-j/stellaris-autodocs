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
}
