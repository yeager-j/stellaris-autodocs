//! Header to format, and format to expected outcome, decided before any decoder runs.
//!
//! This is `AGENTS.md`'s parse-don't-validate rule applied to the asset boundary. A
//! [`Classification`] carries the evidence the decoder needs — which format, where the pixels
//! start, how many bytes the first mip occupies — so the decode path never re-inspects the
//! container. It also means `MalformedMedia` and `UnsupportedFormat` are decided by different
//! code from `ConversionFailure`, which is what makes them three distinct outcomes rather than
//! three names for a decoder returning `Err`.

use crate::header::{self, Header, HeaderError, PixelFormat};

/// A single colour channel, derived from its mask.
///
/// Derivation rather than a table: the mask is what the file actually declares, and a table of
/// known layouts would silently mis-read the first layout it had not anticipated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Channel {
    pub shift: u32,
    pub bits: u32,
}

impl Channel {
    pub fn from_mask(mask: u32) -> Option<Self> {
        if mask == 0 {
            return None;
        }
        let shift = mask.trailing_zeros();
        let bits = (mask >> shift).trailing_ones();
        // A mask with a hole in it — say `0b1011` — is not a channel, and guessing which bits
        // were meant would invent data.
        if (((1u64 << bits) - 1) as u32) << shift != mask {
            return None;
        }
        Some(Self { shift, bits })
    }

    /// Extract this channel from a packed pixel and expand it to eight bits.
    ///
    /// Bit replication rather than a left shift: a 5-bit `11111` must become `11111111`, not
    /// `11111000`, or full white decodes as `0xF8` and every comparison against a reference
    /// decoder fails by a constant.
    pub fn extract(&self, packed: u32) -> u8 {
        let value = (packed >> self.shift) & ((1u32 << self.bits) - 1);
        match self.bits {
            0 => 0,
            8 => value as u8,
            bits => {
                let max = (1u32 << bits) - 1;
                ((value * 255 + max / 2) / max) as u8
            }
        }
    }
}

/// An uncompressed layout, fully described by its bit count and channel positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub bit_count: u32,
    pub red: Channel,
    pub green: Channel,
    pub blue: Channel,
    pub alpha: Option<Channel>,
}

/// The closed set of source formats this adapter claims to decode.
///
/// Closed on purpose. Anything outside it is [`Classification::Unsupported`], which is a typed
/// product outcome with a placeholder behind it — not a crash, and not a silently wrong image.
/// The census reports how much of the corpus each member covers, so the set can be judged
/// against what the corpus contains rather than against what a format list suggests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    /// `DXT1`.
    Bc1RgbaUnorm,
    Bc1RgbaUnormSrgb,
    /// `DXT3`.
    Bc2RgbaUnorm,
    Bc2RgbaUnormSrgb,
    /// `DXT5`.
    Bc3RgbaUnorm,
    Bc3RgbaUnormSrgb,
    /// `ATI1` / `BC4U`.
    Bc4RUnorm,
    /// `ATI2` / `BC5U`.
    Bc5RgUnorm,
    Bc7RgbaUnorm,
    Bc7RgbaUnormSrgb,
    Uncompressed(Layout),
}

impl SourceFormat {
    /// A stable label for records. Uncompressed layouts are named by their masks so two
    /// different 32-bit layouts never share a row in a census table.
    pub fn label(&self) -> String {
        match self {
            Self::Bc1RgbaUnorm => "BC1_UNORM".into(),
            Self::Bc1RgbaUnormSrgb => "BC1_UNORM_SRGB".into(),
            Self::Bc2RgbaUnorm => "BC2_UNORM".into(),
            Self::Bc2RgbaUnormSrgb => "BC2_UNORM_SRGB".into(),
            Self::Bc3RgbaUnorm => "BC3_UNORM".into(),
            Self::Bc3RgbaUnormSrgb => "BC3_UNORM_SRGB".into(),
            Self::Bc4RUnorm => "BC4_UNORM".into(),
            Self::Bc5RgUnorm => "BC5_UNORM".into(),
            Self::Bc7RgbaUnorm => "BC7_UNORM".into(),
            Self::Bc7RgbaUnormSrgb => "BC7_UNORM_SRGB".into(),
            // Keyed by channel *position*, not merely by width. Eleven files in the pinned
            // corpora declare `A8B8G8R8` where 20,834 declare `A8R8G8B8`; a label built from bit
            // counts alone would file them under the same row, and they are the only inputs the
            // corpus contains that can catch a red/blue swap.
            Self::Uncompressed(layout) => {
                let channel = |name: &str, shift: u32, bits: u32| format!("{name}{shift}:{bits}");
                format!(
                    "UNCOMP{}_{}{}{}{}",
                    layout.bit_count,
                    channel("r", layout.red.shift, layout.red.bits),
                    channel("g", layout.green.shift, layout.green.bits),
                    channel("b", layout.blue.shift, layout.blue.bits),
                    layout
                        .alpha
                        .map_or_else(|| "a-".to_string(), |a| channel("a", a.shift, a.bits)),
                )
            }
        }
    }

    /// The same format declared as sRGB-encoded, where such a declaration exists.
    ///
    /// Used by `d3-recipe` to measure whether the declaration changes any output byte. Legacy DDS
    /// carries no colorspace flag, so the recipe has to choose one — and whether that choice is a
    /// conversion or merely a label is a measurement, not a reading of the documentation.
    pub fn srgb_counterpart(&self) -> Option<Self> {
        match self {
            Self::Bc1RgbaUnorm => Some(Self::Bc1RgbaUnormSrgb),
            Self::Bc2RgbaUnorm => Some(Self::Bc2RgbaUnormSrgb),
            Self::Bc3RgbaUnorm => Some(Self::Bc3RgbaUnormSrgb),
            Self::Bc7RgbaUnorm => Some(Self::Bc7RgbaUnormSrgb),
            _ => None,
        }
    }

    /// Bytes per 4x4 block, or `None` for uncompressed layouts.
    pub fn block_bytes(&self) -> Option<usize> {
        match self {
            Self::Bc1RgbaUnorm | Self::Bc1RgbaUnormSrgb | Self::Bc4RUnorm => Some(8),
            Self::Bc2RgbaUnorm
            | Self::Bc2RgbaUnormSrgb
            | Self::Bc3RgbaUnorm
            | Self::Bc3RgbaUnormSrgb
            | Self::Bc5RgUnorm
            | Self::Bc7RgbaUnorm
            | Self::Bc7RgbaUnormSrgb => Some(16),
            Self::Uncompressed(_) => None,
        }
    }

    pub fn is_block_compressed(&self) -> bool {
        self.block_bytes().is_some()
    }

    /// Bytes one mip level of the given size occupies.
    ///
    /// Block-compressed surfaces round up to whole 4x4 blocks, which is why a 18x19 icon is
    /// stored as 5x5 blocks. 844 files in the pinned corpora have dimensions that are not
    /// multiples of four, so this rounding is the common case rather than an edge case.
    pub fn level_bytes(&self, width: u32, height: u32) -> usize {
        match self.block_bytes() {
            Some(block) => {
                let blocks_wide = width.div_ceil(4).max(1) as usize;
                let blocks_high = height.div_ceil(4).max(1) as usize;
                blocks_wide * blocks_high * block
            }
            None => {
                let Self::Uncompressed(layout) = self else {
                    unreachable!("block_bytes returned None for a compressed format")
                };
                let row = (width as usize * layout.bit_count as usize).div_ceil(8);
                row * height as usize
            }
        }
    }
}

/// Why a well-formed container is not decodable by this adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsupported {
    /// A four-character code outside the claimed set.
    FourCc(String),
    /// A `DX10` DXGI enumerant outside the claimed set.
    DxgiFormat(u32),
    /// Masks that do not describe a colour layout this adapter reads.
    MaskLayout {
        bit_count: u32,
        red: u32,
        green: u32,
        blue: u32,
        alpha: u32,
    },
    /// Luminance, alpha-only, or YUV pixel flags.
    PixelFlags(u32),
    /// A volume texture. There are none in the pinned corpora; claiming support for a shape
    /// nothing exercises would be an untested claim.
    VolumeTexture,
    /// `DXT2` or `DXT4`: premultiplied alpha, which this recipe does not un-premultiply.
    PremultipliedAlpha(String),
    /// More than one array layer or cube-map face. Refused by recipe policy rather than by the
    /// container: there is one image per documented slot, and choosing a face silently would be
    /// a presentation decision made in the decoder.
    MultipleLayers { layers: u32 },
    /// A surface larger than the recipe's decoded-size limit.
    TooLarge { pixels: usize, limit: usize },
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FourCc(code) => write!(f, "four-character code {code:?} is not supported"),
            Self::DxgiFormat(value) => write!(f, "DXGI format {value} is not supported"),
            Self::MaskLayout {
                bit_count,
                red,
                green,
                blue,
                alpha,
            } => write!(
                f,
                "{bit_count}-bit masks r={red:#x} g={green:#x} b={blue:#x} a={alpha:#x} are not supported"
            ),
            Self::PixelFlags(flags) => write!(f, "pixel format flags {flags:#x} are not supported"),
            Self::VolumeTexture => write!(f, "volume textures are not supported"),
            Self::PremultipliedAlpha(code) => {
                write!(f, "{code} carries premultiplied alpha, which is not un-premultiplied")
            }
            Self::MultipleLayers { layers } => {
                write!(f, "{layers} array layers; the recipe materializes one image per slot")
            }
            Self::TooLarge { pixels, limit } => {
                write!(f, "{pixels} pixels exceeds the recipe limit of {limit}")
            }
        }
    }
}

/// Why a container cannot be read at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Malformed {
    /// The container itself does not parse.
    Header(HeaderError),
    /// The header parses, but there are fewer pixel bytes than the first mip level needs.
    TruncatedPixels {
        present: usize,
        needed: usize,
    },
}

impl std::fmt::Display for Malformed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Header(error) => write!(f, "{error}"),
            Self::TruncatedPixels { present, needed } => write!(
                f,
                "mip 0 needs {needed} pixel bytes; {present} are present"
            ),
        }
    }
}

/// What the adapter knows about an input before decoding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    Decodable(Decodable),
    Malformed(Malformed),
    Unsupported {
        header: Box<Header>,
        reason: Unsupported,
    },
}

/// A container proved decodable, carrying what the decoders need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decodable {
    pub header: Header,
    pub format: SourceFormat,
    /// Byte range of mip level 0, layer 0, face 0 within the file.
    pub level0: std::ops::Range<usize>,
}

impl Classification {
    pub fn format(&self) -> Option<SourceFormat> {
        match self {
            Self::Decodable(decodable) => Some(decodable.format),
            _ => None,
        }
    }

    /// The stable label a census row is keyed by.
    pub fn label(&self) -> String {
        match self {
            Self::Decodable(decodable) => decodable.format.label(),
            Self::Malformed(_) => "MALFORMED".into(),
            Self::Unsupported { .. } => "UNSUPPORTED".into(),
        }
    }
}

/// Classify raw file bytes.
pub fn classify(bytes: &[u8]) -> Classification {
    let header = match header::parse(bytes) {
        Ok(header) => header,
        Err(error) => return Classification::Malformed(Malformed::Header(error)),
    };

    if header.is_volume() {
        return Classification::Unsupported {
            header: Box::new(header),
            reason: Unsupported::VolumeTexture,
        };
    }

    let format = match source_format(&header) {
        Ok(format) => format,
        Err(reason) => {
            return Classification::Unsupported {
                header: Box::new(header),
                reason,
            }
        }
    };

    let needed = format.level_bytes(header.width, header.height);
    let present = bytes.len().saturating_sub(header.data_offset);
    if present < needed {
        return Classification::Malformed(Malformed::TruncatedPixels { present, needed });
    }

    let start = header.data_offset;
    Classification::Decodable(Decodable {
        header,
        format,
        level0: start..start + needed,
    })
}

fn source_format(header: &Header) -> Result<SourceFormat, Unsupported> {
    let pf = &header.pixel_format;

    if let Some(dx10) = &header.dx10 {
        return dxgi_format(dx10.dxgi_format);
    }

    if pf.is_four_cc() {
        return match &pf.four_cc {
            b"DXT1" => Ok(SourceFormat::Bc1RgbaUnorm),
            b"DXT3" => Ok(SourceFormat::Bc2RgbaUnorm),
            b"DXT5" => Ok(SourceFormat::Bc3RgbaUnorm),
            // DXT2 and DXT4 are DXT3 and DXT5 with premultiplied alpha. `image_dds` maps them to
            // BC2 and BC3 and never un-premultiplies, so decoding them under this recipe's
            // straight-alpha policy would produce a quietly wrong image. Neither appears in the
            // pinned corpora, so refusing them costs nothing and silence would cost correctness.
            b"DXT2" | b"DXT4" => Err(Unsupported::PremultipliedAlpha(
                String::from_utf8_lossy(&pf.four_cc).into_owned(),
            )),
            b"ATI1" | b"BC4U" => Ok(SourceFormat::Bc4RUnorm),
            b"ATI2" | b"BC5U" => Ok(SourceFormat::Bc5RgUnorm),
            other => Err(Unsupported::FourCc(
                String::from_utf8_lossy(other).into_owned(),
            )),
        };
    }

    // Luminance, alpha-only, and YUV are declared through pixel flags rather than through masks,
    // and none of them describe a colour surface this adapter reads.
    let unreadable = header::DDPF_LUMINANCE | header::DDPF_YUV | header::DDPF_ALPHA;
    if pf.flags & unreadable != 0 && pf.flags & header::DDPF_RGB == 0 {
        return Err(Unsupported::PixelFlags(pf.flags));
    }

    uncompressed_layout(pf)
}

fn uncompressed_layout(pf: &PixelFormat) -> Result<SourceFormat, Unsupported> {
    let unsupported = || Unsupported::MaskLayout {
        bit_count: pf.bit_count,
        red: pf.red_mask,
        green: pf.green_mask,
        blue: pf.blue_mask,
        alpha: pf.alpha_mask,
    };

    if !(8..=32).contains(&pf.bit_count) || pf.bit_count % 8 != 0 {
        return Err(unsupported());
    }

    let (Some(red), Some(green), Some(blue)) = (
        Channel::from_mask(pf.red_mask),
        Channel::from_mask(pf.green_mask),
        Channel::from_mask(pf.blue_mask),
    ) else {
        return Err(unsupported());
    };

    // The flag decides, not the mask. An `X8R8G8B8` surface carries a non-zero high byte that is
    // not alpha, and reading it would turn opaque icons transparent.
    let alpha = if pf.has_alpha() {
        match Channel::from_mask(pf.alpha_mask) {
            Some(channel) => Some(channel),
            None => return Err(unsupported()),
        }
    } else {
        None
    };

    let widest = [Some(red), Some(green), Some(blue), alpha]
        .into_iter()
        .flatten()
        .map(|channel| channel.shift + channel.bits)
        .max()
        .unwrap_or(0);
    if widest > pf.bit_count {
        return Err(unsupported());
    }

    Ok(SourceFormat::Uncompressed(Layout {
        bit_count: pf.bit_count,
        red,
        green,
        blue,
        alpha,
    }))
}

/// The DXGI enumerants this adapter claims.
///
/// Values are from `DXGI_FORMAT`. Only members with a counterpart in the closed set appear; the
/// rest are a typed unsupported outcome rather than a guess at a similar layout.
fn dxgi_format(value: u32) -> Result<SourceFormat, Unsupported> {
    const R8G8B8A8_UNORM: u32 = 28;
    const R8G8B8A8_UNORM_SRGB: u32 = 29;
    const B8G8R8A8_UNORM: u32 = 87;
    const B8G8R8X8_UNORM: u32 = 88;
    const B8G8R8A8_UNORM_SRGB: u32 = 91;
    const BC1_UNORM: u32 = 71;
    const BC1_UNORM_SRGB: u32 = 72;
    const BC2_UNORM: u32 = 74;
    const BC2_UNORM_SRGB: u32 = 75;
    const BC3_UNORM: u32 = 77;
    const BC3_UNORM_SRGB: u32 = 78;
    const BC4_UNORM: u32 = 80;
    const BC5_UNORM: u32 = 83;
    const B5G5R5A1_UNORM: u32 = 86;
    const BC7_UNORM: u32 = 98;
    const BC7_UNORM_SRGB: u32 = 99;

    let rgba8 = Layout {
        bit_count: 32,
        red: Channel { shift: 0, bits: 8 },
        green: Channel { shift: 8, bits: 8 },
        blue: Channel { shift: 16, bits: 8 },
        alpha: Some(Channel { shift: 24, bits: 8 }),
    };
    let bgra8 = Layout {
        bit_count: 32,
        red: Channel { shift: 16, bits: 8 },
        green: Channel { shift: 8, bits: 8 },
        blue: Channel { shift: 0, bits: 8 },
        alpha: Some(Channel { shift: 24, bits: 8 }),
    };

    match value {
        BC1_UNORM => Ok(SourceFormat::Bc1RgbaUnorm),
        BC1_UNORM_SRGB => Ok(SourceFormat::Bc1RgbaUnormSrgb),
        BC2_UNORM => Ok(SourceFormat::Bc2RgbaUnorm),
        BC2_UNORM_SRGB => Ok(SourceFormat::Bc2RgbaUnormSrgb),
        BC3_UNORM => Ok(SourceFormat::Bc3RgbaUnorm),
        BC3_UNORM_SRGB => Ok(SourceFormat::Bc3RgbaUnormSrgb),
        BC4_UNORM => Ok(SourceFormat::Bc4RUnorm),
        BC5_UNORM => Ok(SourceFormat::Bc5RgUnorm),
        BC7_UNORM => Ok(SourceFormat::Bc7RgbaUnorm),
        BC7_UNORM_SRGB => Ok(SourceFormat::Bc7RgbaUnormSrgb),
        R8G8B8A8_UNORM | R8G8B8A8_UNORM_SRGB => Ok(SourceFormat::Uncompressed(rgba8)),
        B8G8R8A8_UNORM | B8G8R8A8_UNORM_SRGB => Ok(SourceFormat::Uncompressed(bgra8)),
        B8G8R8X8_UNORM => Ok(SourceFormat::Uncompressed(Layout {
            alpha: None,
            ..bgra8
        })),
        B5G5R5A1_UNORM => Ok(SourceFormat::Uncompressed(Layout {
            bit_count: 16,
            red: Channel { shift: 10, bits: 5 },
            green: Channel { shift: 5, bits: 5 },
            blue: Channel { shift: 0, bits: 5 },
            alpha: Some(Channel { shift: 15, bits: 1 }),
        })),
        other => Err(Unsupported::DxgiFormat(other)),
    }
}
