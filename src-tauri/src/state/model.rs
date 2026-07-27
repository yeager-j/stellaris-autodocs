//! The durable mutable state document (docs/technical-design.md, "Durable mutable
//! state"): user intent, publication references, and the schema version required to
//! interpret them. Everything else about the Mod Library is derived at scan time.

use crate::discovery::identity::{DiscoveryLocationId, ModInstallationId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const CURRENT_SCHEMA: u32 = 1;

/// Opaque published-revision reference. Phase 3 derives it from the canonical revision
/// manifest digest; state preserves it as an exact round-trip token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RevisionId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryLocation {
    pub id: DiscoveryLocationId,
    /// Editable configuration, not identity: a rebind changes this and nothing else.
    pub path: PathBuf,
}

/// The atomically published revision for one installation. `location` is stored because
/// it is not recoverable from the digest-valued installation id, and removing a
/// Discovery Location must cascade its references in one mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationReference {
    pub location: DiscoveryLocationId,
    pub revision: RevisionId,
}

/// Every collection and option carries a read-side default so a document with the
/// field absent still decodes — the revision-bundle spike's lesson.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppState {
    pub schema: u32,
    #[serde(default)]
    pub discovery_locations: Vec<DiscoveryLocation>,
    #[serde(default)]
    pub publication_references: BTreeMap<ModInstallationId, PublicationReference>,
    /// Set when unreadable state was quarantined; cleared only by explicit user
    /// confirmation. Persisted so a restart cannot silently forget that
    /// publication-reference recovery is unresolved. While present, orphan-revision
    /// and Asset Store cleanup stay disabled (docs/technical-design.md, "State
    /// evolution and recovery").
    #[serde(default)]
    pub unresolved_quarantine: Option<String>,
}

impl AppState {
    pub fn first_launch() -> Self {
        Self {
            schema: CURRENT_SCHEMA,
            discovery_locations: Vec::new(),
            publication_references: BTreeMap::new(),
            unresolved_quarantine: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_with_every_optional_field_absent() {
        // Manifest-style validation hashes bytes without parsing them, so an
        // undecodable-but-intact document is the failure mode to guard against
        // (docs/technical-design.md, "Materialized JSON read model").
        let minimal: AppState = serde_json::from_str(r#"{"schema":1}"#).unwrap();
        assert_eq!(minimal, AppState::first_launch());
    }

    #[test]
    fn round_trips_a_populated_state() {
        use crate::canonical::path::LogicalPath;
        use crate::discovery::identity::{DiscoveryLocationId, ModInstallationId};

        let location = DiscoveryLocationId::generate();
        let installation =
            ModInstallationId::derive(location, &LogicalPath::parse("ugc_1").unwrap());
        let state = AppState {
            schema: CURRENT_SCHEMA,
            discovery_locations: vec![DiscoveryLocation {
                id: location,
                path: std::path::PathBuf::from("/tmp/workshop"),
            }],
            publication_references: std::collections::BTreeMap::from([(
                installation,
                PublicationReference {
                    location,
                    revision: RevisionId("abc123".to_owned()),
                },
            )]),
            unresolved_quarantine: Some("state.json.quarantine-1-deadbeef".to_owned()),
        };
        let encoded = serde_json::to_vec(&state).unwrap();
        let decoded: AppState = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, state);
    }
}
