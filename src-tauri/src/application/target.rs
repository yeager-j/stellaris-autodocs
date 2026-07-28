//! What a documentation build is aimed at, and the evidence that its two identifiers agree.
//!
//! [`PublicationCapability::set_publication_reference`] states a precondition its own layer
//! cannot check: the caller owes that the Mod Installation and the Discovery Location it
//! passes are a coherent pair, because `ModInstallationId` derivation is one-way and nothing
//! in `state` can verify that the installation came from the location. A mismatched pair
//! would survive [`StateStore::remove_discovery_location`]'s cascade as an unreachable
//! reference. That doc comment homes the guarantee "in the application layer, the only place
//! a scan result's installation and its owning location are simultaneously in hand".
//!
//! [`BuildTarget`] is that home, and it discharges the obligation by **derivation rather than
//! assertion**: the only way to obtain an installation identifier from one is to have handed
//! in the location it was derived from. Holding a target is the evidence, the way holding a
//! [`RevisionCandidate`](crate::revisions::RevisionCandidate) is the evidence that `new`
//! refused two documents of one identity.
//!
//! **The absent constructor is the design.** There is no `From<ModInstallationId>` and no way
//! to set the fields independently. Phase 9 changes only where the pair comes from —
//! [`discovery::ModInstallation`](crate::discovery::ModInstallation) already carries `id`,
//! `location`, and `relative_path` from one derivation, so a target is built from a scan
//! result rather than from a caller's two arguments. It must not acquire a bare-identifier
//! constructor on the way, or the guarantee reverts to discipline.
//!
//! [`PublicationCapability::set_publication_reference`]: crate::state::PublicationCapability::set_publication_reference
//! [`StateStore::remove_discovery_location`]: crate::state::StateStore::remove_discovery_location

use crate::canonical::path::LogicalPath;
use crate::discovery::identity::{DiscoveryLocationId, ModInstallationId};

/// One Mod Installation, named together with the Discovery Location its identifier was
/// derived from.
///
/// Fields are private and [`derive`](BuildTarget::derive) is the sole constructor, so the
/// three values cannot disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildTarget {
    location: DiscoveryLocationId,
    mod_root: LogicalPath,
    installation: ModInstallationId,
}

impl BuildTarget {
    pub fn derive(location: DiscoveryLocationId, mod_root: LogicalPath) -> Self {
        let installation = ModInstallationId::derive(location, &mod_root);
        Self {
            location,
            mod_root,
            installation,
        }
    }

    pub fn installation(&self) -> ModInstallationId {
        self.installation
    }

    pub fn location(&self) -> DiscoveryLocationId {
        self.location
    }

    pub fn mod_root(&self) -> &LogicalPath {
        &self.mod_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(raw: &str) -> LogicalPath {
        LogicalPath::parse(raw).unwrap()
    }

    #[test]
    fn a_target_carries_the_location_its_installation_identifier_was_derived_from() {
        // The whole of the guarantee `PublicationCapability` cannot check: the pair a caller
        // hands to `set_publication_reference` agrees because one half was computed from the
        // other, not because the caller was careful.
        let location = DiscoveryLocationId::generate();
        let target = BuildTarget::derive(location, path("mod/acot"));

        assert_eq!(target.location(), location);
        assert_eq!(
            target.installation(),
            ModInstallationId::derive(location, &path("mod/acot"))
        );
    }

    #[test]
    fn two_locations_holding_the_same_mod_path_are_different_targets() {
        // Derivation is what makes this true, and it is why a target cannot be assembled from
        // an installation identifier alone: the identifier does not name its location, so
        // recovering one from it is impossible rather than merely inconvenient.
        let first = BuildTarget::derive(DiscoveryLocationId::generate(), path("mod/acot"));
        let second = BuildTarget::derive(DiscoveryLocationId::generate(), path("mod/acot"));

        assert_eq!(first.mod_root(), second.mod_root());
        assert_ne!(first.installation(), second.installation());
    }
}
