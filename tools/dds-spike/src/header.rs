//! An independent reader for the DDS container.
//!
//! Deliberately not `ddsfile`, even though `image_dds` already depends on it. Classification
//! decides whether an input is malformed, unsupported, or decodable, and that decision is the
//! thing `docs/technical-design.md:516` asks the asset module to make. If it were made by the
//! decoder's own parser, then "malformed" and "unsupported" would both reduce to "the decoder
//! said no", and `analysis::finalize` could not scope its issue correctly. Parsing the container
//! here also gives `decode_b` the pixel-format masks it needs to read the uncompressed classes
//! without asking `image_dds` what they mean.
//!
//! Layout per Microsoft's DDS reference: a four-byte magic, a 124-byte `DDS_HEADER` containing a
//! 32-byte `DDS_PIXELFORMAT` at offset 76, and — only when the pixel format's four-character code
//! is `DX10` — a further 20-byte `DDS_HEADER_DXT10`.

/// `DDPF_ALPHAPIXELS`: the alpha mask is meaningful.
pub const DDPF_ALPHAPIXELS: u32 = 0x1;
/// `DDPF_FOURCC`: the format is named by a four-character code rather than by masks.
pub const DDPF_FOURCC: u32 = 0x4;
/// `DDPF_RGB`: uncompressed colour described by masks.
pub const DDPF_RGB: u32 = 0x40;
/// `DDPF_YUV`.
pub const DDPF_YUV: u32 = 0x200;
/// `DDPF_LUMINANCE`.
pub const DDPF_LUMINANCE: u32 = 0x20000;
/// `DDPF_ALPHA`: alpha only.
pub const DDPF_ALPHA: u32 = 0x2;

/// `DDSCAPS2_CUBEMAP`.
pub const DDSCAPS2_CUBEMAP: u32 = 0x200;
/// `DDSCAPS2_VOLUME`.
pub const DDSCAPS2_VOLUME: u32 = 0x20_0000;

/// Why a byte sequence is not a readable DDS container.
///
/// Each variant names a distinct fault rather than collapsing into one "invalid" case, because
/// the record has to show which real-world inputs land where. Vanilla ships two files that fail
/// here — a zero-byte icon and a three-byte file holding only a UTF-8 byte order mark — and they
/// fail for different reasons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderError {
    /// Fewer than the four magic bytes plus 124-byte header.
    TooShort { bytes: usize, needed: usize },
    /// The first four bytes are not `DDS `.
    BadMagic { found: [u8; 4] },
    /// `DDS_HEADER.dwSize` is not 124.
    BadHeaderSize { found: u32 },
    /// `DDS_PIXELFORMAT.dwSize` is not 32.
    BadPixelFormatSize { found: u32 },
    /// A `DX10` four-character code with no `DDS_HEADER_DXT10` behind it.
    TruncatedDx10Header { bytes: usize },
    /// Width or height is zero, so there is no surface to decode.
    ZeroDimensions { width: u32, height: u32 },
}

impl std::fmt::Display for HeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort { bytes, needed } => {
                write!(f, "file is {bytes} bytes; a DDS header needs {needed}")
            }
            Self::BadMagic { found } => write!(f, "magic is {found:02x?}, not `DDS `"),
            Self::BadHeaderSize { found } => write!(f, "header size is {found}, not 124"),
            Self::BadPixelFormatSize { found } => {
                write!(f, "pixel format size is {found}, not 32")
            }
            Self::TruncatedDx10Header { bytes } => {
                write!(f, "DX10 four-character code but only {bytes} bytes")
            }
            Self::ZeroDimensions { width, height } => write!(f, "dimensions are {width}x{height}"),
        }
    }
}

/// The pixel format as the container declares it, before any interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelFormat {
    pub flags: u32,
    pub four_cc: [u8; 4],
    pub bit_count: u32,
    pub red_mask: u32,
    pub green_mask: u32,
    pub blue_mask: u32,
    pub alpha_mask: u32,
}

impl PixelFormat {
    pub fn is_four_cc(&self) -> bool {
        self.flags & DDPF_FOURCC != 0
    }

    pub fn four_cc_str(&self) -> String {
        String::from_utf8_lossy(&self.four_cc).to_string()
    }

    /// Whether the alpha mask participates.
    ///
    /// A mask can be non-zero while the flag is clear — a 32-bit surface stored as `X8R8G8B8`
    /// carries an unused high byte. Reading that byte as alpha turns an opaque icon transparent,
    /// which is the sort of defect the cross-check exists to catch.
    pub fn has_alpha(&self) -> bool {
        self.flags & DDPF_ALPHAPIXELS != 0 && self.alpha_mask != 0
    }
}

/// The `DDS_HEADER_DXT10` extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dx10 {
    pub dxgi_format: u32,
    pub resource_dimension: u32,
    pub misc_flag: u32,
    pub array_size: u32,
    pub misc_flags2: u32,
}

/// A parsed DDS container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    /// As declared. Zero and one both mean a single surface; the distinction is recorded rather
    /// than normalized because 13,607 vanilla files declare zero and the census reports it.
    pub mip_count: u32,
    pub pixel_format: PixelFormat,
    pub caps2: u32,
    pub dx10: Option<Dx10>,
    /// Offset of the first pixel byte: 128, or 148 with a `DX10` extension.
    pub data_offset: usize,
}

impl Header {
    pub fn is_cubemap(&self) -> bool {
        self.caps2 & DDSCAPS2_CUBEMAP != 0
    }

    pub fn is_volume(&self) -> bool {
        self.caps2 & DDSCAPS2_VOLUME != 0
    }

    /// Mip levels actually present, treating a declared zero as one.
    pub fn levels(&self) -> u32 {
        self.mip_count.max(1)
    }

    pub fn array_size(&self) -> u32 {
        self.dx10.as_ref().map_or(1, |dx10| dx10.array_size.max(1))
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

/// Parse the container, or say precisely why it is not one.
pub fn parse(bytes: &[u8]) -> Result<Header, HeaderError> {
    if bytes.len() < 128 {
        // Reported before the magic check on purpose: a zero-byte file has no magic to be wrong,
        // and calling it "bad magic" would describe a byte that is not there.
        return Err(HeaderError::TooShort {
            bytes: bytes.len(),
            needed: 128,
        });
    }
    if &bytes[0..4] != b"DDS " {
        let mut found = [0u8; 4];
        found.copy_from_slice(&bytes[0..4]);
        return Err(HeaderError::BadMagic { found });
    }

    let header_size = u32_at(bytes, 4);
    if header_size != 124 {
        return Err(HeaderError::BadHeaderSize { found: header_size });
    }

    let height = u32_at(bytes, 12);
    let width = u32_at(bytes, 16);
    let depth = u32_at(bytes, 24);
    let mip_count = u32_at(bytes, 28);

    let pixel_format_size = u32_at(bytes, 76);
    if pixel_format_size != 32 {
        return Err(HeaderError::BadPixelFormatSize {
            found: pixel_format_size,
        });
    }

    let mut four_cc = [0u8; 4];
    four_cc.copy_from_slice(&bytes[84..88]);
    let pixel_format = PixelFormat {
        flags: u32_at(bytes, 80),
        four_cc,
        bit_count: u32_at(bytes, 88),
        red_mask: u32_at(bytes, 92),
        green_mask: u32_at(bytes, 96),
        blue_mask: u32_at(bytes, 100),
        alpha_mask: u32_at(bytes, 104),
    };
    let caps2 = u32_at(bytes, 112);

    let (dx10, data_offset) = if pixel_format.is_four_cc() && &four_cc == b"DX10" {
        if bytes.len() < 148 {
            return Err(HeaderError::TruncatedDx10Header { bytes: bytes.len() });
        }
        (
            Some(Dx10 {
                dxgi_format: u32_at(bytes, 128),
                resource_dimension: u32_at(bytes, 132),
                misc_flag: u32_at(bytes, 136),
                array_size: u32_at(bytes, 140),
                misc_flags2: u32_at(bytes, 144),
            }),
            148,
        )
    } else {
        (None, 128)
    };

    if width == 0 || height == 0 {
        // Checked last so that a file which is malformed in several ways is still reported by
        // the fault furthest up the container, rather than by whichever check ran first.
        return Err(HeaderError::ZeroDimensions { width, height });
    }

    Ok(Header {
        width,
        height,
        depth,
        mip_count,
        pixel_format,
        caps2,
        dx10,
        data_offset,
    })
}
