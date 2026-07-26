//! Browser-safe output encoding, and the round trip that proves it lossless.
//!
//! Both encoders are named dependencies at pinned versions rather than defaults reached through
//! a re-exporter, because the encoder participates in the asset key
//! (`docs/technical-design.md:503`). No ancillary chunk carrying a timestamp is written: an
//! output whose bytes depend on when it was produced cannot be content-addressed.
//!
//! Losslessness is measured, not assumed. Lossless WebP is a distinct code path from lossy, and
//! a mistake there is invisible in a 52x52 icon and fatal to a content-addressed store.

use crate::model::DecodedImage;
use crate::recipe::OutputFormat;

#[derive(Debug)]
pub enum EncodeError {
    Png(String),
    Webp(String),
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Png(detail) => write!(f, "png encode failed: {detail}"),
            Self::Webp(detail) => write!(f, "webp encode failed: {detail}"),
        }
    }
}

pub fn encode(image: &DecodedImage, format: OutputFormat) -> Result<Vec<u8>, EncodeError> {
    match format {
        OutputFormat::Png => encode_png(image),
        OutputFormat::WebpLossless => encode_webp(image),
    }
}

/// The pinned PNG settings, named explicitly rather than left to the crate's defaults.
///
/// `png`'s own documentation states that "the implementation details of DEFLATE compression may
/// evolve over time, even without a semver-breaking change to the version of `png` crate". That
/// is the recipe-version problem in the encoder's own words: identical pixels, identical declared
/// settings, and output bytes that may still move under a patch bump. Naming the settings does
/// not fix it; it is why the encoder version belongs in the asset key and why `verify` compares
/// it.
pub const PINNED_COMPRESSION: png::Compression = png::Compression::Balanced;
pub const PINNED_FILTER: png::Filter = png::Filter::Adaptive;

pub fn encode_png(image: &DecodedImage) -> Result<Vec<u8>, EncodeError> {
    encode_png_with(image, PINNED_COMPRESSION, PINNED_FILTER)
}

/// PNG at an explicit compression and filter.
///
/// Exposed with both settings open so `d3-recipe` can encode one identical decoded image several
/// ways and record several distinct digests. That table is the evidence that pixel equality is
/// not key equality, which is why the encoder is in the recipe at all.
pub fn encode_png_with(
    image: &DecodedImage,
    compression: png::Compression,
    filter: png::Filter,
) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(compression);
        encoder.set_filter(filter);
        let mut writer = encoder
            .write_header()
            .map_err(|error| EncodeError::Png(error.to_string()))?;
        writer
            .write_image_data(&image.rgba8)
            .map_err(|error| EncodeError::Png(error.to_string()))?;
        writer
            .finish()
            .map_err(|error| EncodeError::Png(error.to_string()))?;
    }
    Ok(out)
}

pub fn encode_webp(image: &DecodedImage) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::new();
    image_webp::WebPEncoder::new(&mut out)
        .encode(
            &image.rgba8,
            image.width,
            image.height,
            image_webp::ColorType::Rgba8,
        )
        .map_err(|error| EncodeError::Webp(error.to_string()))?;
    Ok(out)
}

/// Decode an encoded output back to RGBA8, so the round trip can be compared with the input.
pub fn decode_encoded(bytes: &[u8], format: OutputFormat) -> Result<DecodedImage, EncodeError> {
    match format {
        OutputFormat::Png => {
            let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
            let mut reader = decoder
                .read_info()
                .map_err(|error| EncodeError::Png(error.to_string()))?;
            let mut buffer = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
            let info = reader
                .next_frame(&mut buffer)
                .map_err(|error| EncodeError::Png(error.to_string()))?;
            buffer.truncate(info.buffer_size());
            Ok(DecodedImage {
                width: info.width,
                height: info.height,
                rgba8: buffer,
            })
        }
        OutputFormat::WebpLossless => {
            let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes))
                .map_err(|error| EncodeError::Webp(error.to_string()))?;
            let (width, height) = decoder.dimensions();
            let mut buffer = vec![0u8; decoder.output_buffer_size().unwrap_or(0)];
            decoder
                .read_image(&mut buffer)
                .map_err(|error| EncodeError::Webp(error.to_string()))?;
            Ok(DecodedImage {
                width,
                height,
                rgba8: buffer,
            })
        }
    }
}
