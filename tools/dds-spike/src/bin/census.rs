//! `d1-census` — what the corpus actually contains, and what of it is reachable.
//!
//! The denominator. Every coverage claim in the later runs is reported beside this one, because
//! "every file decoded" says nothing when the corpus is not known to contain the classes that
//! would be hard. It also answers a question the earlier feasibility note could not: which
//! formats the technology icons are actually stored in, as against which one happened to be
//! opened.
//!
//! Two denominators are reported, not one. Every `.dds` on disk is the wrong measure for a
//! documentation tool — it converts textures its content references. The reachable set is the
//! distinct paths any sprite definition names, and it is also where missing bytes turn out to
//! occur in shipped content.
//!
//! ```text
//! cargo run --release --manifest-path tools/dds-spike/Cargo.toml --bin census
//! ```
//! Pass `--capture` to write `docs/spikes/dds-records/d1-census/`.

use std::collections::BTreeMap;

use dds_spike::classify::{classify, Classification};
use dds_spike::corpus::{self, Corpus, CorpusIdentity};
use dds_spike::header::Header;
use dds_spike::record;
use dds_spike::references;
use serde::Serialize;

const PURPOSE: &str = "Census every DDS file in the pinned corpora by exact pixel-format layout, \
dimensions, mip count, and surface shape, and separately census the reachable set — the distinct \
texture paths that sprite definitions actually name. This is the denominator for every later \
coverage claim. Grouping by bit count rather than by mask layout would hide the files that \
declare the opposite channel order, which are the only inputs in the corpus capable of catching \
a red/blue swap, so the format histogram is keyed by the masks the file declares. The run also \
opens every DLC archive's central directory to record what those archives contain, because the \
technical design asserted they may supply referenced visual assets and that claim was inherited \
rather than measured.";

#[derive(Debug, Default, Serialize)]
struct CorpusCensus {
    id: String,
    files: usize,
    total_bytes: u64,
    formats: BTreeMap<String, usize>,
    dimensions_top: Vec<(String, usize)>,
    mip_counts: BTreeMap<u32, usize>,
    cubemaps: usize,
    volumes: usize,
    block_unaligned: BTreeMap<String, usize>,
    /// 24-bit surfaces whose rows are not a multiple of four bytes. If any decoder assumed a
    /// four-byte row alignment, these are the files where it would break.
    unaligned_24bpp_rows: usize,
    malformed: usize,
    unsupported: usize,
    /// Files whose declared channel order puts red in the low byte, listed by name because there
    /// are few enough to name and they carry more evidentiary weight than the other 20,000.
    reverse_channel_order: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
struct ReachableCensus {
    corpus: String,
    referenced_paths: usize,
    resolved: usize,
    dangling: usize,
    doubled_separators: usize,
    formats: BTreeMap<String, usize>,
}

fn main() -> std::io::Result<()> {
    let corpora = corpus::default_corpora();
    let mut identities: Vec<CorpusIdentity> = Vec::new();
    let mut censuses: Vec<CorpusCensus> = Vec::new();
    let mut reachable: Vec<ReachableCensus> = Vec::new();
    let mut faults: Vec<String> = Vec::new();
    let mut dangling_lines: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    // Keyed by absolute path across every corpus, because a mod's sprite definition may resolve
    // to a vanilla texture and the reachable census still needs to name that texture's format.
    let mut formats_by_absolute: BTreeMap<std::path::PathBuf, String> = BTreeMap::new();

    for corpus_entry in &corpora {
        let files = corpus::enumerate(&corpus_entry.root)?;
        if files.is_empty() {
            warnings.push(format!(
                "corpus {} at {} holds no .dds files",
                corpus_entry.id,
                corpus_entry.root.display()
            ));
        }
        identities.push(corpus::identify(corpus_entry, &files)?);

        let mut census = CorpusCensus {
            id: corpus_entry.id.clone(),
            ..Default::default()
        };
        let mut dimensions: BTreeMap<(u32, u32), usize> = BTreeMap::new();

        for file in &files {
            let bytes = std::fs::read(&file.absolute)?;
            census.files += 1;
            census.total_bytes += bytes.len() as u64;

            let classification = classify(&bytes);
            let label = classification.label();
            *census.formats.entry(label.clone()).or_default() += 1;
            formats_by_absolute.insert(file.absolute.clone(), label);

            match &classification {
                Classification::Malformed(reason) => {
                    census.malformed += 1;
                    faults.push(format!(
                        "{}\t{}\t{}\t{}",
                        corpus_entry.id,
                        file.logical,
                        bytes.len(),
                        reason
                    ));
                }
                Classification::Unsupported { header, reason } => {
                    census.unsupported += 1;
                    tally_shape(&mut census, header, &mut dimensions);
                    faults.push(format!(
                        "{}\t{}\t{}\t{}",
                        corpus_entry.id,
                        file.logical,
                        bytes.len(),
                        reason
                    ));
                }
                Classification::Decodable(decodable) => {
                    tally_shape(&mut census, &decodable.header, &mut dimensions);
                    if decodable.format.is_block_compressed()
                        && (decodable.header.width % 4 != 0 || decodable.header.height % 4 != 0)
                    {
                        *census
                            .block_unaligned
                            .entry(decodable.format.label())
                            .or_default() += 1;
                    }
                    let pf = &decodable.header.pixel_format;
                    if !pf.is_four_cc() && pf.bit_count == 24 && (decodable.header.width * 3) % 4 != 0
                    {
                        census.unaligned_24bpp_rows += 1;
                    }
                    // Red in the low byte: A8B8G8R8 where almost everything is A8R8G8B8.
                    if !pf.is_four_cc() && pf.bit_count == 32 && pf.red_mask == 0x0000_00ff {
                        census.reverse_channel_order.push(file.logical.clone());
                    }
                }
            }
        }

        let mut top: Vec<_> = dimensions.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        census.dimensions_top = top
            .into_iter()
            .take(8)
            .map(|((width, height), count)| (format!("{width}x{height}"), count))
            .collect();
        censuses.push(census);

    }

    // After every corpus is classified, so a mod's reference to a vanilla texture can be named.
    for corpus_entry in &corpora {
        if corpus_entry.id != "fixtures" {
            reachable.push(reachable_census(
                corpus_entry,
                &formats_by_absolute,
                &mut dangling_lines,
            )?);
        }
    }

    for census in &censuses {
        println!(
            "{:9} files {:6}  malformed {:2}  unsupported {:3}  cubemaps {:2}  reverse-order {:2}",
            census.id,
            census.files,
            census.malformed,
            census.unsupported,
            census.cubemaps,
            census.reverse_channel_order.len()
        );
        for (label, count) in &census.formats {
            println!("           {label:28} {count:6}");
        }
    }
    for entry in &reachable {
        println!(
            "{:9} referenced {:5}  resolved {:5}  dangling {:3}  doubled-separator {:2}",
            entry.corpus,
            entry.referenced_paths,
            entry.resolved,
            entry.dangling,
            entry.doubled_separators
        );
    }

    if !record::capture_requested() {
        println!("{}", record::NOT_CAPTURED);
        return Ok(());
    }

    #[derive(Serialize)]
    struct Census {
        corpora: Vec<CorpusCensus>,
        reachable: Vec<ReachableCensus>,
    }
    let census_json = serde_json::to_string_pretty(&Census {
        corpora: censuses,
        reachable,
    })? + "\n";

    faults.sort();
    dangling_lines.sort();

    // Whether DLC archives ship textures, which `docs/technical-design.md:285` asserts they may.
    let mut archive_lines = Vec::new();
    let mut archive_images = 0usize;
    for summary in dds_spike::archives::scan_dlc(&corpus::install_root()) {
        let extensions = summary
            .extensions
            .iter()
            .map(|(extension, count)| format!("{extension}={count}"))
            .collect::<Vec<_>>()
            .join(" ");
        for image in ["dds", "png", "tga", "jpg", "jpeg", "bmp"] {
            archive_images += summary.extensions.get(image).copied().unwrap_or(0);
        }
        archive_lines.push(format!(
            "{}\t{}\t{}",
            summary.logical, summary.entries, extensions
        ));
    }
    println!(
        "dlc archives: {} scanned, {archive_images} image entries",
        archive_lines.len()
    );

    let artifacts = vec![
        ("census.json".to_string(), census_json),
        (
            "header-faults.txt".to_string(),
            table("# corpus\tlogical path\tbytes\tfault", &faults),
        ),
        (
            "dangling-references.txt".to_string(),
            table("# corpus\treferenced path", &dangling_lines),
        ),
        (
            "dlc-archives.txt".to_string(),
            table(
                "# archive\tentries\textension counts",
                &archive_lines,
            ),
        ),
    ];
    let directory = record::write("d1-census", PURPOSE, identities, artifacts, warnings)?;
    println!("captured {}", directory.display());
    Ok(())
}

fn tally_shape(
    census: &mut CorpusCensus,
    header: &Header,
    dimensions: &mut BTreeMap<(u32, u32), usize>,
) {
    *dimensions.entry((header.width, header.height)).or_default() += 1;
    *census.mip_counts.entry(header.mip_count.min(16)).or_default() += 1;
    if header.is_cubemap() {
        census.cubemaps += 1;
    }
    if header.is_volume() {
        census.volumes += 1;
    }
}

/// Census the reachable set for one corpus.
///
/// The workshop root is not one content root but a directory of them, so each installed mod is
/// scanned separately and its references resolve against that mod and then vanilla — which is how
/// Stellaris resolves them. Treating the workshop root as a single tree would report almost every
/// vanilla texture a mod names as missing.
fn reachable_census(
    corpus_entry: &Corpus,
    formats: &BTreeMap<std::path::PathBuf, String>,
    dangling_lines: &mut Vec<String>,
) -> std::io::Result<ReachableCensus> {
    let install = corpus::install_root();
    let roots: Vec<std::path::PathBuf> = if corpus_entry.id == "workshop" {
        let mut mods: Vec<_> = std::fs::read_dir(&corpus_entry.root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        mods.sort();
        mods
    } else {
        vec![corpus_entry.root.clone()]
    };

    let mut census = ReachableCensus {
        corpus: corpus_entry.id.clone(),
        ..Default::default()
    };
    let mut referenced = std::collections::BTreeSet::new();
    let mut dangling = std::collections::BTreeSet::new();

    for root in &roots {
        let resolve: Vec<&std::path::Path> = if corpus_entry.id == "workshop" {
            vec![root.as_path(), install.as_path()]
        } else {
            vec![root.as_path()]
        };
        let found = references::scan(root, &resolve)?;
        census.doubled_separators += found.doubled_separators.len();

        for path in &found.referenced {
            let label = format!("{}\t{}", root.file_name().unwrap_or_default().to_string_lossy(), path);
            if !referenced.insert(label.clone()) {
                continue;
            }
            if found.dangling.contains(path) {
                dangling.insert(label.clone());
                dangling_lines.push(format!("{}\t{}", corpus_entry.id, label));
                continue;
            }
            census.resolved += 1;
            let absolute = resolve
                .iter()
                .map(|base| base.join(path))
                .find(|candidate| candidate.exists());
            if let Some(format) = absolute.and_then(|path| formats.get(&path)) {
                *census.formats.entry(format.clone()).or_default() += 1;
            }
        }
    }

    census.referenced_paths = referenced.len();
    census.dangling = dangling.len();
    Ok(census)
}

/// A tab-separated artifact with a `#` header, as the parser records use.
fn table(header: &str, rows: &[String]) -> String {
    let mut text = String::from(header);
    text.push('\n');
    for row in rows {
        text.push_str(row);
        text.push('\n');
    }
    text
}
