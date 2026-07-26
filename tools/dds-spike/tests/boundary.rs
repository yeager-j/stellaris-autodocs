//! The independence gate.
//!
//! Two things must stay true for the cross-check to mean anything, and neither is enforceable by
//! the type system:
//!
//! 1. The application-owned modules — the container reader, the classifier, the model — must not
//!    name a decoder. If `header.rs` used `ddsfile`, classification would inherit the decoder's
//!    own view of the container, and "malformed" and "unsupported" would collapse into "the
//!    decoder said no" (`docs/technical-design.md:516`).
//! 2. Path B must not name `image_dds`. Two readings that share a library agree about its bugs.
//!
//! A search that silently stopped matching would pass both assertions while checking nothing, so
//! each has a negative control asserting that the same search still finds the dependency where one
//! genuinely exists.

use std::path::PathBuf;

fn source(name: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "src", name].iter().collect();
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Lines that are neither blank nor a `//` comment, so prose about a crate is not a use of it.
fn code_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(str::trim_start)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
}

/// Whether the source depends on a crate, as against merely naming it.
///
/// Path-qualified use — `use image_dds::…` or `image_dds::…` — is what constitutes a dependency.
/// A bare substring match is not: `recipe.rs` legitimately carries `image_dds: String`, the
/// decoder version that participates in the asset key, and a gate that failed on that would be
/// demanding the recipe forget which decoder produced its pixels. The distinction is the point of
/// the gate, so it is drawn here rather than worked around by excluding a file.
fn mentions(text: &str, needle: &str) -> bool {
    let qualified = format!("{needle}::");
    code_lines(text).any(|line| line.contains(&qualified))
}

/// The modules that define what this application means by a DDS.
const APPLICATION_OWNED: &[&str] = &["header.rs", "classify.rs", "model.rs", "recipe.rs"];

#[test]
fn application_owned_modules_name_no_decoder() {
    for module in APPLICATION_OWNED {
        let text = source(module);
        for decoder in ["image_dds", "ddsfile", "texture2ddecoder", "bcdec"] {
            assert!(
                !mentions(&text, decoder),
                "src/{module} names {decoder} in code. The container reading and the outcome \
                 classification must not inherit a decoder's view of the format."
            );
        }
    }
}

#[test]
fn the_independent_path_does_not_name_image_dds() {
    let text = source("decode_b.rs");
    for forbidden in ["image_dds", "ddsfile"] {
        assert!(
            !mentions(&text, forbidden),
            "src/decode_b.rs names {forbidden}. Path B exists to read the same bytes without \
             image_dds; sharing it would make the cross-check an assertion that a library agrees \
             with itself."
        );
    }
}

/// The negative control for both searches above.
///
/// If `mentions` stopped matching — a changed comment convention, a renamed import — every
/// assertion above would pass while checking nothing. These are the two places the dependencies
/// genuinely live, so the same search must find them.
#[test]
fn the_search_detects_a_dependency_where_one_exists() {
    assert!(
        mentions(&source("decode_a.rs"), "image_dds"),
        "src/decode_a.rs no longer names image_dds, so the searches above prove nothing"
    );
    assert!(
        mentions(&source("decode_b.rs"), "texture2ddecoder"),
        "src/decode_b.rs no longer names texture2ddecoder, so the searches above prove nothing"
    );
}

/// Further controls: the filter must be loose enough to catch a real import and tight enough not
/// to fire on prose or on a version string.
#[test]
fn the_search_distinguishes_a_dependency_from_a_mention() {
    assert!(
        !mentions(
            "// image_dds decodes BC through bcdec_rs.\nlet value = 1;\n",
            "image_dds"
        ),
        "a commented mention was counted as code, so the gate would fire on documentation"
    );
    assert!(
        !mentions("image_dds: IMAGE_DDS_VERSION.into(),", "image_dds"),
        "a recorded version string was counted as a dependency"
    );
    assert!(
        mentions("use image_dds::Surface;", "image_dds"),
        "an actual import was not counted, so the gate would never fire"
    );
    assert!(
        mentions("let format = image_dds::ImageFormat::Rgba8Unorm;", "image_dds"),
        "a path-qualified use was not counted, so the gate would never fire"
    );
}
