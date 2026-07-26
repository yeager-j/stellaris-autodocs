//! Path B: the independent reading.
//!
//! Independence is the point, so it is structural rather than asserted:
//!
//! - **Uncompressed classes** are reinterpreted straight from the `DDS_PIXELFORMAT` masks that
//!   `header.rs` parsed. No library is involved at all. This is the reading that catches a
//!   BGR/RGB swap, a dropped alpha mask, or an `X8R8G8B8` high byte read as alpha — defects that
//!   produce a perfectly plausible image and therefore survive visual inspection.
//! - **BC1, BC2, and BC3** are decoded from the S3TC specification, below. That is a stronger
//!   independent reading than a second library, because it is the statement both libraries are
//!   trying to implement — and the corpus proved the point: `texture2ddecoder`, the second
//!   library this path originally used, decodes BC3 in a way the specification forbids. See
//!   [`block_decoder`].
//! - **BC4, BC5, and BC7** still go through `texture2ddecoder`. The pinned corpora contain none
//!   of them, so that support is claimed but unexercised, and the record says so rather than
//!   letting an untested path pass as a measured one.
//!
//! The block iteration is written here rather than taken from any crate's whole-image helper,
//! because clipping partial blocks at a surface whose dimensions are not multiples of four is one
//! of the questions this spike has to answer, and 844 files in the pinned corpora ask it.

use texture2ddecoder::{decode_bc4_block, decode_bc5_block, decode_bc7_block};

use crate::classify::{Channel, Classification, Decodable, Layout, SourceFormat};
use crate::model::{DecodedImage, Outcome};
use crate::recipe::Recipe;

/// Decode mip 0 of layer 0, independently of `decode_a`.
pub fn decode(bytes: &[u8], decodable: &Decodable) -> Outcome {
    let width = decodable.header.width;
    let height = decodable.header.height;
    let data = &bytes[decodable.level0.clone()];

    let rgba8 = match decodable.format {
        SourceFormat::Uncompressed(layout) => uncompressed(data, width, height, layout),
        format => match block_decoder(format) {
            Some(decoder) => blocks(data, width, height, format, decoder),
            None => {
                return Outcome::UnsupportedFormat {
                    detail: format!("no independent decoder for {}", format.label()),
                }
            }
        },
    };

    Outcome::Decoded(DecodedImage {
        width,
        height,
        rgba8,
    })
}

/// The complete independent adapter: classify, apply recipe policy, then decode.
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

/// Ignore the declared masks and read every 32-bit surface as the majority layout.
///
/// The cross-check's second negative control, and the one that proves the corpus can discriminate
/// at all. This is what a decoder keyed on bit count rather than on masks does — the most likely
/// way to be wrong here, and the way that produces a plausible image. If no file diverged under
/// it, the corpus would contain nothing capable of separating a mask-reading decoder from a
/// table-reading one, and every agreement number in this spike would be measuring a check that
/// could not have failed.
pub fn decode_assuming_majority_layout(bytes: &[u8], decodable: &Decodable) -> Outcome {
    let SourceFormat::Uncompressed(layout) = decodable.format else {
        return Outcome::UnsupportedFormat {
            detail: "the layout assumption applies only to uncompressed surfaces".into(),
        };
    };
    if layout.bit_count != 32 {
        return Outcome::UnsupportedFormat {
            detail: "the majority layout is a 32-bit one".into(),
        };
    }
    // A8R8G8B8: what 20,834 of the corpus's 20,845 32-bit surfaces declare.
    let assumed = Layout {
        bit_count: 32,
        red: Channel { shift: 16, bits: 8 },
        green: Channel { shift: 8, bits: 8 },
        blue: Channel { shift: 0, bits: 8 },
        alpha: Some(Channel { shift: 24, bits: 8 }),
    };
    Outcome::Decoded(DecodedImage {
        width: decodable.header.width,
        height: decodable.header.height,
        rgba8: uncompressed(
            &bytes[decodable.level0.clone()],
            decodable.header.width,
            decodable.header.height,
            assumed,
        ),
    })
}

/// Read the red channel where blue belongs, and blue where red belongs.
///
/// The cross-check's primary negative control. `d2-decode --inject swap-rb` runs the whole corpus
/// through this and asserts the shape of the failure rather than merely that it is non-zero: every
/// uncompressed file whose red and blue channels are not already equal in every pixel must
/// diverge, and 20,567 of the 22,346 uncompressed files do. The remaining 1,779 are greyscale
/// icons, masks, and flat backgrounds, which no channel-order fault can change; counting them as
/// misses would make the control fail for a reason unrelated to the check's sensitivity.
pub fn decode_with_swapped_red_blue(bytes: &[u8], decodable: &Decodable) -> Outcome {
    let SourceFormat::Uncompressed(layout) = decodable.format else {
        return Outcome::UnsupportedFormat {
            detail: "the swap injection applies only to uncompressed layouts".into(),
        };
    };
    let swapped = Layout {
        red: layout.blue,
        blue: layout.red,
        ..layout
    };
    Outcome::Decoded(DecodedImage {
        width: decodable.header.width,
        height: decodable.header.height,
        rgba8: uncompressed(
            &bytes[decodable.level0.clone()],
            decodable.header.width,
            decodable.header.height,
            swapped,
        ),
    })
}

/// Reinterpret packed pixels through their declared masks.
fn uncompressed(data: &[u8], width: u32, height: u32, layout: Layout) -> Vec<u8> {
    let stride = (layout.bit_count / 8) as usize;
    // DDS rows of uncompressed data are tightly packed at `width * bytes_per_pixel`; the
    // header's pitch field is advisory and is wrong often enough in real files that reading it
    // would import someone else's arithmetic error.
    let row_bytes = width as usize * stride;
    let mut rgba8 = Vec::with_capacity(width as usize * height as usize * 4);

    for y in 0..height as usize {
        for x in 0..width as usize {
            let start = y * row_bytes + x * stride;
            let mut packed = 0u32;
            for (index, byte) in data[start..start + stride].iter().enumerate() {
                packed |= (*byte as u32) << (8 * index);
            }
            rgba8.push(layout.red.extract(packed));
            rgba8.push(layout.green.extract(packed));
            rgba8.push(layout.blue.extract(packed));
            rgba8.push(layout.alpha.map_or(255, |channel| channel.extract(packed)));
        }
    }
    rgba8
}

type BlockDecoder = fn(&[u8], &mut [u32]);

/// BC1, BC2, and BC3 are decoded here from the S3TC specification rather than through
/// `texture2ddecoder`, because the corpus showed that using it for BC2 and BC3 is wrong.
///
/// `texture2ddecoder::decode_bc3_block` forwards its colour half to `decode_bc1_block`, which
/// still selects BC1's three-colour mode when `c0 <= c1`. The BC2 and BC3 specifications forbid
/// that: their colour block always uses the four-colour interpolation, because alpha is carried
/// separately and the endpoint ordering therefore has no second meaning. `bcdec_rs` implements
/// the rule (`color_block(..., only_opaque_mode = true)`); `texture2ddecoder` does not.
///
/// The disagreement is not theoretical. 22 files in the pinned corpora contain BC3 blocks with
/// `c0 <= c1`, and on them the two readings differed by up to 255 in a channel — found by the
/// corpus, and by nothing else, since no fixture would have been written to contain a block
/// ordering the spec says is meaningless.
///
/// So the three classes the corpus actually contains are read from the spec, which is a stronger
/// independent reading than a second library anyway: it is the statement the libraries are both
/// trying to implement. `texture2ddecoder` is retained for BC4, BC5, and BC7, which the pinned
/// corpora do not contain and which are therefore claimed but unexercised.
fn block_decoder(format: SourceFormat) -> Option<BlockDecoder> {
    match format {
        SourceFormat::Bc1RgbaUnorm | SourceFormat::Bc1RgbaUnormSrgb => Some(bc1_block),
        SourceFormat::Bc2RgbaUnorm | SourceFormat::Bc2RgbaUnormSrgb => Some(bc2_block),
        SourceFormat::Bc3RgbaUnorm | SourceFormat::Bc3RgbaUnormSrgb => Some(bc3_block),
        SourceFormat::Bc4RUnorm => Some(decode_bc4_block),
        SourceFormat::Bc5RgUnorm => Some(decode_bc5_block),
        SourceFormat::Bc7RgbaUnorm | SourceFormat::Bc7RgbaUnormSrgb => Some(decode_bc7_block),
        SourceFormat::Uncompressed(_) => None,
    }
}

/// Pack into `texture2ddecoder`'s pixel convention so both paths share one block layout.
fn pixel(red: u8, green: u8, blue: u8, alpha: u8) -> u32 {
    u32::from_le_bytes([blue, green, red, alpha])
}

/// Expand a 5:6:5 endpoint to 8 bits per channel by bit replication.
fn rgb565(value: u16) -> (u8, u8, u8) {
    let red = ((value >> 11) & 0x1f) as u8;
    let green = ((value >> 5) & 0x3f) as u8;
    let blue = (value & 0x1f) as u8;
    (
        (red << 3) | (red >> 2),
        (green << 2) | (green >> 4),
        (blue << 3) | (blue >> 2),
    )
}

/// The shared colour half of BC1, BC2, and BC3.
///
/// `punch_through` is true only for BC1: it is what lets `c0 <= c1` mean "index 3 is transparent"
/// rather than "interpolate differently".
fn color_block(data: &[u8], outbuf: &mut [u32], punch_through: bool) {
    let c0 = u16::from_le_bytes([data[0], data[1]]);
    let c1 = u16::from_le_bytes([data[2], data[3]]);
    let (r0, g0, b0) = rgb565(c0);
    let (r1, g1, b1) = rgb565(c1);

    let third = |a: u8, b: u8| (((2 * a as u32 + b as u32) + 1) / 3) as u8;
    let half = |a: u8, b: u8| (((a as u32 + b as u32) + 1) / 2) as u8;

    let mut colors = [
        pixel(r0, g0, b0, 255),
        pixel(r1, g1, b1, 255),
        0u32,
        0u32,
    ];
    if c0 > c1 || !punch_through {
        colors[2] = pixel(third(r0, r1), third(g0, g1), third(b0, b1), 255);
        colors[3] = pixel(third(r1, r0), third(g1, g0), third(b1, b0), 255);
    } else {
        colors[2] = pixel(half(r0, r1), half(g0, g1), half(b0, b1), 255);
        colors[3] = pixel(0, 0, 0, 0);
    }

    let mut indices = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    for slot in outbuf.iter_mut().take(16) {
        *slot = colors[(indices & 0b11) as usize];
        indices >>= 2;
    }
}

/// Overwrite the alpha channel of an already-decoded block.
fn set_alpha(outbuf: &mut [u32], index: usize, alpha: u8) {
    let [blue, green, red, _] = outbuf[index].to_le_bytes();
    outbuf[index] = u32::from_le_bytes([blue, green, red, alpha]);
}

fn bc1_block(data: &[u8], outbuf: &mut [u32]) {
    color_block(data, outbuf, true);
}

fn bc2_block(data: &[u8], outbuf: &mut [u32]) {
    color_block(&data[8..], outbuf, false);
    // Four explicit bits per pixel, expanded by multiplying by 17 so 0xF becomes 0xFF.
    for index in 0..16 {
        let nibble = (data[index / 2] >> ((index % 2) * 4)) & 0x0f;
        set_alpha(outbuf, index, nibble * 17);
    }
}

fn bc3_block(data: &[u8], outbuf: &mut [u32]) {
    color_block(&data[8..], outbuf, false);

    let (a0, a1) = (data[0], data[1]);
    let mut alpha = [0u8; 8];
    alpha[0] = a0;
    alpha[1] = a1;
    if a0 > a1 {
        for step in 1..7u32 {
            alpha[1 + step as usize] =
                (((7 - step) * a0 as u32 + step * a1 as u32 + 3) / 7) as u8;
        }
    } else {
        for step in 1..5u32 {
            alpha[1 + step as usize] =
                (((5 - step) * a0 as u32 + step * a1 as u32 + 2) / 5) as u8;
        }
        alpha[6] = 0;
        alpha[7] = 255;
    }

    let bits = u64::from_le_bytes([
        data[2], data[3], data[4], data[5], data[6], data[7], 0, 0,
    ]);
    for index in 0..16 {
        let selector = ((bits >> (3 * index)) & 0b111) as usize;
        set_alpha(outbuf, index, alpha[selector]);
    }
}

/// Walk 4x4 blocks in DDS order, clipping the partial blocks at the right and bottom edges.
fn blocks(
    data: &[u8],
    width: u32,
    height: u32,
    format: SourceFormat,
    decoder: BlockDecoder,
) -> Vec<u8> {
    let block_bytes = format
        .block_bytes()
        .expect("block_decoder returned Some for an uncompressed format");
    let blocks_wide = width.div_ceil(4).max(1) as usize;
    let blocks_high = height.div_ceil(4).max(1) as usize;
    let (width, height) = (width as usize, height as usize);

    let mut rgba8 = vec![0u8; width * height * 4];
    let mut block = [0u32; 16];

    for block_y in 0..blocks_high {
        for block_x in 0..blocks_wide {
            let offset = (block_y * blocks_wide + block_x) * block_bytes;
            decoder(&data[offset..offset + block_bytes], &mut block);

            for row in 0..4 {
                let y = block_y * 4 + row;
                if y >= height {
                    break;
                }
                for column in 0..4 {
                    let x = block_x * 4 + column;
                    if x >= width {
                        break;
                    }
                    // texture2ddecoder packs a pixel as `u32::from_le_bytes([b, g, r, a])`.
                    let [blue, green, red, alpha] = block[row * 4 + column].to_le_bytes();
                    let start = (y * width + x) * 4;
                    rgba8[start] = red;
                    rgba8[start + 1] = green;
                    rgba8[start + 2] = blue;
                    rgba8[start + 3] = alpha;
                }
            }
        }
    }
    rgba8
}
