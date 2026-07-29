//! The corpora and registry rows the core proves itself against.
//!
//! Test-only, and deliberately **not** the real registries. Technologies, events, sprites,
//! and localization are each their own ticket because each is a unit of oracle evidence that
//! deserves its own review (`docs/implementation-plan.md`, "Ticketing"). What the core has to
//! demonstrate is narrower and prior to all of them: that one accept-on-repeat and one
//! reject-on-repeat registry, given the same early-sorting mod file, produce *opposite*
//! winners. Two synthetic rows show that without pre-empting a row ticket's judgment, and
//! the real rows re-assert it later with their own evidence.
//!
//! The corpora are committed under `fixtures/resolver/` and read with `include_bytes!` — a
//! compile-time read, so nothing here traverses a filesystem or depends on a host.

use crate::source::SourceKind;
use crate::source::fixture::FixtureCorpus;
use crate::source::snapshot::SourceSnapshot;

use super::registry::{
    CellStatus, CrossSourceRule, FieldRule, KeyRule, NO_REFERENCES, OrderingRule, ProvenanceRule,
    RegistryPolicy, RepeatRule, Replacement, ShadowUnit, StreamScope, top_level_definitions,
};
use super::resolved::FactKind;
use super::stream::{ContentFamily, FileScope};

macro_rules! fixture {
    ($path:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../fixtures/resolver/",
            $path
        ))
    };
}

/// The stand-in base-game file set. Everything sorts as `00_…`, so an `!!!_…` mod file is
/// read before all of it and a `zz_…` one after all of it.
pub(super) const VANILLA: &[(&str, &[u8])] = &[
    (
        "common/technology/00_baseline_tech.txt",
        fixture!("vanilla/common/technology/00_baseline_tech.txt"),
    ),
    (
        "common/technology/00_collided_tech.txt",
        fixture!("vanilla/common/technology/00_collided_tech.txt"),
    ),
    (
        "events/00_baseline_notices.txt",
        fixture!("vanilla/events/00_baseline_notices.txt"),
    ),
];

/// `r10-loadorder`: one early-sorting filename applied to both registry rules at once.
pub(super) const EARLY_MOD: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("early-mod/descriptor.mod")),
    (
        "common/technology/!!!_early_tech.txt",
        fixture!("early-mod/common/technology/!!!_early_tech.txt"),
    ),
    (
        "events/!!!_early_notices.txt",
        fixture!("early-mod/events/!!!_early_notices.txt"),
    ),
];

/// `r6-pathcollision`: a mod file at a vanilla file's exact logical path.
pub(super) const PATH_COLLISION: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("path-collision/descriptor.mod")),
    (
        "common/technology/00_collided_tech.txt",
        fixture!("path-collision/common/technology/00_collided_tech.txt"),
    ),
];

/// `r3-replace-path`: a directory replacement plus one of the declarer's own files in it.
pub(super) const REPLACE_PATH: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("replace-path/descriptor.mod")),
    (
        "common/technology/zz_replacement_tech.txt",
        fixture!("replace-path/common/technology/zz_replacement_tech.txt"),
    ),
];

/// The stand-in base game golden case 5 redefines. Separate from [`VANILLA`], whose exact key
/// list the r6 and r3 expectations assert.
pub(super) const REDEFINITION_VANILLA: &[(&str, &[u8])] = &[(
    "common/technology/00_redefinition_tech.txt",
    fixture!("redefinition-vanilla/common/technology/00_redefinition_tech.txt"),
)];

/// `r1-target`: a late-sorting mod file redefining vanilla technologies, one of them omitting
/// `potential`.
pub(super) const REDEFINITION: &[(&str, &[u8])] = &[
    ("descriptor.mod", fixture!("redefinition/descriptor.mod")),
    (
        "common/technology/zz_redefinition_tech.txt",
        REDEFINITION_BODY,
    ),
];

/// The same bytes under an early-sorting name, restating `r4-reordered`'s method: identical
/// content, swapped filename, and the winner must move.
pub(super) const REDEFINITION_FLIPPED: &[(&str, &[u8])] = &[
    (
        "descriptor.mod",
        fixture!("redefinition-flipped/descriptor.mod"),
    ),
    (
        "common/technology/!!!_redefinition_tech.txt",
        REDEFINITION_FLIPPED_BODY,
    ),
];

/// The `r0-baseline` shape: a Target Mod that redefines nothing, so the vanilla definitions
/// are read with nothing contesting them. What makes the `r1` result a delta.
pub(super) const NO_REDEFINITION: &[(&str, &[u8])] =
    &[("descriptor.mod", fixture!("redefinition/descriptor.mod"))];

/// Named separately from the tables above so the byte identity the flip depends on can be
/// asserted rather than assumed.
pub(super) const REDEFINITION_BODY: &[u8] =
    fixture!("redefinition/common/technology/zz_redefinition_tech.txt");
pub(super) const REDEFINITION_FLIPPED_BODY: &[u8] =
    fixture!("redefinition-flipped/common/technology/!!!_redefinition_tech.txt");

pub(super) fn corpus(kind: SourceKind, files: &[(&str, &[u8])]) -> SourceSnapshot {
    files
        .iter()
        .fold(FixtureCorpus::new(kind), |corpus, (logical, bytes)| {
            corpus.with_file(logical, bytes)
        })
        .build()
        .expect("a committed fixture corpus establishes")
}

pub(super) fn vanilla() -> SourceSnapshot {
    corpus(SourceKind::VanillaContent, VANILLA)
}

pub(super) fn redefinition_vanilla() -> SourceSnapshot {
    corpus(SourceKind::VanillaContent, REDEFINITION_VANILLA)
}

const TECHNOLOGY_SCOPE: FileScope = FileScope {
    directory: "common/technology",
    extensions: &["txt"],
    recursive: false,
};

const EVENT_SCOPE: FileScope = FileScope {
    directory: "events",
    extensions: &["txt"],
    recursive: false,
};

/// A row with every cell settled, so a test can vary exactly one of them.
const fn settled(
    name: &'static str,
    scope: FileScope,
    duplicates: RepeatRule,
    fields: FieldRule,
    kinds: &'static [FactKind],
) -> RegistryPolicy {
    RegistryPolicy {
        name,
        key: CellStatus::Resolved(KeyRule {
            reader: top_level_definitions,
            shadow: ShadowUnit::CommonFileSelection,
        }),
        stream: CellStatus::Resolved(StreamScope {
            family: ContentFamily::Script,
            scope,
        }),
        duplicates: CellStatus::Resolved(duplicates),
        cross_source: CellStatus::Resolved(CrossSourceRule::DecidedByStreamPosition),
        fields: CellStatus::Resolved(fields),
        ordering: CellStatus::Resolved(OrderingRule::SourceOrderPreserved),
        references: CellStatus::Resolved(NO_REFERENCES),
        provenance: CellStatus::Resolved(ProvenanceRule { kinds }),
    }
}

const WHOLE_OBJECT: FieldRule = FieldRule {
    replacement: Replacement::WholeObject,
    defaults: &[],
};

const CONTESTED_KINDS: &[FactKind] = &[
    FactKind::Contributed,
    FactKind::Duplicate,
    FactKind::Shadowed,
];

/// Later replaces earlier. To win, a mod's file must sort **after** the one it overrides —
/// the group technologies, buildings, megastructures, scripted triggers, and effects belong
/// to.
pub(super) const REPLACE_ON_REPEAT: RegistryPolicy = settled(
    "trial-replace-on-repeat",
    TECHNOLOGY_SCOPE,
    RepeatRule::ReplaceOnRepeat,
    WHOLE_OBJECT,
    CONTESTED_KINDS,
);

/// Later is rejected. To win, a mod's file must sort **before** — the group events and
/// scripted constants belong to.
pub(super) const REJECT_ON_REPEAT: RegistryPolicy = settled(
    "trial-reject-on-repeat",
    EVENT_SCOPE,
    RepeatRule::RejectOnRepeat,
    WHOLE_OBJECT,
    CONTESTED_KINDS,
);

/// The same scope as [`REPLACE_ON_REPEAT`] with the rule inverted.
///
/// Not a row anybody would declare: it exists so the r10 pair can be shown to *fail* when
/// the repeat rule is wrong, without editing the row the passing assertion uses.
pub(super) const REPLACE_SCOPE_REJECTING: RegistryPolicy = settled(
    "trial-replace-scope-rejecting",
    TECHNOLOGY_SCOPE,
    RepeatRule::RejectOnRepeat,
    WHOLE_OBJECT,
    CONTESTED_KINDS,
);

/// The same scope as [`REJECT_ON_REPEAT`] with the rule inverted, for the other half of the
/// same negative control.
pub(super) const EVENT_SCOPE_REPLACING: RegistryPolicy = settled(
    "trial-event-scope-replacing",
    EVENT_SCOPE,
    RepeatRule::ReplaceOnRepeat,
    WHOLE_OBJECT,
    CONTESTED_KINDS,
);

pub(super) const fn with_fields(
    name: &'static str,
    fields: FieldRule,
    kinds: &'static [FactKind],
) -> RegistryPolicy {
    settled(
        name,
        TECHNOLOGY_SCOPE,
        RepeatRule::ReplaceOnRepeat,
        fields,
        kinds,
    )
}

/// The technologies row's field rule inverted, for the control that separates whole-object
/// replacement from inheritance.
///
/// The two rules agree on every assertion golden case 5 makes *except* whether an omitted
/// field comes back, which is the whole reason the omitted-`potential` case is the design's
/// first mandatory oracle case. Declared on a copy so the control cannot be left switched on,
/// and declaring [`FactKind::Inherited`] so the control fails on the field rule rather than on
/// an undeclared kind — a refusal would be a pass for the wrong reason.
pub(super) const TECHNOLOGY_SCOPE_INHERITING: RegistryPolicy = with_fields(
    "trial-technology-scope-inheriting",
    FieldRule {
        replacement: Replacement::InheritAbsentFields,
        defaults: &[],
    },
    &[
        FactKind::Contributed,
        FactKind::Inherited,
        FactKind::Duplicate,
        FactKind::Shadowed,
    ],
);
