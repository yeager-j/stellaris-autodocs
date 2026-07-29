//! The embedded-`$PARAM$` census: how `$NAME$` occurrences actually sit inside the tokens of
//! real inline-script fragments, and which of those fragments a technology can reach.
//!
//! # What this measures and why it is not a test of the resolver
//!
//! [`inline_scripts::substitute`](super::inline_scripts) replaces a `$PARAM$` only where the
//! dialect lexer classified the whole token as [`ScalarKind::Parameter`], which `classify`
//! does only when `$` is both the first and last byte. An embedded shape — `tech_$TIER$`,
//! `"CONTRACT_$KEY$_PROJECT"`, `@base_army_$TYPE$` — is therefore an ordinary scalar that
//! passes through expansion as literal text with **no typed fact**: the one place the
//! mechanism is silent about a shape it did not handle. D-131 covers the two shapes that *are*
//! typed-unresolved; this one is not even detected.
//!
//! So the question is empirical, and it is a question about the corpus rather than about the
//! code: does the shape occur, and does it occur where a row that declares the mechanism can
//! reach it? This module answers it, and answers it the way the resolver would have to — by
//! reading the lexer's own [`ScalarKind`] rather than re-deriving "what is a `$PARAM$`" from
//! raw bytes. A regex over the file would be a second authority on tokenization and would
//! also count the `$PARAM$` occurrences in `00_README.txt`'s prose, which the parser drops
//! with the comments they sit in.
//!
//! Reachability is taken from the production mechanism too: the technologies row is resolved
//! over both installed corpora and the fragment set is read out of the
//! [`InlineScriptFact`]s it recorded, at every nesting depth. Re-walking the include graph
//! here would be a second expander.
//!
//! # Running it
//!
//! The corpora are a licensed local installation, so this is not part of the ordinary suite:
//!
//! ```text
//! cargo test --features test-support embedded_parameter_census -- --ignored --nocapture
//! ```
//!
//! Roots come from [`corpora`], the same overrides the parser conformance run uses. The
//! measurement and its conclusion are recorded in
//! `docs/spikes/inline-parameter-census.md` (STE-34); the decision it settles is D-132.
//!
//! # What the assertions claim, and why none of them can pass vacuously
//!
//! The recorded counts are printed rather than pinned — they are a property of whichever
//! Stellaris build and mod version is installed, and the spike note is where a specific
//! reading belongs. Three claims are asserted, because each would be a *changed conclusion*
//! rather than a changed number:
//!
//! 1. **No technology-reachable fragment carries an embedded shape.** This is the finding the
//!    silent pass-through is vacuous under. If it ever fails, the pass-through has become
//!    live for a shipped row and D-132's condition for adding a typed variant is met.
//! 2. **The corpus at large carries many.** This is the census's own negative control: the
//!    same detector that reports zero above finds hundreds elsewhere, so the zero is a fact
//!    about reachability and not a detector that quietly matches nothing.
//! 3. **The reachable set contains the whole-token `$TECHNOLOGY$` `r11` measured.** Without
//!    it, a reachability computation that returned nothing at all would satisfy claim 1 by
//!    finding no fragments to look in.

use std::collections::{BTreeMap, BTreeSet};

use crate::analysis::corpora::{self, establish_corpus};
use crate::analysis::parser::{self, Item, ScalarKind, SourceIdentity, Value};
use crate::canonical::path::LogicalPath;
use crate::source::SourceSnapshot;

use super::inline_scripts;
use super::resolved::{InlineOutcome, ResolvedRegistry};
use super::stream::{self, ContentFamily};

/// How a `$…$` run sits inside the token that contains it.
///
/// The distinction the whole census turns on, and the only one it draws: `substitute` acts on
/// [`Placement::WholeToken`] and is blind to [`Placement::Embedded`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Placement {
    /// The token *is* the run: `$TECHNOLOGY$`. What `r11` measured.
    WholeToken,
    /// The run sits inside a larger token: `tech_$TIER$`, `job_$JOB$_desc`.
    Embedded,
}

/// A `$`-bearing token, as the census reports it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Occurrence {
    /// The lexer's classification, by name. Named rather than the enum because
    /// [`ScalarKind`] is deliberately not `Ord` and a census keyed on it wants a stable
    /// ordering more than it wants the discriminant.
    kind: &'static str,
    placement: Placement,
    /// The token text, quotes already excluded (`Scalar::raw`).
    token: String,
    /// The fragment the token sits in.
    fragment: String,
}

/// One fragment set's whole census.
#[derive(Debug, Default)]
struct Census {
    fragments: usize,
    /// Distinct occurrences, so a token repeated in one fragment is one finding; the total
    /// count lives in [`Census::occurrences`].
    distinct: BTreeSet<Occurrence>,
    occurrences: BTreeMap<(&'static str, Placement), usize>,
    /// `[[PARAM] … ]` blocks, at any depth. Counted because they are the corpus incidence of
    /// D-131's other unmeasured shape, and this walk is already over every item.
    conditional_blocks: usize,
}

impl Census {
    fn total(&self, placement: Placement) -> usize {
        self.occurrences
            .iter()
            .filter(|((_, seen), _)| *seen == placement)
            .map(|(_, count)| count)
            .sum()
    }

    /// Every whole-token parameter the set states, by name.
    fn whole_token_names(&self) -> BTreeSet<&str> {
        self.distinct
            .iter()
            .filter(|occurrence| occurrence.placement == Placement::WholeToken)
            .map(|occurrence| occurrence.token.as_str())
            .collect()
    }

    /// The first few embedded occurrences, for a failure message.
    ///
    /// The total goes first, so a message that shows ten of several hundred cannot read as the
    /// whole finding — the same reason [`Census::report`] prints its counts above its listing.
    fn embedded_excerpt(&self) -> String {
        const SHOWN: usize = 10;
        let embedded: Vec<&Occurrence> = self
            .distinct
            .iter()
            .filter(|occurrence| occurrence.placement == Placement::Embedded)
            .collect();
        let listed: String = embedded
            .iter()
            .take(SHOWN)
            .map(|occurrence| {
                format!(
                    "\n  [{}] {} <- {}",
                    occurrence.kind, occurrence.token, occurrence.fragment
                )
            })
            .collect();
        format!(
            "{} distinct embedded occurrences, first {}:{listed}",
            embedded.len(),
            SHOWN.min(embedded.len())
        )
    }

    fn report(&self, label: &str) -> String {
        let mut report = format!(
            "## {label}\n{} fragments, {} conditional blocks\n",
            self.fragments, self.conditional_blocks
        );
        for ((kind, placement), count) in &self.occurrences {
            report.push_str(&format!("  {kind:<12} {placement:?}: {count}\n"));
        }
        for occurrence in &self.distinct {
            report.push_str(&format!(
                "    [{} {:?}] {} <- {}\n",
                occurrence.kind, occurrence.placement, occurrence.token, occurrence.fragment
            ));
        }
        report
    }
}

/// The `$…$` runs in one token: the byte spans a whole-token parameter would have to be.
///
/// A run is a `$`, at least one non-`$` byte, and a closing `$`. `district_$A$$B$` therefore
/// holds two runs, and a lone `$` holds none — which is why the census can distinguish "an
/// embedded parameter" from "a token that merely contains a dollar sign".
fn parameter_runs(token: &str) -> usize {
    let bytes = token.as_bytes();
    let mut runs = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'$' {
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while end < bytes.len() && bytes[end] != b'$' {
            end += 1;
        }
        if end < bytes.len() && end > index + 1 {
            runs += 1;
            index = end + 1;
        } else {
            index = end;
        }
    }
    runs
}

fn kind_name(kind: ScalarKind) -> &'static str {
    match kind {
        ScalarKind::Unquoted => "Unquoted",
        ScalarKind::Quoted => "Quoted",
        ScalarKind::Number => "Number",
        ScalarKind::VariableRef => "VariableRef",
        ScalarKind::VariableExpr => "VariableExpr",
        ScalarKind::Parameter => "Parameter",
        ScalarKind::ScopeToken => "ScopeToken",
    }
}

/// Walks one fragment's items, recording every `$`-bearing scalar in key, value, tag, and
/// element position.
///
/// Every position, for the reason `substitute` covers all of them: a walk that skipped a
/// scalar because it is currently uninteresting is how a blind spot is acquired.
fn record_items(items: &[Item], fragment: &str, census: &mut Census) {
    for item in items {
        match item {
            Item::Field(field) => {
                record_scalar(&field.key, fragment, census);
                record_value(&field.value, fragment, census);
            }
            Item::Element(value) => record_value(value, fragment, census),
            Item::Conditional(conditional) => {
                census.conditional_blocks += 1;
                record_items(&conditional.items, fragment, census);
            }
        }
    }
}

fn record_value(value: &Value, fragment: &str, census: &mut Census) {
    match value {
        Value::Scalar(scalar) => record_scalar(scalar, fragment, census),
        Value::Container(container) => record_items(&container.items, fragment, census),
        Value::Tagged { tag, container, .. } => {
            record_scalar(tag, fragment, census);
            record_items(&container.items, fragment, census);
        }
    }
}

fn record_scalar(scalar: &parser::Scalar, fragment: &str, census: &mut Census) {
    let token = scalar.text();
    if !token.contains('$') {
        return;
    }
    let runs = parameter_runs(&token);
    if runs == 0 {
        // A `$` that never closes is not a parameter shape under any reading, and counting it
        // would inflate the finding this census exists to size.
        return;
    }
    // The lexer's decision, not a re-reading of the bytes: `Parameter` is exactly the set
    // `substitute` acts on, so anything else with a run in it is the silent shape.
    let placement = match scalar.kind {
        ScalarKind::Parameter => Placement::WholeToken,
        _ if runs == 1 && token.starts_with('$') && token.ends_with('$') => {
            // A whole-token run the lexer did *not* call a Parameter: a quoted `"$NAME$"`.
            // Reported as whole-token because that is its shape, while its kind records why
            // substitution still cannot see it.
            Placement::WholeToken
        }
        _ => Placement::Embedded,
    };
    let kind = kind_name(scalar.kind);
    *census.occurrences.entry((kind, placement)).or_default() += 1;
    census.distinct.insert(Occurrence {
        kind,
        placement,
        token: token.into_owned(),
        fragment: fragment.to_owned(),
    });
}

/// Censuses the surviving inline-script fragments of a resolution, optionally narrowed to a
/// set of logical paths.
///
/// The fragment set comes from [`stream::build`] over [`inline_scripts::SCOPE`] — the same
/// scope and the same selection the library itself is built from, so "every fragment" here
/// means every fragment the resolver would have.
fn census_fragments(
    selection: &super::selection::FileSelection,
    read: impl Fn(&stream::StreamEntry) -> Option<crate::source::SourceBytes>,
    keep: Option<&BTreeSet<LogicalPath>>,
) -> Census {
    let mut census = Census::default();
    for entry in stream::build(selection, ContentFamily::Script, inline_scripts::SCOPE) {
        if keep.is_some_and(|keep| !keep.contains(&entry.logical)) {
            continue;
        }
        let Some(bytes) = read(&entry) else {
            panic!("a selected fragment did not read back: {entry:?}");
        };
        let identity = SourceIdentity::new(entry.source, entry.logical.clone());
        let file = parser::parse(identity, bytes.as_slice());
        census.fragments += 1;
        let items: Vec<Item> = file.items.into_iter().map(|parsed| parsed.item).collect();
        record_items(&items, entry.logical.as_str(), &mut census);
    }
    census
}

/// The fragment files the technologies row actually reached, and what happened at every site.
///
/// Read out of the recorded facts rather than by walking the include graph again: `expand`
/// records one fact per site at every depth, so the `Expanded` facts' script sites *are* the
/// transitive reachable set — by the same mechanism that would have to substitute an embedded
/// parameter if one were there.
struct Reach {
    fragments: BTreeSet<LogicalPath>,
    outcomes: BTreeMap<String, usize>,
    sites: usize,
}

fn technology_reach(registry: &ResolvedRegistry) -> Reach {
    let mut reach = Reach {
        fragments: BTreeSet::new(),
        outcomes: BTreeMap::new(),
        sites: 0,
    };
    for key in registry.keys() {
        let definition = registry
            .get(key)
            .expect("a key the registry listed resolves");
        for fact in &definition.inline_expansions {
            reach.sites += 1;
            match &fact.outcome {
                InlineOutcome::Expanded { script, .. } => {
                    *reach.outcomes.entry("Expanded".to_owned()).or_default() += 1;
                    if let Some(logical) = script.logical() {
                        reach.fragments.insert(logical.clone());
                    }
                }
                InlineOutcome::Unresolved(why) => {
                    *reach
                        .outcomes
                        .entry(format!("Unresolved({why:?})"))
                        .or_default() += 1;
                }
            }
        }
    }
    reach
}

/// The census STE-34 asked for. See the module comment for how to run it and what it claims.
#[test]
#[ignore = "requires an installed Stellaris and ACOT; run with --ignored"]
fn embedded_parameter_census() {
    let vanilla_corpus = corpora::vanilla();
    let acot_corpus = corpora::acot();
    let (vanilla_source, mut warnings) = establish_corpus(&vanilla_corpus);
    let (acot_source, acot_warnings) = establish_corpus(&acot_corpus);
    warnings.extend(acot_warnings);
    let vanilla: &SourceSnapshot = vanilla_source.snapshot();
    let acot: &SourceSnapshot = acot_source.snapshot();

    let resolution = super::resolve(vanilla, acot);
    let registry = resolution
        .registry("technologies")
        .unwrap_or_else(|refusal| {
            panic!("the technologies row resolves over both corpora: {refusal}")
        });
    let reach = technology_reach(&registry);

    let selection = super::selection::select(vanilla, acot);
    let read = |entry: &stream::StreamEntry| match entry.source {
        crate::source::SourceKind::VanillaContent => vanilla.read(&entry.logical),
        crate::source::SourceKind::TargetMod => acot.read(&entry.logical),
    };
    let everything = census_fragments(&selection, read, None);
    let reachable = census_fragments(&selection, read, Some(&reach.fragments));

    for warning in &warnings {
        println!("warning: {warning}");
    }
    println!(
        "{} technologies, {} inclusion sites, {} distinct fragments reached",
        registry.keys().len(),
        reach.sites,
        reach.fragments.len()
    );
    for (outcome, count) in &reach.outcomes {
        println!("  {outcome}: {count}");
    }
    for fragment in &reach.fragments {
        println!("  reached: {}", fragment.as_str());
    }
    println!("{}", reachable.report("technology-reachable fragments"));
    println!("{}", everything.report("every surviving fragment"));

    // Claim 3 first, because it is what stops claim 1 from passing on an empty set.
    assert!(
        reachable.fragments > 0,
        "no fragment was reached at all, so a zero embedded count below would say nothing \
         about the corpus"
    );
    // Stated as an equality rather than "some were censused", because a `keep` filter that
    // matched only part of the reached set would leave claim 1 true of a subset while looking
    // like a claim about all of it — a narrowing no failure elsewhere would reveal.
    assert_eq!(
        reachable.fragments,
        reach.fragments.len(),
        "every reached fragment must also be in the surviving stream the census walks"
    );
    assert!(
        reachable.whole_token_names().contains("$TECHNOLOGY$"),
        "the reachable set no longer contains the whole-token $TECHNOLOGY$ that `r11` \
         measured, so reachability is measuring something other than what it did: {:?}",
        reachable.whole_token_names()
    );
    // Claim 2: the detector finds the shape where the shape is.
    assert!(
        everything.total(Placement::Embedded) > 0,
        "the census found no embedded parameter anywhere in {} fragments, which is a broken \
         detector rather than a clean corpus",
        everything.fragments
    );
    // Claim 1: the finding.
    assert_eq!(
        reachable.total(Placement::Embedded),
        0,
        "an embedded parameter is now reachable from a technology, so `substitute`'s silent \
         pass-through is live for a shipped row. D-132 conditions a typed \
         `UnresolvedInline` variant on exactly this. {}",
        reachable.embedded_excerpt()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_runs_counts_closed_runs_only() {
        // The negative control for the census's own token reading: without the closing-`$`
        // requirement a bare `$` or a price string would be counted as a parameter shape and
        // the headline finding would be inflated by tokens that carry no parameter at all.
        assert_eq!(parameter_runs("$TECHNOLOGY$"), 1);
        assert_eq!(parameter_runs("tech_$TIER$"), 1);
        assert_eq!(parameter_runs("district_$A$$B$"), 2);
        assert_eq!(parameter_runs("$"), 0);
        assert_eq!(parameter_runs("$$"), 0);
        assert_eq!(parameter_runs("cost_$UNCLOSED"), 0);
        assert_eq!(parameter_runs("no_dollars"), 0);
    }

    /// The census's classification, exercised through the real lexer on hand-written source
    /// so the placement split is checked without an installed corpus.
    fn census_of(source: &str) -> Census {
        let identity = SourceIdentity::new(
            crate::source::SourceKind::TargetMod,
            LogicalPath::parse("common/inline_scripts/unit.txt").expect("a logical path"),
        );
        let file = parser::parse(identity, source.as_bytes());
        let items: Vec<Item> = file.items.into_iter().map(|parsed| parsed.item).collect();
        let mut census = Census {
            fragments: 1,
            ..Default::default()
        };
        record_items(&items, "common/inline_scripts/unit.txt", &mut census);
        census
    }

    #[test]
    fn a_whole_token_parameter_and_an_embedded_one_are_told_apart() {
        let census = census_of("modifier = {\n\tfactor = $F$\n\thas_technology = tech_$TIER$\n}\n");
        assert_eq!(census.total(Placement::WholeToken), 1);
        assert_eq!(census.total(Placement::Embedded), 1);
        assert_eq!(
            census
                .occurrences
                .get(&("Parameter", Placement::WholeToken)),
            Some(&1)
        );
        assert_eq!(
            census.occurrences.get(&("Unquoted", Placement::Embedded)),
            Some(&1)
        );
    }

    #[test]
    fn an_embedded_parameter_in_a_key_or_a_quoted_string_is_still_counted() {
        // Both are positions `substitute` cannot reach — a quoted token is
        // `ScalarKind::Quoted` whatever its bytes say — and both occur in the real corpus.
        let census = census_of("job_$JOB$_add = 1\ntitle = \"CONTRACT_$KEY$_PROJECT\"\n");
        assert_eq!(census.total(Placement::Embedded), 2);
        assert_eq!(
            census.occurrences.get(&("Quoted", Placement::Embedded)),
            Some(&1)
        );
    }

    #[test]
    fn a_conditional_block_is_counted_and_walked() {
        let census =
            census_of("modifier = {\n\tfactor = 2\n\t[[F] has_technology = tech_$T$ ]\n}\n");
        assert_eq!(census.conditional_blocks, 1);
        assert_eq!(
            census.total(Placement::Embedded),
            1,
            "a shape inside a conditional block is still a shape in the fragment"
        );
    }
}
