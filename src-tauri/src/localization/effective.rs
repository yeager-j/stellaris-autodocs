//! The effective desktop language, as one total function of the two things that decide it.
//!
//! ```text
//! explicit app override
//!     -> currently detected Stellaris language
//!     -> English
//! ```
//!
//! "Only the explicit override is durable app state" (docs/technical-design.md, "Localization
//! module"; D-097), which is why this module reads nothing: it takes the stored override and a
//! detection outcome and returns a value. Nothing here caches, so "without an override, a later
//! game-language change changes the effective documentation language automatically" is a
//! property of the code rather than a promise about an invalidation path.
//!
//! This is **not** the selected-language → English → raw-key fallback that a missing revision or
//! key goes through; the design keeps the two chains independent, and that one is key
//! resolution's.

use crate::localization::detect::DetectedGameLanguage;
use crate::localization::language::LanguageTag;

/// Which of the three sources supplied the effective language.
///
/// Provenance rather than a bare tag, for the reason the resolver's `EffectiveField` carries a
/// `FactKind`: an override of `l_english`, a detected `l_english`, and the English fallback are
/// one tag and three different situations, and Phase 10's "use the game's language" control has
/// to tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageSource {
    ExplicitOverride,
    DetectedGameLanguage,
    EnglishFallback,
}

/// The language documentation is read in, which of the three sources decided it, and what
/// detection said.
///
/// **Fields are private with one constructor** ([`derive_effective_language`]) because two of the
/// states this shape can spell are nonsense: [`LanguageSource::ExplicitOverride`] with no
/// override stored, and [`LanguageSource::EnglishFallback`] beside a
/// [`DetectedGameLanguage::Detected`] detection. The derivation is the invariant, so the
/// derivation is the only thing that may build one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveLanguage {
    language: LanguageTag,
    source: LanguageSource,
    /// The detection this derivation ran against, kept whole rather than reduced to a notice
    /// flag. The desktop must show the Documents-access notice **even when an explicit override
    /// decided the language** — the design has it "show a non-blocking access notice, fall back
    /// to English, and continue to offer an explicit language override" — so folding the
    /// condition into `source` would let an override silently erase it.
    detected: DetectedGameLanguage,
}

impl EffectiveLanguage {
    pub fn language(&self) -> &LanguageTag {
        &self.language
    }

    pub fn source(&self) -> LanguageSource {
        self.source
    }

    pub fn detected(&self) -> &DetectedGameLanguage {
        &self.detected
    }

    /// The non-blocking access notice, and the only condition that earns one. Present
    /// independently of [`source`](Self::source): an override decides the language and does not
    /// answer whether the app could read the game's configuration.
    pub fn configuration_access_denied(&self) -> Option<&str> {
        match &self.detected {
            DetectedGameLanguage::AccessDenied { detail } => Some(detail),
            _ => None,
        }
    }
}

/// The derivation order, as one total function of its two inputs and nothing else.
pub fn derive_effective_language(
    explicit_override: Option<LanguageTag>,
    detected: DetectedGameLanguage,
) -> EffectiveLanguage {
    // The five non-detected outcomes collapse to English exactly once, here, so no caller
    // re-decides which of them counts as "no language".
    let (language, source) = match (explicit_override, &detected) {
        (Some(chosen), _) => (chosen, LanguageSource::ExplicitOverride),
        (None, DetectedGameLanguage::Detected(game)) => {
            (game.clone(), LanguageSource::DetectedGameLanguage)
        }
        (None, _) => (LanguageTag::english(), LanguageSource::EnglishFallback),
    };
    EffectiveLanguage {
        language,
        source,
        detected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(text: &str) -> LanguageTag {
        LanguageTag::parse(text).unwrap()
    }

    fn every_detection() -> Vec<DetectedGameLanguage> {
        vec![
            DetectedGameLanguage::Detected(tag("l_french")),
            DetectedGameLanguage::SettingsAbsent,
            DetectedGameLanguage::AccessDenied {
                detail: "operation not permitted".to_owned(),
            },
            DetectedGameLanguage::Unreadable {
                kind: std::io::ErrorKind::InvalidData,
                detail: "bad".to_owned(),
            },
            DetectedGameLanguage::LanguageUnset,
            DetectedGameLanguage::Unrecognized {
                raw: "english".to_owned(),
            },
        ]
    }

    #[test]
    fn the_derivation_order_is_override_then_detection_then_english() {
        for detection in every_detection() {
            let overridden = derive_effective_language(Some(tag("l_polish")), detection.clone());
            assert_eq!(overridden.language(), &tag("l_polish"));
            assert_eq!(overridden.source(), LanguageSource::ExplicitOverride);

            let derived = derive_effective_language(None, detection.clone());
            match &detection {
                DetectedGameLanguage::Detected(game) => {
                    assert_eq!(derived.language(), game);
                    assert_eq!(derived.source(), LanguageSource::DetectedGameLanguage);
                }
                _ => {
                    assert_eq!(derived.language(), &LanguageTag::english());
                    assert_eq!(derived.source(), LanguageSource::EnglishFallback);
                }
            }
        }
    }

    #[test]
    fn an_override_of_english_is_not_the_english_fallback() {
        // Negative control for the provenance field: these two agree on the tag and differ on
        // how it was decided, so a test asserting only the tag could not tell them apart —
        // which is the whole reason LanguageSource exists.
        let chosen = derive_effective_language(
            Some(LanguageTag::english()),
            DetectedGameLanguage::LanguageUnset,
        );
        let fallen = derive_effective_language(None, DetectedGameLanguage::LanguageUnset);
        assert_eq!(chosen.language(), fallen.language());
        assert_ne!(chosen.source(), fallen.source());
    }

    #[test]
    fn an_explicit_override_does_not_erase_the_access_notice() {
        let effective = derive_effective_language(
            Some(tag("l_polish")),
            DetectedGameLanguage::AccessDenied {
                detail: "operation not permitted".to_owned(),
            },
        );
        assert_eq!(effective.language(), &tag("l_polish"));
        assert_eq!(effective.source(), LanguageSource::ExplicitOverride);
        assert_eq!(
            effective.configuration_access_denied(),
            Some("operation not permitted")
        );
    }

    #[test]
    fn only_a_refusal_earns_a_notice() {
        // Including Unreadable, which the design does not grant one: a broken settings file is
        // not an access limitation to report to the user.
        for detection in every_detection() {
            let denied = matches!(detection, DetectedGameLanguage::AccessDenied { .. });
            let effective = derive_effective_language(None, detection);
            assert_eq!(effective.configuration_access_denied().is_some(), denied);
        }
    }

    #[test]
    fn detection_is_carried_whole_rather_than_reduced() {
        // Phase 10 loses nothing by an override winning: the condition it has to report is
        // still the exact value detection produced.
        let detection = DetectedGameLanguage::Unrecognized {
            raw: "english".to_owned(),
        };
        let effective = derive_effective_language(Some(tag("l_german")), detection.clone());
        assert_eq!(effective.detected(), &detection);
    }
}
