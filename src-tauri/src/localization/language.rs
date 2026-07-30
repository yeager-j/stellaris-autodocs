//! What a Stellaris language token is, and nothing else.
//!
//! The game spells a language `l_<name>` in three places this application reads: the
//! `language` assignment in `settings.txt`, the header of every localization `.yml`, and the
//! `localisation/<name>/` directory that holds them. One type, so those three do not each
//! carry their own reading of the same handful of bytes.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A Stellaris language token, prefix included: `l_english`, `l_simp_chinese`.
///
/// **The parse is syntactic, not a closed set of the ten languages the shipped game has.** A
/// translation mod adds a language by adding a `languages.yml` entry and an `l_<name>` header,
/// so enumerating the ten would make a mod-added language indistinguishable from a corrupt
/// value. The design also requires an effective language no revision can serve to still *be*
/// the effective language: "If the effective language is absent from one revision or
/// localization key, the localization module still applies the independent selected-language,
/// English, then raw-key fallback" (docs/technical-design.md, "Localization module"). Which
/// languages a given revision actually contains is a different question, answered by the
/// localization tables.
///
/// `Serialize` without `Deserialize` is deliberate. The only decode site is the persisted
/// language override, which decodes leniently through
/// [`language_override_from_document`]; a strict `Deserialize` would exist solely to be a trap
/// for the state document.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct LanguageTag(String);

/// The longest token accepted. The shipped game's longest is `l_simp_chinese` at 14 bytes; the
/// bound exists so a corrupt `settings.txt` cannot turn a megabyte of bytes into a durable
/// preference, not because 64 means anything.
const MAX_TAG_LEN: usize = 64;

const PREFIX: &str = "l_";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageTagError;

impl fmt::Display for LanguageTagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid language: expected an l_ prefix followed by lowercase ASCII")
    }
}

impl std::error::Error for LanguageTagError {}

impl LanguageTag {
    pub fn parse(text: &str) -> Result<Self, LanguageTagError> {
        let name = text.strip_prefix(PREFIX).ok_or(LanguageTagError)?;
        let acceptable = !name.is_empty()
            && text.len() <= MAX_TAG_LEN
            && name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
        acceptable
            .then(|| Self(text.to_owned()))
            .ok_or(LanguageTagError)
    }

    /// The fallback the effective-language derivation ends at, as a value rather than a literal
    /// repeated at each of its use sites.
    pub fn english() -> Self {
        Self("l_english".to_owned())
    }

    /// `l_english` — the form `settings.txt` and a `.yml` header both use.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `english` — the form the `localisation/<name>/` directory uses.
    pub fn name(&self) -> &str {
        &self.0[PREFIX.len()..]
    }
}

impl fmt::Display for LanguageTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Decodes the persisted language override, treating an unreadable value as absent.
///
/// **A deliberate departure from
/// [`DiscoveryLocationId`](crate::discovery::identity::DiscoveryLocationId), which quarantines
/// the whole document on a malformed value.** An identifier is machine-generated and nobody
/// hand-edits it, so a malformed one is genuine corruption. A language name is the one field in
/// the state document a curious person would edit, and `"english"` for `"l_english"` must not
/// cost them their publication references, their orphan-cleanup eligibility, and a recovery
/// screen (docs/technical-design.md, "State evolution and recovery"). An unreadable value
/// therefore decodes as absent and the derivation falls through to detection, exactly as it
/// would for a user who never set one.
///
/// A non-string is still a decode error: the leniency is about the value, not the type.
pub fn language_override_from_document<'de, D>(
    deserializer: D,
) -> Result<Option<LanguageTag>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?
        .as_deref()
        .and_then(|raw| LanguageTag::parse(raw).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Document {
        #[serde(default, deserialize_with = "language_override_from_document")]
        language: Option<LanguageTag>,
    }

    fn decode(json: &str) -> Result<Option<LanguageTag>, serde_json::Error> {
        serde_json::from_str::<Document>(json).map(|document| document.language)
    }

    #[test]
    fn a_language_tag_round_trips_the_two_forms_the_game_uses() {
        let tag = LanguageTag::parse("l_simp_chinese").unwrap();
        assert_eq!(tag.as_str(), "l_simp_chinese");
        assert_eq!(tag.name(), "simp_chinese");
        assert_eq!(serde_json::to_string(&tag).unwrap(), "\"l_simp_chinese\"");
    }

    #[test]
    fn every_language_the_shipped_game_has_parses() {
        // The ten tags in <install>/localisation/languages.yml on a 4.4.6 install.
        for tag in [
            "l_english",
            "l_braz_por",
            "l_german",
            "l_french",
            "l_spanish",
            "l_polish",
            "l_russian",
            "l_simp_chinese",
            "l_japanese",
            "l_korean",
        ] {
            assert_eq!(LanguageTag::parse(tag).unwrap().as_str(), tag);
        }

        // Negative control for the table above: a green list of ten proves nothing unless
        // `parse` is capable of refusing something, and the length bound is the rule with no
        // other test of its own. Both sides of the boundary, so the bound is a bound.
        let longest = format!("l_{}", "a".repeat(MAX_TAG_LEN - PREFIX.len()));
        assert_eq!(LanguageTag::parse(&longest).unwrap().as_str(), longest);
        assert_eq!(
            LanguageTag::parse(&format!("{longest}a")),
            Err(LanguageTagError)
        );
    }

    #[test]
    fn a_mod_added_language_parses_and_a_malformed_one_does_not() {
        // Community translations ship languages the base game has never heard of; refusing
        // them here would make the whole feature vanilla-only.
        assert_eq!(LanguageTag::parse("l_klingon").unwrap().name(), "klingon");
        for rejected in [
            "",
            "l_",
            "english",
            "L_English",
            "l_English",
            "l_english ",
            " l_english",
            "l_ english",
            "l_english\n",
            "l_日本語",
            "l_english-uk",
        ] {
            assert_eq!(
                LanguageTag::parse(rejected),
                Err(LanguageTagError),
                "{rejected:?} should not parse"
            );
        }
    }

    #[test]
    fn english_is_a_value_rather_than_a_literal() {
        assert_eq!(LanguageTag::english().as_str(), "l_english");
        assert_eq!(
            LanguageTag::english(),
            LanguageTag::parse("l_english").unwrap()
        );
    }

    #[test]
    fn an_unreadable_persisted_override_decodes_as_absent_rather_than_failing() {
        assert_eq!(decode(r#"{}"#).unwrap(), None);
        assert_eq!(decode(r#"{"language":null}"#).unwrap(), None);
        assert_eq!(
            decode(r#"{"language":"l_french"}"#).unwrap(),
            Some(LanguageTag::parse("l_french").unwrap())
        );
        // The hand-edit this leniency exists for, and its blank neighbour.
        assert_eq!(decode(r#"{"language":"english"}"#).unwrap(), None);
        assert_eq!(decode(r#"{"language":""}"#).unwrap(), None);
        // A non-string is a decode error: leniency is about the value, not the type.
        assert!(decode(r#"{"language":123}"#).is_err());
    }

    #[test]
    fn language_tag_error_displays_and_implements_std_error() {
        // Mirrors identity.rs's id_parse_error_displays_and_implements_std_error: call sites
        // want to `?` or log a parse failure, and both need these impls.
        fn assert_error<E: std::error::Error>(_: &E) {}
        assert_error(&LanguageTagError);
        assert!(!LanguageTagError.to_string().is_empty());
    }
}
