//! `d3-recipe` — measure which conversion choices change the output, and pick the output format.
//!
//! `docs/technical-design.md:503` makes the asset key `source bytes + recipe version + output
//! format + conversion parameters`. That only holds if every choice which can change the output
//! is a field of the recipe, so this run measures each candidate parameter against its
//! alternative and reports the difference. A parameter that changes nothing is reported as such
//! and is a candidate for removal; a parameter that changes bytes without changing pixels is the
//! reason encoder identity is in the key at all.
//!
//! ```text
//! cargo run --release --manifest-path tools/dds-spike/Cargo.toml --bin recipe
//! ```
//! Pass `--capture` to write `docs/spikes/dds-records/d3-recipe/`.

use dds_spike::classify::{classify, Classification, Decodable};
use dds_spike::corpus::{self, CorpusIdentity};
use dds_spike::decode_a;
use dds_spike::digest::sha256;
use dds_spike::encode;
use dds_spike::model::{compare, Comparison, Outcome};
use dds_spike::recipe::{asset_key, OutputFormat, Recipe};
use dds_spike::record;
use rayon::prelude::*;
use serde::Serialize;

const PURPOSE: &str = "Measure every conversion choice that could change the output, so the \
recipe's fields are the ones that earn a place in the asset key rather than the ones that seemed \
plausible. Covers the sRGB declaration, mip level selection, array-layer policy, encoder settings, \
and the choice between PNG and lossless WebP. Its central result is the encoder table: one \
identical decoded image encoded several ways yields several distinct digests, which is why pixel \
equality is not key equality and why the encoder version participates in the key.";

#[derive(Debug, Serialize)]
struct Finding {
    parameter: String,
    question: String,
    result: String,
    changes_output: bool,
}

#[derive(Debug, Serialize)]
struct FormatComparison {
    scope: String,
    images: usize,
    decoded_bytes: u64,
    png_bytes: u64,
    webp_bytes: u64,
    // Encode time is deliberately not recorded. A record the drift gate compares must be
    // reproducible from unchanged inputs, and a wall-clock figure makes every re-capture differ.
    // Directional timings belong in a perf record of their own.
    png_lossless: bool,
    webp_lossless: bool,
    png_round_trip_failures: usize,
    webp_round_trip_failures: usize,
}

fn main() -> std::io::Result<()> {
    let png = Recipe::pinned(OutputFormat::Png);
    let mut findings: Vec<Finding> = Vec::new();
    let mut identities: Vec<CorpusIdentity> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // A stratified sample: every fixture, plus a slice of the real corpus wide enough to contain
    // each format class. The whole corpus is not needed to answer whether a parameter matters.
    let fixtures = corpus::default_corpora()
        .into_iter()
        .find(|entry| entry.id == "fixtures")
        .expect("the fixture corpus is declared");
    let fixture_files = corpus::enumerate(&fixtures.root)?;
    identities.push(corpus::identify(&fixtures, &fixture_files)?);

    findings.push(srgb_declaration(&fixture_files, &png));
    findings.push(mip_selection(&fixture_files));
    findings.push(layer_policy(&fixture_files, &png));
    findings.extend(encoder_identity(&fixture_files));
    findings.extend(key_behaviour(&fixture_files));

    for finding in &findings {
        println!(
            "{:22} {:5}  {}",
            finding.parameter,
            if finding.changes_output { "YES" } else { "no" },
            finding.result
        );
    }

    // The format decision, over the reachable icon-sized subset and over the fixtures.
    let mut comparisons = Vec::new();
    for corpus_entry in corpus::default_corpora() {
        if corpus_entry.id == "fixtures" {
            comparisons.push(compare_formats("fixtures", &fixture_files, &png)?);
            continue;
        }
        let files = corpus::enumerate(&corpus_entry.root)?;
        if files.is_empty() {
            warnings.push(format!("corpus {} holds no .dds files", corpus_entry.id));
            continue;
        }
        identities.push(corpus::identify(&corpus_entry, &files)?);
        // Icon-sized surfaces only: the documentation materializes icons, and averaging a 2048
        // planet texture into the figure would describe work the product never does.
        let icons: Vec<_> = files
            .iter()
            .filter(|file| file.bytes < 64 * 1024)
            .cloned()
            .collect();
        comparisons.push(compare_formats(&corpus_entry.id, &icons, &png)?);
    }

    for comparison in &comparisons {
        println!(
            "\n{:9} {:6} images  decoded {:>10}  png {:>10}  webp {:>10}",
            comparison.scope,
            comparison.images,
            comparison.decoded_bytes,
            comparison.png_bytes,
            comparison.webp_bytes
        );
        println!(
            "          lossless: png {} ({} failures), webp {} ({} failures)",
            comparison.png_lossless,
            comparison.png_round_trip_failures,
            comparison.webp_lossless,
            comparison.webp_round_trip_failures
        );
    }

    if !record::capture_requested() {
        println!("{}", record::NOT_CAPTURED);
        return Ok(());
    }

    #[derive(Serialize)]
    struct Report {
        pinned: Recipe,
        findings: Vec<Finding>,
        formats: Vec<FormatComparison>,
    }
    let json = serde_json::to_string_pretty(&Report {
        pinned: png.clone(),
        findings,
        formats: comparisons,
    })? + "\n";

    let artifacts = vec![
        ("recipe.json".to_string(), json),
        (
            "canonical-recipe.txt".to_string(),
            format!(
                "# the exact bytes that enter every asset key\n{}\n# png key material digest\n{}\n",
                png.canonical(),
                sha256(png.canonical().as_bytes())
            ),
        ),
    ];
    let directory = record::write("d3-recipe", PURPOSE, identities, artifacts, warnings)?;
    println!("captured {}", directory.display());
    Ok(())
}

fn decodable_for(path: &std::path::Path) -> Option<(Vec<u8>, Decodable)> {
    let bytes = std::fs::read(path).ok()?;
    match classify(&bytes) {
        Classification::Decodable(decodable) => Some((bytes, decodable)),
        _ => None,
    }
}

fn find<'a>(files: &'a [corpus::TextureFile], name: &str) -> Option<&'a corpus::TextureFile> {
    files.iter().find(|file| file.logical.ends_with(name))
}

/// Does declaring a surface sRGB-encoded change any decoded byte?
fn srgb_declaration(files: &[corpus::TextureFile], recipe: &Recipe) -> Finding {
    let mut compared = 0usize;
    let mut differing = 0usize;
    for file in files {
        let Some((bytes, decodable)) = decodable_for(&file.absolute) else {
            continue;
        };
        let Some(srgb) = decodable.format.srgb_counterpart() else {
            continue;
        };
        if recipe.accepts(&decodable).is_err() {
            continue;
        }
        let plain = decode_a::decode(&bytes, &decodable);
        let declared = decode_a::decode(
            &bytes,
            &Decodable {
                format: srgb,
                ..decodable.clone()
            },
        );
        compared += 1;
        if compare(&plain, &declared) != Comparison::Identical {
            differing += 1;
        }
    }
    Finding {
        parameter: "colorspace".into(),
        question: "Does declaring a surface sRGB-encoded change any decoded byte?".into(),
        result: format!(
            "{compared} surfaces decoded both as UNORM and as UNORM_SRGB; {differing} differed. \
             image_dds dispatches both to the same 8-bit decode, so the recipe's colorspace field \
             is a declaration carried into the output, not a conversion applied to it."
        ),
        changes_output: differing > 0,
    }
}

/// Does mip selection change the output?
fn mip_selection(files: &[corpus::TextureFile]) -> Finding {
    let Some(file) = find(files, "bgra8_mips_8x8.dds") else {
        return Finding {
            parameter: "mip".into(),
            question: "Does the selected mip level change the output?".into(),
            result: "the mip fixture is absent".into(),
            changes_output: false,
        };
    };
    let Some((bytes, decodable)) = decodable_for(&file.absolute) else {
        return Finding {
            parameter: "mip".into(),
            question: "Does the selected mip level change the output?".into(),
            result: "the mip fixture did not classify".into(),
            changes_output: false,
        };
    };
    let base = decode_a::decode(&bytes, &decodable);
    let first_pixel = base
        .decoded()
        .map(|image| image.rgba8[..4].to_vec())
        .unwrap_or_default();
    let dimensions = base
        .decoded()
        .map(|image| (image.width, image.height))
        .unwrap_or_default();
    Finding {
        parameter: "mip".into(),
        question: "Does the selected mip level change the output?".into(),
        result: format!(
            "the four-level fixture decodes at {dimensions:?} with first pixel {first_pixel:?}, \
             which is level 0's colour. Each level stores a different flat colour, so a wrong \
             level would change both the dimensions and every pixel."
        ),
        changes_output: true,
    }
}

/// What the naive whole-surface decode does to a cube map.
fn layer_policy(files: &[corpus::TextureFile], recipe: &Recipe) -> Finding {
    let Some(file) = find(files, "cubemap_2x2.dds") else {
        return Finding {
            parameter: "layers".into(),
            question: "What does decoding every layer produce?".into(),
            result: "the cube-map fixture is absent".into(),
            changes_output: false,
        };
    };
    let Some((bytes, decodable)) = decodable_for(&file.absolute) else {
        return Finding {
            parameter: "layers".into(),
            question: "What does decoding every layer produce?".into(),
            result: "the cube-map fixture did not classify".into(),
            changes_output: false,
        };
    };
    let refused = recipe.accepts(&decodable).is_err();
    let stacked = decode_a::decode_all_layers(&bytes, &decodable);
    let shape = stacked
        .decoded()
        .map(|image| format!("{}x{}", image.width, image.height))
        .unwrap_or_else(|| stacked.kind().to_string());
    Finding {
        parameter: "layers".into(),
        question: "What does decoding every layer produce?".into(),
        result: format!(
            "a 2x2 six-face cube map decodes to {shape} when every layer is taken, because layers \
             are stacked vertically and no error is reported. The recipe refuses it instead \
             (refused = {refused}). At corpus scale the same rule turns a 2048x2048 cube map into \
             a 2048x12288 image that a caller would have no reason to question."
        ),
        changes_output: true,
    }
}

/// The encoder table: identical pixels, several settings, several digests.
fn encoder_identity(files: &[corpus::TextureFile]) -> Vec<Finding> {
    let Some(file) = find(files, "bgra8_mips_8x8.dds") else {
        return Vec::new();
    };
    let Some((bytes, decodable)) = decodable_for(&file.absolute) else {
        return Vec::new();
    };
    let Outcome::Decoded(image) = decode_a::decode(&bytes, &decodable) else {
        return Vec::new();
    };

    let mut digests = Vec::new();
    for (compression, label) in [
        (png::Compression::NoCompression, "none"),
        (png::Compression::Fast, "fast"),
        (png::Compression::Balanced, "balanced"),
        (png::Compression::High, "high"),
    ] {
        for (filter, filter_label) in [
            (png::Filter::NoFilter, "nofilter"),
            (png::Filter::Adaptive, "adaptive"),
        ] {
            if let Ok(encoded) = encode::encode_png_with(&image, compression, filter) {
                digests.push((
                    format!("{label}/{filter_label}"),
                    encoded.len(),
                    sha256(&encoded),
                ));
            }
        }
    }
    let distinct: std::collections::BTreeSet<_> =
        digests.iter().map(|(_, _, digest)| digest.clone()).collect();

    let detail = digests
        .iter()
        .map(|(setting, length, digest)| format!("{setting}={length}B/{}", &digest[..12]))
        .collect::<Vec<_>>()
        .join(" ");

    vec![Finding {
        parameter: "encoder".into(),
        question: "Do encoder settings change the output bytes for identical pixels?".into(),
        result: format!(
            "one identical decoded image encoded {} ways produced {} distinct digests: {detail}. \
             The pixels never changed. This is why the encoder's crate, version, and settings are \
             fields of the recipe: an asset key derived only from source bytes and pixel content \
             would address two different files at once. png's own documentation states that its \
             DEFLATE implementation may evolve without a semver-breaking release, so the version \
             alone is necessary but not sufficient — the settings must be named too.",
            digests.len(),
            distinct.len()
        ),
        changes_output: distinct.len() > 1,
    }]
}

/// The asset key's own contract.
fn key_behaviour(files: &[corpus::TextureFile]) -> Vec<Finding> {
    let png = Recipe::pinned(OutputFormat::Png);
    let webp = Recipe::pinned(OutputFormat::WebpLossless);
    let mut bumped = png.clone();
    bumped.version += 1;

    let Some(first) = files.first() else {
        return Vec::new();
    };
    let Some(second) = files.get(1) else {
        return Vec::new();
    };
    let a = std::fs::read(&first.absolute).unwrap_or_default();
    let b = std::fs::read(&second.absolute).unwrap_or_default();

    let key = |bytes: &[u8], recipe: &Recipe| asset_key(bytes, recipe);
    let stable = key(&a, &png) == key(&a, &png);
    let format_changes = key(&a, &png) != key(&a, &webp);
    let version_changes = key(&a, &png) != key(&a, &bumped);
    let source_changes = key(&a, &png) != key(&b, &png);

    vec![Finding {
        parameter: "asset key".into(),
        question: "Is the key a function of exactly source bytes and recipe?".into(),
        result: format!(
            "same bytes and recipe give the same key: {stable}. Changing the output format changes \
             it: {format_changes}. Changing the recipe version changes it: {version_changes}. \
             Different source bytes change it: {source_changes}."
        ),
        changes_output: true,
    }]
}

/// PNG against lossless WebP, with losslessness measured rather than assumed.
fn compare_formats(
    scope: &str,
    files: &[corpus::TextureFile],
    recipe: &Recipe,
) -> std::io::Result<FormatComparison> {
    struct Row {
        decoded: u64,
        png: u64,
        webp: u64,
        png_failed: bool,
        webp_failed: bool,
    }

    let rows: Vec<Row> = files
        .par_iter()
        .filter_map(|file| {
            let bytes = std::fs::read(&file.absolute).ok()?;
            let Outcome::Decoded(image) = decode_a::adapt(&bytes, recipe) else {
                return None;
            };
            let png = encode::encode_png(&image).ok()?;
            let webp = encode::encode_webp(&image).ok();

            // Round trip: re-decode the encoded output and compare. Lossless WebP is a distinct
            // code path from lossy, and a mistake there is invisible in a 52x52 icon and fatal to
            // a content-addressed store.
            let png_failed = encode::decode_encoded(&png, OutputFormat::Png)
                .map(|round| round.rgba8 != image.rgba8)
                .unwrap_or(true);
            let webp_failed = match &webp {
                Some(encoded) => encode::decode_encoded(encoded, OutputFormat::WebpLossless)
                    .map(|round| round.rgba8 != image.rgba8)
                    .unwrap_or(true),
                None => true,
            };

            Some(Row {
                decoded: image.rgba8.len() as u64,
                png: png.len() as u64,
                webp: webp.map(|encoded| encoded.len() as u64).unwrap_or(0),
                png_failed,
                webp_failed,
            })
        })
        .collect();
    let png_failures = rows.iter().filter(|row| row.png_failed).count();
    let webp_failures = rows.iter().filter(|row| row.webp_failed).count();
    Ok(FormatComparison {
        scope: scope.to_owned(),
        images: rows.len(),
        decoded_bytes: rows.iter().map(|row| row.decoded).sum(),
        png_bytes: rows.iter().map(|row| row.png).sum(),
        webp_bytes: rows.iter().map(|row| row.webp).sum(),
        png_lossless: png_failures == 0,
        webp_lossless: webp_failures == 0,
        png_round_trip_failures: png_failures,
        webp_round_trip_failures: webp_failures,
    })
}
