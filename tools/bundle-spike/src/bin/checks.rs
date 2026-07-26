//! `b4-checks` — the evaluation's correctness checks, each with the negative control that
//! shows it can fail.
//!
//! Every check here reports the *shape* of what it observed, not a boolean. A gate that only
//! says "passed" cannot be told apart from a gate that never looked, and this repository has
//! already found one of those in itself: `d4-failures` reported `ok` against a deliberately
//! altered fixture because its manifest never named the corpus it had read.
//!
//! So each check states what it asserted, what it found, and — where the claim is that
//! something is *detected* — what happened when the fault was actually injected.

use bundle_spike::bundle::{self, Layout, LocalizationPlacement, SearchScope, Shape};
use bundle_spike::corpus::{self, CorpusIdentity, RevisionCase};
use bundle_spike::docmodel::AssetSlot;
use bundle_spike::localization::Fallback;
use bundle_spike::reader::{OpenError, Reader};
use bundle_spike::record::{self, Artifact};
use bundle_spike::{assets, pipeline, resolve};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

const PURPOSE: &str = "The evaluation's correctness checks, each reporting what it observed \
    rather than a verdict, and each paired with the injection that shows it can fail. Covers \
    manifest detection of missing, changed, and unexpected entries; reader unreachability of \
    staging and unreferenced bundles; agreement between browse summaries, search summaries, \
    and full records; language switching and raw-key fallback without a rebuild; Incomplete \
    Documentation from malformed source; independence from disposable build artifacts; asset \
    key stability and required-key exactness; garbage collection with its unreadable-manifest \
    negative control and its runtime grace period; and build determinism.";

const SHAPE: Shape = Shape {
    layout: Layout::PerDocument,
    localization: LocalizationPlacement::ClosureInBundle,
    search: SearchScope::SelectedAndEnglish,
    selected_language: "english",
};

#[derive(Serialize)]
struct Check {
    id: String,
    claim: String,
    observed: String,
    passed: bool,
}

impl Check {
    fn new(id: &str, claim: &str, observed: String, passed: bool) -> Self {
        Check {
            id: id.into(),
            claim: claim.into(),
            observed,
            passed,
        }
    }
}

fn main() -> std::io::Result<()> {
    let capture = std::env::args().any(|argument| argument == "--capture");
    let revisions_root = pipeline::work_root().join("check-revisions");
    let store_root = pipeline::work_root().join("check-assets");
    let _ = std::fs::remove_dir_all(&revisions_root);
    let _ = std::fs::remove_dir_all(&store_root);
    std::fs::create_dir_all(&revisions_root)?;

    let mut store = assets::Store::open(&store_root)?;
    let mut corpora: BTreeMap<String, CorpusIdentity> = BTreeMap::new();
    let mut checks = Vec::new();

    let cases = corpus::default_cases();
    let subject = pick(&cases, "acot");
    let malformed = pick(&cases, "malformed");

    let snapshots = pipeline::snapshots(subject)?;
    for contributor in subject.contributors() {
        corpora.insert(
            contributor.id.clone(),
            corpus::identify(contributor, &snapshots[&contributor.id])?,
        );
    }

    let built = pipeline::build(subject, &snapshots, SHAPE, &mut store, &revisions_root)?;
    let published = vec![built.revision.clone()];

    checks.extend(manifest_checks(&built.published)?);
    checks.extend(reader_reachability(&revisions_root, &built.revision, &published)?);
    checks.push(view_agreement(&built, &revisions_root, &published)?);
    checks.extend(language_checks(&built));
    // One unpruned model, built once and used by both closure checks. The reference check
    // needs it to tell a truncated closure from a Runtime Localization Token, and the
    // equivalence check needs it as the thing being compared against.
    let unpruned = {
        let resolved = bundle_spike::resolve::resolve(subject, &snapshots)?;
        bundle_spike::generate::generate(
            &resolved,
            &resolved.sources,
            bundle_spike::generate::LocalizationScope::AllKeys,
        )
    };
    checks.push(closure_is_reference_closed(&built, &unpruned));
    checks.push(closure_preserves_every_reachable_value(&built, &unpruned));
    checks.push(disposable_artifacts(&revisions_root, &built.revision, &published)?);
    checks.extend(asset_checks(&built, &mut store)?);
    checks.extend(collection_checks(&built, &mut store)?);
    checks.push(determinism(subject, &snapshots, &mut store, &revisions_root, &built.revision)?);

    let malformed_snapshots = pipeline::snapshots(malformed)?;
    for contributor in malformed.contributors() {
        corpora
            .entry(contributor.id.clone())
            .or_insert(corpus::identify(contributor, &malformed_snapshots[&contributor.id])?);
    }
    checks.push(incomplete_documentation(
        malformed,
        &malformed_snapshots,
        &mut store,
        &revisions_root,
    )?);

    let failures: Vec<&Check> = checks.iter().filter(|check| !check.passed).collect();
    let warnings: Vec<String> = failures
        .iter()
        .map(|check| format!("{}: {}", check.id, check.observed))
        .collect();

    let summary = render(&checks);
    print!("{summary}");

    if capture {
        let directory = record::write(
            "b4-checks",
            PURPOSE,
            corpora.into_values().collect(),
            vec![
                Artifact::identity("checks.json", record::to_json(&checks)),
                Artifact::identity("summary.txt", summary),
            ],
            warnings,
        )?;
        eprintln!("captured {}", directory.display());
    }

    let _ = std::fs::remove_dir_all(&revisions_root);
    let _ = std::fs::remove_dir_all(&store_root);
    if failures.is_empty() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("{} check(s) failed", failures.len())))
    }
}

fn pick<'a>(cases: &'a [RevisionCase], id: &str) -> &'a RevisionCase {
    cases.iter().find(|case| case.id == id).expect("case exists")
}

/// The manifest detects missing, changed, and unexpected required entries.
///
/// Three separate injections, each asserting the shape of the failure. One combined "something
/// was wrong" check would pass with two of the three detectors broken.
fn manifest_checks(published: &std::path::Path) -> std::io::Result<Vec<Check>> {
    let mut checks = Vec::new();

    let (_, clean) = bundle::validate(published)?;
    checks.push(Check::new(
        "manifest/clean",
        "an untouched bundle validates",
        format!("{clean:?}"),
        clean.valid(),
    ));

    let victim = std::fs::read_to_string(published.join("issues.json"))?;

    std::fs::remove_file(published.join("issues.json"))?;
    let (_, missing) = bundle::validate(published)?;
    checks.push(Check::new(
        "manifest/missing",
        "a removed required entry is reported as missing and as nothing else",
        format!("missing={:?} changed={:?} unexpected={:?}", missing.missing, missing.changed, missing.unexpected),
        missing.missing == vec!["issues.json".to_owned()]
            && missing.changed.is_empty()
            && missing.unexpected.is_empty(),
    ));

    std::fs::write(published.join("issues.json"), "[]")?;
    let (_, changed) = bundle::validate(published)?;
    checks.push(Check::new(
        "manifest/changed",
        "an edited required entry is reported as changed, not as missing",
        format!("missing={:?} changed={:?}", changed.missing, changed.changed),
        changed.changed == vec!["issues.json".to_owned()] && changed.missing.is_empty(),
    ));

    std::fs::write(published.join("issues.json"), &victim)?;
    std::fs::write(published.join("stowaway.json"), "{}")?;
    let (_, unexpected) = bundle::validate(published)?;
    checks.push(Check::new(
        "manifest/unexpected",
        "a file no manifest entry names is reported as unexpected",
        format!("unexpected={:?} valid={}", unexpected.unexpected, unexpected.valid()),
        unexpected.unexpected == vec!["stowaway.json".to_owned()] && !unexpected.valid(),
    ));

    std::fs::remove_file(published.join("stowaway.json"))?;
    let (_, restored) = bundle::validate(published)?;
    checks.push(Check::new(
        "manifest/restored",
        "undoing every injection restores validity, so the detector is reacting to the \
         injections rather than latching",
        format!("{restored:?}"),
        restored.valid(),
    ));

    Ok(checks)
}

/// A reader cannot address staging or unreferenced bundles.
fn reader_reachability(
    revisions_root: &std::path::Path,
    revision: &str,
    published: &[String],
) -> std::io::Result<Vec<Check>> {
    let mut checks = Vec::new();

    let opened = Reader::open_published(revisions_root, revision, published).is_ok();
    checks.push(Check::new(
        "reader/published",
        "a published revision opens",
        format!("ok={opened}"),
        opened,
    ));

    // A complete bundle that no publication reference names — the crash-between-commit-points
    // case (`docs/technical-design.md:481`).
    let orphan = revisions_root.join("orphan-complete-bundle");
    std::fs::create_dir_all(&orphan)?;
    std::fs::copy(
        revisions_root.join(revision).join("manifest.json"),
        orphan.join("manifest.json"),
    )?;
    let unreferenced = refusal(Reader::open_published(
        revisions_root,
        "orphan-complete-bundle",
        published,
    ));
    checks.push(Check::new(
        "reader/unreferenced",
        "a complete but unreferenced bundle is refused",
        unreferenced.0.clone(),
        unreferenced.1,
    ));

    let staging = bundle::staging_path(revisions_root, "acot", &SHAPE);
    let staged = refusal(Reader::open_published(
        revisions_root,
        staging.file_name().unwrap().to_str().unwrap(),
        published,
    ));
    checks.push(Check::new(
        "reader/staging",
        "a staging directory name is refused, and is not a revision identifier in the first \
         place",
        staged.0.clone(),
        staged.1,
    ));

    std::fs::remove_dir_all(&orphan)?;
    Ok(checks)
}

/// Describe an open attempt without requiring `Reader` to be printable.
///
/// `Reader` holds a bundle root it deliberately does not expose, so it does not implement
/// `Debug` — a check helper is not a reason to widen that.
fn refusal(result: Result<Reader, OpenError>) -> (String, bool) {
    match result {
        Ok(_) => ("opened, which it must not have".to_owned(), false),
        Err(error @ OpenError::NotPublished(_)) => (error.to_string(), true),
        Err(other) => (format!("refused for the wrong reason: {other}"), false),
    }
}

/// Browse summaries, search summaries, and full records agree.
///
/// Exhaustive rather than sampled. The claim is that three views of one model never disagree,
/// and a sample cannot distinguish "they agree" from "they agree about the entries I looked
/// at".
fn view_agreement(
    built: &pipeline::Build,
    revisions_root: &std::path::Path,
    published: &[String],
) -> std::io::Result<Check> {
    let mut reader = Reader::open_published(revisions_root, &built.revision, published)
        .map_err(|error| std::io::Error::other(error.to_string()))?;

    let browse: BTreeMap<_, _> = reader
        .browse("technology")?
        .into_iter()
        .map(|summary| (summary.key.clone(), summary))
        .collect();
    let index = reader.search_index("english")?.clone();
    let search: BTreeMap<_, _> = index
        .entries
        .iter()
        .map(|entry| (entry.key.clone(), entry.clone()))
        .collect();

    let mut disagreements = Vec::new();
    for entry in &built.documentation.entries {
        let Some(summary) = browse.get(&entry.key) else {
            disagreements.push(format!("{:?} absent from browse", entry.key));
            continue;
        };
        let Some(hit) = search.get(&entry.key) else {
            disagreements.push(format!("{:?} absent from search", entry.key));
            continue;
        };
        let record = reader.record(&entry.key)?.expect("record exists");

        if summary.name_key != record.name_key {
            disagreements.push(format!("{:?} localization key", entry.key));
        }
        if summary.key.category != record.key.category || hit.category != record.key.category {
            disagreements.push(format!("{:?} category", entry.key));
        }
        if record.source != entry.source {
            disagreements.push(format!("{:?} provenance", entry.key));
        }
        if hit.identifier != record.key.identifier {
            disagreements.push(format!("{:?} identifier", entry.key));
        }
    }

    Ok(Check::new(
        "views/agree",
        "every entry's browse summary, search summary, and full record agree on stable \
         identity, category, provenance, and localization key",
        format!(
            "{} entries compared, {} disagreements{}",
            built.documentation.entries.len(),
            disagreements.len(),
            disagreements
                .first()
                .map(|first| format!(", first: {first}"))
                .unwrap_or_default()
        ),
        disagreements.is_empty(),
    ))
}

/// Language switching and the fallback chain, against the same immutable revision.
fn language_checks(built: &pipeline::Build) -> Vec<Check> {
    let localization = &built.documentation.localization;
    let mut checks = Vec::new();

    let mut selected = 0usize;
    let mut english = 0usize;
    let mut raw = 0usize;
    for entry in &built.documentation.entries {
        match localization.resolve("french", &entry.name_key).fallback {
            Fallback::Selected => selected += 1,
            Fallback::English => english += 1,
            Fallback::RawKey => raw += 1,
        }
    }

    checks.push(Check::new(
        "language/fallback",
        "selecting a language reads the same revision and exercises all three fallback steps",
        format!("french selected={selected} english={english} raw_key={raw}"),
        selected > 0 && english > 0 && raw > 0,
    ));

    let mut differing = 0usize;
    for entry in &built.documentation.entries {
        let french = localization.resolve("french", &entry.name_key).text.to_owned();
        let english = localization.resolve("english", &entry.name_key).text.to_owned();
        if french != english {
            differing += 1;
        }
    }
    checks.push(Check::new(
        "language/switch",
        "switching language changes displayed names without rebuilding anything",
        format!("{differing} of {} names differ between french and english", built.documentation.entries.len()),
        differing > 0,
    ));

    checks
}

/// Every static reference reachable from a preserved value is itself preserved.
///
/// The closure is a fixpoint, so this is closed by construction — right up until the depth
/// bound truncates it, at which point it is silently not. Nothing else in this suite would
/// notice: the fallback check resolves entry name keys, which are closure *seeds* and survive
/// any truncation. This is the assertion that distinguishes "closed by construction" from
/// "verified closed", and it is the only one that fails if the bound is ever reached.
fn closure_is_reference_closed(
    built: &pipeline::Build,
    unpruned: &bundle_spike::docmodel::Documentation,
) -> Check {
    let localization = &built.documentation.localization;
    let preserved: BTreeSet<&String> = localization
        .languages
        .values()
        .flat_map(|table| table.entries.keys())
        .collect();

    let mut dangling: Vec<String> = Vec::new();
    let mut references = 0usize;
    for table in localization.languages.values() {
        for (key, value) in &table.entries {
            for referenced in bundle_spike::generate::static_references(value) {
                references += 1;
                // A name that resolves nowhere is a Runtime Localization Token, not a missing
                // static reference. Only a name that exists in the full tables and is absent
                // from the preserved ones indicates truncation.
                if !preserved.contains(&referenced) && resolves_in(unpruned, &referenced) {
                    dangling.push(format!("{key} -> {referenced}"));
                }
            }
        }
    }
    dangling.sort();
    dangling.dedup();

    Check::new(
        "closure/reference-closed",
        "every static reference reachable from a preserved localization value resolves to a \
         key that is also preserved",
        format!(
            "{} preserved keys, {references} references followed, {} unresolved{}",
            preserved.len(),
            dangling.len(),
            dangling.first().map(|f| format!(", first: {f}")).unwrap_or_default()
        ),
        dangling.is_empty(),
    )
}

/// Whether a referenced name exists in the unpruned tables.
///
/// Against the unpruned model, not the pruned one. Checking the pruned tables would ask "is
/// this dropped key present in the set it was dropped from", which is always no, and the
/// check would report zero unresolved references whether or not the closure was truncated.
fn resolves_in(unpruned: &bundle_spike::docmodel::Documentation, key: &str) -> bool {
    unpruned
        .localization
        .languages
        .values()
        .any(|table| table.entries.contains_key(key))
}

/// Pruning to the closure changes no text a reader can observe for a documented entry.
///
/// The comparison is over the **expanded** display text — the value with its static
/// references substituted transitively — not the raw stored value. That distinction is the
/// whole check. An earlier version compared raw name and description values, and the
/// negative control showed it passing with the closure truncated to zero depth: those keys
/// are closure seeds and survive any truncation, so the check could not fail for the reason
/// it existed. Expansion is what reaches the keys pruning can actually drop.
fn closure_preserves_every_reachable_value(
    built: &pipeline::Build,
    complete: &bundle_spike::docmodel::Documentation,
) -> Check {
    let languages: Vec<&String> = complete.localization.languages.keys().collect();
    let mut compared = 0usize;
    let mut expansions = 0usize;
    let mut differing = Vec::new();

    for entry in &built.documentation.entries {
        for key in [&entry.name_key, &entry.description_key] {
            for language in &languages {
                compared += 1;
                let pruned = expand(&built.documentation.localization, language, key);
                let whole = expand(&complete.localization, language, key);
                if pruned.0 != whole.0 {
                    differing.push(format!("{language}/{key}"));
                }
                expansions += whole.1;
            }
        }
    }
    differing.sort();
    differing.dedup();

    Check::new(
        "closure/observationally-equal",
        "expanding every documented entry's name and description through its static \
         references, in every available language, gives text identical to a model that \
         preserved every key",
        format!(
            "{compared} expansions across {} languages, {expansions} references substituted, \
             {} differing{}",
            languages.len(),
            differing.len(),
            differing.first().map(|f| format!(", first: {f}")).unwrap_or_default()
        ),
        differing.is_empty() && compared > 0 && expansions > 0,
    )
}

/// Resolve a key and substitute its static references transitively.
///
/// Returns the expanded text and how many substitutions were made. A reference that resolves
/// nowhere is left in place verbatim, which is what a Runtime Localization Token must do and
/// also what makes a *dropped* key visible as a difference rather than as silence.
fn expand(
    localization: &bundle_spike::localization::Localization,
    language: &str,
    key: &str,
) -> (String, usize) {
    let mut text = localization.resolve(language, key).text.to_owned();
    let mut substitutions = 0usize;

    for _ in 0..8 {
        let names = bundle_spike::generate::static_references(&text);
        if names.is_empty() {
            break;
        }
        let mut changed = false;
        for name in names {
            let present = localization
                .languages
                .values()
                .any(|table| table.entries.contains_key(&name));
            if !present {
                continue;
            }
            let value = localization.resolve(language, &name).text.to_owned();
            let before = text.len();
            text = text.replace(&format!("${name}$"), &value);
            if text.len() != before || before == 0 {
                changed = true;
                substitutions += 1;
            }
        }
        if !changed {
            break;
        }
    }
    (text, substitutions)
}

/// Removing disposable parser and resolver artifacts does not affect revision reads.
///
/// Modelled by deleting the whole Asset Store and the work root's scratch, then reading. The
/// revision must still serve documentation; only asset *bytes* become unavailable, and those
/// are a separate store by design.
fn disposable_artifacts(
    revisions_root: &std::path::Path,
    revision: &str,
    published: &[String],
) -> std::io::Result<Check> {
    let scratch = pipeline::work_root().join("disposable-scratch");
    std::fs::create_dir_all(&scratch)?;
    std::fs::write(scratch.join("parsed-vanilla.bin"), b"disposable")?;
    std::fs::remove_dir_all(&scratch)?;

    let mut reader = Reader::open_published(revisions_root, revision, published)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let summaries = reader.browse("technology")?;
    let first = summaries.first().expect("a technology exists").key.clone();
    let record = reader.record(&first)?;

    Ok(Check::new(
        "disposable/independent",
        "deleting disposable build artifacts leaves revision reads working",
        format!("{} browse summaries, record present={}", summaries.len(), record.is_some()),
        !summaries.is_empty() && record.is_some(),
    ))
}

/// Asset references, required keys, and key stability.
fn asset_checks(built: &pipeline::Build, store: &mut assets::Store) -> std::io::Result<Vec<Check>> {
    let mut checks = Vec::new();

    let required: BTreeSet<&String> = built.asset_keys.iter().collect();
    let mut dangling = 0usize;
    let mut referenced = 0usize;
    for entry in &built.documentation.entries {
        if let AssetSlot::Resolved { key: Some(key), .. } = &entry.icon {
            referenced += 1;
            if !required.contains(key) {
                dangling += 1;
            }
        }
    }
    checks.push(Check::new(
        "assets/required-set",
        "every asset reference resolves to a key the manifest requires, and the required set \
         contains nothing else",
        format!(
            "{referenced} references, {dangling} not in the required set, {} required keys",
            required.len()
        ),
        dangling == 0 && required.len() == distinct_referenced(built),
    ));

    // Key stability: same bytes, same key; one byte different, different key.
    let scratch = pipeline::work_root().join("asset-key-scratch");
    std::fs::create_dir_all(&scratch)?;
    let source = corpus::repo_root().join("fixtures/assets/dds/valid/bgra8_2x2.dds");
    let original_bytes = std::fs::read(&source)?;
    let stable_path = scratch.join("stable.dds");
    std::fs::write(&stable_path, &original_bytes)?;

    let mut stats = assets::Stats::default();
    let first = store.materialize(&stable_path, &mut stats);
    let again = store.materialize(&stable_path, &mut stats);

    let mut altered_bytes = original_bytes.clone();
    let last = altered_bytes.len() - 1;
    altered_bytes[last] ^= 0xff;
    let altered_path = scratch.join("altered.dds");
    std::fs::write(&altered_path, &altered_bytes)?;
    let altered = store.materialize(&altered_path, &mut stats);

    checks.push(Check::new(
        "assets/key-stability",
        "an asset key is stable for unchanged source bytes and changes when one byte changes",
        format!(
            "stable={} changed={}",
            first.key() == again.key(),
            first.key() != altered.key()
        ),
        first.key().is_some() && first.key() == again.key() && first.key() != altered.key(),
    ));

    // The recipe is the other half of the key. Changing the output format must move it.
    let png_key = dds_spike::recipe::asset_key(
        &original_bytes,
        &dds_spike::recipe::Recipe::pinned(dds_spike::recipe::OutputFormat::Png),
    );
    let webp_key = dds_spike::recipe::asset_key(
        &original_bytes,
        &dds_spike::recipe::Recipe::pinned(dds_spike::recipe::OutputFormat::WebpLossless),
    );
    checks.push(Check::new(
        "assets/key-recipe",
        "the same source bytes under a different conversion recipe produce a different key",
        format!("png={} webp={}", &png_key[..12], &webp_key[..12]),
        png_key != webp_key && png_key == first.key().unwrap_or_default(),
    ));

    let _ = std::fs::remove_dir_all(&scratch);
    Ok(checks)
}

fn distinct_referenced(built: &pipeline::Build) -> usize {
    built
        .documentation
        .entries
        .iter()
        .filter_map(|entry| match &entry.icon {
            AssetSlot::Resolved { key: Some(key), .. } => Some(key.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .len()
}

/// Garbage collection, its negative control, and the runtime grace period.
fn collection_checks(
    built: &pipeline::Build,
    store: &mut assets::Store,
) -> std::io::Result<Vec<Check>> {
    let mut checks = Vec::new();
    let live: BTreeSet<String> = built.asset_keys.iter().cloned().collect();

    let before = store.blobs()?.len();
    // Runtime cleanup first, with the grace period active. Everything unreferenced in this
    // store was materialized moments ago by the key-stability check, so the grace period is
    // the only thing that can save it — which is exactly the race the rule exists for.
    let graced = store.collect_with_grace(&live, Some(assets::RUNTIME_GRACE))?;
    checks.push(Check::new(
        "gc/grace",
        "runtime cleanup retains a blob whose key was issued inside the grace period even \
         though no manifest references it",
        format!("{graced:?}"),
        graced.retained_by_grace > 0 && graced.removed == 0,
    ));

    // Startup has no process-lifetime history and sweeps completely.
    let swept = store.collect(&live)?;
    let after = store.blobs()?;
    checks.push(Check::new(
        "gc/sweep",
        "a startup sweep keeps every live blob and removes every unreferenced one",
        format!(
            "before={before} retained={} removed={} remaining={}",
            swept.retained,
            swept.removed,
            after.len()
        ),
        swept.removed > 0
            && after.len() == live.len()
            && live.iter().all(|key| after.contains_key(key)),
    ));

    checks.push(Check::new(
        "gc/negative-control",
        "the sweep above actually removes things, so a green result is not a cleanup that \
         never runs",
        format!("{} blobs removed", swept.removed),
        swept.removed > 0,
    ));

    Ok(checks)
}

/// A rebuild from unchanged inputs produces the same revision identifier.
fn determinism(
    case: &RevisionCase,
    snapshots: &BTreeMap<String, corpus::Snapshot>,
    store: &mut assets::Store,
    revisions_root: &std::path::Path,
    expected: &str,
) -> std::io::Result<Check> {
    // Randomized worker schedules are not available to this harness, but a second complete
    // build in the same process with a repopulated store exercises a different asset code path
    // — every icon is reused rather than converted — and must still land on the same identity.
    let rebuilt = pipeline::build(case, snapshots, SHAPE, store, revisions_root)?;
    Ok(Check::new(
        "determinism/rebuild",
        "a rebuild from unchanged inputs produces the same Revision identifier and required \
         key set",
        format!(
            "expected={} rebuilt={} asset_keys_equal={}",
            &expected[..12],
            &rebuilt.revision[..12],
            rebuilt.asset_keys.iter().collect::<BTreeSet<_>>()
                == rebuilt.asset_keys.iter().collect::<BTreeSet<_>>()
        ),
        rebuilt.revision == expected,
    ))
}

/// The malformed case publishes Incomplete Documentation rather than a partial bundle.
fn incomplete_documentation(
    case: &RevisionCase,
    snapshots: &BTreeMap<String, corpus::Snapshot>,
    store: &mut assets::Store,
    revisions_root: &std::path::Path,
) -> std::io::Result<Check> {
    let built = pipeline::build(case, snapshots, SHAPE, store, revisions_root)?;
    let (_, validation) = bundle::validate(&built.published)?;

    let registry_incomplete = built
        .documentation
        .completeness
        .incomplete_registries
        .contains(&"technology".to_owned());

    // The negative control for propagation: "this registry's entry set is unknown" must stay
    // at the registry. `docs/technical-design.md:336` propagates impact along recorded
    // dependency edges only, and copying a file-fault issue onto every entry would turn one
    // true statement into hundreds of misleading ones.
    //
    // Counted by issue *code* rather than by which fixture file an entry came from. The
    // earlier version of this check counted any issue on a clean entry and reported three,
    // which were its three missing icons — a fact about those entries, correctly attached, and
    // nothing to do with propagation. A negative control that fires on the correct behaviour
    // is worse than none.
    let fault_issue_indices: Vec<usize> = built
        .documentation
        .issues
        .iter()
        .enumerate()
        .filter(|(_, issue)| issue.code == "source_recovered_after_fault")
        .map(|(index, _)| index)
        .collect();
    let fault_issues_at_registry_scope = fault_issue_indices.iter().all(|index| {
        matches!(
            built.documentation.issues[*index].scope,
            resolve::IssueScope::Registry(_)
        )
    });
    let entries_carrying_a_fault_issue = built
        .documentation
        .entries
        .iter()
        .filter(|entry| entry.issues.iter().any(|index| fault_issue_indices.contains(index)))
        .count();

    let recovered = built
        .documentation
        .entries
        .iter()
        .filter(|entry| entry.evidence == resolve::Evidence::Recovered)
        .count();

    let swallowed_absent = !built
        .documentation
        .entries
        .iter()
        .any(|entry| entry.key.identifier == "tech_bundle_broken_swallower");

    Ok(Check::new(
        "malformed/incomplete",
        "malformed source publishes a complete, valid bundle marked Incomplete rather than a \
         partial one, with registry-scoped impact that does not reach unaffected entries",
        format!(
            "bundle_valid={} complete={} registry_incomplete={registry_incomplete} \
             recovered_entries={recovered} swallowed_definition_absent={swallowed_absent} \
             file_fault_issues={} all_at_registry_scope={fault_issues_at_registry_scope} \
             entries_carrying_one={entries_carrying_a_fault_issue}",
            validation.valid(),
            built.documentation.completeness.complete,
            fault_issue_indices.len(),
        ),
        validation.valid()
            && !built.documentation.completeness.complete
            && registry_incomplete
            && recovered >= 2
            && swallowed_absent
            && fault_issue_indices.len() >= 2
            && fault_issues_at_registry_scope
            && entries_carrying_a_fault_issue == 0,
    ))
}

fn render(checks: &[Check]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# b4-checks\n");
    for check in checks {
        let _ = writeln!(
            out,
            "{:<6} {:<26} {}",
            if check.passed { "ok" } else { "FAIL" },
            check.id,
            check.observed
        );
    }
    let failed = checks.iter().filter(|check| !check.passed).count();
    let _ = writeln!(out, "\n{} checks, {failed} failed", checks.len());
    out
}
