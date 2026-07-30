//! Inline scripts: the path-addressed fragment [`Library`], and the [`expand`] pass that
//! splices a fragment into the effective fields of the definition that includes it.
//!
//! # What this owns
//!
//! - [`SCOPE`]: `common/inline_scripts`, recursive — the single authority the mechanism and
//!   any consuming row's handling share, so neither can name a different directory.
//! - [`Library`]: every surviving fragment, keyed by its logical path under that directory
//!   with the extension dropped, built once per resolution.
//! - [`expand`]: the rewrite of one winner's effective fields, and the
//!   [`InlineScriptFact`]s that say what happened at every site it reached.
//!
//! # Why this is not a registry row
//!
//! Inline scripts have no declared identifier. They are "path-addressed textual expansion
//! rather than registry entries" (`docs/technical-design.md`, "Resolver contract and game
//! oracle") with one script per file, so there is no repeat rule to apply and no key to
//! collide on. Their only collision mode is the same-path replacement of `r6`, and that is
//! already settled by [`selection`](super::selection) before any stream exists — which is why
//! a mod file at a vanilla script's path overrides it with no code here.
//!
//! # Why expansion runs on the effective fields, after the winners walk
//!
//! An inclusion inside a definition that went on to lose is not a fact about the answer, the
//! same reason reference detection waits (`registry::resolve`). Running *before* that
//! detection is what makes a fragment's content ordinary: a `@constant` a fragment brought
//! with it is scanned and resolved exactly as one the definition wrote out itself, instead of
//! needing a second, fragment-shaped detector.
//!
//! [`ResolvedDefinition::body`](super::resolved::ResolvedDefinition::body) is deliberately
//! left as parsed. It is the only thing carrying real source ranges, which Source Excerpts
//! need, and a fragment's ranges point into a different file — so rewriting the body would
//! quietly make an excerpt of one file's bytes read out of another's.
//!
//! # Failure is per site
//!
//! Every way an inclusion can fail omits *that inclusion* and records a typed
//! [`UnresolvedInline`], leaving sibling sites and the rest of the definition alone. This is
//! `r12`'s survival model: the game diagnoses an unresolved reference and registers the
//! definition anyway with the included content simply absent, so a resolver that refused the
//! whole registry would be stricter than the thing it documents. What it must not do is stay
//! silent — nothing at all is logged when expansion is skipped, which is the quiet failure
//! this whole mechanism exists to prevent.

use std::collections::BTreeMap;

use crate::canonical::path::LogicalPath;

use super::registry::INLINE_SCRIPT;
use super::resolved::{
    FactSite, InlineOutcome, InlineScriptFact, ResolvedDefinition, StreamPosition, UnresolvedInline,
};
use super::selection::FileSelection;
use super::stream::{ContentFamily, FileScope, StreamEntry};

use super::super::parser::{Container, Item, ParsedFile, Scalar, ScalarKind, Value};

/// `common/inline_scripts`, recursive.
///
/// Recursive because the directory genuinely nests — `r11`'s fragments live under
/// `oracle/` and `technologies/` — which is the case
/// [`FileScope::recursive`](super::stream::FileScope) exists for.
pub(super) const SCOPE: FileScope = FileScope {
    directory: "common/inline_scripts",
    extensions: &["txt"],
    recursive: true,
};

/// The field naming the script inside a block-form call: `inline_script = { script = path … }`.
const SCRIPT_KEY: &str = "script";

/// How many inclusion sites [`expand`] will spend on one definition.
///
/// **A resolver-owned resource bound on untrusted input, not a game-measured shape.** No
/// oracle record says anything about it and none could: a mod is arbitrary input, and the
/// build has to terminate whatever it contains.
///
/// The cycle stack guards the ancestor chain, which is a different question from bounding the
/// work. A fragment that includes the next one *twice*, nested `k` deep, is entirely acyclic
/// and describes 2^k sites — thirteen tiny files are enough to ask for four thousand
/// expansions, and a few more for a build that never finishes. Bounding sites is what bounds
/// everything downstream of a site: the cloned fragment items (sites × largest fragment) and
/// the recorded facts alongside them.
///
/// Deliberately far above any legitimate use. Vanilla's heaviest known file uses 31 inclusions
/// across every definition in it, so a single definition reaching 1024 is evidence of a
/// pathological corpus rather than a limit real content can meet — which is why exceeding it
/// is a typed fact about *that* corpus and not a policy about inline scripts.
const EXPANSION_BUDGET: usize = 1024;

/// One fragment: the whole file's items, and where the file sits.
///
/// The file's **items**, not its definitions. A fragment is content spliced into a call site,
/// not a registered definition: `r11`'s fragments are bare `modifier = { … }` bodies and one
/// is a lone `inline_script` line, so a reader that took top-level definitions would be right
/// by accident for some and empty for others.
struct Script {
    items: Vec<Item>,
    /// The file, at ordinal 0. One script per file makes the file the unit, so there is no
    /// second fragment in it for an ordinal to distinguish.
    site: FactSite,
}

/// Every surviving fragment, addressed the way a call site addresses one.
pub(super) struct Library {
    scripts: BTreeMap<String, Script>,
}

/// Builds the [`Library`] by walking the inline-script scope's own stream once.
///
/// No repeat rule is applied and none is needed: one script per file, and two files cannot
/// share a logical path once [`selection`](super::selection) has run. So the map is a plain
/// insert, and the same-path override of `r11`'s fourth subject is already decided by the time
/// this sees the stream at all.
pub(super) fn build_library<R>(selection: &FileSelection, read: &R) -> Library
where
    R: Fn(&StreamEntry) -> Option<ParsedFile>,
{
    let stream = super::stream::build(selection, ContentFamily::Script, SCOPE);
    let mut scripts = BTreeMap::new();
    for entry in &stream {
        let Some(file) = read(entry) else {
            debug_assert!(false, "a surviving file did not read back: {entry:?}");
            continue;
        };
        let Some(key) = library_key(&entry.logical) else {
            debug_assert!(false, "an admitted file has no library key: {entry:?}");
            continue;
        };
        scripts.insert(
            key,
            Script {
                items: file.items.into_iter().map(|parsed| parsed.item).collect(),
                site: FactSite::Stream(StreamPosition {
                    order: entry.order,
                    source: entry.source,
                    logical: entry.logical.clone(),
                    ordinal: 0,
                }),
            },
        );
    }
    Library { scripts }
}

/// The path a call site names this file by: the logical path under [`SCOPE`]'s directory with
/// the extension dropped.
///
/// The extension is dropped by splitting on the final `.` rather than by matching `".txt"`,
/// so [`SCOPE`] stays the one authority for which extensions are admitted at all —
/// [`FileScope::admits`](super::stream::FileScope::admits) has already guaranteed there is
/// one and that it is in the list.
fn library_key(logical: &LogicalPath) -> Option<String> {
    let rest = logical
        .as_str()
        .strip_prefix(SCOPE.directory)
        .and_then(|rest| rest.strip_prefix('/'))?;
    let (stem, _extension) = rest.rsplit_once('.')?;
    Some(stem.to_owned())
}

/// Expands every inline-script site in one winner's effective fields, attaching the facts.
///
/// Root-level placement is refused rather than guessed (see [`Expansion::include`] for the
/// per-site failures): a definition's own top-level `inline_script` field raises a question
/// `r11` never asked — what the spliced fields would be fields *of* — so the field is omitted
/// with [`UnresolvedInline::RootPlacementUnmeasured`]. Omitting it rather than leaving it is
/// also what keeps the post-expansion reference scan free of `inline_script` fields, which is
/// the claim `registry::Scan::record`'s `unreachable!` rests on.
pub(super) fn expand(library: &Library, definition: &mut ResolvedDefinition) {
    let consuming = FactSite::Stream(definition.position.clone());
    let mut expansion = Expansion {
        library,
        field: String::new(),
        facts: Vec::new(),
        stack: Vec::new(),
        spent: 0,
    };
    let mut kept = Vec::with_capacity(definition.fields.len());
    for mut field in std::mem::take(&mut definition.fields) {
        expansion.field = field.field.clone();
        if field.field == INLINE_SCRIPT {
            let reference = parse_call(&field.value).map(|call| call.path);
            expansion.record(
                reference,
                &consuming,
                UnresolvedInline::RootPlacementUnmeasured,
            );
            continue;
        }
        expansion.rewrite_value(&mut field.value, &consuming);
        kept.push(field);
    }
    definition.fields = kept;
    definition.inline_expansions = expansion.facts;
}

/// One definition's expansion. The library is constant for the whole traversal; `field` is the
/// effective field currently being rewritten, which every site found beneath it — however
/// deep, and however many fragments down — is attributed to.
struct Expansion<'a> {
    library: &'a Library,
    field: String,
    facts: Vec<InlineScriptFact>,
    /// The script paths currently being expanded, outermost first. A repeat is a cycle.
    stack: Vec<String>,
    /// Sites expanded so far, against [`EXPANSION_BUDGET`]. Homed here rather than on the
    /// library because the bound is per definition: one pathological technology must not
    /// spend the budget every other definition in the row still needs.
    spent: usize,
}

impl Expansion<'_> {
    fn rewrite_value(&mut self, value: &mut Value, site: &FactSite) {
        match value {
            Value::Scalar(_) => {}
            Value::Container(container) => self.rewrite_container(container, site),
            Value::Tagged { container, .. } => self.rewrite_container(container, site),
        }
    }

    /// Rewrites a container's items, then rebuilds it so its
    /// [`ContainerKind`](super::super::parser::ContainerKind) describes what it now holds. A
    /// fragment of bare elements spliced into an object container changes that classification,
    /// and a kind left at what the source happened to state would be a second, stale authority
    /// on the same container's shape.
    fn rewrite_container(&mut self, container: &mut Container, site: &FactSite) {
        let mut items = std::mem::take(&mut container.items);
        self.rewrite_items(&mut items, site);
        *container = Container::from_items(items, container.range);
    }

    fn rewrite_items(&mut self, items: &mut Vec<Item>, site: &FactSite) {
        let mut rewritten = Vec::with_capacity(items.len());
        for mut item in std::mem::take(items) {
            match &mut item {
                Item::Field(field) if field.key.text() == INLINE_SCRIPT => {
                    rewritten.extend(self.include(&field.value, site));
                    continue;
                }
                Item::Field(field) => self.rewrite_value(&mut field.value, site),
                Item::Element(value) => self.rewrite_value(value, site),
                Item::Conditional(conditional) => self.rewrite_items(&mut conditional.items, site),
            }
            rewritten.push(item);
        }
        *items = rewritten;
    }

    /// One site: the items it contributes, and the fact that says why.
    ///
    /// Every failure returns no items, so the inclusion is absent and its siblings are
    /// untouched. The order of the checks is the order in which each becomes answerable — a
    /// call whose shape is unmeasured has no path to look up, and a path that resolves to
    /// nothing has no fragment to inspect for conditionals or parameters.
    fn include(&mut self, call: &Value, site: &FactSite) -> Vec<Item> {
        let Some(call) = parse_call(call) else {
            self.record(None, site, UnresolvedInline::CallShapeUnmeasured);
            return Vec::new();
        };
        // Before the lookup, so a corpus cannot spend unbounded work on paths that do not even
        // resolve. Charged per site rather than per level, because depth is not what grows: a
        // chain that includes the next fragment twice is shallow and still exponential.
        if self.spent == EXPANSION_BUDGET {
            self.record(
                Some(call.path),
                site,
                UnresolvedInline::ExpansionBudgetExceeded,
            );
            return Vec::new();
        }
        // Copied out so the borrow follows the library's own lifetime rather than `self`'s,
        // which is what lets a fact be recorded while a fragment is in hand.
        let library = self.library;
        let Some(script) = library.scripts.get(&call.path) else {
            self.record(Some(call.path), site, UnresolvedInline::UnknownPath);
            return Vec::new();
        };
        if self.stack.contains(&call.path) {
            self.record(Some(call.path), site, UnresolvedInline::CyclicInclusion);
            return Vec::new();
        }
        if holds_conditional(&script.items) {
            self.record(
                Some(call.path),
                site,
                UnresolvedInline::ConditionalUnmeasured,
            );
            return Vec::new();
        }

        let mut items = script.items.clone();
        substitute(&mut items, &call.bindings);
        // After substitution, because a bound parameter is not unbound — and before the fact
        // is recorded, because a fragment that would leave a `$PARAM$` in an effective field
        // did not expand.
        if let Some(name) = first_parameter(&items) {
            self.record(
                Some(call.path),
                site,
                UnresolvedInline::UnboundParameter { name },
            );
            return Vec::new();
        }

        self.facts.push(InlineScriptFact {
            reference: Some(call.path.clone()),
            field: self.field.clone(),
            site: site.clone(),
            outcome: InlineOutcome::Expanded {
                script: script.site.clone(),
                bindings: call
                    .bindings
                    .iter()
                    .map(|(name, value)| (name.clone(), value.text().into_owned()))
                    .collect(),
            },
        });
        // Charged only once the site has actually expanded, so the budget measures work done
        // rather than calls attempted — a corpus of a thousand unknown paths costs nothing and
        // is refused one fact at a time on its own merits.
        self.spent += 1;
        // Recorded before recursing, so the facts read outermost-first — the order the sites
        // are written in, once the fragments are laid out flat.
        let nested_site = script.site.clone();
        self.stack.push(call.path);
        self.rewrite_items(&mut items, &nested_site);
        self.stack.pop();
        items
    }

    fn record(&mut self, reference: Option<String>, site: &FactSite, why: UnresolvedInline) {
        self.facts.push(InlineScriptFact {
            reference,
            field: self.field.clone(),
            site: site.clone(),
            outcome: InlineOutcome::Unresolved(why),
        });
    }
}

/// A call site's resolved path and its parameter bindings.
///
/// The bindings keep the call's own [`Scalar`]s rather than their text: substituting the text
/// alone would put a token of the wrong [`ScalarKind`] into the fragment, so `factor = $F$`
/// with `F = 1000000` would produce a factor the parser had classified as an ordinary word
/// rather than a number.
struct Call {
    path: String,
    bindings: Vec<(String, Scalar)>,
}

/// The two call forms `r11` measured, and nothing else.
///
/// `inline_script = "path"` and `inline_script = { script = path  F = 1000000 }`. `None` is
/// [`UnresolvedInline::CallShapeUnmeasured`]: a container holding a non-field item, a
/// non-scalar `script` or binding value, no `script` field at all, or two of them — each a
/// shape the record does not settle, and none of them a shape to invent a reading for.
fn parse_call(value: &Value) -> Option<Call> {
    match value {
        Value::Scalar(scalar) => Some(Call {
            path: scalar.text().into_owned(),
            bindings: Vec::new(),
        }),
        Value::Container(container) => {
            let mut path: Option<String> = None;
            let mut bindings = Vec::new();
            for item in &container.items {
                let Item::Field(field) = item else {
                    return None;
                };
                let Value::Scalar(scalar) = &field.value else {
                    return None;
                };
                let key = field.key.text();
                if key == SCRIPT_KEY {
                    if path.is_some() {
                        return None;
                    }
                    path = Some(scalar.text().into_owned());
                } else {
                    bindings.push((key.into_owned(), scalar.clone()));
                }
            }
            Some(Call {
                path: path?,
                bindings,
            })
        }
        Value::Tagged { .. } => None,
    }
}

/// Replaces every bound `$PARAM$` token in `items` with the scalar the call supplied.
///
/// Whole-token substitution, in value, nested-key, and tag positions, because a whole token is
/// what the lexer calls a [`ScalarKind::Parameter`]: `classify` requires the `$` to be both
/// the first and last byte, so an embedded `abc_$X$` is an ordinary unquoted word and this
/// never touches it. That is the measured shape (`r11`'s `factor = $F$`) rather than a
/// text-substitution rule invented on top of the parser's decision.
///
/// An embedded shape therefore passes through as literal text with no fact recorded, which is
/// the one silence in this mechanism. It is bounded rather than unexamined: the census
/// (`super::census`, `docs/spikes/inline-parameter-census.md`) measured 789 embedded
/// occurrences across the fragment corpus and **none** in any fragment the technologies row
/// reaches, and its run fails if that changes. D-132 decides that the shape earns
/// `UnresolvedInline::EmbeddedParameterUnmeasured`, in the follow-up ticket that adds it.
///
/// A binding no fragment references is simply never matched. `r11` supplied one and the game
/// accepted it, so an unused binding is a fact about the call rather than a fault.
fn substitute(items: &mut [Item], bindings: &[(String, Scalar)]) {
    for item in items {
        match item {
            Item::Field(field) => {
                substitute_scalar(&mut field.key, bindings);
                substitute_value(&mut field.value, bindings);
            }
            Item::Element(value) => substitute_value(value, bindings),
            Item::Conditional(conditional) => substitute(&mut conditional.items, bindings),
        }
    }
}

fn substitute_value(value: &mut Value, bindings: &[(String, Scalar)]) {
    match value {
        Value::Scalar(scalar) => substitute_scalar(scalar, bindings),
        Value::Container(container) => substitute(&mut container.items, bindings),
        Value::Tagged { tag, container, .. } => {
            substitute_scalar(tag, bindings);
            substitute(&mut container.items, bindings);
        }
    }
}

fn substitute_scalar(scalar: &mut Scalar, bindings: &[(String, Scalar)]) {
    let Some(name) = parameter_name(scalar) else {
        return;
    };
    if let Some((_, bound)) = bindings.iter().find(|(candidate, _)| *candidate == name) {
        *scalar = bound.clone();
    }
}

/// The parameter a token names, or `None` when the token is not a parameter at all.
///
/// Reads the lexer's own decision ([`ScalarKind::Parameter`]) and then strips the delimiters
/// the raw bytes retain, rather than re-deriving "what is a `$PARAM$`" from the bytes — the
/// same one-authority rule reference detection follows for `@`.
fn parameter_name(scalar: &Scalar) -> Option<String> {
    if !matches!(scalar.kind, ScalarKind::Parameter) {
        return None;
    }
    let text = scalar.text();
    Some(
        text.strip_prefix('$')
            .and_then(|rest| rest.strip_suffix('$'))?
            .to_owned(),
    )
}

/// The first `$PARAM$` still standing in `items`, in source order.
///
/// Run after substitution, so anything left is a parameter the call never bound. First rather
/// than all, because the fact is per site and one named parameter is what makes it actionable.
fn first_parameter(items: &[Item]) -> Option<String> {
    items.iter().find_map(|item| match item {
        Item::Field(field) => parameter_name(&field.key).or_else(|| value_parameter(&field.value)),
        Item::Element(value) => value_parameter(value),
        Item::Conditional(conditional) => first_parameter(&conditional.items),
    })
}

fn value_parameter(value: &Value) -> Option<String> {
    match value {
        Value::Scalar(scalar) => parameter_name(scalar),
        Value::Container(container) => first_parameter(&container.items),
        Value::Tagged { tag, container, .. } => {
            parameter_name(tag).or_else(|| first_parameter(&container.items))
        }
    }
}

/// Whether `items` hold a `[[PARAM] … ]` conditional block at any depth.
fn holds_conditional(items: &[Item]) -> bool {
    items.iter().any(|item| match item {
        Item::Conditional(_) => true,
        Item::Field(field) => value_holds_conditional(&field.value),
        Item::Element(value) => value_holds_conditional(value),
    })
}

fn value_holds_conditional(value: &Value) -> bool {
    match value {
        Value::Scalar(_) => false,
        Value::Container(container) => holds_conditional(&container.items),
        Value::Tagged { container, .. } => holds_conditional(&container.items),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::parser::Value;
    use crate::analysis::resolver::registry::Refusal;
    use crate::analysis::resolver::resolved::{InlineOutcome, ResolvedRegistry, UnresolvedInline};
    use crate::canonical::path::LogicalPath;
    use crate::source::SourceKind;
    use crate::source::fixture::FixtureCorpus;

    /// One Target Mod resolved through the *shipped* technologies row.
    ///
    /// The shipped row rather than a trial copy, because two of the claims under test are
    /// claims about that row specifically: that an inclusion never leaves a `Parameter` scalar
    /// in an effective field (the row does not declare that kind, so a leak would refuse the
    /// whole registry), and that a failing site is a fact rather than a refusal.
    fn registry_over(files: &[(&str, &str)]) -> Result<ResolvedRegistry, Refusal> {
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .with_file("common/technology/00_empty.txt", b"")
            .build()
            .expect("an empty fixture corpus");
        let target = files
            .iter()
            .fold(
                FixtureCorpus::new(SourceKind::TargetMod)
                    .with_file("descriptor.mod", b"name=\"inline-unit\""),
                |corpus, (logical, source)| corpus.with_file(logical, source.as_bytes()),
            )
            .build()
            .expect("a well-formed fixture corpus");
        crate::analysis::resolver::resolve(&vanilla, &target).registry("technologies")
    }

    /// The single technology every unit corpus below defines, resolved.
    fn subject(files: &[(&str, &str)]) -> ResolvedDefinition {
        registry_over(files)
            .unwrap_or_else(|refusal| panic!("the technologies row resolves: {refusal}"))
            .get("tech_subject")
            .expect("the subject resolves")
            .clone()
    }

    /// A corpus of one fragment and one technology including it, the shape most of these
    /// tests vary one thing in.
    fn one_script(fragment: &str, weight_modifier_body: &str) -> Vec<(String, String)> {
        vec![
            (
                "common/inline_scripts/oracle/fragment.txt".to_owned(),
                fragment.to_owned(),
            ),
            (
                "common/technology/zz_subject.txt".to_owned(),
                format!(
                    "tech_subject = {{\n\tweight_modifier = {{\n{weight_modifier_body}\n\t}}\n}}\n"
                ),
            ),
        ]
    }

    fn borrowed(files: &[(String, String)]) -> Vec<(&str, &str)> {
        files
            .iter()
            .map(|(logical, source)| (logical.as_str(), source.as_str()))
            .collect()
    }

    /// The scalar `key = value` pairs of every `modifier` block inside the effective
    /// `weight_modifier` — the one shape these corpora expand to, so a subject's result reads
    /// the same way whichever mechanism produced it.
    fn modifier_blocks(definition: &ResolvedDefinition) -> Vec<Vec<(String, String)>> {
        let Some(Value::Container(container)) = definition.field("weight_modifier") else {
            return Vec::new();
        };
        container
            .fields()
            .filter(|field| field.key.text() == "modifier")
            .map(|field| match &field.value {
                Value::Container(inner) => inner
                    .fields()
                    .filter_map(|nested| match &nested.value {
                        Value::Scalar(scalar) => {
                            Some((nested.key.text().into_owned(), scalar.text().into_owned()))
                        }
                        Value::Container(_) | Value::Tagged { .. } => None,
                    })
                    .collect(),
                Value::Scalar(_) | Value::Tagged { .. } => Vec::new(),
            })
            .collect()
    }

    /// The effective `weight_modifier`'s own field keys. Distinguishes "the site was replaced
    /// by the fragment" from "the site is still sitting there".
    fn weight_modifier_keys(definition: &ResolvedDefinition) -> Vec<String> {
        let Some(Value::Container(container)) = definition.field("weight_modifier") else {
            return Vec::new();
        };
        container
            .fields()
            .map(|field| field.key.text().into_owned())
            .collect()
    }

    fn only_outcome(definition: &ResolvedDefinition) -> InlineOutcome {
        assert_eq!(
            definition.inline_expansions.len(),
            1,
            "{:?}",
            definition.inline_expansions
        );
        definition.inline_expansions[0].outcome.clone()
    }

    const FACTOR_FRAGMENT: &str = "modifier = {\n\tfactor = 1000000\n\talways = yes\n}\n";
    const EXPANDED: [(&str, &str); 2] = [("factor", "1000000"), ("always", "yes")];

    fn expanded() -> Vec<Vec<(String, String)>> {
        vec![
            EXPANDED
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
        ]
    }

    #[test]
    fn a_script_in_a_nested_directory_resolves_by_its_normalized_path() {
        // The library key is the logical path under `common/inline_scripts` with the
        // extension dropped, and the scope is recursive — the two halves of "keyed by file
        // path" that a flat, extension-keeping library would each get wrong.
        let files = [
            (
                "common/inline_scripts/deep/nested/fragment.txt",
                FACTOR_FRAGMENT,
            ),
            (
                "common/technology/zz_subject.txt",
                "tech_subject = {\n\tweight_modifier = {\n\t\tinline_script = \"deep/nested/fragment\"\n\t}\n}\n",
            ),
        ];
        let definition = subject(&files);
        assert_eq!(modifier_blocks(&definition), expanded());
    }

    #[test]
    fn both_call_forms_resolve_the_same_script() {
        let quoted = one_script(FACTOR_FRAGMENT, "\t\tinline_script = \"oracle/fragment\"");
        let block = one_script(
            FACTOR_FRAGMENT,
            "\t\tinline_script = {\n\t\t\tscript = oracle/fragment\n\t\t}",
        );
        assert_eq!(
            modifier_blocks(&subject(&borrowed(&quoted))),
            modifier_blocks(&subject(&borrowed(&block))),
            "the quoted single-argument form and the block form name one script"
        );
        assert_eq!(modifier_blocks(&subject(&borrowed(&quoted))), expanded());
    }

    #[test]
    fn a_bound_parameter_substitutes_in_a_value_position() {
        let files = one_script(
            "modifier = {\n\tfactor = $F$\n\talways = yes\n}\n",
            "\t\tinline_script = {\n\t\t\tscript = oracle/fragment\n\t\t\tF = 1000000\n\t\t}",
        );
        let definition = subject(&borrowed(&files));
        assert_eq!(modifier_blocks(&definition), expanded());
        let InlineOutcome::Expanded { bindings, .. } = only_outcome(&definition) else {
            panic!("expected the parameterised call to expand");
        };
        assert_eq!(bindings, [("F".to_owned(), "1000000".to_owned())]);
    }

    #[test]
    fn a_bound_parameter_substitutes_in_a_nested_key_position() {
        // `$PARAM$` as a nested field's own key is a real shape — the substitution happens
        // before the block is read — and a substituter that only replaced values would leave
        // a `Parameter` key behind, which the technologies row does not declare.
        let files = one_script(
            "modifier = {\n\tfactor = 1000000\n\t$K$ = yes\n}\n",
            "\t\tinline_script = {\n\t\t\tscript = oracle/fragment\n\t\t\tK = always\n\t\t}",
        );
        assert_eq!(modifier_blocks(&subject(&borrowed(&files))), expanded());
    }

    #[test]
    fn a_bound_parameter_substitutes_in_a_tag_position() {
        // `$T$ { 1 2 3 }` parses as a tagged value whose tag is the parameter, so a
        // substituter that only looked at fields and elements would leave the tag standing.
        // The same reasoning `Scan::walk_value` states for walking the tag: skipping a scalar
        // because it is currently uninteresting is how a blind spot is acquired.
        let files = one_script(
            "modifier = {\n\tfactor = 1000000\n\talways = yes\n\tcolour = $T$ { 1 2 3 }\n}\n",
            "\t\tinline_script = {\n\t\t\tscript = oracle/fragment\n\t\t\tT = rgb\n\t\t}",
        );
        let definition = subject(&borrowed(&files));
        let Some(Value::Container(container)) = definition.field("weight_modifier") else {
            panic!("weight_modifier is an effective container");
        };
        let Some(Value::Container(modifier)) = container.fields().next().map(|field| &field.value)
        else {
            panic!("the fragment's modifier block was spliced in");
        };
        let Some(Value::Tagged { tag, .. }) = modifier
            .fields()
            .find(|field| field.key.text() == "colour")
            .map(|field| &field.value)
        else {
            panic!("the tagged value survived expansion: {modifier:?}");
        };
        assert_eq!(tag.text(), "rgb");
    }

    #[test]
    fn an_unused_binding_is_recorded_and_harmless() {
        // `r11` bound `TECHNOLOGY` to a fragment that never referenced it, and the game
        // accepted the call. The binding is recorded because it is a fact about the call.
        let files = one_script(
            FACTOR_FRAGMENT,
            "\t\tinline_script = {\n\t\t\tscript = oracle/fragment\n\t\t\tUNUSED = anything\n\t\t}",
        );
        let definition = subject(&borrowed(&files));
        assert_eq!(modifier_blocks(&definition), expanded());
        let InlineOutcome::Expanded { bindings, .. } = only_outcome(&definition) else {
            panic!("an unused binding must not prevent expansion");
        };
        assert_eq!(bindings, [("UNUSED".to_owned(), "anything".to_owned())]);
    }

    #[test]
    fn a_nested_inclusion_expands_recursively() {
        let files = [
            (
                "common/inline_scripts/oracle/outer.txt",
                "inline_script = \"oracle/inner\"\n",
            ),
            ("common/inline_scripts/oracle/inner.txt", FACTOR_FRAGMENT),
            (
                "common/technology/zz_subject.txt",
                "tech_subject = {\n\tweight_modifier = {\n\t\tinline_script = \"oracle/outer\"\n\t}\n}\n",
            ),
        ];
        let definition = subject(&files);
        assert_eq!(
            modifier_blocks(&definition),
            expanded(),
            "a one-level expander splices the outer fragment and stops, leaving no modifier"
        );
        assert_eq!(
            definition.inline_expansions.len(),
            2,
            "both the consuming site and the site inside the outer fragment are recorded"
        );
        let nested = &definition.inline_expansions[1];
        assert_eq!(nested.reference.as_deref(), Some("oracle/inner"));
        assert_eq!(
            nested.site.logical().map(LogicalPath::as_str),
            Some("common/inline_scripts/oracle/outer.txt"),
            "a nested site sits in the fragment that states it, not in the consuming file"
        );
        assert_eq!(
            nested.field, "weight_modifier",
            "attributed to the effective field a consumer reads"
        );
    }

    /// A fragment chain where every level includes the next one **twice**, so `depth + 1` tiny
    /// files describe 2^depth leaf sites. Entirely acyclic: no path ever repeats on the
    /// ancestor chain, which is exactly why the cycle stack does not bound this.
    fn doubling_chain(depth: usize) -> Vec<(String, String)> {
        let mut files: Vec<(String, String)> = (0..depth)
            .map(|level| {
                let next = level + 1;
                (
                    format!("common/inline_scripts/oracle/level_{level}.txt"),
                    format!(
                        "inline_script = \"oracle/level_{next}\"\n\
                         inline_script = \"oracle/level_{next}\"\n"
                    ),
                )
            })
            .collect();
        files.push((
            format!("common/inline_scripts/oracle/level_{depth}.txt"),
            "modifier = {\n\tfactor = 1\n}\n".to_owned(),
        ));
        files.push((
            "common/technology/zz_subject.txt".to_owned(),
            "tech_subject = {\n\tweight_modifier = {\n\t\tinline_script = \"oracle/level_0\"\n\t}\n}\n"
                .to_owned(),
        ));
        files
    }

    #[test]
    fn a_doubling_chain_stops_at_the_expansion_budget() {
        // Thirteen files asking for 2^13 - 1 expansions. Mod content is untrusted input and
        // nothing about a corpus's size bounds the work it describes, so the budget is what
        // bounds it — and it says so, rather than stopping quietly.
        let definition = subject(&borrowed(&doubling_chain(12)));

        assert!(
            definition.inline_expansions.iter().any(|fact| fact.outcome
                == InlineOutcome::Unresolved(UnresolvedInline::ExpansionBudgetExceeded)),
            "an exhausted budget must be a typed outcome like every other per-site failure, \
             not a silent truncation"
        );
        let expanded = definition
            .inline_expansions
            .iter()
            .filter(|fact| matches!(fact.outcome, InlineOutcome::Expanded { .. }))
            .count();
        assert_eq!(
            expanded, EXPANSION_BUDGET,
            "the budget is spent on sites that actually expanded"
        );
        // Every expanded site splices at most two further sites, so the sites left to refuse
        // are bounded by twice the budget. Without the bound this corpus records 8191 facts.
        assert!(
            definition.inline_expansions.len() <= 3 * EXPANSION_BUDGET,
            "the facts vector must be bounded alongside the work: {}",
            definition.inline_expansions.len()
        );
    }

    #[test]
    fn legitimate_nesting_stays_far_below_the_expansion_budget() {
        // The control that keeps the budget from being a policy about real content: `r11`'s
        // own nesting shape spends two sites out of 1024, so the bound above is evidence of a
        // pathological corpus rather than a limit inline scripts routinely meet.
        let files = [
            (
                "common/inline_scripts/oracle/outer.txt",
                "inline_script = \"oracle/inner\"\n",
            ),
            ("common/inline_scripts/oracle/inner.txt", FACTOR_FRAGMENT),
            (
                "common/technology/zz_subject.txt",
                "tech_subject = {\n\tweight_modifier = {\n\t\tinline_script = \"oracle/outer\"\n\t}\n}\n",
            ),
        ];
        let definition = subject(&files);
        assert_eq!(definition.inline_expansions.len(), 2);
        assert!(
            definition.inline_expansions.iter().all(|fact| fact.outcome
                != InlineOutcome::Unresolved(UnresolvedInline::ExpansionBudgetExceeded)),
            "ordinary nesting must never reach the bound"
        );
        assert_eq!(modifier_blocks(&definition), expanded());
    }

    #[test]
    fn a_cyclic_inclusion_terminates_with_a_typed_outcome() {
        let files = [
            (
                "common/inline_scripts/oracle/a.txt",
                "inline_script = \"oracle/b\"\n",
            ),
            (
                "common/inline_scripts/oracle/b.txt",
                "inline_script = \"oracle/a\"\n",
            ),
            (
                "common/technology/zz_subject.txt",
                "tech_subject = {\n\tweight_modifier = {\n\t\tinline_script = \"oracle/a\"\n\t}\n}\n",
            ),
        ];
        let definition = subject(&files);
        let cyclic = definition
            .inline_expansions
            .last()
            .expect("the repeat is recorded");
        assert_eq!(
            cyclic.outcome,
            InlineOutcome::Unresolved(UnresolvedInline::CyclicInclusion)
        );
        assert_eq!(cyclic.reference.as_deref(), Some("oracle/a"));
        assert!(
            weight_modifier_keys(&definition).is_empty(),
            "the cycle contributes nothing, and no inline_script field is left behind"
        );
    }

    #[test]
    fn an_unknown_path_omits_the_inclusion_and_records_the_reference() {
        let files = one_script(FACTOR_FRAGMENT, "\t\tinline_script = \"oracle/absent\"");
        let definition = subject(&borrowed(&files));
        assert_eq!(
            only_outcome(&definition),
            InlineOutcome::Unresolved(UnresolvedInline::UnknownPath)
        );
        assert_eq!(
            definition.inline_expansions[0].reference.as_deref(),
            Some("oracle/absent"),
            "the fact must name the path that did not resolve"
        );
        assert!(weight_modifier_keys(&definition).is_empty());
    }

    #[test]
    fn an_unbound_parameter_never_leaks_into_an_effective_field() {
        // Substituting empty would fabricate content; leaving `$F$` in place would put a
        // Parameter scalar in an effective field of a row that declares no Parameter kind,
        // refusing the whole registry over one inclusion. The corpus resolving at all is half
        // this test's assertion.
        let files = one_script(
            "modifier = {\n\tfactor = $F$\n}\n",
            "\t\tinline_script = \"oracle/fragment\"",
        );
        let definition = subject(&borrowed(&files));
        assert_eq!(
            only_outcome(&definition),
            InlineOutcome::Unresolved(UnresolvedInline::UnboundParameter {
                name: "F".to_owned()
            })
        );
        assert!(weight_modifier_keys(&definition).is_empty());
    }

    #[test]
    fn a_conditional_block_in_a_fragment_is_typed_unresolved() {
        let files = one_script(
            "modifier = {\n\tfactor = 2\n\t[[F] always = yes ]\n}\n",
            "\t\tinline_script = {\n\t\t\tscript = oracle/fragment\n\t\t\tF = yes\n\t\t}",
        );
        let definition = subject(&borrowed(&files));
        assert_eq!(
            only_outcome(&definition),
            InlineOutcome::Unresolved(UnresolvedInline::ConditionalUnmeasured)
        );
        assert!(
            weight_modifier_keys(&definition).is_empty(),
            "the whole inclusion is omitted, including the unconditional part — no record \
             says which half the game keeps"
        );
    }

    #[test]
    fn a_call_shape_no_record_measures_is_typed_unresolved() {
        let files = one_script(
            FACTOR_FRAGMENT,
            "\t\tinline_script = {\n\t\t\tF = 1000000\n\t\t}",
        );
        let definition = subject(&borrowed(&files));
        assert_eq!(
            only_outcome(&definition),
            InlineOutcome::Unresolved(UnresolvedInline::CallShapeUnmeasured)
        );
        assert_eq!(
            definition.inline_expansions[0].reference, None,
            "a call that named no path has no reference to name — absent, not empty"
        );
    }

    #[test]
    fn a_root_level_inline_script_field_is_typed_unresolved_and_omitted() {
        let files = [
            ("common/inline_scripts/oracle/fragment.txt", FACTOR_FRAGMENT),
            (
                "common/technology/zz_subject.txt",
                "tech_subject = {\n\ttier = 1\n\tinline_script = \"oracle/fragment\"\n}\n",
            ),
        ];
        let definition = subject(&files);
        assert_eq!(
            only_outcome(&definition),
            InlineOutcome::Unresolved(UnresolvedInline::RootPlacementUnmeasured)
        );
        assert_eq!(definition.inline_expansions[0].field, "inline_script");
        assert!(
            !definition.states("inline_script"),
            "the field is omitted, which is what keeps the post-expansion scan free of \
             inline_script fields"
        );
        assert!(definition.states("tier"), "its siblings are untouched");
    }

    #[test]
    fn a_failing_site_leaves_its_siblings_expanded() {
        let files = one_script(
            FACTOR_FRAGMENT,
            "\t\tinline_script = \"oracle/absent\"\n\t\tinline_script = \"oracle/fragment\"",
        );
        let definition = subject(&borrowed(&files));
        assert_eq!(
            definition
                .inline_expansions
                .iter()
                .map(|fact| fact.reference.clone())
                .collect::<Vec<_>>(),
            [
                Some("oracle/absent".to_owned()),
                Some("oracle/fragment".to_owned())
            ]
        );
        assert_eq!(
            modifier_blocks(&definition),
            expanded(),
            "the sibling that resolves still contributes its content"
        );
    }

    #[test]
    fn a_failing_nested_site_does_not_fail_its_parent_site() {
        let files = [
            (
                "common/inline_scripts/oracle/outer.txt",
                "modifier = {\n\tfactor = 1000000\n\talways = yes\n}\ninline_script = \"oracle/absent\"\n",
            ),
            (
                "common/technology/zz_subject.txt",
                "tech_subject = {\n\tweight_modifier = {\n\t\tinline_script = \"oracle/outer\"\n\t}\n}\n",
            ),
        ];
        let definition = subject(&files);
        assert_eq!(
            modifier_blocks(&definition),
            expanded(),
            "the parent's own content is spliced in even though a site inside it failed"
        );
        assert_eq!(
            definition.inline_expansions[1].outcome,
            InlineOutcome::Unresolved(UnresolvedInline::UnknownPath)
        );
        assert!(matches!(
            definition.inline_expansions[0].outcome,
            InlineOutcome::Expanded { .. }
        ));
    }
}
