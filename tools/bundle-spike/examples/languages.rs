//! A probe, not a record: how is preserved localization distributed across languages?
//!
//! Lives in `examples/` rather than `src/`, so it does not enter the source tree digest the
//! drift gate compares and cannot invalidate a captured record.

use std::collections::BTreeMap;

fn main() -> std::io::Result<()> {
    for case_id in ["vanilla", "giga"] {
        let case = bundle_spike::corpus::default_cases()
            .into_iter()
            .find(|case| case.id == case_id)
            .expect("case exists");

        let mut snapshots = BTreeMap::new();
        for contributor in case.contributors() {
            snapshots.insert(
                contributor.id.clone(),
                bundle_spike::corpus::snapshot(contributor)?,
            );
        }
        let resolved = bundle_spike::resolve::resolve(&case, &snapshots)?;

        let mut rows: Vec<(String, usize, u64)> = resolved
            .localization
            .languages
            .iter()
            .map(|(language, table)| {
                let bytes: u64 = table
                    .entries
                    .iter()
                    .map(|(key, value)| (key.len() + value.len()) as u64)
                    .sum();
                (language.clone(), table.entries.len(), bytes)
            })
            .collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.2));

        let total: u64 = rows.iter().map(|row| row.2).sum();
        println!("\n{case_id}: {:.1} MiB across {} languages", mib(total), rows.len());
        for (language, keys, bytes) in &rows {
            println!(
                "  {language:<14} {keys:>9} keys  {:>7.1} MiB  {:>5.1}%",
                mib(*bytes),
                *bytes as f64 / total as f64 * 100.0
            );
        }
    }
    Ok(())
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
