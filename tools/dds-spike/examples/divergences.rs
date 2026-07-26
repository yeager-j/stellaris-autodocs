//! Name every file where the two readings disagree by more than rounding, with its shape.
//!
//! Kept in the tree because the BC3 finding rests on what this printed, and a future reader
//! deserves the observation rather than the conclusion alone. `d2-decode` reports that there are
//! zero such files; this is what produced the list when there were 22 of them, and it is what
//! turned "the decoders disagree" into "all 22 are BC3, all in one mod, and the alpha delta is
//! always 1" — which is what pointed at the colour block rather than the alpha block.
//!
//! It is also the first thing to run after an `image_dds` upgrade or a change to `decode_b`. A
//! divergence count alone says nothing; the shape of the divergences is the diagnosis.
//!
//! ```text
//! cargo run --release --manifest-path tools/dds-spike/Cargo.toml --example divergences
//! ```

use dds_spike::classify::{classify, Classification};
use dds_spike::corpus;
use dds_spike::decode_a;
use dds_spike::decode_b;
use dds_spike::model::{compare, Comparison};
use dds_spike::recipe::{OutputFormat, Recipe};

/// Matches `d2-decode`'s threshold, so this probe and the record agree about what counts.
const ROUNDING_TOLERANCE: u8 = 4;

fn main() -> std::io::Result<()> {
    let recipe = Recipe::pinned(OutputFormat::Png);
    let mut found = 0usize;

    println!("# logical path\tformat\tshape\tdiffering pixels\tmax delta rgba");
    for corpus_entry in corpus::default_corpora() {
        for file in corpus::enumerate(&corpus_entry.root)? {
            let bytes = std::fs::read(&file.absolute).unwrap_or_default();
            let a = decode_a::adapt(&bytes, &recipe);
            let b = decode_b::adapt(&bytes, &recipe);

            let Comparison::PixelsDiffer {
                differing_pixels,
                total_pixels,
                max_delta,
            } = compare(&a, &b)
            else {
                continue;
            };
            if max_delta.iter().copied().max().unwrap_or(0) <= ROUNDING_TOLERANCE {
                continue;
            }

            let classification = classify(&bytes);
            let shape = match &classification {
                Classification::Decodable(decodable) => format!(
                    "{}x{} mips={}",
                    decodable.header.width, decodable.header.height, decodable.header.mip_count
                ),
                _ => "-".into(),
            };
            found += 1;
            println!(
                "{}\t{}\t{shape}\t{differing_pixels}/{total_pixels}\t{max_delta:?}",
                file.logical,
                classification.label()
            );
        }
    }

    println!("\n{found} file(s) diverge beyond a per-channel delta of {ROUNDING_TOLERANCE}");
    Ok(())
}
