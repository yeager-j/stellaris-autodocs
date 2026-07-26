//! Path A: `image_dds`.
//!
//! The entry point is [`decode`], which builds a single-layer, single-mip [`Surface`] from the
//! byte range the classifier already proved present. It deliberately does **not** call
//! `image_dds::image_from_dds`, which decodes every array layer and stacks them vertically —
//! for a cube map that silently yields an image six times too tall. [`decode_via_image_from_dds`]
//! is kept beside it so the recipe run can show that difference rather than assert it.
//!
//! The mapping from the application's [`SourceFormat`] to `image_dds::ImageFormat` is total over
//! the closed set, so the compiler rejects a new source format that nobody decided how to decode.
//! Uncompressed layouts are the exception: `image_dds` names a fixed set of them, and a layout
//! outside that set is a typed `UnsupportedFormat` — decided here, still before any decoding.

use image_dds::{ImageFormat, Surface};

use crate::classify::{Classification, Decodable, Layout, SourceFormat};
use crate::model::{DecodedImage, Outcome};
use crate::recipe::Recipe;

/// Whether this adapter can decode a source format at all.
///
/// The third of the three rules that decide an outcome before any decoding happens — after the
/// classifier's view of the container and the recipe's shape policy. All three are lookups over
/// what the file declares, which is what keeps `UnsupportedFormat` distinguishable from
/// `ConversionFailure` (`docs/technical-design.md:516`).
pub fn supports(format: SourceFormat) -> bool {
    image_format(format).is_some()
}

/// The `image_dds` format for a source format, or `None` when the library names no counterpart.
pub fn image_format(format: SourceFormat) -> Option<ImageFormat> {
    match format {
        SourceFormat::Bc1RgbaUnorm => Some(ImageFormat::BC1RgbaUnorm),
        SourceFormat::Bc1RgbaUnormSrgb => Some(ImageFormat::BC1RgbaUnormSrgb),
        SourceFormat::Bc2RgbaUnorm => Some(ImageFormat::BC2RgbaUnorm),
        SourceFormat::Bc2RgbaUnormSrgb => Some(ImageFormat::BC2RgbaUnormSrgb),
        SourceFormat::Bc3RgbaUnorm => Some(ImageFormat::BC3RgbaUnorm),
        SourceFormat::Bc3RgbaUnormSrgb => Some(ImageFormat::BC3RgbaUnormSrgb),
        SourceFormat::Bc4RUnorm => Some(ImageFormat::BC4RUnorm),
        SourceFormat::Bc5RgUnorm => Some(ImageFormat::BC5RgUnorm),
        SourceFormat::Bc7RgbaUnorm => Some(ImageFormat::BC7RgbaUnorm),
        SourceFormat::Bc7RgbaUnormSrgb => Some(ImageFormat::BC7RgbaUnormSrgb),
        SourceFormat::Uncompressed(layout) => uncompressed_format(layout),
    }
}

/// Named uncompressed layouts, matched by bit count and channel positions.
///
/// Matched structurally rather than by four-character code or DXGI enumerant, because the same
/// layout reaches this point through three different declarations — legacy masks, a D3D format,
/// and a DXGI enumerant — and they must not disagree about what the bytes mean.
fn uncompressed_format(layout: Layout) -> Option<ImageFormat> {
    let channels = (
        (layout.red.shift, layout.red.bits),
        (layout.green.shift, layout.green.bits),
        (layout.blue.shift, layout.blue.bits),
        layout.alpha.map(|channel| (channel.shift, channel.bits)),
    );
    match (layout.bit_count, channels) {
        (32, ((0, 8), (8, 8), (16, 8), Some((24, 8)))) => Some(ImageFormat::Rgba8Unorm),
        (32, ((16, 8), (8, 8), (0, 8), Some((24, 8)))) => Some(ImageFormat::Bgra8Unorm),
        (24, ((16, 8), (8, 8), (0, 8), None)) => Some(ImageFormat::Bgr8Unorm),
        (16, ((10, 5), (5, 5), (0, 5), Some((15, 1)))) => Some(ImageFormat::Bgr5A1Unorm),
        (16, ((8, 4), (4, 4), (0, 4), Some((12, 4)))) => Some(ImageFormat::Bgra4Unorm),
        _ => None,
    }
}

/// Decode mip 0 of layer 0 under the pinned recipe.
pub fn decode(bytes: &[u8], decodable: &Decodable) -> Outcome {
    let Some(format) = image_format(decodable.format) else {
        return Outcome::UnsupportedFormat {
            detail: format!(
                "image_dds names no format for {}",
                decodable.format.label()
            ),
        };
    };

    let surface = Surface {
        width: decodable.header.width,
        height: decodable.header.height,
        depth: 1,
        layers: 1,
        mipmaps: 1,
        image_format: format,
        data: &bytes[decodable.level0.clone()],
    };

    match surface.decode_rgba8() {
        Ok(decoded) => Outcome::Decoded(DecodedImage {
            width: decoded.width,
            height: decoded.height,
            rgba8: decoded.data,
        }),
        Err(error) => Outcome::ConversionFailure {
            detail: error.to_string(),
        },
    }
}

/// The complete adapter: classify, apply recipe policy, then decode.
///
/// Both refusals happen before any decoding, so every input has exactly one outcome and that
/// outcome is decided rather than discovered.
pub fn adapt(bytes: &[u8], recipe: &Recipe) -> Outcome {
    match crate::classify::classify(bytes) {
        Classification::Decodable(decodable) => match recipe.accepts(&decodable) {
            Ok(()) => decode(bytes, &decodable),
            Err(reason) => Outcome::UnsupportedFormat {
                detail: reason.to_string(),
            },
        },
        Classification::Malformed(reason) => Outcome::MalformedMedia {
            detail: reason.to_string(),
        },
        Classification::Unsupported { reason, .. } => Outcome::UnsupportedFormat {
            detail: reason.to_string(),
        },
    }
}

/// Decode every layer at once, the way `image_dds::image_from_dds` does.
///
/// Kept only so `d3-recipe` can measure what the convenience entry point produces for a cube map
/// instead of asserting it. The returned image is the library's vertical stack: `height` times
/// the layer count. This is not the recipe, and `adapt` never reaches it.
pub fn decode_all_layers(bytes: &[u8], decodable: &Decodable) -> Outcome {
    let Some(format) = image_format(decodable.format) else {
        return Outcome::UnsupportedFormat {
            detail: format!("image_dds names no format for {}", decodable.format.label()),
        };
    };
    let layers =
        decodable.header.array_size() * if decodable.header.is_cubemap() { 6 } else { 1 };
    let level = decodable
        .format
        .level_bytes(decodable.header.width, decodable.header.height);

    // Every layer's level 0 sits at a stride of that layer's full mip chain.
    let mut data = Vec::with_capacity(level * layers as usize);
    let chain = mip_chain_bytes(decodable);
    for layer in 0..layers as usize {
        let start = decodable.header.data_offset + layer * chain;
        match bytes.get(start..start + level) {
            Some(slice) => data.extend_from_slice(slice),
            None => {
                return Outcome::MalformedMedia {
                    detail: format!("layer {layer} level 0 is not present"),
                }
            }
        }
    }

    let surface = Surface {
        width: decodable.header.width,
        height: decodable.header.height,
        depth: 1,
        layers,
        mipmaps: 1,
        image_format: format,
        data: data.as_slice(),
    };
    match surface.decode_rgba8() {
        Ok(decoded) => Outcome::Decoded(DecodedImage {
            width: decoded.width,
            // What `into_image` would report: layers stacked vertically.
            height: decoded.height * layers,
            rgba8: decoded.data,
        }),
        Err(error) => Outcome::ConversionFailure {
            detail: error.to_string(),
        },
    }
}

/// Bytes one layer's complete mip chain occupies.
fn mip_chain_bytes(decodable: &Decodable) -> usize {
    (0..decodable.header.levels())
        .map(|level| {
            let width = (decodable.header.width >> level).max(1);
            let height = (decodable.header.height >> level).max(1);
            decodable.format.level_bytes(width, height)
        })
        .sum()
}
