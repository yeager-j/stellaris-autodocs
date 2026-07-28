//! The conversion recipe and the asset key derived from it.
//!
//! `docs/technical-design.md:503` defines the key as source bytes plus recipe version plus output
//! format plus conversion parameters. That only holds if every choice which can change the output
//! is a field here. A parameter with a default but no field is a decision with no home: it will
//! change one day, the key will not, and two different images will share an address in a
//! content-addressed store.
//!
//! Two of these fields are policy rather than mechanism — [`LayerPolicy`] and the premultiplied
//! four-character codes refused in `classify` — and both refuse rather than guess. Refusing costs
//! a placeholder on inputs the pinned corpora do not contain. Guessing costs a wrong image on
//! inputs nobody would think to check.

use crate::classify::{Decodable, Unsupported};
use crate::digest::sha256;
use serde::{Deserialize, Serialize};

/// Which mip level to materialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MipSelection {
    /// Level 0. The only level every input is guaranteed to have: 13,607 vanilla files declare no
    /// mip chain at all. Selecting by target size would also make the key depend on a display
    /// decision, which belongs to the frontend and not to an immutable blob's identity.
    Base,
}

/// How many array layers the recipe will materialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerPolicy {
    /// Exactly one. Anything else is [`Unsupported::MultipleLayers`].
    SingleLayer,
}

/// What the recipe asserts about colour, and what it does about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Colorspace {
    /// Treat stored values as sRGB-encoded and apply no transfer function; write no `gAMA`,
    /// `sRGB`, or `iCCP` chunk, so the output is untagged and browsers read it as sRGB.
    ///
    /// A declaration rather than a conversion. Legacy DDS carries no colorspace flag, and
    /// `image_dds` dispatches its `*Unorm` and `*UnormSrgb` formats to the same u8 decode, so no
    /// transform happens either way. `d3-recipe` asserts that byte-for-byte rather than reading
    /// it off the source, which also makes it a regression detector for a future version bump.
    SrgbDeclaredNoTransform,
}

/// How alpha is carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlphaPolicy {
    /// Straight, never premultiplied.
    Straight,
}

/// The browser-safe output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputFormat {
    Png,
    WebpLossless,
}

impl OutputFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::WebpLossless => "webp-lossless",
        }
    }

    pub fn media_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::WebpLossless => "image/webp",
        }
    }
}

/// The encoder, named at the version that produced the bytes.
///
/// Present in the key because the same pixels encode to different bytes under a different
/// encoder or a different setting. `d3-recipe` demonstrates this directly rather than asserting
/// it: one decoded image, several settings, several distinct output digests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncoderIdentity {
    pub crate_name: String,
    pub crate_version: String,
    pub settings: String,
}

/// One complete, versioned conversion recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recipe {
    /// Changes whenever any other field changes semantically, and therefore changes every
    /// derived asset key.
    ///
    /// Not a hand-maintained integer alone: the decoder's own transitive dependency decides
    /// pixels — `image_dds` decodes BC through `bcdec_rs` — so a bump there changes output with
    /// no first-party change. `decoder` below carries the resolved versions into the key.
    pub version: u32,
    pub decoder: DecoderIdentity,
    pub mip: MipSelection,
    pub layers: LayerPolicy,
    pub colorspace: Colorspace,
    pub alpha: AlphaPolicy,
    pub output: OutputFormat,
    pub encoder: EncoderIdentity,
    /// Upper bound on decoded pixels, so one pathological source cannot allocate without limit.
    pub max_decoded_pixels: usize,
}

/// The decoder versions that decide output pixels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderIdentity {
    pub image_dds: String,
    pub bcdec_rs: String,
}

/// Versions duplicated from the lock file into code, as `analysis::parser::conformance::record` does with
/// Jomini, so a record cannot claim a version the harness did not link against.
pub const IMAGE_DDS_VERSION: &str = "0.7.2";
pub const BCDEC_RS_VERSION: &str = "0.2.0";
pub const TEXTURE2DDECODER_VERSION: &str = "0.1.2";
pub const PNG_VERSION: &str = "0.18.1";
pub const IMAGE_WEBP_VERSION: &str = "0.2.4";

/// 4096x4096. Above every referenced texture in the pinned corpora with headroom, and far above
/// the icon sizes the documentation actually materializes. `d3-recipe` reports the distribution
/// this was chosen against.
pub const MAX_DECODED_PIXELS: usize = 4096 * 4096;

impl Recipe {
    /// The pinned recipe.
    pub fn pinned(output: OutputFormat) -> Self {
        Self {
            version: 1,
            decoder: DecoderIdentity {
                image_dds: IMAGE_DDS_VERSION.into(),
                bcdec_rs: BCDEC_RS_VERSION.into(),
            },
            mip: MipSelection::Base,
            layers: LayerPolicy::SingleLayer,
            colorspace: Colorspace::SrgbDeclaredNoTransform,
            alpha: AlphaPolicy::Straight,
            output,
            encoder: match output {
                OutputFormat::Png => EncoderIdentity {
                    crate_name: "png".into(),
                    crate_version: PNG_VERSION.into(),
                    settings: "compression=balanced;filter=adaptive;no-ancillary-chunks".into(),
                },
                OutputFormat::WebpLossless => EncoderIdentity {
                    crate_name: "image-webp".into(),
                    crate_version: IMAGE_WEBP_VERSION.into(),
                    settings: "lossless".into(),
                },
            },
            max_decoded_pixels: MAX_DECODED_PIXELS,
        }
    }

    /// Apply the recipe's shape policy to a container the classifier proved readable.
    ///
    /// Separate from `classify` on purpose. The classifier states what the bytes are; the recipe
    /// states what this application converts. Keeping them apart is what lets the record show
    /// that a refusal was a decision rather than a decoding limit — and both still happen before
    /// any decoder runs, so the outcome remains decided rather than discovered.
    pub fn accepts(&self, decodable: &Decodable) -> Result<(), Unsupported> {
        let LayerPolicy::SingleLayer = self.layers;
        let layers = decodable.header.array_size() * if decodable.header.is_cubemap() { 6 } else { 1 };
        if layers != 1 {
            return Err(Unsupported::MultipleLayers { layers });
        }

        let pixels = decodable.header.width as usize * decodable.header.height as usize;
        if pixels > self.max_decoded_pixels {
            return Err(Unsupported::TooLarge {
                pixels,
                limit: self.max_decoded_pixels,
            });
        }
        Ok(())
    }

    /// The canonical encoding that enters the key.
    ///
    /// Field order is the struct's declaration order and is fixed by this function, not by a
    /// serializer's incidental behaviour (`docs/technical-design.md:316`).
    pub fn canonical(&self) -> String {
        format!(
            "v{}\nimage_dds={}\nbcdec_rs={}\nmip={:?}\nlayers={:?}\ncolorspace={:?}\nalpha={:?}\noutput={}\nencoder={} {} {}\nmax_decoded_pixels={}\n",
            self.version,
            self.decoder.image_dds,
            self.decoder.bcdec_rs,
            self.mip,
            self.layers,
            self.colorspace,
            self.alpha,
            self.output.label(),
            self.encoder.crate_name,
            self.encoder.crate_version,
            self.encoder.settings,
            self.max_decoded_pixels,
        )
    }
}

/// Domain separator, so an asset key can never collide with another digest in this application.
const ASSET_KEY_DOMAIN: &str = "stellaris-docs/asset-key/v1\n";

/// The content-addressed key for one converted asset.
pub fn asset_key(source_bytes: &[u8], recipe: &Recipe) -> String {
    let mut material = String::from(ASSET_KEY_DOMAIN);
    material.push_str(&sha256(source_bytes));
    material.push('\n');
    material.push_str(&recipe.canonical());
    sha256(material.as_bytes())
}
