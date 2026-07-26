//! File selection and the resolved registries, implementing only the profile rows the
//! resolver spike marks **Resolved**.
//!
//! The rule this module refuses to break is `docs/spikes/resolver-evaluation.md:300`: a
//! content type may be claimed as supported only when every policy it requires is explicit
//! and oracle-backed, and an unresolved cell fails visibly instead of becoming implementation
//! discretion. So there is no megastructure registry here, no ship components, and no
//! cross-source scripted-constant resolution. Their rows are unresolved, and a bundle
//! measurement taken over records this harness invented would be measuring the invention.
//!
//! What is implemented, and what established it:
//!
//! | Row | Rule | Record |
//! | --- | --- | --- |
//! | Exact file-path collision | The winning file replaces the losing file *entirely* | `r6` |
//! | `replace_path` | Excludes every other source in that directory; keeps the declarer's own | `r3` |
//! | Technologies | Last in one global enumeration order wins; whole-object replacement | `r0`, `r1`, `r4` |
//! | Inline scripts | Textual expansion before registration, with `$PARAM$` and nesting | `r11`, `r12` |
//! | Localization | Its own layered stream — see [`crate::localization`] | `r13`–`r16` |
//!
//! The load-bearing asymmetry, which is easy to implement backwards: script registries
//! resolve in **one global logical-path order with no layer precedence**. A Target Mod wins
//! only when its filename sorts on the winning side. `r10` observed vanilla beating a mod
//! file named `!!!_…` for a technology, and the mod beating vanilla for an event in the same
//! run, because those two registries accept and reject repeats in opposite directions.

use crate::corpus::{ContributorKind, RevisionCase, Snapshot, SourceFile};
use crate::localization::{self, Localization, StreamFile};
use parser_spike::model::{Container, Item, ParsedFile, Scalar, Value};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Where one definition came from, in enough detail to cite it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRef {
    pub contributor: String,
    /// Normalized logical path, `/`-separated, relative to the contributor root.
    pub logical: String,
    /// Position of this definition among the file's top-level definitions, from zero.
    pub ordinal: u32,
    pub span_start: u32,
    pub span_end: u32,
}

/// How much the parser could prove about a definition.
///
/// `docs/technical-design.md:271` requires this distinction and forbids collapsing it:
/// definitions proven before the first fault remain Clean, and definitions emitted after
/// heuristic resynchronization are Recovered *because their nesting may have been
/// misattributed*. Recovery is a heuristic about source layout, not a rule of the grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    Clean,
    Recovered,
}

/// How an Analysis Issue affects what depends on it (`docs/technical-design.md:336`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Impact {
    EvidenceAbsent,
    EvidencePresentUnsupported,
    RegistryCompletenessUnknown,
    DerivedFactPotentiallyIncomplete,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "scope", content = "name")]
pub enum IssueScope {
    Revision,
    Registry(String),
    Entry(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub code: String,
    pub scope: IssueScope,
    pub impact: Impact,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
}

/// One effective definition plus the ordered provenance of what it displaced.
#[derive(Debug, Clone)]
pub struct ResolvedDefinition {
    pub key: String,
    /// The winning definition's body, with inline scripts already expanded.
    pub container: Container,
    pub evidence: Evidence,
    pub source: SourceRef,
    /// Definitions this one displaced, in resolution order. `docs/technical-design.md:283`
    /// requires provenance for every contributed, inherited, defaulted, duplicate, and
    /// shadowed fact; for whole-object replacement the shadowed *definition* is that record.
    pub shadowed: Vec<SourceRef>,
}

/// What one revision case resolves to.
pub struct Resolved {
    pub case: String,
    pub technologies: Vec<ResolvedDefinition>,
    pub localization: Localization,
    /// Technology key to the logical path of its icon, within the winning contributor.
    pub icons: BTreeMap<String, IconSlot>,
    pub issues: Vec<Issue>,
    pub stats: Stats,
    /// The exact bytes of every registry file that contributed a definition, keyed by
    /// normalized logical path.
    ///
    /// Retained rather than re-read, because a Source Excerpt must be sliced from *the bytes
    /// that were parsed*. `docs/technical-design.md:414` requires the snapshot protocol to
    /// hash and parse the same bytes and capture excerpts from those bytes; re-opening the
    /// file to cut an excerpt would reintroduce exactly the window the protocol closes.
    pub sources: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IconSlot {
    pub contributor: String,
    pub logical: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub files_selected: usize,
    pub files_shadowed_by_path: usize,
    pub files_excluded_by_replace_path: usize,
    pub technology_files: usize,
    pub technology_definitions_seen: usize,
    pub technology_definitions_effective: usize,
    pub technology_definitions_recovered: usize,
    pub inline_script_expansions: usize,
    pub inline_script_unresolved: usize,
    pub parse_faults: usize,
    pub icons_resolved: usize,
    pub icons_missing: usize,
}

/// The last field with this key, or `None`.
///
/// Last rather than first, because within one definition a repeated field follows the same
/// last-wins rule the technology registry uses for repeated definitions. Repeats do exist in
/// the corpus, and picking the first would quietly document the losing value.
pub fn field<'a>(container: &'a Container, key: &str) -> Option<&'a parser_spike::model::Field> {
    container.fields().filter(|field| field.key.text() == key).last()
}

/// The exact source lexeme of a scalar value.
///
/// Text, never a parsed number. `docs/decision-log.md` D-099 keeps binary floating point out
/// of source equality, identity, and displayed Base Values, and the resolver oracle watched
/// the game compare `0.1 + 0.2` against `0.3` as exactly equal. A `f64` here would be a
/// second, quieter authority for what a cost is.
pub fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::Scalar(scalar) => Some(scalar.text().into_owned()),
        _ => None,
    }
}

/// The bare elements of a container, as text: `category = { computing }`.
pub fn elements(container: &Container) -> Vec<String> {
    container
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Element(value) => scalar_text(value),
            _ => None,
        })
        .collect()
}

/// One file that survived selection, positioned in the global order.
struct Selected {
    contributor: String,
    file: SourceFile,
}

/// Apply exact-path shadowing and `replace_path`, then order globally by logical path.
///
/// The order is *one* order across both contributors. There is no "vanilla layer then mod
/// layer" step, because there is no layer: `r10` proved the winner is decided by where a
/// filename sorts, not by which source shipped it.
fn select(snapshots: &[(&str, &Snapshot)], stats: &mut Stats) -> Vec<Selected> {
    let excluded: Vec<String> = snapshots
        .iter()
        .flat_map(|(_, snapshot)| snapshot.replace_paths.iter().cloned())
        .collect();

    // Later contributors shadow earlier ones at an identical logical path. The Target Mod is
    // passed last, which is what makes its file replace the Vanilla file *entirely* — every
    // key the winner never mentions is gone, not inherited (`r6`).
    let mut winners: BTreeMap<String, Selected> = BTreeMap::new();
    for (id, snapshot) in snapshots {
        for file in &snapshot.script {
            let declarer_owns = snapshot
                .replace_paths
                .iter()
                .any(|prefix| file.logical.starts_with(&format!("{prefix}/")));
            let blocked = !declarer_owns
                && excluded
                    .iter()
                    .any(|prefix| file.logical.starts_with(&format!("{prefix}/")));
            if blocked {
                stats.files_excluded_by_replace_path += 1;
                continue;
            }

            let selected = Selected {
                contributor: (*id).to_owned(),
                file: file.clone(),
            };
            if winners.insert(file.logical.clone(), selected).is_some() {
                stats.files_shadowed_by_path += 1;
            }
        }
    }

    let selected: Vec<Selected> = winners.into_values().collect();
    stats.files_selected = selected.len();
    selected
}

pub fn resolve(case: &RevisionCase, snapshots: &BTreeMap<String, Snapshot>) -> std::io::Result<Resolved> {
    let mut stats = Stats::default();
    let mut issues = Vec::new();

    // Vanilla first, Target Mod second: the order in which an exact-path collision is
    // decided, not a precedence claim about definition keys.
    let ordered: Vec<(&str, &Snapshot)> = case
        .contributors()
        .iter()
        .filter_map(|contributor| {
            snapshots
                .get(&contributor.id)
                .map(|snapshot| (contributor.id.as_str(), snapshot))
        })
        .collect();

    let selected = select(&ordered, &mut stats);

    // BTreeMap iteration already yields normalized logical-path order, which is the global
    // enumeration order every script registry resolves in.
    let mut inline_scripts: BTreeMap<String, &Selected> = BTreeMap::new();
    let mut technology_files: Vec<&Selected> = Vec::new();
    for entry in &selected {
        if let Some(rest) = entry.file.logical.strip_prefix("common/inline_scripts/") {
            if let Some(name) = rest.strip_suffix(".txt") {
                inline_scripts.insert(name.to_owned(), entry);
            }
        } else if entry.file.logical.starts_with("common/technology/") {
            technology_files.push(entry);
        }
    }
    stats.technology_files = technology_files.len();

    let library = InlineLibrary::load(&inline_scripts)?;
    stats.inline_script_unresolved += library.unreadable.len();
    for path in &library.unreadable {
        issues.push(Issue {
            code: "inline_script_unreadable".into(),
            scope: IssueScope::Revision,
            impact: Impact::EvidenceAbsent,
            message: format!("inline script {path} could not be read"),
            source: None,
        });
    }

    // Last in enumeration order wins, so a later definition simply overwrites an earlier one
    // and the earlier one moves into provenance.
    let mut effective: BTreeMap<String, ResolvedDefinition> = BTreeMap::new();
    let mut sources: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for entry in &technology_files {
        let bytes = std::fs::read(&entry.file.absolute)?;
        let parsed = parser_spike::lexer::parse(&bytes);

        if !parsed.faults.is_empty() {
            stats.parse_faults += parsed.faults.len();
            // Registry completeness, not entry completeness. A file that could not be fully
            // read means the complete entry set for this registry cannot be established —
            // and says nothing about the entries that were read cleanly
            // (`docs/technical-design.md:336`).
            issues.push(Issue {
                code: "source_recovered_after_fault".into(),
                scope: IssueScope::Registry("technology".into()),
                impact: Impact::RegistryCompletenessUnknown,
                message: format!(
                    "{} has {} syntax fault(s); {} definition(s) were abandoned inside them",
                    entry.file.logical,
                    parsed.faults.len(),
                    parsed.faults.iter().map(|fault| fault.abandoned).sum::<u32>()
                ),
                source: Some(SourceRef {
                    contributor: entry.contributor.clone(),
                    logical: entry.file.logical.clone(),
                    ordinal: 0,
                    span_start: parsed.faults[0].offset,
                    span_end: parsed.faults[0].offset,
                }),
            });
        }

        let first_fault = parsed.faults.iter().map(|fault| fault.offset).min();
        for (ordinal, field) in parsed.definitions().enumerate() {
            let Value::Container(container) = &field.value else {
                continue;
            };
            let key = field.key.text().into_owned();
            let span = field.span.unwrap_or(parser_spike::model::Span::new(0, 0));
            stats.technology_definitions_seen += 1;

            let evidence = match first_fault {
                Some(offset) if span.start >= offset => {
                    stats.technology_definitions_recovered += 1;
                    Evidence::Recovered
                }
                _ => Evidence::Clean,
            };

            let source = SourceRef {
                contributor: entry.contributor.clone(),
                logical: entry.file.logical.clone(),
                ordinal: ordinal as u32,
                span_start: span.start,
                span_end: span.end,
            };

            let mut expanded = container.clone();
            library.expand(&mut expanded, &mut stats, &mut issues, &key);

            let previous = effective.insert(
                key.clone(),
                ResolvedDefinition {
                    key,
                    container: expanded,
                    evidence,
                    source,
                    shadowed: Vec::new(),
                },
            );
            sources
                .entry(entry.file.logical.clone())
                .or_insert_with(|| bytes.clone());

            if let Some(previous) = previous {
                // Whole-object replacement: an omitted field is absent, not inherited. The
                // omitted-`potential` oracle case is the proof, and it is why the previous
                // definition contributes provenance and nothing else.
                let current = effective.get_mut(&previous.key).expect("just inserted");
                current.shadowed = previous.shadowed;
                current.shadowed.push(previous.source);
            }
        }
    }

    let technologies: Vec<ResolvedDefinition> = effective.into_values().collect();
    stats.technology_definitions_effective = technologies.len();

    let icons = resolve_icons(&ordered, &technologies, &mut stats, &mut issues);
    let localization = ingest_localization(&ordered)?;

    issues.sort_by(|a, b| {
        (&a.scope, &a.code, &a.message).cmp(&(&b.scope, &b.code, &b.message))
    });

    Ok(Resolved {
        case: case.id.clone(),
        technologies,
        localization,
        icons,
        issues,
        stats,
        sources,
    })
}

/// The technology icon slot, by the path convention Stellaris content uses.
///
/// `gfx/interface/icons/technologies/<script identifier>.dds`, resolved by asking each
/// contributor for that one logical path with the Target Mod asked first — the same exact-path
/// rule script files follow, applied to one reference rather than to an enumeration.
///
/// Two things about this are deliberate.
///
/// It is a *convention*, not a declared reference: nothing in a technology definition names
/// its icon. Whether the game also accepts a sprite indirection for technologies is
/// unexercised here and is disclosed as a limitation rather than assumed either way.
///
/// It resolves one path per technology instead of enumerating every `.dds` in the corpus.
/// The DDS spike measured 33,145 texture files against a reachable set of 7,241, and its own
/// finding was that the first number is a fact about the filesystem. A build materializes the
/// icons its content references; enumerating the rest to answer a question about the
/// referenced ones would inflate every asset figure in this spike the same way.
fn resolve_icons(
    ordered: &[(&str, &Snapshot)],
    technologies: &[ResolvedDefinition],
    stats: &mut Stats,
    issues: &mut Vec<Issue>,
) -> BTreeMap<String, IconSlot> {
    // Reversed: the Target Mod is asked first, so its icon at an identical logical path
    // replaces the Vanilla one.
    let contributors: Vec<&(&str, &Snapshot)> = ordered.iter().rev().collect();

    let mut icons = BTreeMap::new();
    for technology in technologies {
        let logical = format!("gfx/interface/icons/technologies/{}.dds", technology.key);
        let found = contributors
            .iter()
            .find(|(_, snapshot)| snapshot.root.join(&logical).is_file());

        match found {
            Some((id, _)) => {
                stats.icons_resolved += 1;
                icons.insert(
                    technology.key.clone(),
                    IconSlot {
                        contributor: (*id).to_owned(),
                        logical,
                    },
                );
            }
            None => {
                stats.icons_missing += 1;
                // Entry-scoped and evidence-absent. The entry is documentable; one slot in it
                // is not, and `analysis::finalize` substitutes a deterministic placeholder
                // rather than a required Asset Store key.
                issues.push(Issue {
                    code: "icon_missing".into(),
                    scope: IssueScope::Entry(technology.key.clone()),
                    impact: Impact::EvidenceAbsent,
                    message: format!("no icon at {logical}"),
                    source: None,
                });
            }
        }
    }
    icons
}

fn ingest_localization(ordered: &[(&str, &Snapshot)]) -> std::io::Result<Localization> {
    // Localization files take part in exact-path collision exactly as script files do, and
    // that collision shadows the *whole* file: every key the winner omits renders as its raw
    // key rather than falling back to the shadowed vanilla value (`r14`). Selection therefore
    // happens before the stream is built, not inside it.
    let mut winners: BTreeMap<&str, (&str, ContributorKind, &SourceFile)> = BTreeMap::new();
    for (id, snapshot) in ordered {
        for file in &snapshot.localisation {
            winners.insert(file.logical.as_str(), (id, snapshot.kind, file));
        }
    }

    let stream: Vec<StreamFile<'_>> = winners
        .values()
        .map(|(id, kind, file)| StreamFile {
            file,
            phase: localization::phase_of(*kind, &file.logical),
            contributor: id,
        })
        .collect();

    localization::ingest(stream)
}

/// The inline-script library, parsed once and expanded into every consuming definition.
///
/// Inline scripts are textual expansion before registration, not registry entries: there is
/// no declared identifier to collide on, and same-path replacement is the only collision mode
/// (`r11`, `r12`). Expansion here is at the parsed-item level rather than over raw bytes.
/// That is a deliberate narrowing: it reproduces the substitution and the nesting, and it
/// does not reproduce the case where an expansion changes how the surrounding text *lexes*.
/// The narrowing is disclosed rather than assumed harmless.
struct InlineLibrary {
    scripts: BTreeMap<String, Vec<Item>>,
    unreadable: Vec<String>,
}

/// Expansion depth ceiling.
///
/// Inclusion nests — `r11` proved a script including another script works — so a cycle is
/// reachable. The game diagnoses an unresolved reference and registers the definition with
/// the inclusion silently omitted; this stops at a bound and records an issue, which is the
/// same outcome with a visible cause.
const MAX_INLINE_DEPTH: usize = 8;

impl InlineLibrary {
    fn load(sources: &BTreeMap<String, &Selected>) -> std::io::Result<Self> {
        let mut scripts = BTreeMap::new();
        let mut unreadable = Vec::new();
        for (name, entry) in sources {
            match std::fs::read(&entry.file.absolute) {
                Ok(bytes) => {
                    let parsed: ParsedFile = parser_spike::lexer::parse(&bytes);
                    scripts.insert(name.clone(), parsed.items);
                }
                Err(_) => unreadable.push(entry.file.logical.clone()),
            }
        }
        Ok(InlineLibrary { scripts, unreadable })
    }

    fn expand(
        &self,
        container: &mut Container,
        stats: &mut Stats,
        issues: &mut Vec<Issue>,
        entry_key: &str,
    ) {
        self.expand_items(&mut container.items, stats, issues, entry_key, 0, &mut BTreeSet::new());
    }

    fn expand_items(
        &self,
        items: &mut Vec<Item>,
        stats: &mut Stats,
        issues: &mut Vec<Issue>,
        entry_key: &str,
        depth: usize,
        active: &mut BTreeSet<String>,
    ) {
        let mut output: Vec<Item> = Vec::with_capacity(items.len());
        for item in std::mem::take(items) {
            match item {
                Item::Field(mut field) if field.key.text() == "inline_script" => {
                    let Some(request) = InlineRequest::read(&field.value) else {
                        stats.inline_script_unresolved += 1;
                        issues.push(unresolved_issue(entry_key, "malformed inline_script"));
                        continue;
                    };

                    if depth >= MAX_INLINE_DEPTH || active.contains(&request.script) {
                        stats.inline_script_unresolved += 1;
                        issues.push(unresolved_issue(
                            entry_key,
                            &format!("inline script {} recurses beyond the bound", request.script),
                        ));
                        continue;
                    }

                    let Some(body) = self.scripts.get(&request.script) else {
                        stats.inline_script_unresolved += 1;
                        issues.push(unresolved_issue(
                            entry_key,
                            &format!("inline script {} is referenced but absent", request.script),
                        ));
                        continue;
                    };

                    stats.inline_script_expansions += 1;
                    let mut expanded = body.clone();
                    substitute(&mut expanded, &request.arguments);

                    active.insert(request.script.clone());
                    self.expand_items(&mut expanded, stats, issues, entry_key, depth + 1, active);
                    active.remove(&request.script);

                    output.extend(expanded);
                    // `field` is consumed; touching it keeps the intent explicit that the
                    // request itself never survives into the definition.
                    field.value = Value::Container(Container::from_items(Vec::new(), None));
                }
                Item::Field(mut field) => {
                    if let Value::Container(container) = &mut field.value {
                        self.expand_items(
                            &mut container.items,
                            stats,
                            issues,
                            entry_key,
                            depth,
                            active,
                        );
                    }
                    output.push(Item::Field(field));
                }
                other => output.push(other),
            }
        }
        *items = output;
    }
}

fn unresolved_issue(entry_key: &str, message: &str) -> Issue {
    Issue {
        code: "inline_script_unresolved".into(),
        scope: IssueScope::Entry(entry_key.to_owned()),
        impact: Impact::EvidenceAbsent,
        message: message.to_owned(),
        source: None,
    }
}

struct InlineRequest {
    script: String,
    arguments: BTreeMap<String, Vec<u8>>,
}

impl InlineRequest {
    /// Both spellings the corpus uses: `inline_script = "path"` and
    /// `inline_script = { script = path PARAM = value }`.
    fn read(value: &Value) -> Option<Self> {
        match value {
            Value::Scalar(scalar) => Some(InlineRequest {
                script: scalar.text().into_owned(),
                arguments: BTreeMap::new(),
            }),
            Value::Container(container) => {
                let mut script = None;
                let mut arguments = BTreeMap::new();
                for field in container.fields() {
                    let key = field.key.text().into_owned();
                    let Value::Scalar(scalar) = &field.value else {
                        continue;
                    };
                    if key == "script" {
                        script = Some(scalar.text().into_owned());
                    } else {
                        arguments.insert(key, scalar.raw.clone());
                    }
                }
                script.map(|script| InlineRequest { script, arguments })
            }
            Value::Tagged { .. } => None,
        }
    }
}

/// Replace `$NAME$` in every scalar with the supplied argument.
///
/// Substitution reaches keys as well as values, because inline scripts use parameters in key
/// position — `$TECHNOLOGY$ = yes` is a real shape in the corpus. A substituter that only
/// touched values would expand half of each script and leave the other half stating a
/// placeholder as a literal identifier.
fn substitute(items: &mut [Item], arguments: &BTreeMap<String, Vec<u8>>) {
    if arguments.is_empty() {
        return;
    }
    for item in items {
        match item {
            Item::Field(field) => {
                substitute_scalar(&mut field.key, arguments);
                substitute_value(&mut field.value, arguments);
            }
            Item::Element(value) => substitute_value(value, arguments),
            Item::Conditional(conditional) => substitute(&mut conditional.items, arguments),
        }
    }
}

fn substitute_value(value: &mut Value, arguments: &BTreeMap<String, Vec<u8>>) {
    match value {
        Value::Scalar(scalar) => substitute_scalar(scalar, arguments),
        Value::Container(container) => substitute(&mut container.items, arguments),
        Value::Tagged { tag, container, .. } => {
            substitute_scalar(tag, arguments);
            substitute(&mut container.items, arguments);
        }
    }
}

fn substitute_scalar(scalar: &mut Scalar, arguments: &BTreeMap<String, Vec<u8>>) {
    if !scalar.raw.contains(&b'$') {
        return;
    }
    let mut text = String::from_utf8_lossy(&scalar.raw).into_owned();
    for (name, replacement) in arguments {
        let needle = format!("${name}$");
        if text.contains(&needle) {
            text = text.replace(&needle, &String::from_utf8_lossy(replacement));
        }
    }
    scalar.raw = text.into_bytes();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_one(text: &str) -> Container {
        let parsed = parser_spike::lexer::parse(text.as_bytes());
        let field = parsed
            .definitions()
            .next()
            .expect("fixture has one definition")
            .clone();
        match field.value {
            Value::Container(container) => container,
            _ => panic!("fixture definition is a container"),
        }
    }

    fn library(scripts: &[(&str, &str)]) -> InlineLibrary {
        InlineLibrary {
            scripts: scripts
                .iter()
                .map(|(name, body)| {
                    ((*name).to_owned(), parser_spike::lexer::parse(body.as_bytes()).items)
                })
                .collect(),
            unreadable: Vec::new(),
        }
    }

    /// Every `key = scalar` pair reachable in the container, depth first, as text.
    ///
    /// Structural rather than a `Debug` string. `Scalar::raw` is a byte vector, so a `Debug`
    /// rendering of a container prints `[116, 101, 99, …]` and an assertion that searches it
    /// for `"tech_alpha"` fails whether or not the substitution worked — a test that cannot
    /// pass tells you nothing about the code.
    fn pairs(container: &Container) -> Vec<(String, String)> {
        let mut found = Vec::new();
        for item in &container.items {
            match item {
                Item::Field(field) => {
                    let key = field.key.text().into_owned();
                    match &field.value {
                        Value::Scalar(scalar) => found.push((key, scalar.text().into_owned())),
                        Value::Container(inner) => found.extend(pairs(inner)),
                        Value::Tagged { container, .. } => found.extend(pairs(container)),
                    }
                }
                Item::Element(Value::Container(inner)) => found.extend(pairs(inner)),
                _ => {}
            }
        }
        found
    }

    #[test]
    fn an_inline_script_expands_in_place_with_its_parameters_substituted() {
        let library = library(&[("weights", "modifier = { factor = 2 has_technology = $TECH$ }")]);
        let mut container = parse_one(
            "t = { weight_modifier = { inline_script = { script = weights TECH = tech_alpha } } }",
        );

        let mut stats = Stats::default();
        let mut issues = Vec::new();
        library.expand(&mut container, &mut stats, &mut issues, "t");

        assert_eq!(stats.inline_script_expansions, 1);
        assert!(issues.is_empty(), "{issues:?}");

        let pairs = pairs(&container);
        assert!(pairs.contains(&("factor".into(), "2".into())), "{pairs:?}");
        assert!(
            pairs.contains(&("has_technology".into(), "tech_alpha".into())),
            "{pairs:?}"
        );
        assert!(
            !pairs.iter().any(|(key, _)| key == "inline_script"),
            "the request itself must not survive into the definition: {pairs:?}"
        );
    }

    #[test]
    fn a_missing_inline_script_is_an_issue_and_the_definition_still_resolves() {
        let library = library(&[]);
        let mut container = parse_one("t = { cost = 100 inline_script = \"absent/script\" }");

        let mut stats = Stats::default();
        let mut issues = Vec::new();
        library.expand(&mut container, &mut stats, &mut issues, "t");

        assert_eq!(stats.inline_script_unresolved, 1);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].impact, Impact::EvidenceAbsent);
        // The game registers the definition with the inclusion silently omitted (`r12`).
        // Documenting it is the same outcome with the cause made visible.
        assert_eq!(
            scalar_text(&field(&container, "cost").expect("cost survives").value),
            Some("100".into())
        );
    }

    #[test]
    fn a_self_including_script_stops_at_the_bound_rather_than_recursing() {
        let library = library(&[("loop", "inline_script = \"loop\"")]);
        let mut container = parse_one("t = { inline_script = \"loop\" }");

        let mut stats = Stats::default();
        let mut issues = Vec::new();
        library.expand(&mut container, &mut stats, &mut issues, "t");

        assert_eq!(stats.inline_script_unresolved, 1);
        assert!(issues[0].message.contains("recurses beyond the bound"));
    }
}
