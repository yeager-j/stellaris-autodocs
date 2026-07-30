//! Scripted constants: the complement reader, the single-pass chain evaluator, and the
//! `Environment` a consuming row looks a symbol up against.
//!
//! # What this owns
//!
//! - [`SCOPE`]: `common/scripted_variables`, the single authority the constants row's stream
//!   cell imports (`docs/spikes/resolver-evaluation.md`, "Scripted constants").
//! - [`constant_declarations`]: the complement of
//!   [`top_level_definitions`](super::registry::top_level_definitions) — yields *only* the
//!   `@`-keyed declarations that reader skips.
//! - [`Environment`]: the global, first-registration-wins binding for every constant symbol,
//!   built by walking the constants scope's own stream once, plus the per-file local
//!   declarations a consuming row collects during its own walk.
//!
//! # Cross-source repeats are ordinary repeats (r19)
//!
//! `r19-constants-cross-source` measured a matched pair redefining two Vanilla constants
//! from one Target Mod, one from an early-sorting file and one from a late-sorting file.
//! Both directions resolved to the *first* declaration in the one global normalized-path
//! stream — the early mod declaration won and the late one lost — with the game's error log
//! rejecting exactly the second registration each time. Cross-source is therefore not a
//! special case here: [`build_environment`]'s single first-wins pass over the stream already
//! states the whole rule, and no source ever participates in the decision. (Until Phase 4L
//! consumed `r19`, a contested symbol carried a per-symbol `CrossSourcePending` fact; the
//! variant and the two bookkeeping passes that produced it retired with the cell.)
//!
//! # Why forward references and cycles need no special-case detection
//!
//! [`evaluate`] processes each symbol's *first* declaration exactly once, in the order the
//! constants stream establishes, looking references up in a map that only ever contains
//! symbols already evaluated earlier in that same order. A forward reference is simply a
//! lookup that misses because the target has not been reached yet
//! (`DeclarationNeverResolves`); a two-symbol cycle is two forward references pointing at
//! each other, so the first one evaluated already fails to find the second, and the second
//! then finds the first already resolved-but-broken and propagates the same outcome. No
//! separate cycle bookkeeping — an in-progress set, recursion, or a visited-count — is
//! needed; the single pass terminates because it never revisits a symbol once its first
//! declaration has been evaluated (`r5`, `r7`).

use std::collections::BTreeMap;

use crate::canonical::numeric::SourceNumber;
use crate::canonical::path::LogicalPath;

use super::registry::{ReadDefinition, is_constant_declaration};
use super::resolved::{
    ConstantOutcome, DefinitionKey, FactSite, StreamPosition, UnresolvedConstant,
};
use super::selection::FileSelection;
use super::stream::{ContentFamily, FileScope, StreamEntry};

use super::super::parser::{ParsedFile, ScalarKind, Value};

/// `common/scripted_variables`. The single authority the constants row's stream cell
/// imports, so the profile row and this module cannot state two different directories for
/// the same registry.
pub(super) const SCOPE: FileScope = FileScope {
    directory: "common/scripted_variables",
    extensions: &["txt"],
    recursive: false,
};

/// The complement of [`top_level_definitions`](super::registry::top_level_definitions):
/// yields *only* the `@`-keyed declarations that reader skips.
///
/// Recognized by the key's [`ScalarKind`] — the same predicate `top_level_definitions` uses
/// to skip these — never by a `@` byte-prefix test on the raw key text; the dialect lexer is
/// the one authority for what an `@` token is.
///
/// Ordinals are the position in the parse, assigned before the filter, for the same reason
/// `top_level_definitions` assigns them before its skip: a technology following a constant
/// declaration keeps the ordinal its file gives it, and a constant declaration following a
/// technology keeps its own. Provenance and Source Excerpts mean the position in the source,
/// not the index among what this reader kept.
pub(super) fn constant_declarations(file: &ParsedFile) -> Vec<ReadDefinition> {
    file.definitions()
        .enumerate()
        .filter(|(_, (field, _))| is_constant_declaration(field))
        .map(|(ordinal, (field, _))| ReadDefinition {
            key: DefinitionKey::new(field.key.text().into_owned()),
            ordinal: ordinal as u32,
            body: field.value.clone(),
        })
        .collect()
}

/// One consuming file's own `@`-declarations, collected during that row's own stream walk
/// (decision 10) so [`Environment::lookup`] can apply the file-local override rule without a
/// second traversal of the same file.
#[derive(Debug, Clone)]
pub(super) struct LocalDeclaration {
    symbol: String,
    ordinal: u32,
    body: Value,
    site: FactSite,
}

impl LocalDeclaration {
    pub(super) fn new(symbol: String, ordinal: u32, body: Value, site: FactSite) -> Self {
        Self {
            symbol,
            ordinal,
            body,
            site,
        }
    }
}

/// The global binding for every scripted-constant symbol, plus whichever consuming files
/// have registered their own local declarations.
///
/// Built once per resolution (`registry::resolve`, decision 10) and read many times: every
/// definition consuming a `@` reference looks the same environment up, and the constants
/// row itself reads its own declarations' outcomes back out of it.
pub(super) struct Environment {
    global: BTreeMap<String, ConstantOutcome>,
    locals: BTreeMap<LogicalPath, Vec<LocalDeclaration>>,
}

impl Environment {
    /// The outcome a consuming definition sees for `symbol`, from `file` at `consumer_ordinal`.
    ///
    /// File-local scoping, not a registry rule (decision 8): once *any* local declaration of
    /// `symbol` exists in `file`, the global binding never applies again for that file — not
    /// even when the local declaration is invalid. A consumer whose local declaration follows
    /// it, or names it twice, gets a fact about *that*, never a silent fall-through to
    /// whatever Vanilla or another file declared.
    ///
    /// A valid local declaration resolves only when its own body is a literal scalar — the
    /// only shape `r1` measured (`@oracle_const_local = 99`). It is never chain-evaluated
    /// against the global environment or against other locals: a `@name` reference in a
    /// local body would either fall through to the very global binding the local exists to
    /// shadow (`@cost = @cost` would silently read the global `@cost`, turning a self-cycle
    /// into a number) or rest on an equally unmeasured claim about resolving one local
    /// against another. [`UnresolvedConstant::LocalReferenceUnmeasured`] names that gap
    /// instead of guessing either way.
    pub(super) fn lookup(
        &self,
        file: &LogicalPath,
        consumer_ordinal: u32,
        symbol: &str,
    ) -> ConstantOutcome {
        if let Some(locals) = self.locals.get(file) {
            let matching: Vec<&LocalDeclaration> = locals
                .iter()
                .filter(|local| local.symbol == symbol)
                .collect();
            match matching.as_slice() {
                [] => {}
                [only] => {
                    return if only.ordinal >= consumer_ordinal {
                        ConstantOutcome::Unresolved(
                            UnresolvedConstant::LocalDeclarationFollowsConsumer,
                        )
                    } else {
                        local_outcome(only)
                    };
                }
                _ => {
                    return ConstantOutcome::Unresolved(
                        UnresolvedConstant::DuplicateLocalDeclaration,
                    );
                }
            }
        }
        self.global
            .get(symbol)
            .cloned()
            .unwrap_or(ConstantOutcome::Unresolved(
                UnresolvedConstant::UndeclaredSymbol,
            ))
    }

    /// The outcome of the constants row's *own* declaration of `symbol` (decision 9): a fact
    /// about the declaration's own value, attached with `field: None` rather than looked up
    /// by a consumer. Every symbol the constants row resolved to a definition was seen by
    /// this same environment's own walk, so an absent entry cannot occur in practice; the
    /// fallback exists only so this is a total function rather than a panic.
    pub(super) fn declaration_outcome(&self, symbol: &str) -> ConstantOutcome {
        self.global
            .get(symbol)
            .cloned()
            .unwrap_or(ConstantOutcome::Unresolved(
                UnresolvedConstant::UndeclaredSymbol,
            ))
    }

    /// Records one consuming file's local declarations, collected during that row's own
    /// stream walk. A file with none is not inserted, so [`Self::lookup`] never mistakes "no
    /// entry" for "declared zero times" — both mean the same thing here, but the distinction
    /// matters if this map is ever inspected directly.
    pub(super) fn record_local_declarations(
        &mut self,
        file: LogicalPath,
        declarations: Vec<LocalDeclaration>,
    ) {
        if !declarations.is_empty() {
            self.locals.insert(file, declarations);
        }
    }
}

/// Builds the global [`Environment`] by walking the constants scope's own stream once, in
/// the one path order every content family shares (`super::stream`).
///
/// One chain-evaluation pass (`evaluate_value`) over the declarations found: each symbol's
/// *first* registration in stream order is evaluated, copying a referenced symbol's value
/// when the body is a `@name` reference, and every later registration of the same symbol is
/// skipped without re-evaluation. First-wins is the whole rule for repeats within one file,
/// across files, and across sources alike — `r19`'s matched pair measured the cross-source
/// directions directly, so the source of a registration never participates.
pub(super) fn build_environment<R>(selection: &FileSelection, read: &R) -> Environment
where
    R: Fn(&StreamEntry) -> Option<ParsedFile>,
{
    let stream = super::stream::build(selection, ContentFamily::Script, SCOPE);
    let mut resolved: BTreeMap<String, ConstantOutcome> = BTreeMap::new();

    for entry in &stream {
        let Some(file) = read(entry) else {
            debug_assert!(false, "a surviving file did not read back: {entry:?}");
            continue;
        };
        for declaration in constant_declarations(&file) {
            let symbol = declaration.key.as_str().to_owned();
            if resolved.contains_key(&symbol) {
                // First registration wins (`RepeatRule::RejectOnRepeat`); a later one is
                // never re-evaluated. The registry engine, not this map, is what records
                // the rejected registration as a duplicate fact.
                continue;
            }
            let site = FactSite::Stream(StreamPosition {
                order: entry.order,
                source: entry.source,
                logical: entry.logical.clone(),
                ordinal: declaration.ordinal,
            });
            let outcome = evaluate_value(&declaration.body, site, &resolved);
            resolved.insert(symbol, outcome);
        }
    }

    Environment {
        global: resolved,
        locals: BTreeMap::new(),
    }
}

/// One declaration's chain outcome: a literal parses through
/// [`SourceNumber::parse`]; a `@name` reference looks the symbol up in `resolved_so_far` —
/// found there means it was already evaluated earlier in this same stream-ordered pass, and
/// missing there means either a forward reference or an undeclared symbol, which this
/// evaluator does not distinguish (decision 2: "not yet registered = forward reference");
/// a `@[ … ]` expression is not evaluated at all.
fn evaluate_value(
    body: &Value,
    site: FactSite,
    resolved_so_far: &BTreeMap<String, ConstantOutcome>,
) -> ConstantOutcome {
    let Value::Scalar(scalar) = body else {
        // No oracle record establishes a scripted constant whose value is a container; this
        // is the unmeasured shape rather than a guess about it.
        return ConstantOutcome::Unresolved(UnresolvedConstant::DeclarationNeverResolves);
    };
    match scalar.kind {
        ScalarKind::VariableExpr => ConstantOutcome::Unresolved(UnresolvedConstant::Expression),
        ScalarKind::VariableRef => match resolved_so_far.get(scalar.text().as_ref()) {
            Some(ConstantOutcome::Resolved { value, declaration }) => ConstantOutcome::Resolved {
                value: value.clone(),
                declaration: declaration.clone(),
            },
            Some(ConstantOutcome::Unresolved(_)) | None => {
                ConstantOutcome::Unresolved(UnresolvedConstant::DeclarationNeverResolves)
            }
        },
        // Every other lexeme — numbers, and non-numeric tokens like `yes` — carries the
        // lexeme through `SourceNumber::parse` unconditionally. `SourceNumber` already keeps
        // a non-numeric lexeme with `value() == None` rather than erroring; that is its own
        // contract, not a new one this evaluator invents.
        _ => ConstantOutcome::Resolved {
            value: SourceNumber::parse(&scalar.text()),
            declaration: site,
        },
    }
}

/// A valid local declaration's outcome — the measured shape only (`r1`:
/// `@oracle_const_local = 99`, a literal). Deliberately not `evaluate_value` reused with the
/// global map as `resolved_so_far`: a local declaration is never chain-evaluated against
/// anything. A `@name` reference in a local body is [`UnresolvedConstant::LocalReferenceUnmeasured`]
/// rather than a lookup, because a lookup has no unmeasured-free place to point — the global
/// registry is the very binding the local shadows, and another local is just as unproven a
/// target. A `@[ … ]` expression is `Unresolved(Expression)`, matching every other body.
fn local_outcome(local: &LocalDeclaration) -> ConstantOutcome {
    let Value::Scalar(scalar) = &local.body else {
        // Same unmeasured-shape reasoning as `evaluate_value`: no record establishes a
        // scripted constant whose value is a container, local or global.
        return ConstantOutcome::Unresolved(UnresolvedConstant::DeclarationNeverResolves);
    };
    match scalar.kind {
        ScalarKind::VariableExpr => ConstantOutcome::Unresolved(UnresolvedConstant::Expression),
        ScalarKind::VariableRef => {
            ConstantOutcome::Unresolved(UnresolvedConstant::LocalReferenceUnmeasured)
        }
        _ => ConstantOutcome::Resolved {
            value: SourceNumber::parse(&scalar.text()),
            declaration: local.site.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::parser::{self, SourceIdentity};
    use crate::analysis::resolver::selection;
    use crate::canonical::path::LogicalPath;
    use crate::source::SourceKind;
    use crate::source::fixture::FixtureCorpus;
    use crate::source::snapshot::SourceSnapshot;

    fn parsed(logical: &str, source: &str) -> ParsedFile {
        let identity = SourceIdentity::new(
            SourceKind::TargetMod,
            LogicalPath::parse(logical).expect("a test path"),
        );
        parser::parse(identity, source.as_bytes())
    }

    // --- The complement reader ---

    #[test]
    fn yields_only_at_keyed_declarations() {
        let file = parsed(
            "common/scripted_variables/zz_test.txt",
            "tech_a = { tier = 1 }\n@base_cost = 20\ntech_b = { tier = 2 }\n",
        );
        let found = constant_declarations(&file);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].key.as_str(), "@base_cost");
    }

    #[test]
    fn ordinals_are_assigned_before_the_skip() {
        // Two technologies precede the declaration, so it keeps the file's third-position
        // ordinal (2) rather than being renumbered to 0 among what this reader kept.
        let file = parsed(
            "common/scripted_variables/zz_test.txt",
            "tech_a = { tier = 1 }\ntech_b = { tier = 2 }\n@base_cost = 20\n",
        );
        let found = constant_declarations(&file);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].ordinal, 2);
    }

    #[test]
    fn recognized_by_scalar_kind_not_byte_prefix() {
        // `not@aref` carries an `@` byte but is not a variable-reference token, so a
        // byte-prefix test would misclassify it as a declaration.
        let file = parsed(
            "common/scripted_variables/zz_test.txt",
            "not@aref = 1\n@real_constant = 2\n",
        );
        let found = constant_declarations(&file);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].key.as_str(), "@real_constant");
    }

    // --- The single-pass chain evaluator and exact numerics ---

    fn environment_over_snapshots(
        vanilla: &SourceSnapshot,
        target: &SourceSnapshot,
    ) -> Environment {
        let selection = selection::select(vanilla, target);
        build_environment(&selection, &|entry: &StreamEntry| {
            let identity = SourceIdentity::new(entry.source, entry.logical.clone());
            let snapshot: &SourceSnapshot = match entry.source {
                SourceKind::VanillaContent => vanilla,
                SourceKind::TargetMod => target,
            };
            let bytes = snapshot.read(&entry.logical)?;
            Some(parser::parse(identity, bytes.as_slice()))
        })
    }

    fn environment_over(logical: &str, source: &str) -> Environment {
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .build()
            .expect("an empty fixture corpus");
        let target = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"constants-test\"")
            .with_file(logical, source.as_bytes())
            .build()
            .expect("a well-formed fixture corpus");
        environment_over_snapshots(&vanilla, &target)
    }

    fn resolved_of(environment: &Environment, symbol: &str) -> ConstantOutcome {
        environment.declaration_outcome(symbol)
    }

    #[test]
    fn a_literal_scalar_binds_an_exact_value() {
        let environment =
            environment_over("common/scripted_variables/zz_test.txt", "@base_cost = 20\n");
        let ConstantOutcome::Resolved { value, .. } = resolved_of(&environment, "@base_cost")
        else {
            panic!("expected a resolved binding");
        };
        assert_eq!(value.value(), SourceNumber::parse("20").value());
    }

    #[test]
    fn a_backward_chain_resolves_to_the_root_value() {
        let environment =
            environment_over("common/scripted_variables/zz_test.txt", "@a = 1\n@b = @a\n");
        let ConstantOutcome::Resolved { value, .. } = resolved_of(&environment, "@b") else {
            panic!("expected @b to resolve through @a");
        };
        assert_eq!(value.value(), SourceNumber::parse("1").value());
    }

    #[test]
    fn a_forward_reference_does_not_resolve() {
        let environment = environment_over(
            "common/scripted_variables/zz_test.txt",
            "@fwd = @fwd_target\n@fwd_target = 7\n",
        );
        assert_eq!(
            resolved_of(&environment, "@fwd"),
            ConstantOutcome::Unresolved(UnresolvedConstant::DeclarationNeverResolves)
        );
        // The control: the target itself is a plain literal and resolves fine, so the
        // failure above is about reference direction and not about the fixture.
        let ConstantOutcome::Resolved { value, .. } = resolved_of(&environment, "@fwd_target")
        else {
            panic!("expected @fwd_target to resolve");
        };
        assert_eq!(value.value(), SourceNumber::parse("7").value());
    }

    #[test]
    fn a_cycle_does_not_resolve_and_the_pass_terminates() {
        let environment = environment_over(
            "common/scripted_variables/zz_test.txt",
            "@cycle_a = @cycle_b\n@cycle_b = @cycle_a\n",
        );
        assert_eq!(
            resolved_of(&environment, "@cycle_a"),
            ConstantOutcome::Unresolved(UnresolvedConstant::DeclarationNeverResolves)
        );
        assert_eq!(
            resolved_of(&environment, "@cycle_b"),
            ConstantOutcome::Unresolved(UnresolvedConstant::DeclarationNeverResolves)
        );
    }

    #[test]
    fn a_variable_expression_is_never_evaluated() {
        let environment = environment_over(
            "common/scripted_variables/zz_test.txt",
            "@computed = @[ 1 + 1 ]\n",
        );
        assert_eq!(
            resolved_of(&environment, "@computed"),
            ConstantOutcome::Unresolved(UnresolvedConstant::Expression)
        );
    }

    #[test]
    fn a_non_numeric_lexeme_is_kept_without_a_value() {
        let environment =
            environment_over("common/scripted_variables/zz_test.txt", "@flag = yes\n");
        let ConstantOutcome::Resolved { value, .. } = resolved_of(&environment, "@flag") else {
            panic!("expected the lexeme to be kept as a resolved, valueless binding");
        };
        assert_eq!(value.lexeme(), "yes");
        assert!(value.value().is_none());
    }

    #[test]
    fn decimal_addition_is_exact_through_exact_value() {
        let environment = environment_over(
            "common/scripted_variables/zz_test.txt",
            "@decimal_a = 0.1\n@decimal_b = 0.2\n",
        );
        let ConstantOutcome::Resolved { value: a, .. } = resolved_of(&environment, "@decimal_a")
        else {
            panic!("expected @decimal_a to resolve");
        };
        let ConstantOutcome::Resolved { value: b, .. } = resolved_of(&environment, "@decimal_b")
        else {
            panic!("expected @decimal_b to resolve");
        };
        let sum = a.value().expect("exact").add(b.value().expect("exact"));
        assert_eq!(sum, *SourceNumber::parse("0.3").value().expect("exact"));
    }

    // --- Environment: global first-wins, cross-source marking, and file-local override ---

    /// Populates `environment`'s local declarations for one file, exactly as `registry::resolve`
    /// does during a consuming row's own stream walk (decision 10) — these tests exercise
    /// `Environment` directly, with no consuming row of its own to perform that walk.
    fn record_locals(
        environment: &mut Environment,
        snapshot: &SourceSnapshot,
        source: SourceKind,
        logical: &str,
    ) {
        let path = LogicalPath::parse(logical).expect("a test path");
        let bytes = snapshot
            .read(&path)
            .expect("the file exists in the fixture");
        let identity = SourceIdentity::new(source, path.clone());
        let file = parser::parse(identity, bytes.as_slice());
        let locals: Vec<LocalDeclaration> = constant_declarations(&file)
            .into_iter()
            .map(|declaration| {
                let site = FactSite::Stream(StreamPosition {
                    order: 0,
                    source,
                    logical: path.clone(),
                    ordinal: declaration.ordinal,
                });
                LocalDeclaration::new(
                    declaration.key.as_str().to_owned(),
                    declaration.ordinal,
                    declaration.body,
                    site,
                )
            })
            .collect();
        environment.record_local_declarations(path, locals);
    }

    fn registration_environment() -> Environment {
        let vanilla = crate::analysis::resolver::trial::registration_vanilla();
        let target = crate::analysis::resolver::trial::corpus(
            SourceKind::TargetMod,
            crate::analysis::resolver::trial::REGISTRATION,
        );
        let mut environment = environment_over_snapshots(&vanilla, &target);
        record_locals(
            &mut environment,
            &target,
            SourceKind::TargetMod,
            "common/technology/zz_consumer_tech.txt",
        );
        environment
    }

    fn exact(outcome: &ConstantOutcome) -> Option<String> {
        match outcome {
            ConstantOutcome::Resolved { value, .. } => value
                .value()
                .map(|exact| exact.to_decimal_string().unwrap_or_default()),
            ConstantOutcome::Unresolved(_) => None,
        }
    }

    #[test]
    fn global_first_wins_within_one_file() {
        // `@const_same_file` is declared twice in `zz_dup_constants_a.txt`; reject-on-repeat
        // keeps the first (1), restating `r1`'s same-file constant case.
        let environment = registration_environment();
        assert_eq!(
            exact(&environment.declaration_outcome("@const_same_file")).as_deref(),
            Some("1")
        );
    }

    #[test]
    fn global_first_wins_across_files() {
        // `@const_cross_file` is declared in both `zz_dup_constants_a.txt` (10) and
        // `zz_dup_constants_b.txt` (20, sorts later); reject-on-repeat keeps the first file's
        // value, restating `r1`'s cross-file constant case.
        let environment = registration_environment();
        assert_eq!(
            exact(&environment.declaration_outcome("@const_cross_file")).as_deref(),
            Some("10")
        );
    }

    #[test]
    fn global_first_wins_across_sources_in_both_directions() {
        // The `r19` matched pair at the environment: one Vanilla symbol is redeclared from an
        // early-sorting Target Mod file and one from a late-sorting file, and each direction
        // resolves to whichever declaration came first in the one global path order —
        // never to a source.
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .with_file(
                "common/scripted_variables/00_pair.txt",
                b"@early_contested = 100\n@late_contested = 100\n",
            )
            .build()
            .expect("a well-formed fixture corpus");
        let target = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"cross-source-pair\"")
            .with_file(
                "common/scripted_variables/!!!_pair_early.txt",
                b"@early_contested = 111\n",
            )
            .with_file(
                "common/scripted_variables/zz_pair_late.txt",
                b"@late_contested = 111\n",
            )
            .build()
            .expect("a well-formed fixture corpus");
        let environment = environment_over_snapshots(&vanilla, &target);

        assert_eq!(
            exact(&environment.declaration_outcome("@early_contested")).as_deref(),
            Some("111"),
            "the mod's early-sorting declaration wins (r19: @speed_slow observed 111_mod_early)"
        );
        assert_eq!(
            exact(&environment.declaration_outcome("@late_contested")).as_deref(),
            Some("100"),
            "Vanilla's earlier declaration wins (r19: @outpost_cost observed 100_vanilla)"
        );
    }

    #[test]
    fn an_alias_of_a_cross_source_contested_symbol_keeps_the_first_value() {
        // `@alias` copies `@base`'s value during the chain pass. Before Phase 4L a later
        // cross-source redeclaration of `@base` retroactively marked both pending; under
        // `r19`'s first-wins rule the copied value is simply correct, because the later
        // registration never contributes a value to anything.
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .with_file(
                "common/scripted_variables/00_alias.txt",
                b"@base = 5\n@alias = @base\n@clean = 7\n",
            )
            .build()
            .expect("a well-formed fixture corpus");
        let target = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"alias-collision\"")
            .with_file("common/scripted_variables/zz_alias.txt", b"@base = 99\n")
            .build()
            .expect("a well-formed fixture corpus");
        let environment = environment_over_snapshots(&vanilla, &target);

        assert_eq!(
            exact(&environment.declaration_outcome("@base")).as_deref(),
            Some("5"),
            "the directly-contested symbol resolves to its first declaration"
        );
        assert_eq!(
            exact(&environment.declaration_outcome("@alias")).as_deref(),
            Some("5"),
            "the alias keeps the value it copied from the winning declaration"
        );
        assert_eq!(
            exact(&environment.declaration_outcome("@clean")).as_deref(),
            Some("7")
        );
    }

    #[test]
    fn a_file_local_declaration_overrides_the_global_binding() {
        // `zz_consumer_tech.txt` declares `@const_local = 99` at ordinal 0, ahead of
        // `tech_local_consumer` at ordinal 1 — the valid-override shape. The global binding
        // for the same symbol, declared in `zz_dup_constants_a.txt`, is 11 and must never be
        // what this file's consumer sees.
        let environment = registration_environment();
        let file = LogicalPath::parse("common/technology/zz_consumer_tech.txt").unwrap();
        assert_eq!(
            exact(&environment.lookup(&file, 1, "@const_local")).as_deref(),
            Some("99")
        );
        // The control: a *different* file with no local declaration of its own still reads
        // the global binding.
        let elsewhere = LogicalPath::parse("common/scripted_variables/zz_dup_constants_b.txt")
            .expect("a test path");
        assert_eq!(
            exact(&environment.lookup(&elsewhere, 0, "@const_local")).as_deref(),
            Some("11")
        );
    }

    #[test]
    fn a_self_referencing_local_declaration_never_falls_through_to_the_global() {
        // `@cost = @cost` locally, with a global `@cost` also declared: falling through to
        // the global would turn a self-cycle into a number, and it is exactly the
        // fall-through the no-fall-through rule (decision 8) forbids. Only a literal local
        // override is measured (`r1`); a reference-valued local body stays typed-unresolved.
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .with_file("common/scripted_variables/00_cost.txt", b"@cost = 5\n")
            .build()
            .expect("a well-formed fixture corpus");
        let target = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"local-reference\"")
            .with_file(
                "common/technology/zz_local_reference.txt",
                b"@cost = @cost\ntech_consumer = { cost = @cost }\n",
            )
            .build()
            .expect("a well-formed fixture corpus");
        let mut environment = environment_over_snapshots(&vanilla, &target);
        record_locals(
            &mut environment,
            &target,
            SourceKind::TargetMod,
            "common/technology/zz_local_reference.txt",
        );
        let file = LogicalPath::parse("common/technology/zz_local_reference.txt").unwrap();
        assert_eq!(
            environment.lookup(&file, 1, "@cost"),
            ConstantOutcome::Unresolved(UnresolvedConstant::LocalReferenceUnmeasured),
            "must never resolve to the global's 5"
        );
    }

    #[test]
    fn a_local_declaration_referencing_a_different_symbol_is_equally_unmeasured() {
        // Not just the self-cycle shape: any reference-valued local body is unmeasured,
        // because resolving it against other locals is just as unproven as resolving it
        // against the global.
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .with_file("common/scripted_variables/00_other.txt", b"@other = 3\n")
            .build()
            .expect("a well-formed fixture corpus");
        let target = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"local-reference-other\"")
            .with_file(
                "common/technology/zz_local_reference_other.txt",
                b"@a = @other\ntech_consumer = { cost = @a }\n",
            )
            .build()
            .expect("a well-formed fixture corpus");
        let mut environment = environment_over_snapshots(&vanilla, &target);
        record_locals(
            &mut environment,
            &target,
            SourceKind::TargetMod,
            "common/technology/zz_local_reference_other.txt",
        );
        let file = LogicalPath::parse("common/technology/zz_local_reference_other.txt").unwrap();
        assert_eq!(
            environment.lookup(&file, 1, "@a"),
            ConstantOutcome::Unresolved(UnresolvedConstant::LocalReferenceUnmeasured)
        );
    }

    #[test]
    fn a_local_declaration_after_the_consumer_does_not_fall_through_to_the_global() {
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .build()
            .expect("an empty fixture corpus");
        let target = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"local-order\"")
            .with_file(
                "common/technology/zz_local_order.txt",
                b"tech_early_consumer = { cost = @late_local }\n@late_local = 5\n",
            )
            .build()
            .expect("a well-formed fixture corpus");
        let mut environment = environment_over_snapshots(&vanilla, &target);
        record_locals(
            &mut environment,
            &target,
            SourceKind::TargetMod,
            "common/technology/zz_local_order.txt",
        );
        let file = LogicalPath::parse("common/technology/zz_local_order.txt").unwrap();
        // Consumer ordinal 0, local declaration ordinal 1: the local follows the consumer.
        assert_eq!(
            environment.lookup(&file, 0, "@late_local"),
            ConstantOutcome::Unresolved(UnresolvedConstant::LocalDeclarationFollowsConsumer)
        );
    }

    #[test]
    fn two_local_declarations_of_one_symbol_in_one_file_refuse_to_pick_a_winner() {
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .build()
            .expect("an empty fixture corpus");
        let target = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"local-dup\"")
            .with_file(
                "common/technology/zz_local_dup.txt",
                b"@dup_local = 1\n@dup_local = 2\ntech_consumer = { cost = @dup_local }\n",
            )
            .build()
            .expect("a well-formed fixture corpus");
        let mut environment = environment_over_snapshots(&vanilla, &target);
        record_locals(
            &mut environment,
            &target,
            SourceKind::TargetMod,
            "common/technology/zz_local_dup.txt",
        );
        let file = LogicalPath::parse("common/technology/zz_local_dup.txt").unwrap();
        assert_eq!(
            environment.lookup(&file, 2, "@dup_local"),
            ConstantOutcome::Unresolved(UnresolvedConstant::DuplicateLocalDeclaration)
        );
    }

    #[test]
    fn a_symbol_declared_nowhere_is_undeclared() {
        let environment = registration_environment();
        let file = LogicalPath::parse("common/technology/zz_consumer_tech.txt").unwrap();
        assert_eq!(
            environment.lookup(&file, 5, "@never_declared"),
            ConstantOutcome::Unresolved(UnresolvedConstant::UndeclaredSymbol)
        );
    }
}
