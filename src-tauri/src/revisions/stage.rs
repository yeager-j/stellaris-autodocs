//! Bundle layout, the staging writer, and the validator that decides whether a directory
//! is a Documentation Revision bundle (docs/technical-design.md, "Revision bundles").
//!
//! # Layout
//!
//! ```text
//! <revisions root>/bundles/<64 lowercase hex>/manifest.json
//!                                            /documents/entry-list.json
//! <revisions root>/staging/<uuid v4>/...
//! ```
//!
//! Staging and bundles are siblings under one application-owned root, which makes
//! "staging happens in an adjacent location on the same filesystem as the revisions
//! location" true by construction rather than by a caller's discipline: a rename between
//! them cannot become a cross-device copy. It also gives the later retention sweep two
//! unconditional rules instead of a search — everything under `staging/` is abandoned
//! when no publication is in flight, and anything under `bundles/` whose name is not 64
//! lowercase hex is junk.
//!
//! **The first rule is unconditional, and that is the decision STE-17 should write against.**
//! One publication failure deliberately leaves its attempt behind
//! ([`PublishError::PublishedBundleUnusable`](crate::revisions::PublishError)), and the
//! sweep removes it at the next open like any other. The two rules do not conflict, because
//! a retained attempt is a diagnostic for the failure being reported *now* — the directory
//! a person can compare against the occupant this protocol refuses to delete — and never
//! recoverable state: nothing reads a staging directory back, and the candidate that
//! produced it is cheaper to rebuild than to recover. A sweep that had to ask whether an
//! attempt was retained on purpose would need a record of intent outliving the process that
//! formed it, kept to protect material no code path consumes.
//!
//! Each attempt gets a fresh UUID v4 with no mod-derived component. Nothing untrusted
//! enters a path, and no later attempt can choose a crashed attempt's name, which is the
//! same reasoning `state::replace` records for `.state-<uuid>.tmp`
//! (`src/state/replace.rs:29`). There is deliberately **no** `remove_dir_all` before
//! staging: a stale crash artifact must never blend into a new attempt and be moved into
//! a bundle as if this attempt had written it.
//!
//! # Order
//!
//! [`stage_bundle`] writes every document first and the manifest last, hashing exactly
//! the bytes it wrote. A manifest can therefore never name an entry that was not
//! produced, whatever step a crash interrupts — the worst intermediate shape is a
//! directory of documents that no manifest claims, which validates as unreadable and is
//! swept.
//!
//! # What validation proves
//!
//! [`validate`] proves integrity, not readability: it hashes bytes and does not parse
//! them (docs/technical-design.md:651). It always reads the manifest back from disk
//! rather than judging an in-memory value, because the publication protocol, STE-17's
//! Revision Reader, and Phase 9's Validate Published Revision are all the same question
//! asked of a directory, and one answer path is what keeps them from disagreeing.
//!
//! It proves the directory is *a* bundle, never that it is a particular one. A caller that
//! reached a directory by identifier — the publication protocol, and STE-17's reader —
//! asks [`validate_as`] instead, because "a directory under `bundles/` is named by its own
//! manifest's identifier" is a property of this protocol's writes and not of the directory.

use crate::canonical::path::LogicalPath;
use crate::revisions::candidate::{DocumentIdentity, RevisionCandidate, RevisionDocument};
use crate::revisions::manifest::{
    BUNDLE_SCHEMA_VERSION, BundleManifest, ManifestParseError, RevisionIdentity,
};
use crate::revisions::publish::PublicationIo;
use crate::source::fingerprint::ContentHash;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// The two children of the revisions root. Named once here because the writer, the
/// validator, and the retention sweep all depend on them.
const STAGING_DIR: &str = "staging";
const BUNDLES_DIR: &str = "bundles";

/// The manifest's bundle-relative name. It is not a required entry of itself, so it is a
/// plain name rather than a [`LogicalPath`] until the validator needs to exclude it from
/// the unaccounted-for set.
///
/// `pub(crate)` for the reason [`staging_root`] and [`bundles_root`] are: anything else
/// that has to recognize or read a manifest — the crash matrix's write injection, the
/// end-to-end test that reads one back off disk, STE-17's reader — would otherwise rebuild
/// the name from its own string literal and become a second authority over the layout.
pub(crate) const MANIFEST_FILE: &str = "manifest.json";

/// The one Phase 3 document's bundle-relative path. Under `documents/` so a later phase's
/// non-document bundle material (search inputs, asset key lists) can sit beside it
/// without either one having to move.
const ENTRY_LIST_FILE: &str = "documents/entry-list.json";

/// Where every abandoned staging directory of every attempt lives.
///
/// `pub(crate)` and stated once: STE-17's retention sweep enumerates this directory, and
/// a sweep that rebuilt the path from its own string literal would be a second authority
/// over the layout — the kind that stays correct until one of the two moves.
pub(crate) fn staging_root(revisions_root: &Path) -> PathBuf {
    revisions_root.join(STAGING_DIR)
}

/// Where every published bundle lives.
///
/// Stated once for the same reason as [`staging_root`]: the publication protocol creates
/// this directory at open and flushes it after the move, and a second literal would be a
/// second authority over the layout.
pub(crate) fn bundles_root(revisions_root: &Path) -> PathBuf {
    revisions_root.join(BUNDLES_DIR)
}

/// The canonical immutable path of one published revision. The host generates it from
/// the identifier, never from a mod name (docs/technical-design.md, "Revision bundles").
pub(crate) fn bundle_path(revisions_root: &Path, identity: RevisionIdentity) -> PathBuf {
    bundles_root(revisions_root).join(identity.to_hex())
}

/// Whether a directory name under [`staging_root`] is one this application wrote.
///
/// The read side of the naming rule, so a sweep can identify abandoned staging without
/// guessing: exactly the hyphenated lowercase UUID spelling [`stage_bundle`] generates,
/// so one written form has one recognition and an unrelated directory a user dropped
/// there is not deleted on the strength of a loose match.
///
/// **The one item of this module's surface with no caller yet.** Demoting it to
/// `pub(crate)` alongside its neighbours makes it dead code under `-D warnings`, which is
/// the compiler stating accurately that STE-17's retention sweep — the consumer this rule
/// was written for, and the reason [`StageError::staging`] can be abandoned safely — does
/// not exist. It is re-exported rather than deleted because the write side would then be
/// the only spelling of the rule, and a test asserting the spelling inline would be a
/// second authority over it. It grants nothing: a predicate over a string cannot stage,
/// validate, or publish. Delete the re-export when the sweep lands and calls it from
/// inside this module.
pub fn is_staging_attempt_name(name: &str) -> bool {
    uuid::Uuid::parse_str(name).is_ok_and(|parsed| parsed.hyphenated().to_string() == name)
}

/// The bundle-relative path of one document, by exhaustive match over what makes two
/// documents the same document.
///
/// The match is over [`DocumentIdentity`] rather than over the payload, so the path is a
/// function of identity alone: [`RevisionCandidate::new`] already refuses two documents
/// of one identity, and that refusal is what makes two bundle paths unable to collide.
/// A caller never supplies a path, so no Stellaris identifier — and nothing else drawn
/// from mod content — can reach the filesystem.
pub(crate) fn document_path(document: &RevisionDocument) -> LogicalPath {
    let raw = match document.identity() {
        DocumentIdentity::EntryList => ENTRY_LIST_FILE,
    };
    host_path(raw)
}

/// Parses a path this module wrote as a literal. Total by construction; an `expect` here
/// reports a defect in the constants above rather than anything a caller can cause.
fn host_path(raw: &str) -> LogicalPath {
    LogicalPath::parse(raw).expect("bundle layout constants are valid logical paths")
}

/// Compact JSON, one file per document: bundle payloads are machine-read and there are
/// hundreds of them per revision, so pretty-printing them would cost size for a
/// readability nothing consumes. The manifest is the human-readable document.
fn encode_document(document: &RevisionDocument) -> Vec<u8> {
    match document {
        RevisionDocument::EntryList(list) => {
            serde_json::to_vec(list).expect("an entry list encodes to JSON")
        }
    }
}

/// Human-readable, matching `state::store::encode` (`src/state/store.rs:240`): the
/// manifest is what a person reads during recovery and what a diagnostic quotes. The
/// trailing newline makes it a well-formed text file.
fn encode_manifest(manifest: &BundleManifest) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(manifest).expect("a manifest encodes to JSON");
    bytes.push(b'\n');
    bytes
}

/// One attempt's staging directory, written and not yet validated or moved.
#[derive(Debug)]
pub struct StagedBundle {
    root: PathBuf,
    identity: RevisionIdentity,
}

impl StagedBundle {
    /// The attempt's directory: the unit of abandonment and the source of the move.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// What [`BundleManifest::seal`] derived from the candidate and the bytes written.
    /// The final path comes from this value through [`bundle_path`], never from the
    /// staging name.
    ///
    /// **Two identities exist for one bundle, and that is the point.** This one is what
    /// the writer sealed in memory; [`ValidatedBundle::identity`] is what the manifest on
    /// disk says. They are not two authorities — the protocol's step 6 hands this value to
    /// [`validate_as`] and refuses to move anything unless the disk agrees, so a bundle
    /// whose written bytes do not derive the identifier the writer computed never reaches
    /// a final path.
    pub fn identity(&self) -> RevisionIdentity {
        self.identity
    }
}

/// A staging attempt that did not finish, carrying the directory it owns so the caller
/// can remove it. The path is part of the error rather than something the caller
/// reconstructs: the attempt's name is a UUID only this module has seen.
#[derive(Debug)]
pub struct StageError {
    staging: PathBuf,
    detail: String,
}

impl StageError {
    /// The abandoned directory. Always under [`staging_root`] and always recognized by
    /// [`is_staging_attempt_name`], so a caller that cannot remove it now loses nothing:
    /// the retention sweep finds it by the same rule.
    pub fn staging(&self) -> &Path {
        &self.staging
    }
}

impl fmt::Display for StageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "staging a revision bundle in {}: {}",
            self.staging.display(),
            self.detail
        )
    }
}

impl std::error::Error for StageError {}

/// Writes one candidate into a fresh staging directory, documents first and manifest
/// last.
///
/// The manifest is sealed over the hashes of exactly the bytes this function wrote, not
/// over a re-encoding: a second encoding pass could differ from the first and the
/// disagreement would look identical to tampering.
pub fn stage_bundle(
    io_seam: &mut dyn PublicationIo,
    revisions_root: &Path,
    candidate: &RevisionCandidate,
) -> Result<StagedBundle, StageError> {
    let staging = staging_root(revisions_root).join(uuid::Uuid::new_v4().hyphenated().to_string());
    let fail = |detail: String| StageError {
        staging: staging.clone(),
        detail,
    };

    // Created before anything is written so that a crash at any later step leaves a
    // directory the sweep can recognize, rather than files under a root nothing names.
    io_seam
        .create_dir(&staging)
        .map_err(|error| fail(format!("creating the staging directory: {error}")))?;

    let mut required_entries = BTreeMap::new();
    let mut written_dirs = BTreeSet::new();
    for document in candidate.documents() {
        let logical = document_path(document);
        let bytes = encode_document(document);
        let path = resolve(&staging, &logical);
        io_seam
            .write_file(&path, &bytes)
            .map_err(|error| fail(format!("writing {logical}: {error}")))?;
        if let Some(parent) = path.parent() {
            written_dirs.insert(parent.to_path_buf());
        }
        required_entries.insert(logical, ContentHash::of(&bytes));
    }

    let manifest = BundleManifest::seal(candidate, required_entries);
    let identity = manifest.identifier();
    io_seam
        .write_file(&staging.join(MANIFEST_FILE), &encode_manifest(&manifest))
        .map_err(|error| fail(format!("writing {MANIFEST_FILE}: {error}")))?;
    written_dirs.insert(staging.clone());

    // Each file was flushed as it was written; these flush the directory entries that
    // name them, so the subtree the move commits is durable rather than merely visible.
    // Deepest first — a longer path sorts after its own prefix — because a parent's entry
    // is worth nothing if the child directory's contents have not reached disk.
    for directory in written_dirs.iter().rev() {
        io_seam
            .sync_dir(directory)
            .map_err(|error| fail(format!("flushing {}: {error}", directory.display())))?;
    }

    Ok(StagedBundle {
        root: staging,
        identity,
    })
}

/// Resolves a bundle-relative logical path against a root, component by component.
///
/// Every component is pushed separately rather than joining the `/`-separated string, so
/// the result is built from the path's own structure on every platform. A [`LogicalPath`]
/// is already relative and free of `.` and `..` (`canonical::path`), so the result cannot
/// leave `root`.
fn resolve(root: &Path, logical: &LogicalPath) -> PathBuf {
    let mut path = root.to_path_buf();
    for component in logical.components() {
        path.push(component);
    }
    path
}

/// A bundle whose manifest was read, whose identifier is the digest of its own body, and
/// every entry that manifest names is present with the bytes it names.
///
/// **Why this exists rather than a `bool` or a comment.** It is the value the publication
/// protocol's move step takes by move (`publish::commit_bundle`), and [`validate`] is its
/// only constructor. Delete it, and that signature takes two paths, so "validation precedes
/// the durable move" becomes a line of prose inside whichever function happens to call
/// `validate` before `rename` today — a sentence a later edit can reorder without anything
/// failing. Keep it, and a caller with no proof in hand has nothing to pass.
///
/// **What it does not enforce**, so the claim is not larger than the mechanism:
/// [`PublicationIo::rename`] remains a method over two paths for the test double's sake, so
/// bypassing `commit_bundle` compiles and is caught only by the unused-variable warning
/// `-D warnings` promotes to an error. The behavioural gate for the ordering is
/// `publish.rs::a_document_corrupted_after_it_was_hashed_is_never_moved`.
///
/// Deliberately not `Clone`: a proof is about a directory as it was observed, and a copy
/// kept past the move would outlive what it attests to.
#[derive(Debug)]
pub struct ValidatedBundle {
    path: PathBuf,
    identity: RevisionIdentity,
    unaccounted: Vec<String>,
}

impl ValidatedBundle {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Bundle-relative names of everything present that the manifest does not account for.
    ///
    /// A *reported* finding rather than a disqualifying one: see
    /// [`ValidationReport::unexpected`] for why it does not decide validity, and
    /// `publish::validate_for_commit` for the one decision it does decide.
    pub fn unaccounted(&self) -> &[String] {
        &self.unaccounted
    }

    /// Which revision this directory holds, read from the manifest on disk rather than
    /// taken from whoever named the directory.
    ///
    /// [`validate`] proves a directory is *a* bundle; this is what lets a caller ask
    /// whether it is *this* revision's. The two questions are separate because the answer
    /// to the second depends on an expectation the directory itself cannot carry — the
    /// staging directory a publication validates is named by a UUID, and only a directory
    /// under `bundles/` is named by its own manifest's identifier.
    pub fn identity(&self) -> RevisionIdentity {
        self.identity
    }
}

/// A manifest whose bundle shape belongs to another build. Separated from every other
/// finding because the answer is different: rebuild the revision rather than repair it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaMismatch {
    pub found: u32,
    pub supported: u32,
}

/// A complete, self-consistent bundle of the wrong revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionMismatch {
    pub expected: RevisionIdentity,
    pub found: RevisionIdentity,
}

/// Why a directory cannot be used as the bundle a caller named.
///
/// Separate answers rather than one widened report, because they call for different
/// actions. A [`NotABundle`] directory disagrees with its own manifest: it is damaged, and
/// the response is to repair or rebuild it. An [`AnotherRevision`] directory disagrees with
/// nothing — it is somebody's complete revision, sitting where this caller expected its own
/// — and the only correct response is to leave it exactly as it is. An
/// [`UnaccountedMaterial`] directory is this revision, intact, with something else beside
/// it; the response is to look at what that something is, and the refusal names it.
/// Folding any of them into [`ValidationReport`] would put findings in it that only some of
/// its entry points can ever produce, and make [`ValidationReport::is_valid`] mean
/// something different depending on who asked.
///
/// [`NotABundle`]: BundleRefusal::NotABundle
/// [`AnotherRevision`]: BundleRefusal::AnotherRevision
/// [`UnaccountedMaterial`]: BundleRefusal::UnaccountedMaterial
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleRefusal {
    NotABundle(ValidationReport),
    AnotherRevision(RevisionMismatch),
    /// A valid bundle of the expected revision that also holds files the manifest does not
    /// account for. Produced by `publish::validate_for_commit` and never by [`validate_as`]
    /// — reading such a directory is fine, and only commit point 1 refuses it.
    UnaccountedMaterial(Vec<String>),
}

impl fmt::Display for BundleRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotABundle(report) => report.fmt(f),
            Self::AnotherRevision(mismatch) => write!(
                f,
                "this directory holds revision {}, not the revision {} expected here",
                mismatch.found, mismatch.expected
            ),
            Self::UnaccountedMaterial(names) => write!(
                f,
                "this revision's directory also holds files nothing accounts for, so it \
                 cannot be published as this revision's complete contents: {}",
                join(names.iter().map(String::as_str))
            ),
        }
    }
}

impl std::error::Error for BundleRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotABundle(report) => Some(report),
            // Nothing wraps a cause here: the finding *is* the message, and the values a
            // reader needs are already in it.
            Self::AnotherRevision(_) | Self::UnaccountedMaterial(_) => None,
        }
    }
}

/// Every way a directory can disagree with the bundle it claims to be. A report rather
/// than a first failure, because a person diagnosing a damaged bundle needs the whole
/// disagreement, and because the protocol's decision to adopt or rebuild rests on which
/// kinds are present.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationReport {
    /// The manifest file could not be read, or could be read and not decoded. Both fail
    /// closed, and when either happens nothing else is knowable: the file set can only be
    /// judged against a manifest, so the remaining fields stay empty rather than
    /// reporting every file in the directory as a surprise.
    pub manifest_unreadable: Option<String>,
    pub schema_mismatch: Option<SchemaMismatch>,
    /// The stored identifier is not the digest of the manifest body. The manifest was
    /// edited, truncated, or written by something that is not this derivation, so nothing
    /// it claims about its entries can be trusted either.
    pub identifier_mismatch: bool,
    /// Required entries with no readable bytes: absent, or present and unreadable, or
    /// occupied by something that is not a regular file. One kind, because the
    /// consequence is one — the revision cannot serve that entry.
    pub missing: Vec<LogicalPath>,
    /// Required entries whose bytes are not the bytes the manifest names.
    pub changed: Vec<LogicalPath>,
    /// Bundle-relative names of everything the manifest does not account for, and of any
    /// directory whose contents could not be enumerated.
    ///
    /// **Reported, and deliberately not part of [`is_valid`](ValidationReport::is_valid).**
    /// Every other field names something the revision cannot serve; this one names material
    /// beside a revision that is entirely intact. Since a published path is never deleted
    /// (docs/decision-log.md, D-119) and the identifier is a pure function of content, a
    /// single `.DS_Store` or `Thumbs.db` that a file browser dropped into a published bundle
    /// would otherwise make that revision permanently unreadable and unrebuildable, with no
    /// repair path in this phase. The finding still decides one thing, at the place it is
    /// actually load-bearing: `publish::validate_for_commit` refuses to move onto or adopt a
    /// directory whose full contents it cannot account for.
    ///
    /// **Raw names, not [`LogicalPath`]s.** A stowaway's name need not be a valid logical
    /// path — a backslash, a `.` component, or bytes that are not UTF-8 are all possible
    /// on a real filesystem — and that fact is itself evidence, so it is reported rather
    /// than dropped by a parse that would fail. Sorted, so the report is a function of
    /// the directory rather than of the order the platform happened to walk it in.
    pub unexpected: Vec<String>,
}

impl ValidationReport {
    /// The one definition of "this directory is a bundle". [`validate`] returns
    /// [`ValidatedBundle`] exactly when this holds, so no caller re-decides which
    /// findings are tolerable.
    ///
    /// It is exactly the design's list — "the manifest, every required entry"
    /// (docs/technical-design.md, "Revision bundles") — and no more. `unexpected` is
    /// deliberately absent: it is carried on the proof instead, because it bars a directory
    /// from being *adopted* rather than from being *read*, and only one caller makes that
    /// decision. See [`ValidationReport::unexpected`].
    pub fn is_valid(&self) -> bool {
        self.manifest_unreadable.is_none()
            && self.schema_mismatch.is_none()
            && !self.identifier_mismatch
            && self.missing.is_empty()
            && self.changed.is_empty()
    }
}

impl fmt::Display for ValidationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut findings = Vec::new();
        if let Some(detail) = &self.manifest_unreadable {
            findings.push(format!("manifest could not be read: {detail}"));
        }
        if let Some(mismatch) = &self.schema_mismatch {
            findings.push(format!(
                "bundle schema version {} is not {}: this revision was written by a \
                 different build and must be rebuilt",
                mismatch.found, mismatch.supported
            ));
        }
        if self.identifier_mismatch {
            findings.push(
                "the stored revision identifier is not the digest of the manifest body".to_owned(),
            );
        }
        if !self.missing.is_empty() {
            findings.push(format!(
                "required entries with no readable bytes: {}",
                join(self.missing.iter().map(LogicalPath::as_str))
            ));
        }
        if !self.changed.is_empty() {
            findings.push(format!(
                "required entries whose bytes are not the bytes the manifest names: {}",
                join(self.changed.iter().map(LogicalPath::as_str))
            ));
        }
        if !self.unexpected.is_empty() {
            findings.push(format!(
                "files the manifest does not account for: {}",
                join(self.unexpected.iter().map(String::as_str))
            ));
        }
        if findings.is_empty() {
            return f.write_str("revision bundle validated");
        }
        write!(
            f,
            "revision bundle did not validate: {}",
            findings.join("; ")
        )
    }
}

impl std::error::Error for ValidationReport {}

fn join<'a>(names: impl Iterator<Item = &'a str>) -> String {
    names.collect::<Vec<_>>().join(", ")
}

/// Decides whether `root` is a Documentation Revision bundle, reporting every
/// disagreement rather than the first.
///
/// Reads with plain `std::fs` and not through [`PublicationIo`], for the reason that seam
/// records: a read is not a commit point, and corruption tests are more honest when they
/// corrupt real bytes.
pub fn validate(root: &Path) -> Result<ValidatedBundle, ValidationReport> {
    let mut report = ValidationReport::default();

    let manifest = match read_manifest(root) {
        Ok(manifest) => manifest,
        Err(finding) => {
            match finding {
                ManifestFinding::Unreadable(detail) => report.manifest_unreadable = Some(detail),
                ManifestFinding::Schema(mismatch) => report.schema_mismatch = Some(mismatch),
            }
            return Err(report);
        }
    };

    report.identifier_mismatch = manifest.recomputed_identity() != manifest.identifier();

    for (logical, expected) in manifest.required_entries() {
        // Reads the whole entry: the hash covers exactly the bytes the writer hashed, and
        // a streaming read would have to re-establish that equivalence for no gain at
        // bundle-document sizes.
        match fs::read(resolve(root, logical)) {
            Ok(bytes) if ContentHash::of(&bytes) == *expected => {}
            Ok(_) => report.changed.push(logical.clone()),
            Err(_) => report.missing.push(logical.clone()),
        }
    }

    let mut accounted = BTreeSet::from([host_path(MANIFEST_FILE)]);
    accounted.extend(manifest.required_entries().keys().cloned());
    let directories = accounted.iter().flat_map(ancestors).collect();
    collect_unexpected(root, "", &accounted, &directories, &mut report.unexpected);
    report.unexpected.sort();

    if report.is_valid() {
        Ok(ValidatedBundle {
            path: root.to_path_buf(),
            identity: manifest.identifier(),
            unaccounted: report.unexpected,
        })
    } else {
        Err(report)
    }
}

/// Decides whether `root` is a Documentation Revision bundle **and** is `expected`'s.
///
/// [`validate`] cannot answer the second half. It proves a directory agrees with its own
/// manifest, which a valid bundle of any revision does; that a directory under `bundles/`
/// is named by the identifier its own manifest carries is a separate claim, established
/// only by this protocol always deriving the name from the sealed manifest. "Only one
/// writer exists right now" is not an invariant, and adoption of an already-present bundle
/// is exactly where the assumption is load-bearing: without this, a valid bundle for
/// revision B hand-copied to `bundles/<A>` would be adopted and published as A.
///
/// Every caller that opens a bundle it addressed by identifier wants this rather than
/// [`validate`] — the publication protocol at both of its bundle-completion paths, and
/// STE-17's Revision Reader when it opens a published bundle by identifier.
pub fn validate_as(
    root: &Path,
    expected: RevisionIdentity,
) -> Result<ValidatedBundle, BundleRefusal> {
    let validated = validate(root).map_err(BundleRefusal::NotABundle)?;
    if validated.identity() == expected {
        return Ok(validated);
    }
    Err(BundleRefusal::AnotherRevision(RevisionMismatch {
        expected,
        found: validated.identity(),
    }))
}

enum ManifestFinding {
    Unreadable(String),
    Schema(SchemaMismatch),
}

/// The one place an unreadable *file* and an unreadable *manifest* are told apart.
/// [`BundleManifest::parse`] takes bytes and never a path, so both failures must fail
/// closed here rather than being distinguished by whoever calls it next.
fn read_manifest(root: &Path) -> Result<BundleManifest, ManifestFinding> {
    let bytes = fs::read(root.join(MANIFEST_FILE))
        .map_err(|error| ManifestFinding::Unreadable(error.to_string()))?;
    BundleManifest::parse(&bytes).map_err(|error| match error {
        ManifestParseError::UnsupportedSchema { found } => {
            ManifestFinding::Schema(SchemaMismatch {
                found,
                supported: BUNDLE_SCHEMA_VERSION,
            })
        }
        ManifestParseError::Malformed { detail } => ManifestFinding::Unreadable(detail),
    })
}

/// Every proper ancestor directory of a bundle-relative entry. A prefix of a valid
/// logical path is a valid logical path, which is what makes the `expect` in
/// [`host_path`] unreachable for these.
fn ancestors(logical: &LogicalPath) -> Vec<LogicalPath> {
    let components: Vec<&str> = logical.components().collect();
    (1..components.len())
        .map(|end| host_path(&components[..end].join("/")))
        .collect()
}

/// Walks the bundle for everything the manifest does not account for.
///
/// A stray directory is named and not descended into: one finding that names the subtree
/// is the evidence, and enumerating its contents would report a user's stray folder as a
/// hundred surprises. A directory that cannot be enumerated is named for the same reason
/// this walk exists — the claim is "the bundle holds nothing else", and a directory whose
/// contents are unknown cannot support it.
fn collect_unexpected(
    dir: &Path,
    prefix: &str,
    accounted: &BTreeSet<LogicalPath>,
    directories: &BTreeSet<LogicalPath>,
    out: &mut Vec<String>,
) {
    let listing = match fs::read_dir(dir) {
        Ok(listing) => listing,
        Err(_) => {
            out.push(if prefix.is_empty() {
                ".".to_owned()
            } else {
                prefix.trim_end_matches('/').to_owned()
            });
            return;
        }
    };
    for entry in listing {
        let Ok(entry) = entry else {
            out.push(format!("{prefix}<unreadable directory entry>"));
            continue;
        };
        let raw = format!("{prefix}{}", entry.file_name().to_string_lossy());
        let logical = LogicalPath::parse(&raw).ok();
        // `file_type` does not follow symlinks, so a symlink pointing at a required
        // entry's content is reported rather than silently accepted as that entry.
        let kind = entry.file_type().ok();
        let is_dir = kind.is_some_and(|kind| kind.is_dir());
        let is_file = kind.is_some_and(|kind| kind.is_file());
        match &logical {
            Some(logical) if is_dir && directories.contains(logical) => collect_unexpected(
                &entry.path(),
                &format!("{raw}/"),
                accounted,
                directories,
                out,
            ),
            Some(logical) if is_file && accounted.contains(logical) => {}
            _ => out.push(raw),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::version::AnalysisVersionVector;
    use crate::discovery::identity::{DiscoveryLocationId, ModInstallationId};
    use crate::revisions::candidate::{EntryList, EntrySummary, RevisionInputs};
    use crate::revisions::publish::RealPublicationIo;
    use crate::source::ObservationGaps;
    use crate::source::fingerprint::SourceFingerprint;
    use std::io;
    use tempfile::TempDir;

    fn fingerprint(bytes: &[u8]) -> SourceFingerprint {
        SourceFingerprint::of(
            [(host_path("descriptor.mod"), ContentHash::of(bytes))],
            &ObservationGaps::default(),
        )
        .unwrap()
    }

    fn candidate_of(documents: Vec<RevisionDocument>) -> RevisionCandidate {
        RevisionCandidate::new(
            ModInstallationId::derive(DiscoveryLocationId::generate(), &host_path("ugc_1")),
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

    fn candidate(entries: Vec<EntrySummary>) -> RevisionCandidate {
        candidate_of(vec![RevisionDocument::EntryList(EntryList { entries })])
    }

    fn one_entry() -> Vec<EntrySummary> {
        vec![EntrySummary {
            category: "technology".to_owned(),
            identifier: "tech_a".to_owned(),
            display_name: None,
        }]
    }

    /// Stages one bundle into a fresh revisions root and returns both.
    fn stage_fixture(entries: Vec<EntrySummary>) -> (TempDir, StagedBundle) {
        let root = TempDir::new().unwrap();
        let staged =
            stage_bundle(&mut RealPublicationIo, root.path(), &candidate(entries)).unwrap();
        (root, staged)
    }

    /// Every bundle-relative file name under `root`, sorted. The walk the assertions use
    /// to say "and nothing else".
    fn file_set(root: &Path) -> Vec<String> {
        fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
            for entry in fs::read_dir(dir).unwrap().flatten() {
                let raw = format!("{prefix}{}", entry.file_name().to_string_lossy());
                if entry.file_type().unwrap().is_dir() {
                    walk(&entry.path(), &format!("{raw}/"), out);
                } else {
                    out.push(raw);
                }
            }
        }
        let mut out = Vec::new();
        walk(root, "", &mut out);
        out.sort();
        out
    }

    fn manifest_of(root: &Path) -> BundleManifest {
        BundleManifest::parse(&fs::read(root.join(MANIFEST_FILE)).unwrap()).unwrap()
    }

    /// Rewrites the staged manifest, which is pretty-printed, by textual substitution.
    fn edit_manifest(root: &Path, from: &str, to: &str) {
        let text = fs::read_to_string(root.join(MANIFEST_FILE)).unwrap();
        assert!(text.contains(from), "manifest does not contain {from}");
        fs::write(root.join(MANIFEST_FILE), text.replace(from, to)).unwrap();
    }

    /// Records every write in order; otherwise real. The order of the writes is a
    /// protocol claim, and it is not observable from the finished directory.
    struct RecordingIo {
        writes: Vec<String>,
        inner: RealPublicationIo,
    }

    impl PublicationIo for RecordingIo {
        fn create_dir(&mut self, path: &Path) -> io::Result<()> {
            self.inner.create_dir(path)
        }
        fn write_file(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
            self.writes
                .push(path.file_name().unwrap().to_string_lossy().into_owned());
            self.inner.write_file(path, bytes)
        }
        fn sync_dir(&mut self, path: &Path) -> io::Result<()> {
            self.inner.sync_dir(path)
        }
        fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
            self.inner.rename(from, to)
        }
        fn remove_dir_all(&mut self, path: &Path) -> io::Result<()> {
            self.inner.remove_dir_all(path)
        }
    }

    /// Fails the write of one named file; every other step is real.
    struct FailWriting {
        name: &'static str,
        inner: RealPublicationIo,
    }

    impl PublicationIo for FailWriting {
        fn create_dir(&mut self, path: &Path) -> io::Result<()> {
            self.inner.create_dir(path)
        }
        fn write_file(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
            if path.file_name().is_some_and(|name| name == self.name) {
                return Err(io::Error::other("injected write failure"));
            }
            self.inner.write_file(path, bytes)
        }
        fn sync_dir(&mut self, path: &Path) -> io::Result<()> {
            self.inner.sync_dir(path)
        }
        fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
            self.inner.rename(from, to)
        }
        fn remove_dir_all(&mut self, path: &Path) -> io::Result<()> {
            self.inner.remove_dir_all(path)
        }
    }

    #[test]
    fn a_staged_bundle_lands_as_a_manifest_and_its_documents_with_matching_hashes() {
        // The writer's core claim: the manifest names exactly the files that exist, each
        // with the hash of exactly the bytes written there. A second encoding pass to
        // produce those hashes could differ from the first, and the disagreement would be
        // indistinguishable from tampering.
        let (_root, staged) = stage_fixture(one_entry());
        assert_eq!(
            file_set(staged.root()),
            ["documents/entry-list.json", "manifest.json"]
        );

        let manifest = manifest_of(staged.root());
        assert_eq!(manifest.identifier(), staged.identity());
        assert_eq!(manifest.recomputed_identity(), manifest.identifier());
        assert_eq!(manifest.schema(), BUNDLE_SCHEMA_VERSION);

        let entry = host_path(ENTRY_LIST_FILE);
        assert_eq!(manifest.required_entries().len(), 1);
        assert_eq!(
            manifest.required_entries()[&entry],
            ContentHash::of(&fs::read(resolve(staged.root(), &entry)).unwrap())
        );
    }

    #[test]
    fn the_manifest_is_human_readable_and_document_payloads_are_compact() {
        // Two different jobs, asserted on real bytes rather than on the encoder call:
        // the manifest is what a person reads during recovery, and there are hundreds of
        // documents per revision whose only reader is this program.
        let (_root, staged) = stage_fixture(one_entry());

        let manifest = fs::read_to_string(staged.root().join(MANIFEST_FILE)).unwrap();
        assert!(
            manifest.starts_with("{\n  \"identifier\": \""),
            "{manifest}"
        );
        assert!(manifest.ends_with("}\n"), "{manifest}");

        let document =
            fs::read_to_string(resolve(staged.root(), &host_path(ENTRY_LIST_FILE))).unwrap();
        assert_eq!(
            document,
            r#"{"entries":[{"category":"technology","identifier":"tech_a","display_name":null}]}"#
        );
    }

    #[test]
    fn documents_are_written_before_the_manifest() {
        // Ordering is the reason a manifest can never name an entry that was not
        // produced. It is invisible in the finished directory, so the seam observes it.
        let root = TempDir::new().unwrap();
        let mut recorder = RecordingIo {
            writes: Vec::new(),
            inner: RealPublicationIo,
        };
        stage_bundle(&mut recorder, root.path(), &candidate(one_entry())).unwrap();
        assert_eq!(recorder.writes, ["entry-list.json", MANIFEST_FILE]);
    }

    #[test]
    fn a_failed_manifest_write_leaves_an_identifiable_staging_directory_and_no_manifest() {
        // The half-written shape a crash can leave: documents on disk with nothing
        // claiming them. It must be identifiable as abandoned staging without guessing,
        // because the sweep that removes it runs in a later process.
        let root = TempDir::new().unwrap();
        let error = stage_bundle(
            &mut FailWriting {
                name: MANIFEST_FILE,
                inner: RealPublicationIo,
            },
            root.path(),
            &candidate(one_entry()),
        )
        .unwrap_err();

        assert_eq!(file_set(error.staging()), ["documents/entry-list.json"]);
        assert_eq!(
            error.staging().parent(),
            Some(staging_root(root.path()).as_path())
        );
        assert!(is_staging_attempt_name(
            &error.staging().file_name().unwrap().to_string_lossy()
        ));
        // And the directory is not a bundle: no manifest means nothing can be adopted.
        assert!(validate(error.staging()).is_err());
    }

    #[test]
    fn each_attempt_gets_a_distinct_recognizable_staging_directory() {
        // A fresh UUID v4 per attempt with no mod-derived component, the reasoning
        // `state::replace` records for `.state-<uuid>.tmp` (src/state/replace.rs:29): no
        // later attempt can choose a crashed attempt's name, so a stale artifact cannot
        // blend into a new attempt. There is deliberately no cleanup before staging, so
        // the earlier directory is still there afterwards.
        let root = TempDir::new().unwrap();
        let first = stage_bundle(&mut RealPublicationIo, root.path(), &candidate(vec![])).unwrap();
        let second = stage_bundle(&mut RealPublicationIo, root.path(), &candidate(vec![])).unwrap();

        assert_ne!(first.root(), second.root());
        assert!(first.root().exists());
        for staged in [&first, &second] {
            assert_eq!(
                staged.root().parent(),
                Some(staging_root(root.path()).as_path())
            );
            assert!(is_staging_attempt_name(
                &staged.root().file_name().unwrap().to_string_lossy()
            ));
        }
    }

    #[test]
    fn a_freshly_staged_bundle_validates() {
        // The loop that everything else in this ticket rests on: what the writer produces
        // is what the validator accepts, with no step in between.
        let (_root, staged) = stage_fixture(one_entry());
        let validated = validate(staged.root()).unwrap();
        assert_eq!(validated.path(), staged.root());

        // Including a revision with no documents at all: an empty required-entry set is
        // an identity, not an absence, and the manifest alone is a complete bundle. This
        // is also the only shape that exercises `required_entries` as an absent field,
        // which is the manifest's one defaulted key.
        let empty_root = TempDir::new().unwrap();
        let empty = stage_bundle(
            &mut RealPublicationIo,
            empty_root.path(),
            &candidate_of(vec![]),
        )
        .unwrap();
        assert_eq!(file_set(empty.root()), [MANIFEST_FILE]);
        assert!(manifest_of(empty.root()).required_entries().is_empty());
        assert!(validate(empty.root()).is_ok());
    }

    #[test]
    fn a_valid_bundle_of_another_revision_is_refused_when_a_revision_was_named() {
        // The half `validate` cannot answer. A bundle that agrees with its own manifest is
        // a valid bundle of *some* revision, and the publication protocol's adoption of an
        // already-present bundle at a final path treats it as *this* revision's. That is
        // sound only because the name under `bundles/` is always derived from the sealed
        // manifest — and "this protocol is the only writer there" is not an invariant, so
        // it is confirmed rather than assumed.
        let (_root, staged) = stage_fixture(one_entry());
        let (_other_root, other) = stage_fixture(vec![]);
        assert_ne!(staged.identity(), other.identity());

        let validated = validate_as(staged.root(), staged.identity()).unwrap();
        assert_eq!(validated.identity(), staged.identity());
        assert_eq!(validated.path(), staged.root());

        let refusal = validate_as(staged.root(), other.identity()).unwrap_err();
        assert_eq!(
            refusal,
            BundleRefusal::AnotherRevision(RevisionMismatch {
                expected: other.identity(),
                found: staged.identity(),
            })
        );
        // Kept out of `ValidationReport` because nothing here is damaged: `validate`, which
        // is never told what to expect, still accepts the same directory.
        assert_eq!(
            validate(staged.root()).unwrap().identity(),
            staged.identity()
        );
    }

    #[test]
    fn a_refused_bundle_displays_its_reason_and_implements_std_error() {
        // The publication protocol `?`s these and logs them, so every refusal needs
        // Display, and the damaged one must expose its report as a source rather than
        // flatten every finding into one line (the same rationale as
        // state/mutations.rs:638). The other two have no source: they wrap no cause, and
        // the values a reader needs are already in their messages.
        let (_root, staged) = stage_fixture(one_entry());
        let (_other_root, other) = stage_fixture(vec![]);

        let mismatch = validate_as(staged.root(), other.identity()).unwrap_err();
        let rendered = mismatch.to_string();
        assert!(rendered.contains(&staged.identity().to_hex()), "{rendered}");
        assert!(rendered.contains(&other.identity().to_hex()), "{rendered}");
        assert!(!rendered.contains("BundleRefusal"), "{rendered}");
        assert!(std::error::Error::source(&mismatch).is_none());

        // The adoption refusal names the files it found, because that list is the whole
        // repair instruction: delete them and the same rebuild publishes.
        let stray = BundleRefusal::UnaccountedMaterial(vec![".DS_Store".to_owned()]);
        let rendered = stray.to_string();
        assert!(rendered.contains(".DS_Store"), "{rendered}");
        assert!(!rendered.contains("BundleRefusal"), "{rendered}");
        assert!(std::error::Error::source(&stray).is_none());

        fs::remove_file(staged.root().join(MANIFEST_FILE)).unwrap();
        let damaged = validate_as(staged.root(), staged.identity()).unwrap_err();
        assert!(damaged.to_string().contains("manifest"), "{damaged}");
        assert!(std::error::Error::source(&damaged).is_some());
    }

    #[test]
    fn a_bundle_whose_manifest_cannot_be_read_is_refused() {
        // Manifest validation hashes bytes and does not parse them, so an intact but
        // undecodable manifest would otherwise publish and fail its first read
        // (docs/technical-design.md:651). Both an absent file and an undecodable one fail
        // closed, and the distinction between them belongs here because
        // `BundleManifest::parse` takes bytes and never a path.
        let (_root, staged) = stage_fixture(one_entry());
        fs::write(staged.root().join(MANIFEST_FILE), b"not json at all").unwrap();
        let report = validate(staged.root()).unwrap_err();
        assert!(report.manifest_unreadable.is_some(), "{report:?}");
        // Nothing else is knowable without a manifest, so no file is reported as a
        // surprise merely because the document that would have accounted for it is gone.
        assert_eq!(report.unexpected, Vec::<String>::new());
        assert_eq!(report.missing, Vec::<LogicalPath>::new());

        fs::remove_file(staged.root().join(MANIFEST_FILE)).unwrap();
        assert!(
            validate(staged.root())
                .unwrap_err()
                .manifest_unreadable
                .is_some()
        );
    }

    #[test]
    fn a_bundle_written_under_another_schema_is_refused_as_a_schema_mismatch() {
        // Kept apart from every other finding because the answer differs: a bundle from
        // another build is rebuilt, not repaired, and reporting it as damage would send a
        // reader looking for corruption that is not there.
        let (_root, staged) = stage_fixture(one_entry());
        edit_manifest(staged.root(), "\"schema\": 1", "\"schema\": 2");
        let report = validate(staged.root()).unwrap_err();
        assert_eq!(
            report.schema_mismatch,
            Some(SchemaMismatch {
                found: 2,
                supported: BUNDLE_SCHEMA_VERSION
            })
        );
        assert!(report.manifest_unreadable.is_none(), "{report:?}");
    }

    #[test]
    fn a_manifest_whose_identifier_is_not_its_own_digest_is_refused() {
        // The check `BundleManifest::parse` deliberately does not perform: a parsed
        // manifest's identifier is whatever the file said. Substituting another
        // well-formed 64-hex value keeps the document decodable, so this reaches
        // recomputation rather than failing as damage.
        let (_root, staged) = stage_fixture(one_entry());
        let claimed = manifest_of(staged.root()).identifier().to_hex();
        edit_manifest(staged.root(), &claimed, &"0".repeat(64));
        let report = validate(staged.root()).unwrap_err();
        assert!(report.identifier_mismatch, "{report:?}");
        // The entries themselves are untouched, so this finding stands alone.
        assert!(
            report.missing.is_empty() && report.changed.is_empty(),
            "{report:?}"
        );
    }

    #[test]
    fn a_required_entry_that_is_gone_is_reported_as_missing() {
        let (_root, staged) = stage_fixture(one_entry());
        fs::remove_file(resolve(staged.root(), &host_path(ENTRY_LIST_FILE))).unwrap();
        let report = validate(staged.root()).unwrap_err();
        assert_eq!(report.missing, [host_path(ENTRY_LIST_FILE)]);
        assert!(report.changed.is_empty(), "{report:?}");

        // A required path occupied by something that is not a regular file satisfies
        // nothing and is also unaccounted for: `fs::read` of a directory fails, and the
        // walk names it. Two findings for one anomaly is the honest report.
        let (_second_root, second) = stage_fixture(one_entry());
        let entry = resolve(second.root(), &host_path(ENTRY_LIST_FILE));
        fs::remove_file(&entry).unwrap();
        fs::create_dir(&entry).unwrap();
        let report = validate(second.root()).unwrap_err();
        assert_eq!(report.missing, [host_path(ENTRY_LIST_FILE)]);
        assert_eq!(report.unexpected, ["documents/entry-list.json"]);
    }

    #[test]
    fn a_required_entry_whose_bytes_changed_is_reported_as_changed() {
        // Distinguished from `missing` because the two mean different things to a person
        // reading the report: an absent file is an interrupted write, and a present file
        // with other bytes is an edit of an artifact that is supposed to be immutable.
        let (_root, staged) = stage_fixture(one_entry());
        fs::write(
            resolve(staged.root(), &host_path(ENTRY_LIST_FILE)),
            r#"{"entries":[]}"#,
        )
        .unwrap();
        let report = validate(staged.root()).unwrap_err();
        assert_eq!(report.changed, [host_path(ENTRY_LIST_FILE)]);
        assert!(report.missing.is_empty(), "{report:?}");
    }

    #[test]
    fn a_bundle_carrying_a_file_nothing_accounts_for_is_still_readable_and_says_so() {
        // Half of the split this validator makes, and the reason it makes it: a directory
        // whose every required entry is intact is a readable revision, whatever else a file
        // browser or a backup tool dropped beside it. The design's list is "the manifest,
        // every required entry" (docs/technical-design.md, "Revision bundles"); "and nothing
        // else" is the publication protocol's addition, so it decides at the commit point
        // and not here. `publish.rs::a_published_bundle_holding_a_stray_file_is_readable_but_
        // not_adoptable` is the other half.
        let (_root, staged) = stage_fixture(one_entry());
        fs::write(staged.root().join("stowaway.json"), b"{}").unwrap();
        fs::create_dir(staged.root().join("assets")).unwrap();
        fs::write(staged.root().join("assets").join("blob.png"), b"\x89PNG").unwrap();
        fs::write(staged.root().join("documents").join("extra.json"), b"{}").unwrap();

        let validated = validate(staged.root()).unwrap();
        // Sorted, and a stray directory is named once rather than enumerated: the finding
        // is a function of the directory, not of the order the platform walked it in, and
        // one name for the subtree is the whole evidence.
        assert_eq!(
            validated.unaccounted(),
            ["assets", "documents/extra.json", "stowaway.json"]
        );
        // And it is still this revision's bundle, addressed by identifier: a stray file
        // changes nothing about which revision the directory holds.
        assert_eq!(
            validate_as(staged.root(), staged.identity())
                .unwrap()
                .unaccounted(),
            validated.unaccounted()
        );
    }

    #[test]
    fn a_bundle_missing_a_required_entry_is_refused_even_though_a_stray_file_is_not() {
        // The negative control for the split above: excluding `unexpected` from validity
        // must not have excluded anything else. A directory carrying both findings is
        // refused, and the finding that refuses it is the entry it cannot serve.
        let (_root, staged) = stage_fixture(one_entry());
        fs::remove_file(resolve(staged.root(), &host_path(ENTRY_LIST_FILE))).unwrap();
        fs::write(staged.root().join("stowaway.json"), b"{}").unwrap();

        let report = validate(staged.root()).unwrap_err();
        assert_eq!(report.missing, [host_path(ENTRY_LIST_FILE)]);
        assert_eq!(report.unexpected, ["stowaway.json"]);
        assert!(!report.is_valid());
    }

    #[cfg(unix)]
    #[test]
    fn an_unexpected_name_that_is_not_a_valid_logical_path_is_still_reported() {
        // Why `unexpected` holds raw names. A backslash, a leading `..`, and bytes that
        // are not UTF-8 are all refused by `LogicalPath::parse`, so a report typed as
        // logical paths would have to drop exactly the stowaways whose names are most
        // suspicious. Unix-only because these names cannot be created on Windows.
        use std::os::unix::ffi::OsStrExt;

        let (_root, staged) = stage_fixture(one_entry());
        fs::write(staged.root().join("back\\slash"), b"{}").unwrap();
        fs::write(staged.root().join("..hidden"), b"{}").unwrap();
        let mut expected = vec!["..hidden".to_owned(), "back\\slash".to_owned()];
        // APFS enforces UTF-8 file names and refuses this one, so the lossy rendering is
        // asserted only where the filesystem can actually stage the case. Attempting it
        // is what keeps the claim honest on the platforms that can.
        let non_utf8 = staged.root().join(std::ffi::OsStr::from_bytes(b"raw-\xff"));
        if fs::write(&non_utf8, b"{}").is_ok() {
            expected.push("raw-\u{fffd}".to_owned());
            expected.sort();
        }

        assert_eq!(validate(staged.root()).unwrap().unaccounted(), expected);
    }

    #[test]
    fn a_bundle_whose_entry_list_is_undecodable_still_validates() {
        // Not a bug: manifest validation proves integrity, not readability. It hashes
        // bytes and does not parse them (docs/technical-design.md:651), so a document
        // that is hash-valid and undecodable publishes and then fails its first read.
        // That is exactly the defect the revision-bundle spike shipped — a serializer
        // that omitted empty collections with no matching read-side default — and it is
        // why every persisted type in this ticket carries an absent-field round-trip
        // test. Validation is not the mechanism that catches it, and pretending
        // otherwise here would put a second, weaker decoder in the publication path.
        let (_root, staged) = stage_fixture(one_entry());
        let entry = resolve(staged.root(), &host_path(ENTRY_LIST_FILE));
        // An `EntrySummary` with no `identifier`: valid JSON, correct hash, and no
        // read-side default that could rescue it.
        let payload = br#"{"entries":[{"category":"technology"}]}"#;
        let staged_hash = ContentHash::of(&fs::read(&entry).unwrap()).to_hex();
        fs::write(&entry, payload).unwrap();
        edit_manifest(
            staged.root(),
            &staged_hash,
            &ContentHash::of(payload).to_hex(),
        );
        // The manifest's identifier no longer matches its edited body, so restate it: the
        // point of this test is the entry hash, not the identifier check.
        let restated = manifest_of(staged.root());
        edit_manifest(
            staged.root(),
            &restated.identifier().to_hex(),
            &restated.recomputed_identity().to_hex(),
        );

        assert!(validate(staged.root()).is_ok());
        assert!(serde_json::from_slice::<EntryList>(payload).is_err());
    }

    #[test]
    fn untrusted_document_content_cannot_influence_any_path() {
        // "The revision identifier and final path are generated by the host rather than
        // derived from untrusted mod names" (docs/technical-design.md, "Revision
        // bundles"). A Stellaris identifier is raw mod content; here one is a traversal.
        // It must reach the payload and nothing else — no file appears outside the
        // staging directory, and the file set is the same one a harmless entry produces.
        let root = TempDir::new().unwrap();
        let hostile = stage_bundle(
            &mut RealPublicationIo,
            root.path(),
            &candidate(vec![EntrySummary {
                category: "../../escape".to_owned(),
                identifier: "../../escape".to_owned(),
                display_name: Some("../../escape".to_owned()),
            }]),
        )
        .unwrap();

        assert_eq!(
            file_set(hostile.root()),
            ["documents/entry-list.json", "manifest.json"]
        );
        assert_eq!(file_set(root.path()).len(), 2);
        assert!(validate(hostile.root()).is_ok());

        let payload =
            fs::read_to_string(resolve(hostile.root(), &host_path(ENTRY_LIST_FILE))).unwrap();
        assert!(payload.contains("../../escape"), "{payload}");
    }

    #[test]
    fn the_document_path_is_derived_from_identity_and_stays_inside_the_bundle() {
        // One decision, made once: the path is a function of `DocumentIdentity`, so
        // `RevisionCandidate`'s refusal of two documents of one identity is what makes two
        // bundle paths unable to collide. Nothing re-decides it per call site.
        let document = RevisionDocument::EntryList(EntryList {
            entries: one_entry(),
        });
        assert_eq!(document_path(&document), host_path(ENTRY_LIST_FILE));

        let other = RevisionDocument::EntryList(EntryList {
            entries: Vec::new(),
        });
        assert_eq!(document_path(&document), document_path(&other));

        // Documents live under a directory of their own and never beside the manifest,
        // so a later phase's non-document material cannot collide with one.
        assert_ne!(document_path(&document), host_path(MANIFEST_FILE));
        assert_eq!(
            document_path(&document).components().next(),
            Some("documents")
        );
    }

    #[test]
    fn the_layout_places_staging_beside_bundles_under_one_root() {
        // Adjacency is what makes the move a rename rather than a cross-device copy, and
        // it is structural rather than a caller's discipline. The final path is 64
        // lowercase hex, which is also the sweep's rule for what under `bundles/` is junk.
        let root = Path::new("/app/revisions");
        let identity = stage_fixture(vec![]).1.identity();
        assert_eq!(staging_root(root), root.join("staging"));
        assert_eq!(
            bundle_path(root, identity),
            root.join("bundles").join(identity.to_hex())
        );
        assert_eq!(
            staging_root(root).parent(),
            bundle_path(root, identity).parent().unwrap().parent()
        );
    }

    #[test]
    fn only_the_generated_staging_name_spelling_is_recognized() {
        // The sweep deletes what this says yes to, so a loose match is a data-loss bug: a
        // directory a user dropped into `staging/` must not be recognized because it
        // happens to resemble a UUID in some other spelling.
        let generated = uuid::Uuid::new_v4().hyphenated().to_string();
        assert!(is_staging_attempt_name(&generated));
        for other in [
            generated.replace('-', ""),
            generated.to_uppercase(),
            format!("{{{generated}}}"),
            format!("urn:uuid:{generated}"),
            "not-a-uuid".to_owned(),
            String::new(),
            ".".to_owned(),
        ] {
            assert!(!is_staging_attempt_name(&other), "recognized {other}");
        }
    }

    #[test]
    fn staging_and_validation_errors_display_and_implement_std_error() {
        // Task 6's protocol `?` and logs these; both need Display, and std::error::Error
        // is what makes `?` conversion available (the same rationale as
        // state/mutations.rs:638).
        fn assert_error<E: std::error::Error>(error: &E) -> String {
            error.to_string()
        }

        let staging = StageError {
            staging: PathBuf::from("/app/revisions/staging/attempt"),
            detail: "writing manifest.json: injected write failure".to_owned(),
        };
        let rendered = assert_error(&staging);
        assert!(
            rendered.contains("/app/revisions/staging/attempt"),
            "{rendered}"
        );
        assert!(rendered.contains("injected write failure"), "{rendered}");
        assert!(!rendered.contains("StageError"), "{rendered}");

        let report = ValidationReport {
            manifest_unreadable: Some("expected value at line 1 column 1".to_owned()),
            schema_mismatch: Some(SchemaMismatch {
                found: 2,
                supported: 1,
            }),
            identifier_mismatch: true,
            missing: vec![host_path(ENTRY_LIST_FILE)],
            changed: vec![host_path("documents/other.json")],
            unexpected: vec!["stowaway.json".to_owned()],
        };
        let rendered = assert_error(&report);
        for expected in [
            "line 1 column 1",
            "must be rebuilt",
            "identifier",
            ENTRY_LIST_FILE,
            "documents/other.json",
            "stowaway.json",
        ] {
            assert!(
                rendered.contains(expected),
                "{expected} missing from {rendered}"
            );
        }
        assert!(!rendered.contains("ValidationReport"), "{rendered}");

        // Total for the empty value too: a report with no findings is what a valid bundle
        // produces, and rendering it as a failure would misreport a healthy revision.
        assert_eq!(
            ValidationReport::default().to_string(),
            "revision bundle validated"
        );
        assert!(ValidationReport::default().is_valid());
    }
}
