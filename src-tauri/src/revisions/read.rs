//! Opening a published Documentation Revision, and the typed reader that results
//! (docs/technical-design.md, "Revision reader"; docs/decision-log.md, D-087, D-124).
//!
//! [`RevisionStore::open_revision`] is the whole of the read side's entrance: it parses the
//! stored publication token, pins the revision against retention, validates the directory
//! through the same path the publication protocol uses, and hands back a
//! [`RevisionReader`]. Nothing it returns lets a caller reach a bundle file: no bundle root,
//! no absolute path, no accessor keyed by name. A refusal may still *name* a damaged
//! bundle-relative entry, which is diagnosis rather than addressing — see the parent
//! module's "The layout is hidden behind that" for where that line falls and why.
//!
//! # What an open reader proves, and what it does not
//!
//! It proves the bundle was *intact* at open: the manifest was read back from disk, its
//! identifier re-derived from its own body, every required entry re-hashed, and the
//! directory confirmed to be this revision's. That is [`validate_as`], the publication
//! protocol's own validator, reused rather than restated — a second implementation would be
//! a second authority on what a bundle is,
//! and the two would diverge in the direction that matters (D-118).
//!
//! It does not prove any document *decodes*. Validation hashes bytes and never parses them
//! (docs/technical-design.md:651), which is the revision-bundle spike's actual defect and
//! the standing hole `stage.rs::a_bundle_whose_entry_list_is_undecodable_still_validates`
//! pins. So every document accessor is fallible, and [`DocumentUnavailable`] is where that
//! question is finally answered — at the accessor rather than at open, because a revision
//! is 712–1,475 documents plus a search index from Phase 6 on, and a companion request
//! opens a reader per request (docs/technical-design.md, "Authorized revision access").
//!
//! # Three answers, decided once
//!
//! Every accessor returns `Result<Option<T>, DocumentUnavailable>`, and the manifest's
//! required-entry map is what decides which: `None` means this revision carries no such
//! document at all, `Ok(Some(empty))` means it carries one that documents nothing, and an
//! error means it names one this build cannot produce. Phase 7's per-language search index
//! legitimately may not have been materialized (docs/technical-design.md, "Host-owned
//! search"), so all three answers are load-bearing for every later accessor, not just for
//! this one. Deciding the distinction here, against `required_entries`, is what keeps them
//! one convention instead of several.

use std::fmt;
use std::fs;
use std::path::PathBuf;

use crate::revisions::RevisionStore;
use crate::revisions::candidate::{DocumentIdentity, EntryList};
use crate::revisions::manifest::{BundleManifest, IdentityParseError, RevisionIdentity};
use crate::revisions::pin::{BundlePin, RetirementClaim, RetirementRefused};
use crate::revisions::stage::{
    BundleUnusable, RevisionMismatch, SchemaMismatch, ValidationReport, bundle_path, document_file,
    document_path, validate_as,
};
use crate::source::fingerprint::ContentHash;
use crate::state::RevisionId;

/// A pinned, validated view of one published Documentation Revision.
///
/// Constructed only by [`RevisionStore::open_revision`]. Private fields and no public
/// constructor are what make "`revisions` is the sole owner of Documentation Revision
/// bundle I/O" true by construction rather than by discipline (D-087): a caller cannot
/// assemble one around a directory nothing validated.
///
/// **Owned and free of borrows, deliberately.** It is `Send + Sync + 'static`, which
/// `publish.rs::a_revision_reader_is_send_and_sync_and_owns_no_borrow` enforces rather than
/// asserts in prose. Phase 11's companion and desktop revision handles wrap *this* value
/// after their different access policies have been resolved (docs/technical-design.md,
/// "Authorized revision access"), and a companion request is served on a worker thread with
/// no access to the frame that opened it — so a `RevisionReader<'a>` borrowing the store
/// could not reach the code that reads it, and the lifetime would infect both handle types
/// and everything returning through them. Borrowing is the right shape for
/// [`PublicationCapability`](crate::state::PublicationCapability), which lives and dies
/// inside one call; it is the wrong shape here.
///
/// **It holds no operating-system handle.** Every read is read-and-close, so a reader never
/// refuses a rename or a delete with a Windows sharing violation (see the platform caveat
/// in this module's parent). What it holds is the pin, and what the pin blocks is retention.
pub struct RevisionReader {
    /// The whole of what keeps this bundle out of retention's reach, released on drop.
    pin: BundlePin,
    /// Never exposed: no accessor returns it, no error carries it, and [`fmt::Debug`] is
    /// hand-written to omit it (D-087, D-090).
    root: PathBuf,
    /// The manifest this open validated, taken from the proof rather than re-read: a
    /// second read could observe different bytes than the ones that were hashed.
    manifest: BundleManifest,
}

impl RevisionReader {
    /// Which revision this reader serves: the one it holds a pin on, which [`validate_as`]
    /// proved is the one this bundle's own manifest carries.
    ///
    /// Read from the pin rather than from the manifest, so the value a caller sees is the
    /// same one retention is being held off. The two agree on every constructible reader —
    /// refusing a directory whose manifest names a different revision is the whole reason
    /// `validate_as` exists rather than `validate` — and a directory's *name* is not
    /// consulted by either.
    pub fn revision(&self) -> RevisionIdentity {
        self.pin.revision()
    }

    /// This revision's entry list, or `None` when the revision carries no such document.
    ///
    /// `None` and an empty list are different answers, and the manifest tells them apart:
    /// a revision whose manifest names no entry-list entry carries no such document at all,
    /// while `Some(EntryList { entries: [] })` is a revision that documents nothing.
    /// `stage.rs::a_freshly_staged_bundle_validates` already produces the first shape, so it
    /// is a state of this format rather than a hypothetical.
    ///
    /// Returned owned rather than borrowed. A borrow would force the reader to cache, and
    /// caching decided here would fix the storage strategy the design deliberately keeps
    /// replaceable — "replacing bundle internals with SQLite would require a new Revision
    /// Reader implementation but would not change ... application read use cases"
    /// (docs/technical-design.md, "Revision reader"). Bounded caching belongs to the
    /// application layer, where the design already homes it.
    pub fn entry_list(&self) -> Result<Option<EntryList>, DocumentUnavailable> {
        self.document(DocumentIdentity::EntryList)
    }

    /// The one body every accessor shares: is it named, can it be read, are the bytes the
    /// manifest's, and does it decode.
    ///
    /// The bytes are re-hashed against the manifest even though the open already did so.
    /// With reads deferred to the accessor the open-time proof is genuinely stale by now,
    /// and this is what gives [`DocumentUnavailable::Changed`] an honest home distinct from
    /// [`Undecodable`](DocumentUnavailable::Undecodable): "somebody edited an immutable
    /// artifact" and "this build cannot interpret its own format" send a person to
    /// different places. It re-uses `ContentHash::of` against the manifest's own map, so it
    /// is the same rule applied again rather than a second one.
    fn document<T: serde::de::DeserializeOwned>(
        &self,
        identity: DocumentIdentity,
    ) -> Result<Option<T>, DocumentUnavailable> {
        let logical = document_path(identity);
        let Some(expected) = self.manifest.required_entries().get(&logical) else {
            return Ok(None);
        };
        let bytes = fs::read(document_file(&self.root, identity)).map_err(|error| {
            DocumentUnavailable::Unreadable {
                document: identity,
                detail: error.to_string(),
            }
        })?;
        if ContentHash::of(&bytes) != *expected {
            return Err(DocumentUnavailable::Changed { document: identity });
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| DocumentUnavailable::Undecodable {
                document: identity,
                detail: error.to_string(),
            })
    }
}

/// Names the revision and nothing else. A derived `Debug` would print `root`, which is the
/// one thing this type exists not to expose — and `Debug` reaches logs, so it is as much a
/// boundary as `Display`. The precedent is `source::snapshot::SourceBytes`.
impl fmt::Debug for RevisionReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RevisionReader({})", self.revision())
    }
}

/// Why a document this revision names could not be produced.
///
/// Separate from [`OpenRevisionError`] because the questions differ: opening asks whether a
/// directory is this revision's bundle, and this asks whether one entry inside a bundle
/// already proven intact can be turned into a value. Folding them would give the open path
/// variants it cannot produce and every accessor variants it cannot produce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentUnavailable {
    /// The entry the manifest names could not be read now. The bundle was intact at open
    /// and a pinned bundle is not retired, so this means something outside this process
    /// changed an artifact the application treats as immutable.
    Unreadable {
        document: DocumentIdentity,
        detail: String,
    },
    /// The bytes are not the bytes the manifest names.
    Changed { document: DocumentIdentity },
    /// Hash-valid, and not a value this build can decode. The failure manifest validation
    /// structurally cannot catch, and the reason every persisted revision type carries an
    /// absent-field round-trip test (docs/technical-design.md:651, 1059).
    Undecodable {
        document: DocumentIdentity,
        detail: String,
    },
}

impl fmt::Display for DocumentUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { document, detail } => {
                write!(f, "this revision's {document} could not be read: {detail}")
            }
            Self::Changed { document } => write!(
                f,
                "this revision's {document} does not hold the bytes its manifest names, so \
                 it was changed after the revision was published"
            ),
            Self::Undecodable { document, detail } => write!(
                f,
                "this revision's {document} is intact and could not be understood by this \
                 build, so the revision must be rebuilt: {detail}"
            ),
        }
    }
}

impl std::error::Error for DocumentUnavailable {}

/// Every way opening a published revision can fail to produce a reader. Each variant names
/// what is true of the revisions location, and none carries a filesystem path.
///
/// **No variant restates which revision was asked for.** The caller supplied the token and
/// still holds it, so carrying the identity back would be a second copy of a fact nobody
/// reads. [`AnotherRevision`] is the exception that proves the rule: the revision it names
/// is one the caller did *not* ask for, and is the only way to learn it.
///
/// It is also what keeps this union small enough not to trip `clippy::result_large_err`,
/// whose threshold `Damaged`'s [`ValidationReport`] alone sits just under. That is a
/// consequence and not the reason — the lint measures this variant against a fixed bound and
/// says nothing about what a successful open costs, since the `Ok` side is the larger of the
/// two either way.
///
/// [`AnotherRevision`]: OpenRevisionError::AnotherRevision
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenRevisionError {
    /// The stored publication reference is not a revision identifier. `state` preserves a
    /// token it never parsed, so a previous build or a hand edit may have left anything
    /// there; the refusal happens before any path is built from it.
    UnreadableReference(IdentityParseError),
    /// No directory holds this revision. Distinct from damage, because there is nothing to
    /// diagnose and the answer is to build: absence is the expected state before a first
    /// build, and a rebuild republishes to this same path, the identifier being a pure
    /// function of content.
    NotFound,
    /// A retirement claim is outstanding, so this revision is being removed. The caller's
    /// pointer is stale; it re-resolves rather than retries.
    Retired,
    /// The bundle was written by a different build. Lifted out of [`Damaged`] because the
    /// answer differs — rebuild rather than diagnose — which is the split D-118 records,
    /// and the desktop shows different copy for it. Total by construction: `stage::validate`
    /// returns as soon as it reads a schema it does not support, so a `Damaged` report can
    /// never also carry a schema mismatch.
    ///
    /// [`Damaged`]: OpenRevisionError::Damaged
    IncompatibleSchema(SchemaMismatch),
    /// The directory disagrees with its own manifest.
    ///
    /// **The whole report travels**, rather than a summary. Phase 9's Validate Published
    /// Revision is read-only diagnostics over exactly these findings
    /// (docs/implementation-plan.md, Phase 9 item 2), and a reader that flattened them would
    /// force that use case to open a second, weaker path to get them back — the second
    /// authority D-118 forbids.
    Damaged(ValidationReport),
    /// A complete bundle of another revision occupies this revision's path. Not damage: the
    /// directory disagrees with nothing, and is simply not what was asked for.
    AnotherRevision(RevisionMismatch),
}

impl fmt::Display for OpenRevisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnreadableReference(error) => write!(
                f,
                "the published revision recorded for this mod is not a revision \
                 identifier, so no documentation can be opened from it: {error}"
            ),
            Self::NotFound => f.write_str(
                "no bundle holds this revision, so its documentation must be built again",
            ),
            Self::Retired => f.write_str(
                "this revision is being removed, so a newer one must be resolved instead",
            ),
            Self::IncompatibleSchema(mismatch) => write!(
                f,
                "this revision was written by a different build (bundle schema version {} \
                 is not {}), so it must be rebuilt",
                mismatch.found, mismatch.supported
            ),
            Self::Damaged(report) => write!(f, "this revision cannot be read: {report}"),
            Self::AnotherRevision(mismatch) => write!(
                f,
                "the bundle where this revision should be holds revision {} instead, so \
                 this revision cannot be read from it",
                mismatch.found
            ),
        }
    }
}

impl std::error::Error for OpenRevisionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnreadableReference(error) => Some(error),
            // Exposed as a source rather than flattened into one line — the choice
            // `BundleRefusal::NotABundle` already makes, and what keeps Phase 9's
            // diagnostics reading the findings rather than a rendering of them.
            Self::Damaged(report) => Some(report),
            // Nothing wraps a cause here: the finding is the message, and the values a
            // reader needs are already in it.
            Self::NotFound
            | Self::Retired
            | Self::IncompatibleSchema(_)
            | Self::AnotherRevision(_) => None,
        }
    }
}

impl RevisionStore {
    /// Opens the published bundle `published` names, pins it against retention, and returns
    /// a typed reader.
    ///
    /// **Takes the stored token, not a parsed identifier.** The caller arrives holding a
    /// `PublicationReference` it read from `state`, and "is this token a revision
    /// identifier" is a question this module owns — `state` deliberately preserves a token
    /// it never parsed. Making the caller parse would move that refusal outside the module
    /// that defines the identifier, and would make `companion` import
    /// [`IdentityParseError`] to handle what is really a state-corruption diagnostic.
    ///
    /// **It does not resolve "the current revision", by design.** `revisions` never reads
    /// the publication pointer — the same rule [`publish`](RevisionStore::publish) states
    /// for `expected_prior` — and choosing which published revision a caller is entitled to
    /// belongs to the desktop read use case and to the companion access module (D-063).
    /// That is also what makes "a read that began before a publication continues against the
    /// revision it began on, and one that begins afterward uses the new revision" a property
    /// of this signature rather than of a policy hidden inside it.
    ///
    /// **It does not take the publication lock.** Documentation reads and Validate Published
    /// Revision stay available while a build publishes (docs/technical-design.md, "Build
    /// concurrency"), so this contends only on the pin registry, and only for the duration
    /// of a map update.
    ///
    /// The revision is pinned **before** the directory is validated. Pinning performs no
    /// I/O, and doing it second would leave a window in which a retirement claim could be
    /// granted between proving the bundle intact and taking a hold on it.
    pub fn open_revision(
        &self,
        published: &RevisionId,
    ) -> Result<RevisionReader, OpenRevisionError> {
        let revision = RevisionIdentity::try_from(published)
            .map_err(OpenRevisionError::UnreadableReference)?;
        let pin = self
            .pins
            .pin(revision)
            .map_err(|_| OpenRevisionError::Retired)?;

        // The pin is live from here on, so every path out of this function releases it:
        // `?` drops it, and success moves it into the reader.
        let path = bundle_path(&self.root, revision);
        // `try_exists` and not `exists`: a stat that failed for a permission or device
        // reason is not evidence of absence, and reporting it as `NotFound` would answer
        // "build it again" to a condition that needs diagnosing. Only a definite `Ok(false)`
        // is absence; everything else falls through to validation, which fails closed. The
        // rule `publish::observe_final_path` already applies.
        if matches!(path.try_exists(), Ok(false)) {
            return Err(OpenRevisionError::NotFound);
        }
        // Total over `BundleUnusable`, and that type is the reason this is only two arms.
        // Adoption's extra finding — intact, with a stray file beside it — bars publishing
        // and not reading (D-119), so it is not in this function's range at all. Were it,
        // the only way to handle an arm that cannot occur would be to invent a value for
        // it, and the value would be a `ValidationReport` whose own `is_valid` says the
        // bundle validated, inside an error saying it did not.
        let validated = validate_as(&path, revision).map_err(|unusable| match unusable {
            BundleUnusable::NotABundle(report) => match report.schema_mismatch {
                Some(mismatch) => OpenRevisionError::IncompatibleSchema(mismatch),
                None => OpenRevisionError::Damaged(report),
            },
            BundleUnusable::AnotherRevision(mismatch) => {
                OpenRevisionError::AnotherRevision(mismatch)
            }
        })?;

        Ok(RevisionReader {
            pin,
            root: validated.path().to_path_buf(),
            manifest: validated.into_manifest(),
        })
    }

    /// Claims one revision for deletion, refusing while any reader holds it.
    ///
    /// The primitive Phase 9's retention sweep drives (docs/implementation-plan.md, Phase 9
    /// item 4). Deliberately **not** `fn is_pinned(&self, _) -> bool`: check-then-delete
    /// leaves a window exactly one [`open_revision`](RevisionStore::open_revision) wide, and
    /// in that window the deletion would *succeed* — on Windows too, precisely because this
    /// module holds no operating-system handle — leaving a live reader serving from a
    /// directory that is going away. The claim closes the window from both sides: while it
    /// is outstanding, opening that revision returns [`OpenRevisionError::Retired`].
    ///
    /// **This ticket ships the claim and no sweep**, and the two remaining halves are not
    /// oversights. Nothing about `staging/` belongs here: that sweep's precondition — state
    /// recovery resolved, and every retained manifest having contributed to a complete live
    /// set (docs/technical-design.md, "Revision retention") — names facts `revisions` cannot
    /// observe. Nor does this decide *retirement eligibility*, whose second condition this
    /// module also cannot see: the previous bundle becomes eligible only after the
    /// replacement pointer is observed committed (D-090). It answers the half `revisions`
    /// owns, and its name says which half.
    ///
    /// # What a claim does not exclude, and what Phase 9 must therefore decide
    ///
    /// **A publication.** [`publish`](RevisionStore::publish) never consults the pin
    /// registry, so a rebuild whose candidate re-derives a claimed revision's identifier —
    /// reachable when a user reverts mod content — observes a complete bundle at the final
    /// path, adopts it, and commits the pointer to a directory a sweep is deleting. The
    /// exclusion here is claim-against-reader, not claim-against-writer. Publication is
    /// never blocked by a reader, and it is not thereby safe against a concurrent deletion.
    ///
    /// **A half-deleted directory.** Abandoning a claim is safe for the registry, which
    /// holds a permission and not a record of work; it is weaker on disk. A sweep that stops
    /// partway leaves a corpse the next open classifies as [`Damaged`] — diagnose this —
    /// when the truthful answer is [`NotFound`] — build it again. Deletion order, or a
    /// tombstone, is Phase 9's to choose.
    ///
    /// [`Damaged`]: OpenRevisionError::Damaged
    /// [`NotFound`]: OpenRevisionError::NotFound
    pub fn claim_for_retirement(
        &self,
        revision: RevisionIdentity,
    ) -> Result<RetirementClaim, RetirementRefused> {
        self.pins.claim(revision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::version::AnalysisVersionVector;
    use crate::canonical::path::LogicalPath;
    use crate::discovery::identity::{DiscoveryLocationId, ModInstallationId};
    use crate::revisions::candidate::{
        EntrySummary, RevisionCandidate, RevisionDocument, RevisionInputs,
    };
    use crate::revisions::publish::RealPublicationIo;
    use crate::revisions::stage::{MANIFEST_FILE, bundles_root, stage_bundle};
    use crate::source::ObservationGaps;
    use crate::source::fingerprint::SourceFingerprint;
    use std::path::Path;
    use tempfile::TempDir;

    /// A revisions root under a directory whose name is a token no path-free rendering may
    /// contain. Named rather than random so
    /// `no_reader_value_and_no_open_error_carries_a_filesystem_path` has something to look
    /// for.
    const ROOT_TOKEN: &str = "a-directory-name-no-message-may-contain";

    struct Fixture {
        _dir: TempDir,
        root: PathBuf,
        store: RevisionStore,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            let root = dir.path().join(ROOT_TOKEN).join("revisions");
            let store = RevisionStore::open(&root).unwrap();
            Self {
                _dir: dir,
                root,
                store,
            }
        }

        /// Publishes without the publication protocol: stages a bundle for real and moves it
        /// to its canonical path. Everything below is about *reading* a bundle, and pulling
        /// a `StateStore` into each row to reach the same bytes would make these tests
        /// depend on a module they say nothing about. `publish.rs` owns the end-to-end
        /// claim that the protocol produces what this reads.
        fn plant(&self, candidate: &RevisionCandidate) -> RevisionIdentity {
            let staged = stage_bundle(&mut RealPublicationIo, &self.root, candidate).unwrap();
            let revision = staged.identity();
            fs::rename(staged.root(), bundle_path(&self.root, revision)).unwrap();
            revision
        }

        fn open(&self, revision: RevisionIdentity) -> Result<RevisionReader, OpenRevisionError> {
            self.store.open_revision(&revision.into())
        }

        fn bundle(&self, revision: RevisionIdentity) -> PathBuf {
            bundle_path(&self.root, revision)
        }
    }

    fn installation() -> ModInstallationId {
        ModInstallationId::derive(
            DiscoveryLocationId::generate(),
            &LogicalPath::parse("ugc_1").unwrap(),
        )
    }

    fn fingerprint(bytes: &[u8]) -> SourceFingerprint {
        SourceFingerprint::of(
            [(
                LogicalPath::parse("descriptor.mod").unwrap(),
                ContentHash::of(bytes),
            )],
            &ObservationGaps::default(),
        )
        .unwrap()
    }

    fn candidate(documents: Vec<RevisionDocument>) -> RevisionCandidate {
        RevisionCandidate::new(
            installation(),
            RevisionInputs {
                target_mod: fingerprint(b"name=\"Fixture\"\n"),
                vanilla_content: fingerprint(b"name=\"Stellaris\"\n"),
            },
            AnalysisVersionVector::current(),
            true,
            documents,
        )
        .unwrap()
    }

    fn with_entries(entries: Vec<EntrySummary>) -> RevisionCandidate {
        candidate(vec![RevisionDocument::EntryList(EntryList { entries })])
    }

    fn one_entry() -> Vec<EntrySummary> {
        vec![EntrySummary {
            category: "technology".to_owned(),
            identifier: "tech_a".to_owned(),
            display_name: Some("Technology A".to_owned()),
        }]
    }

    fn edit_manifest(bundle: &Path, from: &str, to: &str) {
        let text = fs::read_to_string(bundle.join(MANIFEST_FILE)).unwrap();
        assert!(text.contains(from), "manifest does not contain {from}");
        fs::write(bundle.join(MANIFEST_FILE), text.replace(from, to)).unwrap();
    }

    #[test]
    fn a_published_bundle_opens_as_a_reader_that_serves_its_entry_list() {
        // The positive control every refusal below rests on: without it, an
        // `open_revision` that refused unconditionally would satisfy all of them.
        let fixture = Fixture::new();
        let revision = fixture.plant(&with_entries(one_entry()));

        let reader = fixture.open(revision).unwrap();

        assert_eq!(reader.revision(), revision);
        assert_eq!(
            reader.entry_list().unwrap(),
            Some(EntryList {
                entries: one_entry()
            })
        );
    }

    #[test]
    fn a_revision_carrying_no_entry_list_document_reports_absence_rather_than_an_empty_list() {
        // Empty and absent are different answers, and conflating them would make "this
        // revision documents nothing" indistinguishable from "this revision has no such
        // document" — the distinction Phase 7 needs for a language whose search index was
        // never materialized. A bundle with no documents is a state this format already
        // produces (`stage.rs::a_freshly_staged_bundle_validates`), not a hypothetical.
        let fixture = Fixture::new();
        let carries_none = fixture.plant(&candidate(Vec::new()));
        let documents_nothing = fixture.plant(&with_entries(Vec::new()));

        assert_eq!(
            fixture.open(carries_none).unwrap().entry_list().unwrap(),
            None
        );
        assert_eq!(
            fixture
                .open(documents_nothing)
                .unwrap()
                .entry_list()
                .unwrap(),
            Some(EntryList {
                entries: Vec::new()
            })
        );
    }

    #[test]
    fn an_entry_list_that_is_hash_valid_and_undecodable_is_a_typed_refusal() {
        // **The hole this ticket closes.** Built exactly as
        // `stage.rs::a_bundle_whose_entry_list_is_undecodable_still_validates` builds it:
        // valid JSON, correct hash, and an `EntrySummary` with no `identifier`, which has
        // no read-side default that could rescue it. Validation proves integrity and not
        // readability, so it passes such a bundle; the reader is what refuses it, and it
        // refuses with a typed outcome rather than a panic or a raw serde error.
        let fixture = Fixture::new();
        let staged_revision = fixture.plant(&with_entries(one_entry()));
        let staged_bundle = fixture.bundle(staged_revision);
        let entry = document_file(&staged_bundle, DocumentIdentity::EntryList);

        let payload = br#"{"entries":[{"category":"technology"}]}"#;
        let staged_hash = ContentHash::of(&fs::read(&entry).unwrap()).to_hex();
        fs::write(&entry, payload).unwrap();
        edit_manifest(
            &staged_bundle,
            &staged_hash,
            &ContentHash::of(payload).to_hex(),
        );
        // The manifest's identifier no longer matches its edited body, so restate it and
        // move the directory to the name that identifier gives: a published bundle is named
        // by its own manifest's identifier, and the subject here is the entry rather than
        // either of those checks. What lands is a bundle indistinguishable from one this
        // protocol wrote.
        let restated = BundleManifest::parse(&fs::read(staged_bundle.join(MANIFEST_FILE)).unwrap())
            .unwrap()
            .recomputed_identity();
        edit_manifest(
            &staged_bundle,
            &staged_revision.to_hex(),
            &restated.to_hex(),
        );
        fs::rename(&staged_bundle, fixture.bundle(restated)).unwrap();
        // Negative control for the whole row: the directory still validates, so the reader
        // is catching something validation genuinely lets through rather than refusing a
        // bundle that was already broken.
        assert!(validate_as(&fixture.bundle(restated), restated).is_ok());

        let reader = fixture.open(restated).unwrap();

        assert_eq!(
            reader.entry_list().unwrap_err(),
            DocumentUnavailable::Undecodable {
                document: DocumentIdentity::EntryList,
                detail: serde_json::from_slice::<EntryList>(payload)
                    .unwrap_err()
                    .to_string(),
            }
        );
    }

    #[test]
    fn a_bundle_that_is_not_there_is_reported_as_absent_rather_than_as_damage() {
        // Absence and damage send a person to different places — build it, versus find out
        // what happened to it — so they are separate answers. An *empty* directory is the
        // other side of the same distinction: something is there, and it disagrees with
        // itself, which is damage.
        let fixture = Fixture::new();
        let revision = fixture.plant(&with_entries(one_entry()));
        fs::remove_dir_all(fixture.bundle(revision)).unwrap();

        assert_eq!(
            fixture.open(revision).unwrap_err(),
            OpenRevisionError::NotFound
        );

        fs::create_dir(fixture.bundle(revision)).unwrap();
        let OpenRevisionError::Damaged(report) = fixture.open(revision).unwrap_err() else {
            panic!("an empty directory is damage, not absence");
        };
        assert!(report.manifest_unreadable.is_some());
    }

    #[test]
    fn a_bundle_whose_manifest_was_edited_is_refused_and_carries_the_whole_validation_report() {
        // The reader wraps `validate_as` rather than deciding for itself what a bundle is
        // (D-118), and it hands the findings on whole: Phase 9's Validate Published Revision
        // is read-only diagnostics over exactly these fields, and a flattened summary would
        // force it to open a second, weaker path to get them back.
        let fixture = Fixture::new();
        let revision = fixture.plant(&with_entries(one_entry()));
        let bundle = fixture.bundle(revision);
        edit_manifest(
            &bundle,
            "\"documentation_is_complete\": true",
            "\"documentation_is_complete\": false",
        );

        let error = fixture.open(revision).unwrap_err();

        let OpenRevisionError::Damaged(report) = &error else {
            panic!("expected damage, got {error:?}");
        };
        assert!(report.identifier_mismatch);
        // The report reaches a diagnostic through the error chain, not only through a
        // rendering of it.
        let source = std::error::Error::source(&error).unwrap();
        assert_eq!(source.to_string(), report.to_string());
    }

    #[test]
    fn a_bundle_written_under_another_schema_is_refused_as_an_incompatible_schema() {
        // A different answer from damage — rebuild rather than diagnose — so it is lifted
        // out of the report rather than left as one finding among several. It is also total
        // by construction: validation stops as soon as it reads a schema it does not
        // support, so a `Damaged` report can never also carry a schema mismatch, which is
        // what makes the two arms genuinely exclusive rather than merely ordered.
        let fixture = Fixture::new();
        let revision = fixture.plant(&with_entries(one_entry()));
        edit_manifest(&fixture.bundle(revision), "\"schema\": 1", "\"schema\": 2");

        let error = fixture.open(revision).unwrap_err();

        assert_eq!(
            error,
            OpenRevisionError::IncompatibleSchema(SchemaMismatch {
                found: 2,
                supported: crate::revisions::BUNDLE_SCHEMA_VERSION,
            })
        );
        assert!(std::error::Error::source(&error).is_none());
    }

    #[test]
    fn a_valid_bundle_of_another_revision_is_refused_when_this_revision_was_named() {
        // The reader asks `validate_as` and not `validate`, because "a directory under
        // `bundles/` is named by its own manifest's identifier" is a property of the
        // publication protocol's writes and not of the directory. Without it, a bundle for
        // B hand-copied to `bundles/<A>` would be served as A.
        let fixture = Fixture::new();
        let mine = fixture.plant(&with_entries(one_entry()));
        let theirs = fixture.plant(&with_entries(vec![EntrySummary {
            category: "building".to_owned(),
            identifier: "somebody_elses".to_owned(),
            display_name: None,
        }]));
        assert_ne!(mine, theirs);

        fs::remove_dir_all(fixture.bundle(mine)).unwrap();
        fs::rename(fixture.bundle(theirs), fixture.bundle(mine)).unwrap();

        assert_eq!(
            fixture.open(mine).unwrap_err(),
            OpenRevisionError::AnotherRevision(RevisionMismatch {
                expected: mine,
                found: theirs,
            })
        );
    }

    #[test]
    fn a_stored_reference_that_is_not_a_revision_identifier_is_refused_before_any_path_is_built() {
        // `state` preserves a token it never parsed, so a previous build or a hand edit can
        // leave anything there. The refusal has to happen before the token becomes a path —
        // and the negative control for that claim is that nothing under `bundles/` was
        // created, touched, or even stat-ed on the way to the answer.
        let fixture = Fixture::new();
        let before = fs::read_dir(bundles_root(&fixture.root)).unwrap().count();

        let error = fixture
            .store
            .open_revision(&RevisionId("../../etc/passwd".to_owned()))
            .unwrap_err();

        assert_eq!(
            error,
            OpenRevisionError::UnreadableReference(IdentityParseError)
        );
        assert_eq!(
            fs::read_dir(bundles_root(&fixture.root)).unwrap().count(),
            before
        );
    }

    #[test]
    fn a_bundle_carrying_a_file_nothing_accounts_for_is_still_readable() {
        // The read-side half of D-118/D-119. A `.DS_Store` a file browser dropped into a
        // published directory bars *adoption* at commit point 1 and must not bar *reading*:
        // a published path is never deleted and the identifier is a pure function of
        // content, so refusing to read would make that revision permanently unreadable and
        // unrebuildable, with no repair path.
        let fixture = Fixture::new();
        let revision = fixture.plant(&with_entries(one_entry()));
        fs::write(fixture.bundle(revision).join(".DS_Store"), b"stowaway").unwrap();

        let reader = fixture.open(revision).unwrap();

        assert_eq!(
            reader.entry_list().unwrap(),
            Some(EntryList {
                entries: one_entry()
            })
        );
    }

    #[test]
    fn a_document_whose_bytes_changed_after_the_bundle_was_opened_is_refused_rather_than_decoded() {
        // Documents are read at the accessor and not at open, so the open-time proof is
        // stale by the time the bytes are needed. Re-hashing against the manifest is what
        // keeps "somebody edited an immutable artifact" a different answer from "this build
        // cannot understand its own format" — the payload below decodes perfectly, so
        // without the check this row would return a plausible wrong answer rather than an
        // error.
        let fixture = Fixture::new();
        let revision = fixture.plant(&with_entries(one_entry()));
        let reader = fixture.open(revision).unwrap();

        let payload = br#"{"entries":[{"category":"forged","identifier":"forged"}]}"#;
        assert!(serde_json::from_slice::<EntryList>(payload).is_ok());
        fs::write(
            document_file(&fixture.bundle(revision), DocumentIdentity::EntryList),
            payload,
        )
        .unwrap();

        assert_eq!(
            reader.entry_list().unwrap_err(),
            DocumentUnavailable::Changed {
                document: DocumentIdentity::EntryList,
            }
        );
    }

    #[test]
    fn an_open_that_fails_leaves_the_revision_unpinned() {
        // The acquire/release pairing on every failing path. The pin is taken before the
        // directory is validated — the order that closes the window a claim could otherwise
        // slip through — which is exactly what makes a leaked pin on failure easy to write:
        // a bundle nothing can read would become a bundle retention can never delete.
        let fixture = Fixture::new();
        let revision = fixture.plant(&with_entries(one_entry()));
        fs::remove_dir_all(fixture.bundle(revision)).unwrap();

        assert!(fixture.open(revision).is_err());

        assert!(fixture.store.claim_for_retirement(revision).is_ok());
    }

    #[test]
    fn a_revision_being_retired_cannot_be_opened_until_the_claim_is_abandoned() {
        // The other side of the same window, at the store's surface: a sweep holds its claim
        // across `remove_dir_all`, and anything opening that revision meanwhile is refused
        // rather than handed a reader onto a directory that is going away.
        let fixture = Fixture::new();
        let revision = fixture.plant(&with_entries(one_entry()));

        let claim = fixture.store.claim_for_retirement(revision).unwrap();
        assert_eq!(claim.revision(), revision);
        assert_eq!(
            fixture.open(revision).unwrap_err(),
            OpenRevisionError::Retired
        );

        drop(claim);
        assert!(fixture.open(revision).is_ok());
    }

    #[test]
    fn a_revision_being_retired_is_refused_as_retiring_even_once_its_directory_is_gone() {
        // **The gate on pinning before validating**, and the only thing that observes that
        // ordering: a sweep holds its claim across `remove_dir_all`, so mid-deletion is
        // exactly when a concurrent open arrives, and by then the directory may be half
        // gone or wholly gone. Consulting the registry first means the answer is "this is
        // being removed, resolve a newer revision" rather than "build it again" — and an
        // implementation that validated first would report the directory's state instead,
        // which is both the wrong answer and the window a claim exists to close.
        let fixture = Fixture::new();
        let revision = fixture.plant(&with_entries(one_entry()));

        let _claim = fixture.store.claim_for_retirement(revision).unwrap();
        fs::remove_dir_all(fixture.bundle(revision)).unwrap();

        assert_eq!(
            fixture.open(revision).unwrap_err(),
            OpenRevisionError::Retired
        );
    }

    #[test]
    fn a_revision_with_an_open_reader_cannot_be_claimed_until_the_last_reader_releases() {
        // Acceptance criterion: a bundle with an open handle is not eligible for deletion,
        // and eligibility begins only after the *last* handle releases. Two readers, so a
        // flag-shaped implementation cannot pass by reporting the revision free after the
        // first drop.
        let fixture = Fixture::new();
        let revision = fixture.plant(&with_entries(one_entry()));
        let first = fixture.open(revision).unwrap();
        let second = fixture.open(revision).unwrap();

        assert_eq!(
            fixture.store.claim_for_retirement(revision).unwrap_err(),
            RetirementRefused::Pinned { handles: 2 }
        );
        drop(first);
        assert_eq!(
            fixture.store.claim_for_retirement(revision).unwrap_err(),
            RetirementRefused::Pinned { handles: 1 }
        );

        drop(second);
        assert!(fixture.store.claim_for_retirement(revision).is_ok());
    }

    #[test]
    fn no_reader_value_and_no_open_error_carries_a_bundle_root() {
        // The D-087/D-090 gate: nothing this module returns gives a caller the ability to
        // open a bundle file, and nothing it renders tells a log where one is. `Debug` is
        // asserted alongside `Display` deliberately — a derived `Debug` on `RevisionReader`
        // would print its bundle root, and `Debug` reaches logs just as `Display` does.
        //
        // The variants are taken from *real* refusals wherever one can be provoked, not
        // only from hand-built values: a hand-built `Damaged` carries empty name vectors,
        // so it would pass this gate no matter what the report renders. The companion test
        // below states what a real one is allowed to name.
        let fixture = Fixture::new();
        let revision = fixture.plant(&with_entries(one_entry()));
        let reader = fixture.open(revision).unwrap();

        let mut renderings = vec![format!("{reader:?}")];
        for error in [
            DocumentUnavailable::Unreadable {
                document: DocumentIdentity::EntryList,
                detail: fs::read(fixture.bundle(revision).join("absent"))
                    .unwrap_err()
                    .to_string(),
            },
            DocumentUnavailable::Changed {
                document: DocumentIdentity::EntryList,
            },
            DocumentUnavailable::Undecodable {
                document: DocumentIdentity::EntryList,
                detail: "missing field".to_owned(),
            },
        ] {
            renderings.push(error.to_string());
            renderings.push(format!("{error:?}"));
        }
        for error in open_error_variants(revision) {
            renderings.push(error.to_string());
            renderings.push(format!("{error:?}"));
        }
        // A real damaged bundle, whose report carries populated name vectors.
        fs::remove_file(document_file(
            &fixture.bundle(revision),
            DocumentIdentity::EntryList,
        ))
        .unwrap();
        let damaged = fixture.open(revision).unwrap_err();
        assert!(
            matches!(&damaged, OpenRevisionError::Damaged(report) if !report.missing.is_empty())
        );
        renderings.push(damaged.to_string());
        renderings.push(format!("{damaged:?}"));

        for rendering in renderings {
            assert!(
                !rendering.contains(ROOT_TOKEN),
                "a bundle root leaked into {rendering:?}"
            );
        }
    }

    #[test]
    fn a_damaged_bundles_refusal_names_the_entry_at_fault_and_nothing_that_locates_it() {
        // Where the boundary actually falls, stated so it is a decision rather than an
        // accident. A refusal names bundle-relative entries — that is the whole purpose of
        // `ValidationReport` (D-118), and Phase 9's Validate Published Revision reads
        // exactly these findings. Naming is diagnosis: it says which part of a revision is
        // wrong, and hands over no root to join it against and no operation that would take
        // it. What must never escape is anything that *locates* a file, which the gate above
        // covers.
        let fixture = Fixture::new();
        let revision = fixture.plant(&with_entries(one_entry()));
        fs::remove_file(document_file(
            &fixture.bundle(revision),
            DocumentIdentity::EntryList,
        ))
        .unwrap();

        let rendered = fixture.open(revision).unwrap_err().to_string();

        assert!(
            rendered.contains("documents/entry-list.json"),
            "a person diagnosing this needs to be told which entry is gone: {rendered}"
        );
        assert!(!rendered.contains(ROOT_TOKEN), "{rendered}");
    }

    #[test]
    fn revision_reading_errors_display_and_implement_std_error() {
        // Application and transport call sites `?` and log these; both need `Display`, and
        // `std::error::Error` is what makes `?` conversion available (the rationale
        // `state/mutations.rs:690` records). No rendering may leak a debug variant name,
        // because these reach a person.
        let revision = RevisionIdentity::parse(&"ab".repeat(32)).unwrap();
        let leaked = [
            "UnreadableReference",
            "NotFound",
            "Retired",
            "IncompatibleSchema",
            "Damaged",
            "AnotherRevision",
            "Unreadable",
            "Changed",
            "Undecodable",
        ];

        let mut rendered: Vec<String> = open_error_variants(revision)
            .into_iter()
            .map(|error| error.to_string())
            .collect();
        rendered.push(
            DocumentUnavailable::Unreadable {
                document: DocumentIdentity::EntryList,
                detail: "no such file".to_owned(),
            }
            .to_string(),
        );
        rendered.push(
            DocumentUnavailable::Changed {
                document: DocumentIdentity::EntryList,
            }
            .to_string(),
        );
        rendered.push(
            DocumentUnavailable::Undecodable {
                document: DocumentIdentity::EntryList,
                detail: "missing field".to_owned(),
            }
            .to_string(),
        );

        for message in &rendered {
            assert!(!message.is_empty());
            for name in leaked {
                assert!(
                    !message.contains(name),
                    "the variant name {name} leaked into {message:?}"
                );
            }
        }

        // A cause is exposed exactly where one is wrapped, so a diagnostic reads the
        // finding rather than a rendering of it.
        for error in open_error_variants(revision) {
            let expected = matches!(
                error,
                OpenRevisionError::UnreadableReference(_) | OpenRevisionError::Damaged(_)
            );
            assert_eq!(
                std::error::Error::source(&error).is_some(),
                expected,
                "wrong source() for {error:?}"
            );
        }
    }

    /// Every `OpenRevisionError` variant, so the boundary tests above cannot pass by
    /// covering only the arms that happen to be easy to reach from disk.
    fn open_error_variants(revision: RevisionIdentity) -> Vec<OpenRevisionError> {
        vec![
            OpenRevisionError::UnreadableReference(IdentityParseError),
            OpenRevisionError::NotFound,
            OpenRevisionError::Retired,
            OpenRevisionError::IncompatibleSchema(SchemaMismatch {
                found: 2,
                supported: 1,
            }),
            OpenRevisionError::Damaged(ValidationReport {
                identifier_mismatch: true,
                ..ValidationReport::default()
            }),
            OpenRevisionError::AnotherRevision(RevisionMismatch {
                expected: revision,
                found: revision,
            }),
        ]
    }

    #[cfg(windows)]
    #[test]
    fn a_reader_holds_no_operating_system_handle_inside_the_bundle() {
        // The platform claim this module's parent records, made observable. On Windows a
        // rename or delete of a directory tree is refused with a sharing violation while a
        // handle inside it is open; every read here is read-and-close, so a live reader must
        // not refuse one. On Unix the rename would succeed either way, which is why this is
        // gated rather than merely written — CI runs a Windows leg for exactly this class of
        // claim.
        let fixture = Fixture::new();
        let revision = fixture.plant(&with_entries(one_entry()));
        let reader = fixture.open(revision).unwrap();
        let bundle = fixture.bundle(revision);
        let aside = bundle.with_file_name("moved-aside");

        fs::rename(&bundle, &aside).expect("a live reader refused a rename of its own bundle");
        fs::rename(&aside, &bundle).unwrap();

        assert!(reader.entry_list().unwrap().is_some());
    }
}
