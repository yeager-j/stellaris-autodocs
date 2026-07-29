//! The corpora an acceptance case names, and what each one currently contributes.
//!
//! A corpus is the left-hand side of the acceptance diagram: the pair of Source Snapshots a
//! revision is built from, plus the identity inputs the Mod Installation is derived through.
//!
//! # What a corpus contributes today, and what it does not
//!
//! Three things reach a published revision from a corpus, and only one of them is *derived*:
//!
//! - Its `location_path` and `mod_root`, which [`AcceptanceThread::boot`] turns into the Mod
//!   Installation identifier the revision documents.
//! - Its [`documentation_typed_by_hand`](AcceptanceCorpus::documenting), published unchanged.
//! - Its snapshot **bytes — and those reach a revision as exactly two values, the fingerprints
//!   in its [`RevisionInputs`], because nothing parses them.**
//!
//! That last line is the one a reader must not soften. The documentation field is a stand-in for
//! what `analysis` will derive from those bytes in Phase 6; until then `tech_a` appearing both in
//! a fixture file and in an entry summary is the author typing it twice, not a derivation.
//! `published_thread::the_fixture_bytes_reach_the_revision_and_nothing_a_reader_can_see` asserts
//! that gap rather than leaving this paragraph as the only place it is stated.
//!
//! [`AcceptanceThread::boot`]: crate::harness::AcceptanceThread::boot

use std::path::{Path, PathBuf};

use stellaris_docs_lib::analysis::version::AnalysisVersionVector;
use stellaris_docs_lib::canonical::path::LogicalPath;
use stellaris_docs_lib::discovery::identity::ModInstallationId;
use stellaris_docs_lib::revisions::{
    EntryList, EntrySummary, RevisionCandidate, RevisionDocument, RevisionInputs,
};
use stellaris_docs_lib::source::fixture::FixtureCorpus;
use stellaris_docs_lib::source::snapshot::{SourceKind, SourceSnapshot};

/// One acceptance run's source observations: the Target Mod, the Vanilla Content it is
/// documented against, and where the mod is installed.
pub struct AcceptanceCorpus {
    name: String,
    target_mod: SourceSnapshot,
    vanilla_content: SourceSnapshot,
    location_path: PathBuf,
    mod_root: LogicalPath,
    /// The stand-in for what Phase 6's `analysis` will derive from the bytes above.
    ///
    /// **This field is a Phase 6 deletion target** (docs/implementation-plan.md, Phase 6 entry
    /// conditions). It lives on the corpus rather than being a parameter of
    /// `AcceptanceThread::boot` so that deleting it is a field removal: no acceptance case
    /// names it, so no acceptance case changes when analysis starts producing the real thing.
    documentation_typed_by_hand: Vec<RevisionDocument>,
}

impl AcceptanceCorpus {
    /// Named so a failure over a parameterised suite says which corpus failed rather than
    /// which line did.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Borrowed rather than owned, and that is the forward-compatibility decision, not a
    /// convenience: `source::snapshot::establish` yields a `LiveSource` that exposes only
    /// `snapshot(&self) -> &SourceSnapshot`, and `SourceSnapshot` is not `Clone` (it holds a
    /// `Mutex` of captured asset reads). Phase 4 task 8 can therefore make these fields an
    /// enum over fixture and live backing without touching this signature, the harness, or a
    /// single case — which is what "the harness accepts `SourceSnapshot` values rather than
    /// assuming memory backing" has to mean in practice.
    pub fn target_mod(&self) -> &SourceSnapshot {
        &self.target_mod
    }

    pub fn vanilla_content(&self) -> &SourceSnapshot {
        &self.vanilla_content
    }

    /// Where the mod is installed. A marker for a fixture corpus; a real workshop root once
    /// Phase 4 task 8 points a run at an installed Vanilla and ACOT.
    pub fn location_path(&self) -> &Path {
        &self.location_path
    }

    pub fn mod_root(&self) -> &LogicalPath {
        &self.mod_root
    }

    /// The two fingerprints a revision records as the observations it was built from — this
    /// phase, the whole of what a corpus's *bytes* contribute to a published revision. Its mod
    /// root and its documentation reach one by other routes; see the module comment.
    pub fn inputs(&self) -> RevisionInputs {
        RevisionInputs {
            target_mod: self.target_mod.fingerprint(),
            vanilla_content: self.vanilla_content.fingerprint(),
        }
    }

    /// The candidate this corpus stands in for.
    ///
    /// Takes `&self` so a case can still ask the corpus what its fingerprints were after the
    /// thread has booted, which is what the honesty control compares.
    pub(crate) fn candidate(&self, installation: ModInstallationId) -> RevisionCandidate {
        RevisionCandidate::new(
            installation,
            self.inputs(),
            AnalysisVersionVector::current(),
            // Hand-authored documentation can honestly say "complete": the typed unsupported
            // and unresolved facts a real analysis retains arrive with the analysis that
            // produces them, and no case this phase can assert on completeness.
            true,
            self.documentation_typed_by_hand.clone(),
        )
        .expect("a candidate carrying at most one document of each identity")
    }

    fn fixture(name: &str, target_mod: SourceSnapshot, vanilla_content: SourceSnapshot) -> Self {
        Self {
            name: name.to_owned(),
            target_mod,
            vanilla_content,
            // A marker, not a directory, and the spelling is safe on the Windows CI leg for a
            // reason worth stating: nothing resolves, joins, or canonicalizes this path. `state`
            // stores any UTF-8 path without checking that it exists, no Phase 3 code traverses a
            // Discovery Location, and the installation identifier is derived from the location's
            // *identifier* and the mod root rather than from this. Phase 4 task 8 is where it
            // becomes a real workshop root and where that stops being true.
            location_path: PathBuf::from(format!("/fixture-workshop/{name}")),
            mod_root: LogicalPath::parse("fixture_mod").expect("a literal logical path"),
            documentation_typed_by_hand: Vec::new(),
        }
    }

    fn documenting(mut self, documents: Vec<RevisionDocument>) -> Self {
        self.documentation_typed_by_hand = documents;
        self
    }
}

/// The ordinary case: a mod that adds a technology, documented by one entry.
pub fn trivial() -> AcceptanceCorpus {
    AcceptanceCorpus::fixture(
        "trivial",
        target_mod(b"tech_a = { cost = 1 }\n"),
        vanilla_content(b"tech_vanilla = { cost = 2 }\n"),
    )
    .documenting(entry_list(vec![EntrySummary {
        category: "technology".to_owned(),
        identifier: "tech_a".to_owned(),
        display_name: Some("Fixture Technology".to_owned()),
    }]))
}

/// The same hand-authored documentation as [`trivial`], over different fixture bytes.
///
/// Its only job is to be what
/// `published_thread::the_fixture_bytes_reach_the_revision_and_nothing_a_reader_can_see` rebuilds
/// over: a second observation of the same installation that differs in everything a fingerprint
/// covers and in nothing a reader can see.
pub fn trivial_over_different_bytes() -> AcceptanceCorpus {
    AcceptanceCorpus::fixture(
        "trivial-over-different-bytes",
        target_mod(b"tech_a = { cost = 7 }\n"),
        vanilla_content(b"tech_vanilla = { cost = 11 }\n"),
    )
    .documenting(entry_list(vec![EntrySummary {
        category: "technology".to_owned(),
        identifier: "tech_a".to_owned(),
        display_name: Some("Fixture Technology".to_owned()),
    }]))
}

/// A corpus whose revision carries an entry list documenting nothing.
pub fn documents_nothing() -> AcceptanceCorpus {
    AcceptanceCorpus::fixture(
        "documents-nothing",
        target_mod(b"# a mod file this build documents nothing from\n"),
        vanilla_content(b"tech_vanilla = { cost = 3 }\n"),
    )
    .documenting(entry_list(Vec::new()))
}

/// A corpus whose revision carries no entry-list document at all — the absent half of the
/// empty-versus-absent line the reader draws.
///
/// **Phase 6 cannot produce this input.** A candidate with zero documents is constructible only
/// while candidates are typed by hand; an `analysis` that documented nothing would still emit
/// an empty entry list, which is [`documents_nothing`]. Recorded in Phase 6's entry-condition
/// checklist as a case to relocate to a focused `application::read` test or delete, so it is a
/// decision then rather than a surprise.
pub fn carries_no_entry_list() -> AcceptanceCorpus {
    AcceptanceCorpus::fixture(
        "carries-no-entry-list",
        target_mod(b"tech_undocumented = { cost = 5 }\n"),
        vanilla_content(b"tech_vanilla = { cost = 13 }\n"),
    )
    .documenting(Vec::new())
}

/// The three golden-case fixture mods, as corpora this thread runs over.
///
/// # What these prove here, and where they are actually observed
///
/// Only what the thread can see: the corpus is a distinct observation, its build publishes, and
/// the read serves the revision back. **They are not parsed here** — see the module comment, and
/// the honesty control that asserts it. `analysis::resolver` and `analysis::parser` are
/// crate-private, so "this fixture parses with these faults" and "this row refuses on this named
/// cell" are not claims this target is able to make at all. They are made in-crate, by
/// `analysis::resolver::golden`, over the same committed bytes.
///
/// That split is why these constructors are worth having anyway. The fixtures reach the thread
/// the production way now, so when Phase 6 makes bytes stop being inert these three cases become
/// golden cases 2, 3, and 4 end to end without a corpus being introduced at that moment.
///
/// # Why these do read `include_bytes!`
///
/// [`target_mod`]'s comment explains why the Phase 3 corpora do not borrow `fixtures/oracle/*`:
/// those bytes are frozen, their SHA-256 pinned in every captured record's manifest, so an edit
/// there invalidates evidence. `fixtures/resolver/*` are pinned by nothing — they are edited by
/// resolver work as a matter of course — and no case in this target depends on a revision
/// identifier's *value*, only on two of them differing. So the coupling that argument is about
/// does not exist here, and the alternative would be worse: a corpus whose bytes were a
/// paraphrase of the fixture would share its name and not its content.
pub fn malformed() -> AcceptanceCorpus {
    AcceptanceCorpus::fixture(
        "malformed",
        corpus(SourceKind::TargetMod, MALFORMED),
        corpus(SourceKind::VanillaContent, MALFORMED_VANILLA),
    )
    .documenting(entry_list(vec![technology("tech_malformed_clean")]))
}

/// Golden case 2's shape. See [`malformed`] for what a constructor here does and does not prove.
pub fn zero_weight() -> AcceptanceCorpus {
    AcceptanceCorpus::fixture(
        "zero-weight",
        corpus(SourceKind::TargetMod, ZERO_WEIGHT),
        corpus(SourceKind::VanillaContent, ZERO_WEIGHT_VANILLA),
    )
    .documenting(entry_list(vec![technology("tech_zero_weight_subject")]))
}

/// Golden case 3's shape. See [`malformed`] for what a constructor here does and does not prove.
pub fn enigmalith() -> AcceptanceCorpus {
    AcceptanceCorpus::fixture(
        "enigmalith",
        corpus(SourceKind::TargetMod, ENIGMALITH),
        corpus(SourceKind::VanillaContent, ENIGMALITH_VANILLA),
    )
    .documenting(entry_list(vec![technology("tech_enigmalith_subject")]))
}

/// One hand-authored entry summary.
///
/// Hand-authored in the full sense the module comment means: the identifier below appears in a
/// fixture file *and* here because it was typed twice, and nothing checks that the two agree.
/// Phase 6 deletes this by deriving the entry list from the bytes.
///
/// No `display_name`, unlike [`trivial`]'s. A display name is localization output — Phase 5
/// generates it — so inventing one here would be inventing the one field these corpora have no
/// source for, in files that carry no `localisation/` directory at all.
fn technology(identifier: &str) -> EntrySummary {
    EntrySummary {
        category: "technology".to_owned(),
        identifier: identifier.to_owned(),
        display_name: None,
    }
}

macro_rules! fixture {
    ($path:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../fixtures/resolver/",
            $path
        ))
    };
}

const MALFORMED: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("malformed/descriptor.mod")),
    (
        "common/technology/malformed_intact.txt",
        fixture!("malformed/common/technology/malformed_intact.txt"),
    ),
    (
        "common/technology/malformed_recovery.txt",
        fixture!("malformed/common/technology/malformed_recovery.txt"),
    ),
    (
        "common/technology/malformed_stray_brace.txt",
        fixture!("malformed/common/technology/malformed_stray_brace.txt"),
    ),
];

const MALFORMED_VANILLA: &[(&str, &[u8])] = &[(
    "common/technology/00_malformed_baseline_tech.txt",
    fixture!("malformed-vanilla/common/technology/00_malformed_baseline_tech.txt"),
)];

const ZERO_WEIGHT: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("zero-weight/descriptor.mod")),
    (
        "common/technology/zz_zero_weight_tech.txt",
        fixture!("zero-weight/common/technology/zz_zero_weight_tech.txt"),
    ),
];

const ZERO_WEIGHT_VANILLA: &[(&str, &[u8])] = &[(
    "common/technology/00_zero_weight_baseline_tech.txt",
    fixture!("zero-weight-vanilla/common/technology/00_zero_weight_baseline_tech.txt"),
)];

const ENIGMALITH: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("enigmalith/descriptor.mod")),
    (
        "common/megastructures/zz_enigmalith_megastructures.txt",
        fixture!("enigmalith/common/megastructures/zz_enigmalith_megastructures.txt"),
    ),
    (
        "common/technology/zz_enigmalith_tech.txt",
        fixture!("enigmalith/common/technology/zz_enigmalith_tech.txt"),
    ),
    (
        "events/zz_enigmalith_events.txt",
        fixture!("enigmalith/events/zz_enigmalith_events.txt"),
    ),
];

const ENIGMALITH_VANILLA: &[(&str, &[u8])] = &[(
    "common/scripted_variables/00_enigmalith_constants.txt",
    fixture!("enigmalith-vanilla/common/scripted_variables/00_enigmalith_constants.txt"),
)];

/// One corpus table, with the fixture directory it claims to be the whole of.
type FixtureTree = (&'static str, &'static [(&'static str, &'static [u8])]);

/// Every fixture tree above, paired with its directory name.
const TREES: &[FixtureTree] = &[
    ("malformed", MALFORMED),
    ("malformed-vanilla", MALFORMED_VANILLA),
    ("zero-weight", ZERO_WEIGHT),
    ("zero-weight-vanilla", ZERO_WEIGHT_VANILLA),
    ("enigmalith", ENIGMALITH),
    ("enigmalith-vanilla", ENIGMALITH_VANILLA),
];

fn corpus(kind: SourceKind, files: &[(&str, &[u8])]) -> SourceSnapshot {
    files
        .iter()
        .fold(FixtureCorpus::new(kind), |corpus, (logical, bytes)| {
            corpus.with_file(logical, bytes)
        })
        .build()
        .expect("a committed fixture corpus establishes")
}

/// The gate that makes the tables above safe to state twice.
///
/// `analysis::resolver::trial` names the same six trees for the in-crate expectations, and it is
/// crate-private, so this target cannot borrow its tables and the file lists are genuinely
/// duplicated. Duplicated knowledge with no check is how a fixture file gets added for one suite
/// and silently skipped by the other — and a *skipped* file is the failure mode that hides, since
/// every corpus still builds and every case still passes.
///
/// So the committed directory is the authority and both tables are derivations of it. This walks
/// the tree with `std::fs`, which is a test-time read of a compile-time-included fixture and not a
/// widening of `source::fixture`'s no-`from_directory` rule: nothing here builds a snapshot from
/// what it finds.
#[test]
fn every_committed_fixture_file_reaches_a_corpus() {
    for (tree, declared) in TREES {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/resolver")
            .join(tree);
        let mut committed = Vec::new();
        collect(&root, &root, &mut committed);
        committed.sort();

        let mut named: Vec<String> = declared
            .iter()
            .map(|(logical, _)| (*logical).to_owned())
            .collect();
        named.sort();

        assert_eq!(
            committed, named,
            "fixtures/resolver/{tree}/ and its corpus table disagree about which files exist",
        );
    }
}

fn collect(root: &Path, directory: &Path, found: &mut Vec<String>) {
    for entry in std::fs::read_dir(directory).expect("a committed fixture directory is readable") {
        let path = entry.expect("a readable directory entry").path();
        if path.is_dir() {
            collect(root, &path, found);
        } else {
            let logical = path
                .strip_prefix(root)
                .expect("every entry is under the tree root")
                .to_str()
                .expect("a fixture path is UTF-8")
                .replace('\\', "/");
            found.push(logical);
        }
    }
}

/// Every Phase 3 fixture corpus is built through [`FixtureCorpus`] rather than `include_bytes!` of
/// `fixtures/oracle/*`: those bytes are the resolver's evidence, frozen because every captured
/// record's manifest pins their SHA-256, and borrowing them would tie this target's revision
/// identifiers to edits made for an unrelated reason. The golden-case corpora above read
/// `fixtures/resolver/*`, which neither of those things is true of; the reasoning is with them.
fn target_mod(technology: &[u8]) -> SourceSnapshot {
    FixtureCorpus::new(SourceKind::TargetMod)
        .with_file(
            "descriptor.mod",
            b"name=\"Acceptance Fixture\"\nsupported_version=\"4.4\"\n",
        )
        .with_file("common/technology/00_fixture.txt", technology)
        .build()
        .expect("the target-mod fixture corpus is well formed")
}

fn vanilla_content(technology: &[u8]) -> SourceSnapshot {
    FixtureCorpus::new(SourceKind::VanillaContent)
        .with_file("common/technology/00_vanilla.txt", technology)
        .build()
        .expect("the vanilla fixture corpus is well formed")
}

fn entry_list(entries: Vec<EntrySummary>) -> Vec<RevisionDocument> {
    vec![RevisionDocument::EntryList(EntryList { entries })]
}
