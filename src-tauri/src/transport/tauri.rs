//! Tauri command adapter. Populated from Phase 3. Inside this module, paths beginning
//! `tauri::` in `use` statements resolve to the Tauri crate, not this module.
//!
//! # This module holds no rules
//!
//! Each command does exactly four things: take arguments the framework already parsed, move
//! the call onto a blocking worker, project the application's answer into a wire DTO, and hand
//! the result to [`resolve`]. Any line here that decided *what* an outcome means would be a
//! second authority over behaviour `application` owns.
//!
//! # Why the projection is an exhaustive match rather than a derive
//!
//! Application and deep-module error types must not derive `Serialize`, and projecting them by
//! hand is the redaction boundary for *expected* outcomes — the counterpart to
//! [`Rejection`]'s for unexpected ones. It is not ceremony:
//! [`DocumentUnavailable`](crate::revisions::DocumentUnavailable) carries `detail` strings
//! built from `io::Error` and `serde_json::Error`, and
//! [`StageError`](crate::revisions::StageError) carries a staging path — all of which
//! §"Serializable result contract" forbids on a wire. Writing the match without a `_` arm also
//! makes a later-added application variant a compile error rather than a silent wire change
//! (D-081, D-095).
//!
//! # Arguments are parsed by the framework, and that is deliberate
//!
//! The commands take [`DiscoveryLocationId`], [`LogicalPath`], and [`ModInstallationId`]
//! rather than strings. Each validates in its own `Deserialize`, so a malformed value never
//! reaches application code and the failure is exactly what the design calls a malformed
//! invocation: the command "cannot validly execute", and Tauri rejects it with the framework's
//! own error rather than resolving with a Result.
//!
//! # What a panic does here, honestly
//!
//! `[profile.release] panic = "abort"` means a release build never unwinds: a panic inside the
//! blocking task aborts the process, and no rejection is delivered. That is the design's
//! intent — "a process-aborting panic remains a crash rather than a false application
//! response" — and it is why the `JoinError` arm below is reachable only in `dev` and `test`,
//! where tokio can catch the unwind. These entrypoints catch and redact unexpected failures
//! "where the runtime can safely unwind", and nowhere else; do not read the arm as a panic
//! barrier a shipped binary has.

use std::sync::Arc;

use serde::Serialize;

use crate::application::{
    AnalysisFailure, DocumentationEntries, DocumentationHost, DocumentationPublished,
    PublishDocumentationError, PublishedDurability, ReadEntryListError, StorageFailure,
    UnavailableReason,
};
use crate::canonical::path::LogicalPath;
use crate::discovery::identity::{DiscoveryLocationId, ModInstallationId};
use crate::error::{OpResult, Unexpected};
use crate::transport::envelope::{Envelope, Rejection, resolve};

/// What a completed build reports.
///
/// **No revision identifier.** The documentation-client interface "does not expose a generic
/// query language, revision identifier, bundle path, JSON filename, absolute source path, or
/// arbitrary asset key", and React has nothing to do with one. What is here is the single fact
/// a workflow is told to surface: whether the commit's durability is confirmed.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BuildReport {
    pub durability: Durability,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Durability {
    Confirmed,
    /// Published, on a volume that provides no way to prove the bundle's directory entry
    /// reached disk (D-123).
    BundleEntryNotFlushedByPlatform,
}

/// Every expected outcome of the build command, flat and discriminated.
///
/// Four of the design's six build variants are absent because nothing in this phase can
/// produce them; see [`PublishDocumentationError`], which records which and why.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "reason")]
pub enum BuildRefusal {
    BuildInProgress {
        installation: ModInstallationId,
    },
    InstallationUnavailable {
        installation: ModInstallationId,
    },
    AnalysisFailed {
        installation: ModInstallationId,
        cause: AnalysisCause,
    },
    StorageUnavailable {
        installation: ModInstallationId,
        cause: StorageCause,
    },
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AnalysisCause {
    NoAnalysisInThisBuild,
    NothingDocumentedForThisInstallation,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StorageCause {
    RevisionNotStaged,
    RevisionNotCompleted,
    RevisionDurabilityUnconfirmed,
    RevisionsLocationUnusable,
    StateNotWritten,
    StateRecoveryRequired,
}

/// One published revision's entries.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EntryListView {
    pub installation: ModInstallationId,
    pub entries: Vec<EntryView>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
    pub category: String,
    pub identifier: String,
    /// Absent means no localized name was resolved, which is not a resolved empty one. It
    /// serializes as `null` rather than being skipped, so the property is always present.
    pub display_name: Option<String>,
}

/// Every expected outcome of the read command.
///
/// The application layer's two-level shape — "nothing published" beside "published and
/// unavailable, for this reason" — is flattened to one discriminated union on the wire,
/// because a decoder switching on one `reason` is what the frontend actually needs. Nothing is
/// lost: the pairing is still one-to-one.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "reason")]
pub enum ReadRefusal {
    NoPublishedRevision,
    ReferenceUnreadable,
    RevisionMissing,
    RevisionFromAnotherBuild { found: u32, supported: u32 },
    RevisionDamaged,
    RevisionDisplaced,
    RevisionCarriesNoEntryList,
    DocumentUnreadable,
    DocumentChanged,
    DocumentUndecodable,
}

/// Builds and publishes documentation for one Mod Installation.
///
/// **Named for what this phase does.** Phase 9 replaces it with the design's two
/// intention-revealing workflows, Ensure Documentation and Rebuild Documentation; this one has
/// neither's cache policy, so calling it either now would be a name that lies.
///
/// It takes the Discovery Location and the mod's path within it rather than a Mod Installation
/// identifier, because the identifier is *derived* from that pair and the derivation is
/// one-way — see [`BuildTarget`](crate::application::BuildTarget). Phase 9's discovery-backed
/// resolution takes an installation identifier and looks the pair up; nothing else about the
/// command changes.
#[tauri::command]
pub async fn build_documentation(
    host: ::tauri::State<'_, Arc<DocumentationHost>>,
    location_id: DiscoveryLocationId,
    mod_root: LogicalPath,
) -> Result<Envelope<BuildReport, BuildRefusal>, Rejection> {
    // Cloned before the spawn: `State<'_, T>` borrows the message being handled and cannot
    // cross into a `'static` task.
    let host = Arc::clone(&host);
    let outcome = on_blocking_worker(move || {
        host.publish_documentation(&crate::application::BuildTarget::derive(
            location_id,
            mod_root,
        ))
    })
    .await;
    resolve(outcome.map(build_report).map_err(project(build_refusal)))
}

/// Serves the entries of the revision currently published for one Mod Installation.
#[tauri::command]
pub async fn get_entry_list(
    host: ::tauri::State<'_, Arc<DocumentationHost>>,
    installation: ModInstallationId,
) -> Result<Envelope<EntryListView, ReadRefusal>, Rejection> {
    let host = Arc::clone(&host);
    // Also on a blocking worker: opening a revision re-hashes every entry the manifest names,
    // and a synchronous command would run that on the thread the async runtime needs.
    let outcome = on_blocking_worker(move || host.entry_list(installation)).await;
    resolve(outcome.map(entry_list_view).map_err(project(read_refusal)))
}

/// Runs blocking work off the async runtime, turning a task that did not complete into an
/// unexpected failure so it reaches the one redaction path with everything else.
async fn on_blocking_worker<T, E>(
    work: impl FnOnce() -> OpResult<T, E> + Send + 'static,
) -> OpResult<T, E>
where
    T: Send + 'static,
    E: Send + 'static,
{
    ::tauri::async_runtime::spawn_blocking(work)
        .await
        .unwrap_or_else(|error| {
            Err(Unexpected::new(format!("a command's work did not complete: {error}")).into())
        })
}

/// Applies a projection to the expected half of a failure and passes the unexpected half
/// through untouched — the half [`resolve`] redacts.
fn project<E, D>(
    projection: impl FnOnce(E) -> D,
) -> impl FnOnce(crate::error::Failure<E>) -> crate::error::Failure<D> {
    move |failure| match failure {
        crate::error::Failure::Expected(expected) => {
            crate::error::Failure::Expected(projection(expected))
        }
        crate::error::Failure::Unexpected(unexpected) => {
            crate::error::Failure::Unexpected(unexpected)
        }
    }
}

fn build_report(published: DocumentationPublished) -> BuildReport {
    BuildReport {
        durability: match published.durability {
            PublishedDurability::Confirmed => Durability::Confirmed,
            PublishedDurability::BundleEntryNotFlushedByPlatform => {
                Durability::BundleEntryNotFlushedByPlatform
            }
        },
    }
}

fn build_refusal(error: PublishDocumentationError) -> BuildRefusal {
    match error {
        PublishDocumentationError::BuildInProgress { installation } => {
            BuildRefusal::BuildInProgress { installation }
        }
        PublishDocumentationError::InstallationUnavailable { installation } => {
            BuildRefusal::InstallationUnavailable { installation }
        }
        PublishDocumentationError::AnalysisFailed {
            installation,
            reason,
        } => BuildRefusal::AnalysisFailed {
            installation,
            cause: match reason {
                AnalysisFailure::NoAnalysisInThisBuild => AnalysisCause::NoAnalysisInThisBuild,
                AnalysisFailure::NothingDocumentedForThisInstallation => {
                    AnalysisCause::NothingDocumentedForThisInstallation
                }
            },
        },
        PublishDocumentationError::StorageUnavailable {
            installation,
            reason,
        } => BuildRefusal::StorageUnavailable {
            installation,
            cause: match reason {
                StorageFailure::RevisionNotStaged => StorageCause::RevisionNotStaged,
                StorageFailure::RevisionNotCompleted => StorageCause::RevisionNotCompleted,
                StorageFailure::RevisionDurabilityUnconfirmed => {
                    StorageCause::RevisionDurabilityUnconfirmed
                }
                StorageFailure::RevisionsLocationUnusable => {
                    StorageCause::RevisionsLocationUnusable
                }
                StorageFailure::StateNotWritten => StorageCause::StateNotWritten,
                StorageFailure::StateRecoveryRequired => StorageCause::StateRecoveryRequired,
            },
        },
    }
}

fn entry_list_view(entries: DocumentationEntries) -> EntryListView {
    EntryListView {
        installation: entries.installation,
        entries: entries
            .entries
            .into_iter()
            .map(|entry| EntryView {
                category: entry.category,
                identifier: entry.identifier,
                display_name: entry.display_name,
            })
            .collect(),
    }
}

fn read_refusal(error: ReadEntryListError) -> ReadRefusal {
    match error {
        ReadEntryListError::NoPublishedRevision => ReadRefusal::NoPublishedRevision,
        ReadEntryListError::DocumentationUnavailable { reason } => match reason {
            UnavailableReason::ReferenceUnreadable => ReadRefusal::ReferenceUnreadable,
            UnavailableReason::RevisionMissing => ReadRefusal::RevisionMissing,
            UnavailableReason::RevisionFromAnotherBuild { found, supported } => {
                ReadRefusal::RevisionFromAnotherBuild { found, supported }
            }
            UnavailableReason::RevisionDamaged => ReadRefusal::RevisionDamaged,
            UnavailableReason::RevisionDisplaced => ReadRefusal::RevisionDisplaced,
            UnavailableReason::RevisionCarriesNoEntryList => {
                ReadRefusal::RevisionCarriesNoEntryList
            }
            UnavailableReason::DocumentUnreadable => ReadRefusal::DocumentUnreadable,
            UnavailableReason::DocumentChanged => ReadRefusal::DocumentChanged,
            UnavailableReason::DocumentUndecodable => ReadRefusal::DocumentUndecodable,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::RevisionId;
    use serde_json::{Value, json};

    /// The commands themselves are not invoked: this crate has no Tauri mock runtime, and the
    /// behaviour under test is entirely in the projections and the envelope. What exercises a
    /// real command is the acceptance harness and a run of the app.
    fn value_of(serializable: &impl Serialize) -> Value {
        serde_json::to_value(serializable).unwrap()
    }

    fn installation() -> ModInstallationId {
        ModInstallationId::parse(&"ab".repeat(32)).unwrap()
    }

    #[test]
    fn a_completed_build_reports_its_durability_as_an_explicit_object() {
        let report = build_report(DocumentationPublished {
            installation: installation(),
            revision: RevisionId("cd".repeat(32)),
            durability: PublishedDurability::BundleEntryNotFlushedByPlatform,
        });

        assert_eq!(
            value_of(&report),
            json!({ "durability": "bundleEntryNotFlushedByPlatform" })
        );
    }

    #[test]
    fn a_completed_build_never_reports_the_revision_it_published() {
        // The identifier is in hand at this boundary and stops here: the documentation-client
        // interface is forbidden one, and nothing downstream has a use for it.
        let revision = "cd".repeat(32);
        let report = build_report(DocumentationPublished {
            installation: installation(),
            revision: RevisionId(revision.clone()),
            durability: PublishedDurability::Confirmed,
        });

        assert!(!serde_json::to_string(&report).unwrap().contains(&revision));
    }

    #[test]
    fn every_build_refusal_serializes_with_its_own_discriminant() {
        let rows = [
            (
                PublishDocumentationError::BuildInProgress {
                    installation: installation(),
                },
                json!({ "reason": "buildInProgress", "installation": installation().to_string() }),
            ),
            (
                PublishDocumentationError::InstallationUnavailable {
                    installation: installation(),
                },
                json!({
                    "reason": "installationUnavailable",
                    "installation": installation().to_string()
                }),
            ),
            (
                PublishDocumentationError::AnalysisFailed {
                    installation: installation(),
                    reason: AnalysisFailure::NoAnalysisInThisBuild,
                },
                json!({
                    "reason": "analysisFailed",
                    "installation": installation().to_string(),
                    "cause": "noAnalysisInThisBuild"
                }),
            ),
            (
                PublishDocumentationError::StorageUnavailable {
                    installation: installation(),
                    reason: StorageFailure::RevisionDurabilityUnconfirmed,
                },
                json!({
                    "reason": "storageUnavailable",
                    "installation": installation().to_string(),
                    "cause": "revisionDurabilityUnconfirmed"
                }),
            ),
        ];

        for (error, expected) in rows {
            assert_eq!(value_of(&build_refusal(error)), expected);
        }
    }

    #[test]
    fn every_read_refusal_serializes_with_its_own_discriminant() {
        let rows = [
            (
                ReadEntryListError::NoPublishedRevision,
                "noPublishedRevision",
            ),
            (
                ReadEntryListError::DocumentationUnavailable {
                    reason: UnavailableReason::ReferenceUnreadable,
                },
                "referenceUnreadable",
            ),
            (
                ReadEntryListError::DocumentationUnavailable {
                    reason: UnavailableReason::RevisionMissing,
                },
                "revisionMissing",
            ),
            (
                ReadEntryListError::DocumentationUnavailable {
                    reason: UnavailableReason::RevisionDamaged,
                },
                "revisionDamaged",
            ),
            (
                ReadEntryListError::DocumentationUnavailable {
                    reason: UnavailableReason::RevisionDisplaced,
                },
                "revisionDisplaced",
            ),
            (
                ReadEntryListError::DocumentationUnavailable {
                    reason: UnavailableReason::RevisionCarriesNoEntryList,
                },
                "revisionCarriesNoEntryList",
            ),
            (
                ReadEntryListError::DocumentationUnavailable {
                    reason: UnavailableReason::DocumentUnreadable,
                },
                "documentUnreadable",
            ),
            (
                ReadEntryListError::DocumentationUnavailable {
                    reason: UnavailableReason::DocumentChanged,
                },
                "documentChanged",
            ),
            (
                ReadEntryListError::DocumentationUnavailable {
                    reason: UnavailableReason::DocumentUndecodable,
                },
                "documentUndecodable",
            ),
        ];

        for (error, discriminant) in rows {
            assert_eq!(
                value_of(&read_refusal(error))["reason"],
                json!(discriminant)
            );
        }

        assert_eq!(
            value_of(&read_refusal(
                ReadEntryListError::DocumentationUnavailable {
                    reason: UnavailableReason::RevisionFromAnotherBuild {
                        found: 7,
                        supported: 1
                    }
                }
            )),
            json!({ "reason": "revisionFromAnotherBuild", "found": 7, "supported": 1 })
        );
    }

    #[test]
    fn an_entry_without_a_localized_name_keeps_the_property_as_null() {
        let view = entry_list_view(DocumentationEntries {
            installation: installation(),
            entries: vec![crate::application::DocumentationEntry {
                category: "technology".to_owned(),
                identifier: "tech_a".to_owned(),
                display_name: None,
            }],
        });

        let encoded = value_of(&view);
        assert_eq!(
            encoded,
            json!({
                "installation": installation().to_string(),
                "entries": [{
                    "category": "technology",
                    "identifier": "tech_a",
                    "displayName": null
                }]
            })
        );
        assert!(
            encoded["entries"][0]
                .as_object()
                .unwrap()
                .contains_key("displayName")
        );
    }

    #[test]
    fn a_revision_documenting_nothing_is_an_empty_list_rather_than_a_refusal() {
        // The empty-versus-absent line, held at the wire: `entries: []` is a success, and the
        // revision that carries no entry-list document at all is a different answer.
        let view = entry_list_view(DocumentationEntries {
            installation: installation(),
            entries: Vec::new(),
        });

        assert_eq!(value_of(&view)["entries"], json!([]));
        assert_eq!(
            value_of(&read_refusal(
                ReadEntryListError::DocumentationUnavailable {
                    reason: UnavailableReason::RevisionCarriesNoEntryList
                }
            ))["reason"],
            json!("revisionCarriesNoEntryList")
        );
    }
}
