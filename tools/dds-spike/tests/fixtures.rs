//! Every fixture's committed claim, checked against both decoders.
//!
//! These are the same functions `d5-failures` captures, so a green test run and a green record
//! cannot disagree about what passed.

use dds_spike::classify::{classify, Classification};
use dds_spike::corpus;
use dds_spike::decode_a;
use dds_spike::decode_b;
use dds_spike::fixtures::{self, Expected};
use dds_spike::model::{compare, Comparison, Outcome};
use dds_spike::recipe::{OutputFormat, Recipe};

fn recipe() -> Recipe {
    Recipe::pinned(OutputFormat::Png)
}

#[test]
fn every_fixture_on_disk_matches_its_generator() {
    let root = corpus::fixtures_root();
    for fixture in fixtures::all() {
        let expected = (fixture.bytes)();
        let actual = std::fs::read(root.join(fixture.path))
            .unwrap_or_else(|error| panic!("{}: {error}", fixture.path));
        assert_eq!(
            actual, expected,
            "{} on disk differs from the generator that describes it",
            fixture.path
        );
    }
}

#[test]
fn every_fixture_produces_the_outcome_it_claims() {
    let recipe = recipe();
    for fixture in fixtures::all() {
        let bytes = (fixture.bytes)();
        let outcome = decode_a::adapt(&bytes, &recipe);
        assert!(
            fixture.expected.matches(&outcome),
            "{}: claimed {}, got {} ({})\n  claim: {}",
            fixture.path,
            fixture.expected.kind(),
            outcome.kind(),
            outcome.detail(),
            fixture.claim
        );
    }
}

#[test]
fn both_paths_agree_on_every_valid_fixture() {
    let recipe = recipe();
    for fixture in fixtures::all() {
        if !matches!(fixture.expected, Expected::Decoded { .. }) {
            continue;
        }
        let bytes = (fixture.bytes)();
        let a = decode_a::adapt(&bytes, &recipe);
        let b = decode_b::adapt(&bytes, &recipe);
        assert_eq!(
            compare(&a, &b),
            Comparison::Identical,
            "{}: the two readings disagree\n  claim: {}",
            fixture.path,
            fixture.claim
        );
    }
}

/// Spec-derived expectations, where the fixture states them.
///
/// Two decoders agreeing shows they are not independently wrong. A value computed from the format
/// specification shows they are not jointly wrong, which agreement alone cannot.
#[test]
fn spec_derived_pixels_match_both_paths() {
    let recipe = recipe();
    for fixture in fixtures::all() {
        if fixture.expected_pixels.is_empty() {
            continue;
        }
        let bytes = (fixture.bytes)();
        for (label, outcome) in [
            ("image_dds", decode_a::adapt(&bytes, &recipe)),
            ("independent", decode_b::adapt(&bytes, &recipe)),
        ] {
            let Outcome::Decoded(image) = outcome else {
                panic!("{} did not decode through {label}", fixture.path);
            };
            for (index, expected) in fixture.expected_pixels.iter().enumerate() {
                let actual = &image.rgba8[index * 4..index * 4 + 4];
                assert_eq!(
                    actual,
                    expected.as_slice(),
                    "{} pixel {index} through {label}\n  claim: {}",
                    fixture.path,
                    fixture.claim
                );
            }
        }
    }
}

/// The mip fixture proves level 0 is selected rather than assumed.
#[test]
fn the_recipe_selects_the_base_mip_level() {
    let recipe = recipe();
    let fixture = fixtures::all()
        .into_iter()
        .find(|fixture| fixture.path == "valid/bgra8_mips_8x8.dds")
        .expect("the mip fixture exists");
    let bytes = (fixture.bytes)();

    let Outcome::Decoded(image) = decode_a::adapt(&bytes, &recipe) else {
        panic!("the mip fixture did not decode");
    };
    assert_eq!((image.width, image.height), (8, 8));
    // Every level is a different flat colour, so a wrong level shows up in the first pixel.
    assert_eq!(&image.rgba8[..4], &[0x10, 0x20, 0x30, 0xff]);
}

/// A negative control for the spec-derived pixel check itself.
///
/// If a corrupted expectation still passed, the check would be decorative.
#[test]
fn the_pixel_check_detects_a_wrong_expectation() {
    let recipe = recipe();
    let fixture = fixtures::all()
        .into_iter()
        .find(|fixture| fixture.path == "valid/dxt1_opaque_4x4.dds")
        .expect("the BC1 fixture exists");
    let bytes = (fixture.bytes)();
    let Outcome::Decoded(image) = decode_a::adapt(&bytes, &recipe) else {
        panic!("the BC1 fixture did not decode");
    };

    let mut wrong = fixture.expected_pixels[0];
    wrong[0] = wrong[0].wrapping_add(1);
    assert_ne!(
        &image.rgba8[..4],
        wrong.as_slice(),
        "a deliberately wrong expectation was accepted, so the check proves nothing"
    );
}

/// Malformed and unsupported must be decided before any decoding happens.
///
/// Three rules decide it, and all three are lookups over what the file declares: the classifier's
/// reading of the container, the recipe's shape policy, and the adapter's supported-format set.
/// None of them decodes. That is what keeps `UnsupportedFormat` distinguishable from
/// `ConversionFailure` — if both reduced to "the decoder returned Err", `analysis::finalize` could
/// not scope its issue correctly (`docs/technical-design.md:516`).
#[test]
fn malformed_and_unsupported_are_decided_before_decoding() {
    for fixture in fixtures::all() {
        let bytes = (fixture.bytes)();
        let classification = classify(&bytes);
        match fixture.expected {
            Expected::MalformedMedia => assert!(
                matches!(classification, Classification::Malformed(_)),
                "{} must be classified malformed without decoding",
                fixture.path
            ),
            Expected::UnsupportedFormat => {
                let refused = match &classification {
                    Classification::Unsupported { .. } => true,
                    Classification::Decodable(decodable) => {
                        recipe().accepts(decodable).is_err()
                            || !decode_a::supports(decodable.format)
                    }
                    Classification::Malformed(_) => false,
                };
                assert!(
                    refused,
                    "{} must be refused by the container reading, the recipe policy, or the \
                     supported-format set — not by the decoder",
                    fixture.path
                );
            }
            Expected::Decoded { .. } => {}
        }
    }
}

/// No fixture may reach the decoder and fail there.
///
/// `ConversionFailure` indicts the adapter rather than the input, so any fixture producing it
/// would mean one of the three pre-decode rules let something through that it should have caught.
#[test]
fn no_fixture_produces_a_conversion_failure() {
    let recipe = recipe();
    for fixture in fixtures::all() {
        let bytes = (fixture.bytes)();
        let outcome = decode_a::adapt(&bytes, &recipe);
        assert!(
            !matches!(outcome, Outcome::ConversionFailure { .. }),
            "{} reached the decoder and failed there: {}",
            fixture.path,
            outcome.detail()
        );
    }
}
