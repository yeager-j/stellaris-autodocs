//! `b2-shape` — how large is a revision bundle, in what, and how much of it repeats?
//!
//! Sizes and counts only. Nothing here is a latency, so the whole record is byte-comparable
//! and the drift gate treats it exactly as it treats a parser or DDS record.

use bundle_spike::bundle::{Layout, LocalizationPlacement, SearchScope, Shape};
use bundle_spike::corpus::{self, CorpusIdentity};
use bundle_spike::docmodel::Documentation;
use bundle_spike::record::{self, Artifact};
use bundle_spike::generate::LocalizationScope;
use bundle_spike::{generate, locstore, pipeline, resolve, timing};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;

const PURPOSE: &str = "Bytes and file count for every candidate bundle layout, against the \
    canonical unsharded read-model payload declared as the denominator before capture. Reports \
    what each artifact class contributes, what the materialized views duplicate, and how much \
    of the cross-revision localization total a content-addressed store would recover. Sizes \
    and counts only: no figure here is a wall-clock reading, so the whole record is \
    byte-comparable.";

#[derive(Serialize)]
struct ShapeRow {
    case: String,
    shape: String,
    file_count: usize,
    total_bytes: u64,
    bytes_excluding_localization: u64,
    bytes_by_class: BTreeMap<String, u64>,
    ratio_to_unsharded: f64,
    meets_file_budget: bool,
    meets_size_budget: bool,
}

#[derive(Serialize)]
struct CaseRow {
    case: String,
    entries: usize,
    languages: usize,
    localization_keys: usize,
    /// The declared 2.0x denominator, complete.
    unsharded_bytes: u64,
    /// The same payload with preserved localization removed, which is the denominator the
    /// budget compares against when localization lives in a shared store.
    unsharded_bytes_excluding_localization: u64,
    /// The same payload carrying only the cited-key closure.
    unsharded_bytes_closure: u64,
    /// Keys the documentation cites, before the static-reference closure is taken.
    cited_seed_keys: usize,
    /// Keys after the closure, across every language.
    cited_closure_keys: usize,
    /// Bytes of the closure across every language.
    closure_localization_bytes: u64,
    /// Bytes of every key across every language.
    full_localization_bytes: u64,
    browse_bytes: u64,
    search_bytes: u64,
    /// One language's share, so a size result can name per-language search material as its
    /// cause rather than leaving a reader to divide.
    search_bytes_english: u64,
    record_bytes: u64,
    issue_bytes: u64,
    /// Bytes the browse summaries restate from the full records.
    browse_duplication_bytes: u64,
    /// Bytes the search material restates from preserved localization.
    search_duplication_bytes: u64,
    asset_keys: usize,
    placeholder_icons: usize,
}

fn main() -> std::io::Result<()> {
    let capture = std::env::args().any(|argument| argument == "--capture");
    let revisions_root = pipeline::work_root().join("shape-revisions");
    let _ = std::fs::remove_dir_all(&revisions_root);
    std::fs::create_dir_all(&revisions_root)?;

    let all = SearchScope::AllLanguages;
    let pair = SearchScope::SelectedAndEnglish;
    let closure = LocalizationPlacement::ClosureInBundle;
    let every = LocalizationPlacement::AllKeysInBundle;
    let shared = LocalizationPlacement::AllKeysSharedStore;
    let en = "english";

    // The three localization arms, then the two layouts and two search scopes that the
    // earlier capture showed to be the only ones that move a budget. Sharding is retained at
    // one configuration as the control it now is, rather than at four.
    let shapes = [
        Shape { layout: Layout::PerDocument, localization: closure, search: pair, selected_language: en },
        Shape { layout: Layout::PerDocument, localization: closure, search: pair, selected_language: "french" },
        Shape { layout: Layout::PerDocument, localization: closure, search: all, selected_language: en },
        Shape { layout: Layout::ByCategory, localization: closure, search: pair, selected_language: en },
        Shape { layout: Layout::PerDocument, localization: every, search: all, selected_language: en },
        Shape { layout: Layout::PerDocument, localization: shared, search: all, selected_language: en },
        Shape { layout: Layout::PerDocument, localization: shared, search: pair, selected_language: en },
    ];

    let mut corpora: BTreeMap<String, CorpusIdentity> = BTreeMap::new();
    let mut case_rows = Vec::new();
    let mut shape_rows = Vec::new();
    let mut whole = BTreeMap::new();
    let mut content = BTreeMap::new();
    let mut warnings = Vec::new();

    for case in corpus::default_cases() {
        eprintln!("shape: {}", case.id);
        let snapshots = pipeline::snapshots(&case)?;
        for contributor in case.contributors() {
            if !corpora.contains_key(&contributor.id) {
                let snapshot = &snapshots[&contributor.id];
                corpora.insert(
                    contributor.id.clone(),
                    corpus::identify(contributor, snapshot)?,
                );
            }
        }

        let resolved = resolve::resolve(&case, &snapshots)?;
        // Two models per case, differing only in how much localization they preserve. Each
        // arm's views are still derived from one model — the one whose localization scope it
        // is measuring — so a comparison between arms never compares two generators.
        let every_key =
            generate::generate(&resolved, &resolved.sources, LocalizationScope::AllKeys);
        let closure =
            generate::generate(&resolved, &resolved.sources, LocalizationScope::CitedClosure);

        whole.insert(case.id.clone(), locstore::whole_language(&every_key.localization));
        content.insert(
            case.id.clone(),
            locstore::content_defined(&every_key.localization),
        );

        let row = measure_case(&case.id, &every_key, &closure);
        for shape in shapes {
            let model = if shape.localization == LocalizationPlacement::ClosureInBundle {
                &closure
            } else {
                &every_key
            };
            let written = bundle_spike::bundle::write(
                model,
                shape,
                &revisions_root,
                &[],
                &[],
            )?;

            // The denominator matches the arm: an arm that carries localization is compared
            // against a payload that contains it, and an arm that does not is compared against
            // one that does not. Mixing them would credit or charge an arm for bytes that are
            // not on the same side of the boundary the budget describes.
            let (numerator, denominator) = match shape.localization {
                LocalizationPlacement::ClosureInBundle => {
                    (written.total_bytes, row.unsharded_bytes_closure)
                }
                LocalizationPlacement::AllKeysInBundle => {
                    (written.total_bytes, row.unsharded_bytes)
                }
                LocalizationPlacement::AllKeysSharedStore => (
                    written.bytes_excluding_localization,
                    row.unsharded_bytes_excluding_localization,
                ),
            };
            let ratio = numerator as f64 / denominator.max(1) as f64;

            shape_rows.push(ShapeRow {
                case: case.id.clone(),
                shape: shape.id(),
                file_count: written.file_count,
                total_bytes: written.total_bytes,
                bytes_excluding_localization: written.bytes_excluding_localization,
                bytes_by_class: written.bytes_by_class.clone(),
                ratio_to_unsharded: (ratio * 1000.0).round() / 1000.0,
                meets_file_budget: written.file_count <= timing::budget::BUNDLE_FILES,
                meets_size_budget: ratio <= timing::budget::BUNDLE_SIZE_RATIO
                    && numerator <= timing::budget::BUNDLE_SIZE_BYTES,
            });

            std::fs::remove_dir_all(&written.root)?;
        }
        case_rows.push(row);
    }

    for row in &shape_rows {
        if !row.meets_file_budget {
            warnings.push(format!(
                "{} {}: {} files exceeds the declared {} budget",
                row.case, row.shape, row.file_count, timing::budget::BUNDLE_FILES
            ));
        }
        if !row.meets_size_budget {
            warnings.push(format!(
                "{} {}: {:.3}x the unsharded payload exceeds the declared {:.1}x budget",
                row.case, row.shape, row.ratio_to_unsharded, timing::budget::BUNDLE_SIZE_RATIO
            ));
        }
    }

    let stores = vec![
        locstore::measure("whole_language", &whole),
        locstore::measure("content_defined", &content),
    ];

    let summary = render(&case_rows, &shape_rows, &stores);
    print!("{summary}");

    if capture {
        let directory = record::write(
            "b2-shape",
            PURPOSE,
            corpora.into_values().collect(),
            vec![
                Artifact::identity("cases.json", record::to_json(&case_rows)),
                Artifact::identity("shapes.json", record::to_json(&shape_rows)),
                Artifact::identity("localization-store.json", record::to_json(&stores)),
                Artifact::identity("summary.txt", summary),
            ],
            warnings,
        )?;
        eprintln!("captured {}", directory.display());
    }
    let _ = std::fs::remove_dir_all(&revisions_root);
    Ok(())
}

fn measure_case(case: &str, documentation: &Documentation, closure: &Documentation) -> CaseRow {
    let unsharded = documentation.unsharded_payload();
    let unsharded_closure = closure.unsharded_payload();

    // The denominator with localization removed. Computed by serializing the same value with
    // the localization emptied, rather than by subtracting an estimate, so the numerator and
    // denominator are produced by the same code path.
    let mut without = documentation.clone();
    without.localization = Default::default();
    let unsharded_excluding = without.unsharded_payload();

    let browse: u64 = documentation
        .browse_index()
        .values()
        .map(|summaries| record::to_compact_json(summaries).len() as u64)
        .sum();
    let search: u64 = documentation
        .search_material()
        .values()
        .map(|entries| record::to_compact_json(entries).len() as u64)
        .sum();
    let search_english: u64 = documentation
        .search_material()
        .get("english")
        .map(|entries| record::to_compact_json(entries).len() as u64)
        .unwrap_or(0);
    let records: u64 = documentation
        .full_records()
        .map(|(_, entry)| record::to_compact_json(entry).len() as u64)
        .sum();
    let issues = record::to_compact_json(&documentation.issues).len() as u64;

    // What a browse summary restates: every field in it also appears in the full record. The
    // whole summary is therefore duplication, which is the price paid for not loading every
    // record to draw a list.
    let browse_duplication = browse;
    // What search material restates: the localized name, which preserved localization already
    // holds. The rest of a search entry — the key, the normalized form, the category — is not
    // in localization and is not duplication.
    let search_duplication: u64 = documentation
        .search_material()
        .values()
        .flat_map(|entries| entries.iter())
        .map(|entry| entry.name.len() as u64)
        .sum();

    let placeholders = documentation
        .entries
        .iter()
        .filter(|entry| matches!(entry.icon, bundle_spike::docmodel::AssetSlot::Placeholder { .. }))
        .count();

    CaseRow {
        case: case.to_owned(),
        entries: documentation.entries.len(),
        languages: documentation.localization.languages.len(),
        localization_keys: documentation.localization.total_entries(),
        unsharded_bytes: unsharded.len() as u64,
        unsharded_bytes_excluding_localization: unsharded_excluding.len() as u64,
        unsharded_bytes_closure: unsharded_closure.len() as u64,
        cited_seed_keys: documentation.entries.len() * 2,
        cited_closure_keys: closure
            .localization
            .languages
            .values()
            .flat_map(|table| table.entries.keys())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        closure_localization_bytes: closure.localization.total_value_bytes(),
        full_localization_bytes: documentation.localization.total_value_bytes(),
        browse_bytes: browse,
        search_bytes: search,
        search_bytes_english: search_english,
        record_bytes: records,
        issue_bytes: issues,
        browse_duplication_bytes: browse_duplication,
        search_duplication_bytes: search_duplication,
        asset_keys: documentation.entries.len() - placeholders,
        placeholder_icons: placeholders,
    }
}

fn render(cases: &[CaseRow], shapes: &[ShapeRow], stores: &[locstore::StoreMeasurement]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# b2-shape\n");

    let _ = writeln!(
        out,
        "{:<14} {:>7} {:>10} {:>12} {:>12} {:>12} {:>10} {:>10}",
        "case", "entries", "loc keys", "all keys", "closure", "excl. loc", "records", "browse"
    );
    for case in cases {
        let _ = writeln!(
            out,
            "{:<14} {:>7} {:>10} {:>11.1}M {:>11.2}M {:>11.2}M {:>9.2}M {:>9.2}M",
            case.case,
            case.entries,
            case.localization_keys,
            mib(case.unsharded_bytes),
            mib(case.unsharded_bytes_closure),
            mib(case.unsharded_bytes_excluding_localization),
            mib(case.record_bytes),
            mib(case.browse_bytes),
        );
    }

    let _ = writeln!(out, "\ncited-key closure against preserved localization");
    let _ = writeln!(
        out,
        "{:<14} {:>8} {:>10} {:>12} {:>12} {:>8}",
        "case", "seeds", "closure", "closure MiB", "all MiB", "share"
    );
    for case in cases {
        let _ = writeln!(
            out,
            "{:<14} {:>8} {:>10} {:>11.2}M {:>11.1}M {:>7.2}%",
            case.case,
            case.cited_seed_keys,
            case.cited_closure_keys,
            mib(case.closure_localization_bytes),
            mib(case.full_localization_bytes),
            case.closure_localization_bytes as f64 / case.full_localization_bytes.max(1) as f64
                * 100.0,
        );
    }

    let _ = writeln!(out, "\n{:<14} {:<48} {:>7} {:>12} {:>8} {:>7} {:>7}", "case", "shape", "files", "bytes", "ratio", "files?", "size?");
    for shape in shapes {
        let _ = writeln!(
            out,
            "{:<14} {:<48} {:>7} {:>11.1}M {:>8.3} {:>7} {:>7}",
            shape.case,
            shape.shape,
            shape.file_count,
            mib(shape.total_bytes),
            shape.ratio_to_unsharded,
            if shape.meets_file_budget { "ok" } else { "MISS" },
            if shape.meets_size_budget { "ok" } else { "MISS" },
        );
    }

    let _ = writeln!(out, "\nlocalization store, across every revision measured");
    for store in stores {
        let _ = writeln!(
            out,
            "  {:<16} {:>8.1}M carried -> {:>8.1}M unique in {:>6} chunks, {:.2}x, recovering {:.1}M",
            store.scheme,
            mib(store.per_revision_total_bytes),
            mib(store.unique_bytes),
            store.unique_chunks,
            store.deduplication_ratio,
            mib(store.recovered_bytes),
        );
    }
    out
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
