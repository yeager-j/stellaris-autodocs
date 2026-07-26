//! A probe, not a record: resolve and generate each revision case and print what came out.
//!
//! Kept because "the corpus is what I think it is" is the assumption every later number rests
//! on, and it costs one pass to check rather than infer.

use std::collections::BTreeMap;

fn main() -> std::io::Result<()> {
    for case in bundle_spike::corpus::default_cases() {
        let mut snapshots = BTreeMap::new();
        for contributor in case.contributors() {
            snapshots.insert(
                contributor.id.clone(),
                bundle_spike::corpus::snapshot(contributor)?,
            );
        }

        let started = std::time::Instant::now();
        let resolved = bundle_spike::resolve::resolve(&case, &snapshots)?;
        let resolve_time = started.elapsed();

        let started = std::time::Instant::now();
        let documentation = bundle_spike::generate::generate(
            &resolved,
            &resolved.sources,
            bundle_spike::generate::LocalizationScope::CitedClosure,
        );
        let generate_time = started.elapsed();

        let unsharded = documentation.unsharded_payload();
        let browse = bundle_spike::record::to_json(&documentation.browse_index());
        let search = bundle_spike::record::to_json(&documentation.search_material());
        let records: usize = documentation
            .full_records()
            .map(|(_, entry)| bundle_spike::record::to_json(entry).len())
            .sum();

        println!("{} — {}", case.id, case.title);
        println!(
            "  {} entries, {} issues, complete={}, incomplete registries {:?}",
            documentation.entries.len(),
            documentation.issues.len(),
            documentation.completeness.complete,
            documentation.completeness.incomplete_registries,
        );
        println!(
            "  resolve {:.2}s  generate {:.2}s",
            resolve_time.as_secs_f64(),
            generate_time.as_secs_f64()
        );
        println!(
            "  unsharded {:.1} MiB  browse {:.1} MiB  search {:.1} MiB  records {:.1} MiB",
            mib(unsharded.len()),
            mib(browse.len()),
            mib(search.len()),
            mib(records),
        );
        println!(
            "  localization {} languages, {} keys, {:.1} MiB of key+value bytes",
            documentation.localization.languages.len(),
            documentation.localization.total_entries(),
            documentation.localization.total_value_bytes() as f64 / (1024.0 * 1024.0),
        );
    }
    Ok(())
}

fn mib(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
