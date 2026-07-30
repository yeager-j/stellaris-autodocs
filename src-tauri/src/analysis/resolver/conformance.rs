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
//! artifact resolution.txt: this run produced different content
//! ```
//!
//! — the corpus identity moved, and the resolver's outcome moved with it, each named
//! separately.

use std::collections::BTreeMap;

use crate::analysis::conformance as record;
use crate::analysis::corpora::{self, establish_corpus};

use record::{CorpusRecord, LocalizationOutcome, ResolutionRecord, RowOutcome, RowRecord};

use super::registry::Refusal;
use super::resolved::{
    ConstantOutcome, InlineOutcome, ResolvedRegistry, SpriteTextureOutcome, UnresolvedInline,
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

/// One row's outcome as the record holds it.
///
/// Every typed count lands in `facts`; the unresolved subset is *also* counted in
/// `visible_failures`, keyed identically, so "what did this row fail to finish, and how
/// often" is a map a reader takes in at a glance rather than a filter they run over the
/// fact names. Both maps are drift-compared.
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
        facts,
        visible_failures,
    }
}

fn row_lines(row: &RowRecord) -> String {
    match &row.outcome {
        RowOutcome::Resolved {
            definitions,
            removed_files,
            facts,
            visible_failures,
        } => {
            let mut lines = format!(
                "# row {}: resolved — {definitions} definitions, {removed_files} files removed \
                 by selection\n",
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
