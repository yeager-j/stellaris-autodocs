//! The whole-corpus parse-and-resolve run: every declared Resolution Profile row, resolved
//! over the installed Vanilla and ACOT corpora, with the outcome recorded and drift-checked.
//!
//! # What this run is for
//!
//! Fixtures prove each row's policy against hand-authored collisions; the oracle records
//! prove the policies against the game. Neither says what the resolver does when it meets
//! the real corpora whole — how many definitions each row actually yields, which open cells
//! real content actually hits, and how often. This run answers that, and pins the answer:
//! from Phase 4M onward it is a required acceptance activity, and a failure after a game or
//! mod update is signal, not noise (`docs/implementation-plan.md`, Phase 4 task 8).
//!
//! Every declared row is asked for, and both outcomes are recorded on equal terms: a
//! resolved row records its definition and typed-fact counts, and a refusing row records
//! the typed refusal — the policy cell it stopped at and the reason. **A hit on an
//! unresolved cell is a visible failure to record, never an error**: a run that panicked on
//! `megastructures`' open field cell could never record the open cells it exists to witness,
//! and a run that skipped the row would report a support surface the profile does not claim.
//!
//! # Running it
//!
//! The corpora are a licensed local installation, so this is not part of the ordinary suite:
//! plain `cargo test` never selects it, and normal CI does not need Stellaris installed.
//!
//! ```text
//! cargo test --features test-support parse_and_resolve_conformance -- --ignored --nocapture
//! ```
//!
//! Roots are environment-overridable — `STELLARIS_INSTALL_ROOT` and
//! `STELLARIS_WORKSHOP_ROOT`, defaulting to the macOS Steam locations (see
//! [`crate::analysis::corpora`]). If the run is asked for and a root is missing it
//! **fails**; it does not skip.
//!
//! Pass `PARSE_AND_RESOLVE_CAPTURE=1` to write the record under
//! `docs/conformance/parser/c2-parse-and-resolve/`. Without it the run checks against the
//! committed record and writes nothing.
//!
//! # Re-run it when
//!
//! - the **Stellaris build** or the **installed ACOT version** changes,
//! - the **Resolution Profile** changes — a row added, a cell settled, a policy revised,
//! - anything the parser conformance run re-runs for (`docs/adr/0008`): the parse this run
//!   resolves over is the production adapter's.
//!
//! # Negative controls
//!
//! The drift comparisons for the resolution block have CI-runnable negative controls in
//! [`crate::analysis::conformance`]. The gate over a real corpus was additionally proven red
//! once (STE-33): pointing `STELLARIS_WORKSHOP_ROOT` at a copy of ACOT with one scripted
//! variable, `@ste33_drift_control = 1`, appended to a scripted-variables file failed the
//! run with
//!
//! ```text
//! corpus acot: fingerprint 6f6095c2… -> 649d9a62… (1120 files, 18521905 bytes recorded;
//!                                                   1120 files, 18521931 bytes observed)
//! row scripted-constants: definitions 3367 -> 3368
//! row scripted-constants: fact constants.Resolved 3367 -> 3368
//! row scripted-constants: semantic digest 2a39cf0d… -> cc7e4527…. The source is unchanged
//!   if no fingerprint drifted above, so this is the resolver producing different output
//!   from the same bytes — a winner swap or a policy change can move this while every
//!   count holds still.
//! artifact resolution.txt: this run produced different content
//! ```
//!
//! — the corpus identity moved, and the resolver's outcome moved with it, each named
//! separately. The count-blind case — a swapped winner that changes *no* count — is covered
//! in ordinary CI by [`the_semantic_digest_observes_a_winner_swap_the_counts_cannot`].

use std::collections::BTreeMap;

use crate::analysis::conformance as record;
use crate::analysis::corpora::{self, establish_corpus};
use crate::canonical::encode::CanonicalDigest;
use crate::source::SourceKind;
use crate::source::fingerprint::ContentHash;

use record::{CorpusRecord, LocalizationOutcome, ResolutionRecord, RowOutcome, RowRecord};

use super::registry::Refusal;
use super::resolved::{
    ConstantOutcome, InlineOutcome, LocalizationFileStream, ResolvedRegistry, SpriteTextureOutcome,
    UnresolvedInline,
};
use super::{profile, resolve};

const RUN: &str = "c2-parse-and-resolve";

const PURPOSE: &str = "\
Resolve every declared Resolution Profile row, and the localization file stream, over the \
installed Vanilla and ACOT corpora through the production resolver — common file selection, \
per-family semantic streams, repeat and cross-source rules, constant evaluation, inline-script \
expansion, and sprite reference resolution, over the production adapter's parse of every file \
each row's scope selects. Each row's outcome is recorded on equal terms: definition and \
typed-fact counts for a row that resolves, and the typed refusal — the policy cell it stopped \
at — for a row that does not. A hit on an unresolved cell is a recorded visible failure, \
never an error and never a fallback, so the record states exactly which open cells the real \
corpora reach and how often.";

/// The name each typed count is recorded under.
///
/// Explicit functions rather than `format!("{:?}")`, because two variants carry payloads —
/// a parameter name, a lexer kind — and a count keyed on payload text would make every
/// distinct mod identifier its own drift-compared row.
fn inline_variant(unresolved: &UnresolvedInline) -> &'static str {
    match unresolved {
        UnresolvedInline::UnknownPath => "UnknownPath",
        UnresolvedInline::CyclicInclusion => "CyclicInclusion",
        UnresolvedInline::CallShapeUnmeasured => "CallShapeUnmeasured",
        UnresolvedInline::UnboundParameter { .. } => "UnboundParameter",
        UnresolvedInline::ConditionalUnmeasured => "ConditionalUnmeasured",
        UnresolvedInline::RootPlacementUnmeasured => "RootPlacementUnmeasured",
        UnresolvedInline::ExpansionBudgetExceeded => "ExpansionBudgetExceeded",
    }
}

fn texture_variant(outcome: &SpriteTextureOutcome) -> &'static str {
    match outcome {
        SpriteTextureOutcome::Resolved(_) => "Resolved",
        SpriteTextureOutcome::MissingTexture => "MissingTexture",
        SpriteTextureOutcome::MissingTarget { .. } => "MissingTarget",
        SpriteTextureOutcome::UnresolvedScalar { .. } => "UnresolvedScalar",
        SpriteTextureOutcome::CyclicReference { .. } => "CyclicReference",
    }
}

/// Which policy cell (or refusal kind) a refusal names, for the record.
fn refusal_cell(refusal: &Refusal) -> String {
    match refusal {
        Refusal::UndeclaredRegistry { .. } => "undeclared registry".to_owned(),
        Refusal::UnresolvedCell { cell, .. } => cell.to_string(),
        Refusal::UnusableReplacePath { .. } => "unusable replace_path".to_owned(),
        Refusal::UndeclaredFactKind { .. } => "undeclared fact kind".to_owned(),
        Refusal::UndeclaredReferenceKind { .. } => "undeclared reference kind".to_owned(),
    }
}

/// A deterministic digest of one resolved registry's complete semantic output.
///
/// The counts a [`RowOutcome`] carries are diagnostics; this is the identity. A resolver
/// change that swaps a duplicate winner, moves a provenance site, or rewrites an expanded
/// field can leave every count untouched — same definitions, same fact totals — while the
/// documentation input is different, and only a digest over the output itself notices
/// (the same gap `parsed_corpus_digest` closes for the parser run).
///
/// Definitions are folded in `BTreeMap` key order through their `Debug` rendering, which
/// covers every semantic field — position, body, effective fields, displaced provenance,
/// references, constant, inline, and sprite outcomes. `Debug` rather than a bespoke
/// encoding, deliberately: this digest gates a developer-run record, not a shipped
/// identity, and the only thing that changes a `Debug` rendering besides the output itself
/// is an edit to the resolved model — which is exactly a change this gate should surface
/// for re-capture.
fn row_digest(registry: &ResolvedRegistry) -> String {
    let mut digest = CanonicalDigest::new("stellaris-docs/conformance/resolved-registry/v1");
    digest.text(registry.registry);
    digest.begin_seq(registry.definitions.len());
    for (key, definition) in &registry.definitions {
        digest.text(key.as_str());
        digest.text(&format!("{definition:?}"));
    }
    digest.begin_seq(registry.removed_files.len());
    for provenance in &registry.removed_files {
        digest.text(&format!("{provenance:?}"));
    }
    digest.finish().to_hex()
}

/// The localization stream's counterpart to [`row_digest`]: order, source, path, and exact
/// bytes of every surviving and shadowed file. Counts alone would miss a reordering or a
/// swap of which file survived selection.
fn stream_digest(stream: &LocalizationFileStream) -> String {
    let mut digest = CanonicalDigest::new("stellaris-docs/conformance/localization-stream/v1");
    let source_ordinal = |source: SourceKind| match source {
        SourceKind::VanillaContent => 1u64,
        SourceKind::TargetMod => 2u64,
    };
    digest.begin_seq(stream.files.len());
    for file in &stream.files {
        digest
            .u64(u64::from(file.order))
            .u64(source_ordinal(file.source))
            .text(file.logical.as_str())
            .text(&ContentHash::of(&file.bytes).to_hex());
    }
    digest.begin_seq(stream.shadowed_files.len());
    for shadowed in &stream.shadowed_files {
        digest
            .text(&format!("{:?}", shadowed.provenance))
            .text(&ContentHash::of(&shadowed.bytes).to_hex());
    }
    digest.finish().to_hex()
}

/// One row's outcome as the record holds it.
///
/// Every typed count lands in `facts`; the unresolved subset is *also* counted in
/// `visible_failures`, keyed identically, so "what did this row fail to finish, and how
/// often" is a map a reader takes in at a glance rather than a filter they run over the
/// fact names. Both maps are drift-compared, and [`row_digest`] pins the output they
/// summarize.
fn summarize(registry: &ResolvedRegistry) -> RowOutcome {
    let mut facts: BTreeMap<String, usize> = BTreeMap::new();
    let mut visible_failures: BTreeMap<String, usize> = BTreeMap::new();
    let mut fact = |key: String| *facts.entry(key).or_default() += 1;

    for provenance in registry.provenance() {
        fact(format!("provenance.{:?}", provenance.kind));
    }
    for definition in registry.definitions.values() {
        for reference in &definition.references {
            fact(format!("reference.{:?}", reference.kind));
        }
        for constant in &definition.constants {
            match &constant.outcome {
                ConstantOutcome::Resolved { .. } => fact("constants.Resolved".to_owned()),
                ConstantOutcome::Unresolved(unresolved) => {
                    let key = format!("constants.{unresolved:?}");
                    fact(key.clone());
                    *visible_failures.entry(key).or_default() += 1;
                }
            }
        }
        for inline in &definition.inline_expansions {
            match &inline.outcome {
                InlineOutcome::Expanded { .. } => fact("inline.Expanded".to_owned()),
                InlineOutcome::Unresolved(unresolved) => {
                    let key = format!("inline.{}", inline_variant(unresolved));
                    fact(key.clone());
                    *visible_failures.entry(key).or_default() += 1;
                }
            }
        }
        if let Some(sprite) = &definition.sprite {
            let texture = format!("sprite.texture.{}", texture_variant(&sprite.texture));
            fact(texture.clone());
            if !matches!(sprite.texture, SpriteTextureOutcome::Resolved(_)) {
                *visible_failures.entry(texture).or_default() += 1;
            }
            for edge in &sprite.references {
                let key = format!("sprite.edge.{}", texture_variant(&edge.outcome));
                fact(key.clone());
                if !matches!(edge.outcome, SpriteTextureOutcome::Resolved(_)) {
                    *visible_failures.entry(key).or_default() += 1;
                }
            }
        }
    }

    RowOutcome::Resolved {
        definitions: registry.definitions.len(),
        removed_files: registry.removed_files.len(),
        semantic_digest: row_digest(registry),
        facts,
        visible_failures,
    }
}

fn row_lines(row: &RowRecord) -> String {
    match &row.outcome {
        RowOutcome::Resolved {
            definitions,
            removed_files,
            semantic_digest,
            facts,
            visible_failures,
        } => {
            let mut lines = format!(
                "# row {}: resolved — {definitions} definitions, {removed_files} files removed \
                 by selection\n  semantic digest {semantic_digest}\n",
                row.registry
            );
            for (key, count) in facts {
                lines.push_str(&format!("  fact {key}: {count}\n"));
            }
            for (key, count) in visible_failures {
                lines.push_str(&format!("  visible failure {key}: {count}\n"));
            }
            lines
        }
        RowOutcome::Refused { cell, message } => {
            format!("# row {}: refused at {cell}\n  {message}\n", row.registry)
        }
    }
}

fn summary(row: &RowRecord) -> String {
    match &row.outcome {
        RowOutcome::Resolved {
            definitions,
            visible_failures,
            ..
        } => format!(
            "{}: resolved, {definitions} definitions, {} visible failures",
            row.registry,
            visible_failures.values().sum::<usize>()
        ),
        RowOutcome::Refused { cell, .. } => format!("{}: refused at {cell}", row.registry),
    }
}

/// The drift-checked parse-and-resolve run. See the module comment for how to invoke it and
/// when to re-run it.
#[test]
#[ignore = "requires an installed Stellaris and ACOT; run with --ignored"]
fn parse_and_resolve_conformance() {
    let vanilla_corpus = corpora::vanilla();
    let acot_corpus = corpora::acot();
    let (vanilla_source, mut warnings) = establish_corpus(&vanilla_corpus);
    let (acot_source, acot_warnings) = establish_corpus(&acot_corpus);
    warnings.extend(acot_warnings);
    let vanilla = vanilla_source.snapshot();
    let acot = acot_source.snapshot();

    let resolution = resolve(vanilla, acot);
    let rows: Vec<RowRecord> = profile::DECLARED
        .iter()
        .map(|policy| RowRecord {
            registry: policy.name.to_owned(),
            outcome: match resolution.registry(policy.name) {
                Ok(registry) => summarize(&registry),
                Err(refusal) => RowOutcome::Refused {
                    cell: refusal_cell(&refusal),
                    message: refusal.to_string(),
                },
            },
        })
        .collect();
    let localization = match resolution.localization_files() {
        Ok(stream) => LocalizationOutcome::Streamed {
            files: stream.files.len(),
            shadowed_files: stream.shadowed_files.len(),
            stream_digest: stream_digest(&stream),
        },
        Err(refusal) => LocalizationOutcome::Refused {
            message: refusal.to_string(),
        },
    };

    for row in &rows {
        println!("{}", summary(row));
    }
    println!("localization: {localization:?}");
    for warning in &warnings {
        println!("warning: {warning}");
    }

    // Non-vacuity: a run in which nothing resolved would drift-compare a record of pure
    // refusals and prove nothing about resolution. The refusals themselves stay recordable —
    // this fails only when *no* row produced content at all.
    assert!(
        rows.iter().any(|row| matches!(
            row.outcome,
            RowOutcome::Resolved { definitions, .. } if definitions > 0
        )),
        "no declared row resolved any definition, so this run verified nothing"
    );

    let corpora_records = vec![
        CorpusRecord {
            identity: record::script_identity(&vanilla_corpus, vanilla),
            outcome: None,
        },
        CorpusRecord {
            identity: record::script_identity(&acot_corpus, acot),
            outcome: None,
        },
    ];
    let resolution_record = ResolutionRecord {
        profile_version: super::RESOLUTION_PROFILE_VERSION,
        rows,
        localization,
    };
    let environment = record::environment(record::installed_build());
    // Built once and used twice, so the bytes the drift check compares are the bytes a
    // capture would write.
    let listing: String = resolution_record.rows.iter().map(row_lines).collect();
    let listing = format!(
        "{listing}# localization: {:?}\n",
        resolution_record.localization
    );
    let artifacts = vec![("resolution.txt", listing)];

    // Capture is the deliberate act of accepting a new state, so it reports drift rather
    // than failing on it — the same contract as the parser conformance run. The checks above
    // are unconditional either way: a failing run is never recorded.
    let capturing = record::capture_requested("PARSE_AND_RESOLVE_CAPTURE");
    let recorded = record::read(RUN);
    match &recorded {
        Some(recorded) => {
            let drift = record::drift(
                recorded,
                &environment,
                &corpora_records,
                Some(&resolution_record),
                &artifacts,
            );
            let location = record::records_root().join(RUN);
            assert!(
                drift.is_empty() || capturing,
                "the run drifted from {}:\n{}\n\nDrift is not always wrong: a game or mod \
                 update, a profile revision, or a toolchain bump all belong here. Re-capture \
                 with PARSE_AND_RESOLVE_CAPTURE=1 once the change is understood.",
                location.display(),
                drift.join("\n")
            );
            for reason in drift {
                println!("drift accepted by capture: {reason}");
            }
        }
        None => assert!(
            capturing,
            "no record exists at {}. Capture one with PARSE_AND_RESOLVE_CAPTURE=1 — an \
             absent record must not read as an undrifted run.",
            record::records_root().join(RUN).display()
        ),
    }

    if capturing {
        let written = record::write(
            RUN,
            PURPOSE,
            environment,
            corpora_records,
            Some(resolution_record),
            &artifacts,
            warnings,
        )
        .expect("the record directory is writable");
        println!("captured {}", written.display());
    }
}

/// The negative control for [`row_digest`], and the reason it exists at all: `r4`'s method —
/// identical definition bodies under swapped filenames — moves the duplicate winner while
/// leaving **every count identical**. Definitions, removed files, provenance totals,
/// reference totals, and visible failures all match, so the diagnostic maps alone would let
/// a winner swap pass the drift gate; the semantic digest is the one value that moves.
#[test]
fn the_semantic_digest_observes_a_winner_swap_the_counts_cannot() {
    use crate::source::fixture::FixtureCorpus;

    let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
        .with_file(
            "common/technology/mm_swap_tech.txt",
            b"tech_swap = {\n\ttier = 1\n}\n",
        )
        .build()
        .expect("a well-formed fixture corpus");
    let contested = |filename: &str| {
        FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"swap\"")
            .with_file(
                &format!("common/technology/{filename}"),
                b"tech_swap = {\n\ttier = 2\n}\n",
            )
            .build()
            .expect("a well-formed fixture corpus")
    };

    let late = contested("zz_swap_tech.txt");
    let early = contested("!!!_swap_tech.txt");
    let mod_wins = summarize(
        &resolve(&vanilla, &late)
            .registry("technologies")
            .expect("the declared row resolves"),
    );
    let vanilla_wins = summarize(
        &resolve(&vanilla, &early)
            .registry("technologies")
            .expect("the declared row resolves"),
    );

    let (
        RowOutcome::Resolved {
            definitions: left_definitions,
            removed_files: left_removed,
            semantic_digest: left_digest,
            facts: left_facts,
            visible_failures: left_failures,
        },
        RowOutcome::Resolved {
            definitions: right_definitions,
            removed_files: right_removed,
            semantic_digest: right_digest,
            facts: right_facts,
            visible_failures: right_failures,
        },
    ) = (&mod_wins, &vanilla_wins)
    else {
        panic!("both corpora resolve: {mod_wins:?} / {vanilla_wins:?}");
    };

    // The precondition that makes this a control: everything the record counts is identical.
    assert_eq!(left_definitions, right_definitions);
    assert_eq!(left_removed, right_removed);
    assert_eq!(left_facts, right_facts, "the counts must not discriminate");
    assert_eq!(left_failures, right_failures);
    // And the digest still tells them apart, because the winning definition differs.
    assert_ne!(
        left_digest, right_digest,
        "a swapped winner left the semantic digest unchanged, so the gate observes counts \
         rather than resolved output"
    );
}
