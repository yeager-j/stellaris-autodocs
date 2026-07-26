//! The application-owned types. No decoder type appears here, and `tests/boundary.rs` enforces
//! that with a negative control.
//!
//! This is the shape `docs/technical-design.md:263` requires of the asset module: exactly one
//! typed outcome per requested slot, so `analysis::finalize` can choose a placeholder and scope
//! an Analysis Issue without re-deriving why the conversion failed.

/// A decoded surface: straight-alpha RGBA8, one mip level, one layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, row-major from the top left.
    pub rgba8: Vec<u8>,
}

impl DecodedImage {
    pub fn pixels(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }

    /// Byte length the buffer must have for the declared dimensions.
    pub fn expected_len(&self) -> usize {
        self.pixels() * 4
    }

    pub fn is_well_formed(&self) -> bool {
        self.rgba8.len() == self.expected_len()
    }
}

/// The closed set of outcomes the asset module may return for one slot.
///
/// `MissingBytes` is named here even though this adapter never produces it: the source module
/// owns that case (`docs/technical-design.md:516`), and a closed set that omitted it would let a
/// caller believe four possibilities were three.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Decoded(DecodedImage),
    /// The referenced path produced no bytes. Decided at the source boundary, not here.
    MissingBytes { detail: String },
    /// The bytes are not a readable container, or the container's own declarations are
    /// inconsistent with the bytes behind them.
    MalformedMedia { detail: String },
    /// A well-formed container in a format this adapter does not claim to decode.
    UnsupportedFormat { detail: String },
    /// A supported format whose decode nonetheless failed. Distinct from the two above because
    /// it is the only one that indicts the adapter rather than the input.
    ConversionFailure { detail: String },
}

impl Outcome {
    /// A short stable label, for record artifacts and for grouping.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Decoded(_) => "decoded",
            Self::MissingBytes { .. } => "missing-bytes",
            Self::MalformedMedia { .. } => "malformed-media",
            Self::UnsupportedFormat { .. } => "unsupported-format",
            Self::ConversionFailure { .. } => "conversion-failure",
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Self::Decoded(_) => "",
            Self::MissingBytes { detail }
            | Self::MalformedMedia { detail }
            | Self::UnsupportedFormat { detail }
            | Self::ConversionFailure { detail } => detail,
        }
    }

    pub fn decoded(&self) -> Option<&DecodedImage> {
        match self {
            Self::Decoded(image) => Some(image),
            _ => None,
        }
    }
}

/// How two readings of the same bytes compare.
///
/// The counts matter as much as the verdict. A channel swap makes every pixel differ by a large
/// amount; a rounding disagreement makes a few differ by one. Reporting only "they differed"
/// would leave a reader unable to tell a defect from a tie-break.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Comparison {
    /// Byte-for-byte equal.
    Identical,
    /// Same dimensions, different pixels.
    PixelsDiffer {
        differing_pixels: usize,
        total_pixels: usize,
        max_delta: [u8; 4],
    },
    DimensionsDiffer {
        a: (u32, u32),
        b: (u32, u32),
    },
    /// One path produced an image and the other did not.
    OutcomesDiffer {
        a: &'static str,
        b: &'static str,
    },
    /// Neither path produced an image, and both agreed on the kind of failure.
    BothFailed {
        kind: &'static str,
    },
    /// The comparison does not apply: `decode_b` has no independent reading for this class.
    NotCompared {
        reason: &'static str,
    },
}

impl Comparison {
    pub fn is_divergence(&self) -> bool {
        matches!(
            self,
            Self::PixelsDiffer { .. } | Self::DimensionsDiffer { .. } | Self::OutcomesDiffer { .. }
        )
    }
}

/// Compare two outcomes as the cross-check defines agreement.
pub fn compare(a: &Outcome, b: &Outcome) -> Comparison {
    match (a.decoded(), b.decoded()) {
        (Some(left), Some(right)) => compare_images(left, right),
        (None, None) => {
            if a.kind() == b.kind() {
                Comparison::BothFailed { kind: a.kind() }
            } else {
                Comparison::OutcomesDiffer {
                    a: a.kind(),
                    b: b.kind(),
                }
            }
        }
        _ => Comparison::OutcomesDiffer {
            a: a.kind(),
            b: b.kind(),
        },
    }
}

pub fn compare_images(a: &DecodedImage, b: &DecodedImage) -> Comparison {
    if a.width != b.width || a.height != b.height {
        return Comparison::DimensionsDiffer {
            a: (a.width, a.height),
            b: (b.width, b.height),
        };
    }
    if a.rgba8 == b.rgba8 {
        return Comparison::Identical;
    }

    let mut differing = 0usize;
    let mut max_delta = [0u8; 4];
    for (left, right) in a.rgba8.chunks_exact(4).zip(b.rgba8.chunks_exact(4)) {
        if left == right {
            continue;
        }
        differing += 1;
        for channel in 0..4 {
            let delta = left[channel].abs_diff(right[channel]);
            if delta > max_delta[channel] {
                max_delta[channel] = delta;
            }
        }
    }
    Comparison::PixelsDiffer {
        differing_pixels: differing,
        total_pixels: a.pixels(),
        max_delta,
    }
}
