//! The fixture set, as data, with each file's claim and predicted outcome beside its bytes.
//!
//! `fixtures/parser/malformed/*.txt` states its prediction in a header comment, written before the
//! measurement so a surprising number is a finding rather than a retrofitted expectation. A binary
//! fixture cannot carry a comment, so the discipline moves here: every fixture is a pure function
//! of a [`Fixture`] whose `claim` and `expected` are committed source, and `generate --check`
//! regenerates each file and compares it byte for byte with what is on disk. A fixture therefore
//! cannot drift from the statement that describes it.
//!
//! Headers are written from the DDS specification rather than through `image_dds`, deliberately:
//! a fixture produced by the library under test is a snapshot of that library, not an independent
//! statement about what the format means.

use crate::model::Outcome;

/// What a fixture claims about itself, written before it was ever decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expected {
    /// Decodes cleanly through both paths to these dimensions.
    Decoded { width: u32, height: u32 },
    /// Not a readable container.
    MalformedMedia,
    /// A well-formed container the recipe refuses.
    UnsupportedFormat,
}

impl Expected {
    pub fn matches(&self, outcome: &Outcome) -> bool {
        match (self, outcome) {
            (Self::Decoded { width, height }, Outcome::Decoded(image)) => {
                image.width == *width && image.height == *height && image.is_well_formed()
            }
            (Self::MalformedMedia, Outcome::MalformedMedia { .. }) => true,
            (Self::UnsupportedFormat, Outcome::UnsupportedFormat { .. }) => true,
            _ => false,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Decoded { .. } => "decoded",
            Self::MalformedMedia => "malformed-media",
            Self::UnsupportedFormat => "unsupported-format",
        }
    }
}

pub struct Fixture {
    /// Path under `fixtures/assets/dds/`.
    pub path: &'static str,
    /// The one thing this file exists to establish.
    pub claim: &'static str,
    pub expected: Expected,
    /// Spec-derived ground truth for the top-left pixels, where it is computable by hand.
    ///
    /// Two decoders agreeing shows they are not independently wrong. A value derived from the
    /// format specification shows they are not *jointly* wrong, which agreement alone cannot.
    pub expected_pixels: &'static [[u8; 4]],
    pub bytes: fn() -> Vec<u8>,
}

// ---------------------------------------------------------------------------------------------
// Header construction, from the DDS specification.
// ---------------------------------------------------------------------------------------------

const DDSD_CAPS: u32 = 0x1;
const DDSD_HEIGHT: u32 = 0x2;
const DDSD_WIDTH: u32 = 0x4;
const DDSD_PIXELFORMAT: u32 = 0x1000;
const DDSD_MIPMAPCOUNT: u32 = 0x2_0000;
const DDSCAPS_TEXTURE: u32 = 0x1000;

struct HeaderSpec {
    width: u32,
    height: u32,
    mip_count: u32,
    pixel_flags: u32,
    four_cc: [u8; 4],
    bit_count: u32,
    masks: [u32; 4],
    caps2: u32,
    dx10: Option<[u32; 5]>,
}

impl HeaderSpec {
    fn write(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(148);
        out.extend_from_slice(b"DDS ");
        let mut push = |value: u32| out.extend_from_slice(&value.to_le_bytes());
        push(124);
        push(DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT | DDSD_MIPMAPCOUNT);
        push(self.height);
        push(self.width);
        push(0); // pitch: advisory, and deliberately left zero so nothing can read it
        push(0); // depth
        push(self.mip_count);
        for _ in 0..11 {
            push(0); // reserved1
        }
        push(32); // DDS_PIXELFORMAT.dwSize
        push(self.pixel_flags);
        out.extend_from_slice(&self.four_cc);
        let mut push = |value: u32| out.extend_from_slice(&value.to_le_bytes());
        push(self.bit_count);
        for mask in self.masks {
            push(mask);
        }
        push(DDSCAPS_TEXTURE);
        push(self.caps2);
        push(0); // caps3
        push(0); // caps4
        push(0); // reserved2
        if let Some(dx10) = self.dx10 {
            for value in dx10 {
                push(value);
            }
        }
        out
    }
}

fn uncompressed_header(width: u32, height: u32, bit_count: u32, masks: [u32; 4]) -> HeaderSpec {
    const DDPF_ALPHAPIXELS: u32 = 0x1;
    const DDPF_RGB: u32 = 0x40;
    HeaderSpec {
        width,
        height,
        mip_count: 1,
        pixel_flags: DDPF_RGB | if masks[3] != 0 { DDPF_ALPHAPIXELS } else { 0 },
        four_cc: [0; 4],
        bit_count,
        masks,
        caps2: 0,
        dx10: None,
    }
}

fn four_cc_header(width: u32, height: u32, code: &[u8; 4]) -> HeaderSpec {
    const DDPF_FOURCC: u32 = 0x4;
    HeaderSpec {
        width,
        height,
        mip_count: 1,
        pixel_flags: DDPF_FOURCC,
        four_cc: *code,
        bit_count: 0,
        masks: [0; 4],
        caps2: 0,
        dx10: None,
    }
}

/// The four-pixel colour pattern every uncompressed fixture stores, in RGBA.
///
/// Chosen so red, green, and blue are all distinct and none is zero: a channel swap, a dropped
/// channel, and a mask misread each produce a different visible result. A grey or symmetric
/// pattern would survive all three.
const PATTERN: [[u8; 4]; 4] = [
    [0xff, 0x40, 0x80, 0xff],
    [0x20, 0xc0, 0x60, 0x80],
    [0x00, 0x00, 0xff, 0x00],
    [0xff, 0xff, 0xff, 0xff],
];

/// Pack the pattern into a layout described by its masks.
fn pack(bit_count: u32, masks: [u32; 4], pixels: &[[u8; 4]]) -> Vec<u8> {
    let stride = (bit_count / 8) as usize;
    let mut out = Vec::with_capacity(pixels.len() * stride);
    for pixel in pixels {
        let mut packed = 0u32;
        for (channel, mask) in masks.iter().enumerate() {
            if *mask == 0 {
                continue;
            }
            let shift = mask.trailing_zeros();
            let bits = (mask >> shift).trailing_ones();
            let max = (1u32 << bits) - 1;
            // Quantize down to the channel's width, so the fixture's stored value is exactly what
            // a correct decoder must expand back.
            let value = (pixel[channel] as u32 * max + 127) / 255;
            packed |= (value & max) << shift;
        }
        out.extend_from_slice(&packed.to_le_bytes()[..stride]);
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Fixture bodies.
// ---------------------------------------------------------------------------------------------

const BGRA8_MASKS: [u32; 4] = [0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0xff00_0000];
const RGBA8_MASKS: [u32; 4] = [0x0000_00ff, 0x0000_ff00, 0x00ff_0000, 0xff00_0000];
const BGR8_MASKS: [u32; 4] = [0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0];
const BGR5A1_MASKS: [u32; 4] = [0x7c00, 0x03e0, 0x001f, 0x8000];

fn bgra8() -> Vec<u8> {
    let mut out = uncompressed_header(2, 2, 32, BGRA8_MASKS).write();
    out.extend(pack(32, BGRA8_MASKS, &PATTERN));
    out
}

fn rgba8() -> Vec<u8> {
    let mut out = uncompressed_header(2, 2, 32, RGBA8_MASKS).write();
    out.extend(pack(32, RGBA8_MASKS, &PATTERN));
    out
}

fn bgr8() -> Vec<u8> {
    // 5 pixels wide: the row is 15 bytes, so any decoder that assumed four-byte row alignment
    // reads the second row shifted. 520 files in the pinned corpora have this property.
    let pixels: Vec<[u8; 4]> = (0..15)
        .map(|index| {
            let base = PATTERN[index % 4];
            [base[0], base[1], base[2], 255]
        })
        .collect();
    let mut out = uncompressed_header(5, 3, 24, BGR8_MASKS).write();
    out.extend(pack(24, BGR8_MASKS, &pixels));
    out
}

fn bgr5a1() -> Vec<u8> {
    let mut out = uncompressed_header(2, 2, 16, BGR5A1_MASKS).write();
    out.extend(pack(16, BGR5A1_MASKS, &PATTERN));
    out
}

/// A BC1 colour block with endpoints chosen so both interpolants are exact.
///
/// `c0 = 0xF800` is pure red at full 5-bit precision and `c1 = 0x001F` is pure blue. With
/// `c0 > c1` the block is in four-colour mode, so index 2 is `(2*c0 + c1)/3` and index 3 is
/// `(c0 + 2*c1)/3`. Both divide exactly: 255 and 0 give 170 and 85.
fn bc1_block(opaque: bool) -> [u8; 8] {
    let (c0, c1) = if opaque {
        (0xf800u16, 0x001fu16)
    } else {
        // c0 <= c1 selects BC1's three-colour punch-through mode, where index 3 is transparent.
        (0x001fu16, 0xf800u16)
    };
    let mut block = [0u8; 8];
    block[0..2].copy_from_slice(&c0.to_le_bytes());
    block[2..4].copy_from_slice(&c1.to_le_bytes());
    // Indices 0,1,2,3 across the first row, then all zero.
    block[4] = 0b11_10_01_00;
    block
}

fn dxt1_opaque() -> Vec<u8> {
    let mut out = four_cc_header(4, 4, b"DXT1").write();
    out.extend_from_slice(&bc1_block(true));
    out
}

fn dxt1_punchthrough() -> Vec<u8> {
    let mut out = four_cc_header(4, 4, b"DXT1").write();
    out.extend_from_slice(&bc1_block(false));
    out
}

fn dxt3() -> Vec<u8> {
    let mut out = four_cc_header(4, 4, b"DXT3").write();
    // Four explicit alpha bits per pixel; 0x0 and 0xF expand by x17 to 0 and 255.
    let mut alpha = [0u8; 8];
    alpha[0] = 0xf0; // pixel 0 alpha 0, pixel 1 alpha 255
    out.extend_from_slice(&alpha);
    out.extend_from_slice(&bc1_block(true));
    out
}

fn dxt5_eight_value() -> Vec<u8> {
    let mut out = four_cc_header(4, 4, b"DXT5").write();
    let mut alpha = [0u8; 8];
    // a0 > a1 selects the eight-value interpolation.
    alpha[0] = 255;
    alpha[1] = 0;
    out.extend_from_slice(&alpha);
    out.extend_from_slice(&bc1_block(true));
    out
}

fn dxt5_six_value() -> Vec<u8> {
    let mut out = four_cc_header(4, 4, b"DXT5").write();
    let mut alpha = [0u8; 8];
    // a0 <= a1 selects the six-value interpolation plus explicit 0 and 255. A different code
    // path from the eight-value form, so it is a separate claim.
    alpha[0] = 0;
    alpha[1] = 255;
    out.extend_from_slice(&alpha);
    out.extend_from_slice(&bc1_block(true));
    out
}

/// A BC3 block whose colour endpoints are ordered `c0 <= c1`.
///
/// The BC2 and BC3 specifications say the colour block always uses four-colour interpolation, so
/// this ordering carries no second meaning. `texture2ddecoder` disagrees, and 22 files in the
/// pinned corpora contain such blocks. This fixture is the fault in isolation.
fn dxt5_reversed_endpoints() -> Vec<u8> {
    let mut out = four_cc_header(4, 4, b"DXT5").write();
    let mut alpha = [0u8; 8];
    alpha[0] = 255;
    alpha[1] = 0;
    out.extend_from_slice(&alpha);
    out.extend_from_slice(&bc1_block(false));
    out
}

fn dxt5_unaligned() -> Vec<u8> {
    // 3x2 is stored as one whole 4x4 block and cropped on decode. `gfx/transparent.dds` in
    // vanilla is 2x2 DXT5 and a workshop texture is 1x1, so this is the shipped case.
    let mut out = four_cc_header(3, 2, b"DXT5").write();
    let mut alpha = [0u8; 8];
    alpha[0] = 255;
    alpha[1] = 0;
    out.extend_from_slice(&alpha);
    out.extend_from_slice(&bc1_block(true));
    out
}

fn dxt5_one_pixel() -> Vec<u8> {
    let mut out = four_cc_header(1, 1, b"DXT5").write();
    let mut alpha = [0u8; 8];
    alpha[0] = 255;
    alpha[1] = 0;
    out.extend_from_slice(&alpha);
    out.extend_from_slice(&bc1_block(true));
    out
}

/// Four mip levels whose contents differ, so selecting the wrong one is visible.
fn mipped() -> Vec<u8> {
    let mut header = uncompressed_header(8, 8, 32, BGRA8_MASKS);
    header.mip_count = 4;
    let mut out = header.write();
    for level in 0..4u32 {
        let side = 8u32 >> level;
        // Every level is a flat colour, and every level's colour is different.
        let shade = [0x10 * (level as u8 + 1), 0x20, 0x30, 0xff];
        let pixels = vec![shade; (side * side) as usize];
        out.extend(pack(32, BGRA8_MASKS, &pixels));
    }
    out
}

fn dx10_rgba8_srgb() -> Vec<u8> {
    const DXGI_R8G8B8A8_UNORM_SRGB: u32 = 29;
    const D3D10_RESOURCE_DIMENSION_TEXTURE2D: u32 = 3;
    const DDPF_FOURCC: u32 = 0x4;
    let header = HeaderSpec {
        width: 2,
        height: 2,
        mip_count: 1,
        pixel_flags: DDPF_FOURCC,
        four_cc: *b"DX10",
        bit_count: 0,
        masks: [0; 4],
        caps2: 0,
        dx10: Some([
            DXGI_R8G8B8A8_UNORM_SRGB,
            D3D10_RESOURCE_DIMENSION_TEXTURE2D,
            0,
            1,
            0,
        ]),
    };
    let mut out = header.write();
    out.extend(pack(32, RGBA8_MASKS, &PATTERN));
    out
}

fn cubemap() -> Vec<u8> {
    const DDSCAPS2_CUBEMAP_ALLFACES: u32 = 0x200 | 0xfc00;
    let mut header = uncompressed_header(2, 2, 32, BGRA8_MASKS);
    header.caps2 = DDSCAPS2_CUBEMAP_ALLFACES;
    let mut out = header.write();
    for face in 0..6u8 {
        let pixels = vec![[face * 40, 0x20, 0x30, 0xff]; 4];
        out.extend(pack(32, BGRA8_MASKS, &pixels));
    }
    out
}

fn x8r8g8b8() -> Vec<u8> {
    // A well-formed 32-bit surface whose high byte is padding, not alpha. `image_dds` names no
    // format for it. Zero occurrences locally, and the likeliest form a foreign mod ships.
    const DDPF_RGB: u32 = 0x40;
    let header = HeaderSpec {
        width: 2,
        height: 2,
        mip_count: 1,
        // No DDPF_ALPHAPIXELS: the high byte is explicitly not alpha.
        pixel_flags: DDPF_RGB,
        four_cc: [0; 4],
        bit_count: 32,
        masks: [0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0],
        caps2: 0,
        dx10: None,
    };
    let mut out = header.write();
    out.extend(pack(32, BGR8_MASKS, &PATTERN));
    out
}

fn dxt2_premultiplied() -> Vec<u8> {
    let mut out = four_cc_header(4, 4, b"DXT2").write();
    out.extend_from_slice(&[0u8; 8]);
    out.extend_from_slice(&bc1_block(true));
    out
}

fn volume() -> Vec<u8> {
    const DDSCAPS2_VOLUME: u32 = 0x20_0000;
    let mut header = uncompressed_header(2, 2, 32, BGRA8_MASKS);
    header.caps2 = DDSCAPS2_VOLUME;
    let mut out = header.write();
    out.extend(pack(32, BGRA8_MASKS, &PATTERN));
    out.extend(pack(32, BGRA8_MASKS, &PATTERN));
    out
}

fn unknown_four_cc() -> Vec<u8> {
    let mut out = four_cc_header(4, 4, b"ZZZZ").write();
    out.extend_from_slice(&[0u8; 16]);
    out
}

fn empty() -> Vec<u8> {
    Vec::new()
}

fn bom_only() -> Vec<u8> {
    vec![0xef, 0xbb, 0xbf]
}

fn bad_magic() -> Vec<u8> {
    let mut out = uncompressed_header(2, 2, 32, BGRA8_MASKS).write();
    out[0..4].copy_from_slice(b"XDS ");
    out.extend(pack(32, BGRA8_MASKS, &PATTERN));
    out
}

fn bad_header_size() -> Vec<u8> {
    let mut out = uncompressed_header(2, 2, 32, BGRA8_MASKS).write();
    out[4..8].copy_from_slice(&120u32.to_le_bytes());
    out.extend(pack(32, BGRA8_MASKS, &PATTERN));
    out
}

fn bad_pixel_format_size() -> Vec<u8> {
    let mut out = uncompressed_header(2, 2, 32, BGRA8_MASKS).write();
    out[76..80].copy_from_slice(&28u32.to_le_bytes());
    out.extend(pack(32, BGRA8_MASKS, &PATTERN));
    out
}

fn zero_height() -> Vec<u8> {
    let mut out = uncompressed_header(2, 2, 32, BGRA8_MASKS).write();
    out[12..16].copy_from_slice(&0u32.to_le_bytes());
    out.extend(pack(32, BGRA8_MASKS, &PATTERN));
    out
}

fn truncated_pixels() -> Vec<u8> {
    let mut out = four_cc_header(4, 4, b"DXT5").write();
    // A DXT5 4x4 needs 16 bytes; 12 are present.
    out.extend_from_slice(&[0u8; 12]);
    out
}

fn truncated_dx10_header() -> Vec<u8> {
    let mut out = dx10_rgba8_srgb();
    out.truncate(140);
    out
}

// ---------------------------------------------------------------------------------------------
// The set.
// ---------------------------------------------------------------------------------------------

/// Expected pixels for the standard four-pixel pattern under an eight-bit-per-channel layout.
const PATTERN_RGBA8: [[u8; 4]; 4] = PATTERN;

/// The same pattern quantized to five- and one-bit channels, which is all `bgr5a1` can store.
///
/// Derived by hand from the quantize-then-replicate rule, not copied from a decoder. Worked for
/// pixel 1: green `0xC0` is 192, and `(192*31 + 127)/255 = 23`, which expands back to
/// `(23*255 + 15)/31 = 189`. Alpha `0x80` is 128, and `(128*1 + 127)/255 = 1`, so a one-bit alpha
/// of 128 stores as opaque — the round trip is lossy, and this fixture is where that is stated.
const PATTERN_BGR5A1: [[u8; 4]; 4] = [
    [255, 66, 132, 255],
    [33, 189, 99, 255],
    [0, 0, 255, 0],
    [255, 255, 255, 255],
];

/// The first row of a four-colour BC1 block with endpoints `0xF800` and `0x001F`.
const BC1_FIRST_ROW: [[u8; 4]; 4] = [
    [255, 0, 0, 255],
    [0, 0, 255, 255],
    [170, 0, 85, 255],
    [85, 0, 170, 255],
];

/// The same block with its endpoints exchanged, decoded in four-colour mode.
///
/// This is the whole claim of `dxt5_reversed_endpoints_4x4.dds`. Because BC3's colour block must
/// use four-colour interpolation regardless of endpoint order, exchanging the endpoints simply
/// exchanges indices 0 and 1 and mirrors the two interpolants. A decoder that instead applied
/// BC1's punch-through rule here would return `[85, 0, 85]` at index 2 and a transparent pixel at
/// index 3 — which is how the 22 corpus files diverged.
const BC1_FIRST_ROW_REVERSED: [[u8; 4]; 4] = [
    [0, 0, 255, 255],
    [255, 0, 0, 255],
    [85, 0, 170, 255],
    [170, 0, 85, 255],
];

pub fn all() -> Vec<Fixture> {
    vec![
        Fixture {
            path: "valid/bgra8_2x2.dds",
            claim: "A 32-bit surface with red in the third byte: the file's byte order is B, G, R, A.",
            expected: Expected::Decoded { width: 2, height: 2 },
            expected_pixels: &PATTERN_RGBA8,
            bytes: bgra8,
        },
        Fixture {
            path: "valid/rgba8_2x2.dds",
            claim: "A 32-bit surface with red in the first byte. A decoder keyed on bit count \
                    alone reads this identically to bgra8_2x2 and swaps red with blue. Eleven \
                    files in the pinned corpora declare this layout.",
            expected: Expected::Decoded { width: 2, height: 2 },
            expected_pixels: &PATTERN_RGBA8,
            bytes: rgba8,
        },
        Fixture {
            path: "valid/bgr8_5x3.dds",
            claim: "24-bit with a 15-byte row: rows are tightly packed and no four-byte \
                    alignment is applied. 520 files in the pinned corpora have this property.",
            expected: Expected::Decoded { width: 5, height: 3 },
            expected_pixels: &[],
            bytes: bgr8,
        },
        Fixture {
            path: "valid/bgr5a1_2x2.dds",
            claim: "Five-bit channels expand to eight by bit replication, not by a left shift, \
                    so full white decodes to 255 rather than 248. Alpha is one bit.",
            expected: Expected::Decoded { width: 2, height: 2 },
            expected_pixels: &PATTERN_BGR5A1,
            bytes: bgr5a1,
        },
        Fixture {
            path: "valid/dxt1_opaque_4x4.dds",
            claim: "BC1 four-colour mode. Endpoints chose so the 1/3 and 2/3 interpolants are \
                    exactly 170 and 85, which makes the expected pixels spec-derived rather than \
                    copied from a decoder.",
            expected: Expected::Decoded { width: 4, height: 4 },
            expected_pixels: &BC1_FIRST_ROW,
            bytes: dxt1_opaque,
        },
        Fixture {
            path: "valid/dxt1_punchthrough_4x4.dds",
            claim: "BC1 three-colour mode: index 3 decodes to a fully transparent pixel, not to \
                    opaque black.",
            expected: Expected::Decoded { width: 4, height: 4 },
            expected_pixels: &[],
            bytes: dxt1_punchthrough,
        },
        Fixture {
            path: "valid/dxt3_4x4.dds",
            claim: "BC2 explicit four-bit alpha expands by multiplying by 17, so 0xF becomes 255.",
            expected: Expected::Decoded { width: 4, height: 4 },
            expected_pixels: &[],
            bytes: dxt3,
        },
        Fixture {
            path: "valid/dxt5_eight_value_4x4.dds",
            claim: "BC3 with alpha0 > alpha1: eight interpolated alpha values.",
            expected: Expected::Decoded { width: 4, height: 4 },
            expected_pixels: &[],
            bytes: dxt5_eight_value,
        },
        Fixture {
            path: "valid/dxt5_six_value_4x4.dds",
            claim: "BC3 with alpha0 <= alpha1: six interpolated values plus explicit 0 and 255. \
                    A different code path from the eight-value form.",
            expected: Expected::Decoded { width: 4, height: 4 },
            expected_pixels: &[],
            bytes: dxt5_six_value,
        },
        Fixture {
            path: "valid/dxt5_reversed_endpoints_4x4.dds",
            claim: "BC3 whose colour endpoints are ordered c0 <= c1. The BC2 and BC3 \
                    specifications say the colour block always uses four-colour interpolation, so \
                    this ordering carries no second meaning. 22 files in the pinned corpora \
                    contain such blocks, and a decoder that applies BC1's punch-through rule here \
                    differs by up to 255 in a channel.",
            expected: Expected::Decoded { width: 4, height: 4 },
            expected_pixels: &BC1_FIRST_ROW_REVERSED,
            bytes: dxt5_reversed_endpoints,
        },
        Fixture {
            path: "valid/dxt5_3x2.dds",
            claim: "Block-unaligned: 3x2 is stored as one whole 4x4 block and cropped on decode. \
                    844 files in the pinned corpora are block-unaligned.",
            expected: Expected::Decoded { width: 3, height: 2 },
            expected_pixels: &[],
            bytes: dxt5_unaligned,
        },
        Fixture {
            path: "valid/dxt5_1x1.dds",
            claim: "The degenerate extreme the workshop corpus actually contains.",
            expected: Expected::Decoded { width: 1, height: 1 },
            expected_pixels: &[[255, 0, 0, 255]],
            bytes: dxt5_one_pixel,
        },
        Fixture {
            path: "valid/bgra8_mips_8x8.dds",
            claim: "Four mip levels, each a different flat colour. The recipe selects level 0, \
                    so the output is 8x8 and its first pixel is level 0's colour.",
            expected: Expected::Decoded { width: 8, height: 8 },
            expected_pixels: &[[0x10, 0x20, 0x30, 0xff]],
            bytes: mipped,
        },
        Fixture {
            path: "valid/dx10_rgba8_srgb_2x2.dds",
            claim: "The only form that carries a colorspace declaration. The recipe declares \
                    rather than converts, so its pixels are identical to the untagged form.",
            expected: Expected::Decoded { width: 2, height: 2 },
            expected_pixels: &PATTERN_RGBA8,
            bytes: dx10_rgba8_srgb,
        },
        Fixture {
            path: "unsupported/cubemap_2x2.dds",
            claim: "Six faces. Refused by recipe policy: the convenience entry point would stack \
                    them vertically into an image six times too tall and report success.",
            expected: Expected::UnsupportedFormat,
            expected_pixels: &[],
            bytes: cubemap,
        },
        Fixture {
            path: "unsupported/x8r8g8b8_2x2.dds",
            claim: "A well-formed 32-bit surface whose high byte is padding rather than alpha. \
                    image_dds names no format for it. Zero occurrences locally, and the likeliest \
                    form a foreign mod ships.",
            expected: Expected::UnsupportedFormat,
            expected_pixels: &[],
            bytes: x8r8g8b8,
        },
        Fixture {
            path: "unsupported/dxt2_premultiplied_4x4.dds",
            claim: "DXT2 is DXT3 with premultiplied alpha. image_dds maps it to BC2 and never \
                    un-premultiplies, so decoding it under a straight-alpha recipe would produce \
                    a quietly wrong image.",
            expected: Expected::UnsupportedFormat,
            expected_pixels: &[],
            bytes: dxt2_premultiplied,
        },
        Fixture {
            path: "unsupported/volume_2x2x2.dds",
            claim: "A volume texture. None exist in the pinned corpora, so support would be an \
                    untested claim.",
            expected: Expected::UnsupportedFormat,
            expected_pixels: &[],
            bytes: volume,
        },
        Fixture {
            path: "unsupported/fourcc_zzzz_4x4.dds",
            claim: "An unrecognized four-character code is a typed unsupported outcome, not a \
                    guess at a similar format.",
            expected: Expected::UnsupportedFormat,
            expected_pixels: &[],
            bytes: unknown_four_cc,
        },
        Fixture {
            path: "malformed/empty.dds",
            claim: "Zero bytes. Mirrors gfx/interface/icons/traits/all_negative.dds, which the \
                    installed vanilla corpus contains.",
            expected: Expected::MalformedMedia,
            expected_pixels: &[],
            bytes: empty,
        },
        Fixture {
            path: "malformed/bom_only.dds",
            claim: "Three bytes holding a UTF-8 byte order mark. Mirrors \
                    gfx/interface/main/paused_bar_glow.dds, which vanilla ships and which a \
                    sprite definition in interface/main.gfx actually references.",
            expected: Expected::MalformedMedia,
            expected_pixels: &[],
            bytes: bom_only,
        },
        Fixture {
            path: "malformed/bad_magic.dds",
            claim: "A full-length header whose first four bytes are not `DDS `. Distinct from the \
                    byte-order-mark case, which is too short to have a magic at all.",
            expected: Expected::MalformedMedia,
            expected_pixels: &[],
            bytes: bad_magic,
        },
        Fixture {
            path: "malformed/header_size_120.dds",
            claim: "DDS_HEADER.dwSize is 120 rather than 124.",
            expected: Expected::MalformedMedia,
            expected_pixels: &[],
            bytes: bad_header_size,
        },
        Fixture {
            path: "malformed/pixelformat_size_28.dds",
            claim: "DDS_PIXELFORMAT.dwSize is 28 rather than 32.",
            expected: Expected::MalformedMedia,
            expected_pixels: &[],
            bytes: bad_pixel_format_size,
        },
        Fixture {
            path: "malformed/zero_height.dds",
            claim: "Height is zero, so there is no surface to decode.",
            expected: Expected::MalformedMedia,
            expected_pixels: &[],
            bytes: zero_height,
        },
        Fixture {
            path: "malformed/truncated_pixels_dxt5.dds",
            claim: "The header declares a 4x4 DXT5 surface, which needs 16 pixel bytes; 12 are \
                    present. Detected before decoding, from the container's own declarations.",
            expected: Expected::MalformedMedia,
            expected_pixels: &[],
            bytes: truncated_pixels,
        },
        Fixture {
            path: "malformed/truncated_dx10_header.dds",
            claim: "A DX10 four-character code with only part of the DDS_HEADER_DXT10 behind it.",
            expected: Expected::MalformedMedia,
            expected_pixels: &[],
            bytes: truncated_dx10_header,
        },
    ]
}
