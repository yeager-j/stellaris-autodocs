//! The application-owned display-token vocabulary.
//!
//! This is the contract reference resolution, plain-text projection, and the React renderer
//! consume instead of Stellaris markup. Nothing downstream reads a localization value again,
//! which is what lets the frontend map tokens to controlled components and CSS rather than
//! parsing strings (docs/technical-design.md, "Localization module").
//!
//! Two properties are worth stating before the types, because both are load-bearing and
//! neither is guessable.
//!
//! **Form, never knownness.** A `§` code is proven to be one character the engine could look
//! up, not one it will find: `interface/fonts.gfx` defines `bitmapfonts.textcolors` as a data
//! table, mods redefine it, and the installed vanilla build declares 30 codes while its own
//! text uses 29. An allowlist derived from either would render a mod's legitimate code as
//! garbage. The same reasoning governs [`TokenKind::Icon`] names and [`TokenKind::Reference`]
//! keys: mods define their own, so this module proves shape and leaves existence to the phase
//! that holds the table.
//!
//! **Owned text, no lifetime.** Every payload is a `String`. A revision preserves only the
//! localization its documentation cites plus that set's static-reference closure — 1.74 to
//! 2.59 MiB measured, not the 151 to 178 MiB tables — because ADR 0009's shared Localization
//! Store is not built, and every consumer needs owned text at its boundary regardless.
//! Borrowing would buy nothing measurable and would put a lifetime in the interface every
//! later phase inherits. If some content type ever balloons preserved localization by orders
//! of magnitude, changing this is contained: the vocabulary is the module's interface.

/// Half-open byte offsets into the value the token was produced from.
///
/// Both ends fall on `char` boundaries, because every marker this module recognizes is a
/// whole character. `usize` rather than the parser model's `u64`: this indexes one value
/// rather than a file, and it is never hashed and never crosses a process boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::localization) struct TextSpan {
    pub start: usize,
    pub end: usize,
}

impl TextSpan {
    pub fn new(start: usize, end: usize) -> Self {
        debug_assert!(start <= end);
        Self { start, end }
    }

    pub fn len(self) -> usize {
        self.end - self.start
    }

    pub fn slice(self, source: &str) -> Option<&str> {
        source.get(self.start..self.end)
    }
}

/// A `§` code in proven form: one byte from `[A-Za-z0-9_]`.
///
/// Whether the code names a colour is a table lookup this module does not have; see the
/// module header. `§!` is [`TokenKind::StyleReset`] instead, because a reset is not a code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::localization) struct StyleCode(u8);

impl StyleCode {
    pub fn parse(byte: u8) -> Option<Self> {
        (byte.is_ascii_alphanumeric() || byte == b'_').then_some(Self(byte))
    }

    pub fn as_char(self) -> char {
        char::from(self.0)
    }
}

/// Why a span is displayed exactly as it was authored.
///
/// D-041 renders all of these identically today — "the renderer does not attempt to emulate
/// live game scope or silently remove syntax it cannot interpret" — so the arms exist for the
/// reasons that outlive rendering. Each answers a different question a later phase asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(in crate::localization) enum VerbatimKind {
    /// A closed bracket run: `[Root.GetName]`, `[GetOAGalCommImp]`, `[prop&genitive]`,
    /// `[prop::variant|||other]`. A Runtime Localization Token needs live game scope, so its
    /// value is not knowable from a revision at all (`CONTEXT.md`).
    RuntimeToken = 1,
    /// A closed bracket run introduced by a quoted key: `['concept_gateways']`,
    /// `['building:building_x', $display$]`. The boundary is classified and the interior is
    /// deliberately not parsed, which is what makes it structurally impossible for reference
    /// resolution to reach the `$display$` inside one. Interactive concept links are
    /// post-MVP (ADR 0004).
    ConceptLink = 2,
    /// A closed `$@name$` run. The `@` sigil names the scripted-variable namespace, so the
    /// replacement cannot be determined from localization files alone and it is therefore
    /// not a Static Localization Reference (`CONTEXT.md`). Resolution must never see it as a
    /// key.
    ScriptVariable = 3,
    /// A closed `£$NAME$£` run: the icon is selected by a reference, so no icon name exists
    /// to look up here. Modelling it as an [`TokenKind::Icon`] whose name is not a name would
    /// push the case onto every consumer.
    DynamicIcon = 4,
    /// One marker character that begins nothing this module recognizes — an unclosed `£`, `$`
    /// or `[`, or a `§` whose follower is no code. Its span is that single character and
    /// scanning resumes immediately after it, so the rest of the value still tokenizes.
    /// Shipped vanilla files contain all four shapes.
    UnpairedMarker = 5,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::localization) enum TokenKind {
    /// Player-visible text exactly as authored. U+00A0 and U+000A are ordinary characters
    /// here: the no-break space is the `£icon£`-to-label glue idiom and appears 120,488 times
    /// in the installed corpus, so collapsing it would change displayed text. Runs are
    /// maximal — two `Text` tokens never sit next to each other.
    Text {
        text: String,
    },
    /// `§Y`. Sets the current style. Not a span opener; see `markup`'s header on flatness.
    StyleOn {
        code: StyleCode,
    },
    /// `§!`. Returns to the default style.
    StyleReset,
    /// `£energy£`, `£fleet_status|2£`. `variant` selects among an icon's states and is opaque
    /// here; the sprite table that gives either field meaning arrives in Phase 8, and the
    /// readable fallback D-041 promises is `name` itself.
    Icon {
        name: String,
        variant: Option<String>,
    },
    /// `$KEY$`, `$KEY|0Y$`. A reference whose key is well-formed; it becomes a Static
    /// Localization Reference exactly when resolution finds the key, and falls back to raw
    /// when it does not (ADR 0004). `$VALUE$` and `$ORD$` are indistinguishable from
    /// localization keys by syntax, so no token kind can promise more than form.
    ///
    /// `format` is the text after the first `|`, carried opaquely: interpreting `|0=+%` needs
    /// a numeric model that does not exist before Phase 6 and a colour table that belongs
    /// with the CSS.
    Reference {
        key: String,
        format: Option<String>,
    },
    Verbatim {
        kind: VerbatimKind,
        text: String,
    },
}

/// One token and the slice of the value it accounts for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::localization) struct DisplayToken {
    pub span: TextSpan,
    pub kind: TokenKind,
}

impl DisplayToken {
    /// The exact text this token was produced from, re-derived from the payload rather than
    /// read back out of the value.
    ///
    /// This is what makes "nothing is silently discarded" checkable: `spans` asserts that
    /// every token's span cuts precisely this string, so a payload that loses a character
    /// fails even though the span still tiles. Re-deriving is deliberate — reading the slice
    /// would make the check agree with itself.
    pub fn source_form(&self) -> String {
        match &self.kind {
            TokenKind::Text { text } => text.clone(),
            TokenKind::StyleOn { code } => format!("§{}", code.as_char()),
            TokenKind::StyleReset => "§!".to_owned(),
            TokenKind::Icon { name, variant } => match variant {
                Some(variant) => format!("£{name}|{variant}£"),
                None => format!("£{name}£"),
            },
            TokenKind::Reference { key, format } => match format {
                Some(format) => format!("${key}|{format}$"),
                None => format!("${key}$"),
            },
            TokenKind::Verbatim { text, .. } => text.clone(),
        }
    }
}
