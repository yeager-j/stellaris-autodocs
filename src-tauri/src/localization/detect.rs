//! Reading the language the player's Stellaris is currently set to.
//!
//! The game records it as a top-level `language` assignment in its own `settings.txt`. This is
//! the "currently detected Stellaris language" of the effective-language derivation
//! (docs/technical-design.md, "Localization module"; D-097), and it is read fresh rather than
//! persisted: "The detected Stellaris language is refreshed from current game configuration
//! during startup and explicit Refresh rather than copied into the mutable-state authority."
//!
//! The path is a parameter because "where Stellaris keeps things on this platform" belongs to
//! [`discovery::proposals`](crate::discovery::proposals), and taking it as an argument is what
//! keeps this module from acquiring an edge to `discovery` for one `join`.

use crate::localization::language::LanguageTag;
use std::fs;
use std::io;
use std::path::Path;

/// What the current Stellaris configuration says the game's language is.
///
/// Six variants because six different things can be true of `settings.txt`, and the design
/// requires exactly one of them — [`AccessDenied`](Self::AccessDenied) — to be visible to the
/// user: "the desktop shows a non-blocking access notice, falls back to English, and continues
/// to offer an explicit language override". The other non-answers are kept apart from one
/// another because "the file is not there", "it is there and says nothing about language", and
/// "it names a language this build cannot read" are different facts about the machine, and they
/// are the first questions anybody debugging a wrong language asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectedGameLanguage {
    Detected(LanguageTag),

    /// No `settings.txt`. Ordinary rather than exceptional: the game may never have run, or may
    /// not be installed at all.
    SettingsAbsent,

    /// The operating system refused the read. On macOS this is the Documents-folder privacy
    /// condition. **Deliberately not folded into [`SettingsAbsent`](Self::SettingsAbsent):**
    /// absent means the game wrote no setting, denied means it may have written any setting at
    /// all and this process is not permitted to see it — and no retry resolves the second
    /// without user action. That is the argument `source::enumerate::RejectionReason::Unreadable`
    /// makes for carrying `kind`, and this is the condition ADR 0005 requires be visible rather
    /// than mistaken for an absence: "If settings access prevents language detection, the app
    /// shows a non-blocking notice and falls back to the explicit app override or English."
    ///
    /// `detail` is the host's own message: for a log and a notice, never durable and never an
    /// identity.
    AccessDenied {
        detail: String,
    },

    /// The file is there and the read failed for some other reason. `kind` is carried for the
    /// reason `source::RootError::Unreadable` carries it — a caller may tell a transient failure
    /// from a permanent one — and, for the same reason, it never enters an identity or a digest.
    Unreadable {
        kind: io::ErrorKind,
        detail: String,
    },

    /// Read, and holding no top-level `language` assignment.
    LanguageUnset,

    /// A top-level `language` assignment whose value is not a token [`LanguageTag`] accepts.
    /// `raw` is a lossy, length-capped rendering for a human-facing report only — never
    /// identity. `language=""` lands here rather than in [`LanguageUnset`](Self::LanguageUnset):
    /// the file stated a value, and saying so is what tells "the game never chose" from "the
    /// game's choice is unreadable".
    Unrecognized {
        raw: String,
    },
}

/// How much of an unrecognized value is kept for a report. Chosen for the same reason the tag
/// parse has a length bound rather than derived from it — a value longer than a tag can be is
/// already not one, and this one only has to be short enough to log.
const MAX_RAW_LEN: usize = 64;

pub fn detect_language(settings: &Path) -> DetectedGameLanguage {
    match fs::read(settings) {
        Ok(bytes) => language_from_settings(&bytes),
        Err(error) => from_read_error(&error),
    }
}

/// The content half, with no filesystem in it, so every content case — the neighbouring
/// `soundgroup` key, a nested `language`, CRLF, a BOM, comments, quoting — is a byte literal
/// rather than a directory somebody has to stage. The precedent is
/// `discovery::classify_entries`.
pub fn language_from_settings(bytes: &[u8]) -> DetectedGameLanguage {
    match top_level_language(bytes) {
        None => DetectedGameLanguage::LanguageUnset,
        Some(value) => match std::str::from_utf8(value).ok().map(LanguageTag::parse) {
            Some(Ok(tag)) => DetectedGameLanguage::Detected(tag),
            _ => DetectedGameLanguage::Unrecognized {
                raw: lossy_capped(value),
            },
        },
    }
}

/// The one place an `io::Error` becomes a detection outcome. Separated so the
/// `PermissionDenied` classification is provable without a filesystem that can be made to deny
/// — which is what makes it provable on every platform, including the Windows CI leg.
fn from_read_error(error: &io::Error) -> DetectedGameLanguage {
    match error.kind() {
        io::ErrorKind::NotFound => DetectedGameLanguage::SettingsAbsent,
        io::ErrorKind::PermissionDenied => DetectedGameLanguage::AccessDenied {
            detail: error.to_string(),
        },
        kind => DetectedGameLanguage::Unreadable {
            kind,
            detail: error.to_string(),
        },
    }
}

fn lossy_capped(value: &[u8]) -> String {
    // A truncation landing mid-codepoint yields U+FFFD, which is fine for a diagnostic and is
    // why `raw` is documented as never identity.
    String::from_utf8_lossy(&value[..value.len().min(MAX_RAW_LEN)]).into_owned()
}

/// Extracts the value of the first top-level `language` assignment.
///
/// **This is not a Clausewitz parser and must not become one.** It finds one assignment and
/// understands nothing else, which is the whole reason it is allowed to exist beside the real
/// parser in `analysis` — a parser whose seam is private, takes a `SourceIdentity` no caller has
/// here, and whose dialect lexer carries a corpus-conformance obligation
/// (src-tauri/AGENTS.md, "Building and running"). Startup language detection is the wrong second
/// consumer for that.
///
/// Three rules carry the correctness:
///
/// - **Whole-token key match.** The real file holds `language="l_english"` and, thirty lines
///   later, `soundgroup="l_english"`. Those two can disagree, so a substring or `l_[a-z_]+`
///   match returns a plausible wrong answer rather than an obvious failure.
/// - **Depth zero only.** The real file holds a `graphics={ … }` block; a future build putting
///   `language` inside one must not be read as though it were the game's setting.
/// - **First one wins.** An arbitrary tie-break over a shape the game does not produce. No
///   oracle record settles what the game's own settings reader does with two top-level
///   assignments; a capture over a hand-duplicated `settings.txt` is what would settle it.
fn top_level_language(bytes: &[u8]) -> Option<&[u8]> {
    // Paradox tooling emits BOMs (the shipped `languages.yml` carries one); `settings.txt`
    // currently does not, and stripping is three bytes of insurance.
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);

    let mut depth: u32 = 0;
    let mut index = 0;
    // The last `[A-Za-z0-9_.]` run seen, which is the key if the next non-space byte is `=`.
    let mut token: Option<(usize, usize)> = None;

    while index < bytes.len() {
        match bytes[index] {
            // `\r` is whitespace here rather than a line-splitting concern, so a lone `\r`, a
            // lone `\n`, and the real file's `\r\n` all behave.
            b'#' => index = end_of_line(bytes, index),
            b'"' => {
                // A newline ends an unterminated string, so one stray quote cannot swallow the
                // rest of the file.
                let close = end_of_string(bytes, index + 1);
                index = if bytes.get(close) == Some(&b'"') {
                    close + 1
                } else {
                    close
                };
                token = None;
            }
            b'{' => {
                depth += 1;
                index += 1;
                token = None;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                index += 1;
                token = None;
            }
            b'=' => {
                let key = token.take();
                index += 1;
                if depth == 0 && key.is_some_and(|(start, end)| &bytes[start..end] == b"language") {
                    return Some(assignment_value(bytes, index));
                }
            }
            byte if byte.is_ascii_whitespace() => index += 1,
            byte if is_token_byte(byte) => {
                let start = index;
                while index < bytes.len() && is_token_byte(bytes[index]) {
                    index += 1;
                }
                token = Some((start, index));
            }
            _ => {
                index += 1;
                token = None;
            }
        }
    }
    None
}

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.'
}

/// The value of an assignment whose `=` has just been consumed: quoted, or a bare run of
/// non-whitespace up to a delimiter. An empty value is returned as empty, which is what makes
/// `language=""` an unrecognized value rather than an unset one.
fn assignment_value(bytes: &[u8], from: usize) -> &[u8] {
    let mut index = from;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    if bytes.get(index) == Some(&b'"') {
        let start = index + 1;
        return &bytes[start..end_of_string(bytes, start)];
    }
    let start = index;
    while index < bytes.len()
        && !bytes[index].is_ascii_whitespace()
        && !matches!(bytes[index], b'{' | b'}' | b'#' | b'=')
    {
        index += 1;
    }
    &bytes[start..index]
}

fn end_of_line(bytes: &[u8], from: usize) -> usize {
    let mut index = from;
    while index < bytes.len() && bytes[index] != b'\n' {
        index += 1;
    }
    index
}

/// The position of the closing quote, or of the newline that ended an unterminated string, or
/// the end of input. Never past the value, so the caller decides whether to step over a quote.
fn end_of_string(bytes: &[u8], from: usize) -> usize {
    let mut index = from;
    while index < bytes.len() && !matches!(bytes[index], b'"' | b'\n') {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tag(text: &str) -> LanguageTag {
        LanguageTag::parse(text).unwrap()
    }

    fn detected(bytes: &[u8]) -> DetectedGameLanguage {
        language_from_settings(bytes)
    }

    #[test]
    fn reads_the_top_level_language_assignment_from_the_real_file_shape() {
        // The installed file's layout: the assignment on line 2, a nested graphics block, and
        // `soundgroup` thirty lines later. CRLF throughout, as the real file has. The two
        // values disagree on purpose — that is what a substring match would get wrong.
        let body = b"force_pow2_textures=no\r\nlanguage=\"l_french\"\r\ngraphics=\r\n{\r\n\tsize=\r\n\t{\r\n\t\tx=1710\r\n\t}\r\n\tvsync=yes\r\n}\r\nsoundgroup=\"l_english\"\r\n";
        assert_eq!(
            detected(body),
            DetectedGameLanguage::Detected(tag("l_french"))
        );
    }

    #[test]
    fn a_neighbouring_key_ending_in_the_same_bytes_is_not_the_language() {
        // Negative control for the decoy: every one of these holds `l_german` and none of them
        // is a language setting.
        for body in [
            &b"soundgroup=\"l_german\"\r\n"[..],
            &b"sound_language=\"l_german\"\r\n"[..],
            &b"language_extra=\"l_german\"\r\n"[..],
            &b"gui_language=\"l_german\"\r\n"[..],
        ] {
            assert_eq!(detected(body), DetectedGameLanguage::LanguageUnset);
        }
    }

    #[test]
    fn a_nested_language_assignment_is_not_the_top_level_one() {
        // Negative control for the depth guard.
        let nested = b"graphics=\r\n{\r\n\tlanguage=\"l_german\"\r\n}\r\n";
        assert_eq!(detected(nested), DetectedGameLanguage::LanguageUnset);

        let mut both = nested.to_vec();
        both.extend_from_slice(b"language=\"l_polish\"\r\n");
        assert_eq!(
            detected(&both),
            DetectedGameLanguage::Detected(tag("l_polish"))
        );
    }

    #[test]
    fn a_byte_order_mark_and_crlf_do_not_hide_the_first_line() {
        let body = b"\xef\xbb\xbflanguage=\"l_english\"\r\n";
        assert_eq!(
            detected(body),
            DetectedGameLanguage::Detected(tag("l_english"))
        );
    }

    #[test]
    fn quoted_unquoted_spaced_and_tab_indented_forms_all_read() {
        for body in [
            &b"language=\"l_korean\""[..],
            &b"language=l_korean"[..],
            &b"language = \"l_korean\"\n"[..],
            &b"\tlanguage\t=\tl_korean\r\n"[..],
        ] {
            assert_eq!(
                detected(body),
                DetectedGameLanguage::Detected(tag("l_korean")),
                "{:?} should read",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn a_commented_out_language_is_not_a_language_and_a_hash_in_quotes_is_not_a_comment() {
        assert_eq!(
            detected(b"# language=\"l_german\"\r\n"),
            DetectedGameLanguage::LanguageUnset
        );
        assert_eq!(
            detected(b"name=\"a#b\"\r\nlanguage=\"l_russian\"\r\n"),
            DetectedGameLanguage::Detected(tag("l_russian"))
        );
    }

    #[test]
    fn the_first_top_level_assignment_wins() {
        // An arbitrary tie-break over a shape the game does not produce, pinned so it is a
        // decision rather than an accident. What would settle it is an oracle capture over a
        // hand-duplicated settings.txt.
        assert_eq!(
            detected(b"language=\"l_spanish\"\r\nlanguage=\"l_polish\"\r\n"),
            DetectedGameLanguage::Detected(tag("l_spanish"))
        );
    }

    #[test]
    fn a_blank_value_is_unrecognized_and_an_absent_key_is_unset() {
        // The "empty and absent conflated" diagnostic, as an assertion: the file stating an
        // empty language is not the file saying nothing about language.
        let blank = detected(b"language=\"\"\r\n");
        assert_eq!(
            blank,
            DetectedGameLanguage::Unrecognized { raw: String::new() }
        );
        assert_ne!(blank, DetectedGameLanguage::LanguageUnset);
        assert_eq!(
            detected(b"force_pow2_textures=no\r\n"),
            DetectedGameLanguage::LanguageUnset
        );
    }

    #[test]
    fn a_non_ascii_or_overlong_value_is_unrecognized_with_a_capped_lossy_raw() {
        let latin1 = detected(b"language=\"l_fran\xe7ais\"\r\n");
        assert!(matches!(latin1, DetectedGameLanguage::Unrecognized { .. }));

        let long = format!("language=\"l_{}\"\r\n", "a".repeat(4096));
        match detected(long.as_bytes()) {
            DetectedGameLanguage::Unrecognized { raw } => assert_eq!(raw.len(), MAX_RAW_LEN),
            other => panic!("expected an unrecognized value, got {other:?}"),
        }
    }

    #[test]
    fn a_stray_quote_does_not_swallow_the_rest_of_the_file() {
        assert_eq!(
            detected(b"title=\"unterminated\r\nlanguage=\"l_japanese\"\r\n"),
            DetectedGameLanguage::Detected(tag("l_japanese"))
        );
        // The value's own closing quote may be the missing one, and the last byte of the value
        // is still part of it.
        assert_eq!(
            detected(b"language=\"l_japanese"),
            DetectedGameLanguage::Detected(tag("l_japanese"))
        );
    }

    #[test]
    fn an_operating_system_refusal_is_a_typed_access_condition_not_an_absent_file() {
        // The condition the design requires be typed: "That condition is not treated as an
        // empty or missing language value." Asserted as three pairwise-distinct outcomes,
        // because the claim is about telling them apart rather than about any one of them.
        let denied = from_read_error(&io::Error::from(io::ErrorKind::PermissionDenied));
        let absent = from_read_error(&io::Error::from(io::ErrorKind::NotFound));
        let broken = from_read_error(&io::Error::from(io::ErrorKind::InvalidData));

        assert!(matches!(denied, DetectedGameLanguage::AccessDenied { .. }));
        assert_eq!(absent, DetectedGameLanguage::SettingsAbsent);
        assert!(matches!(
            broken,
            DetectedGameLanguage::Unreadable {
                kind: io::ErrorKind::InvalidData,
                ..
            }
        ));
        assert_ne!(denied, absent);
        assert_ne!(denied, broken);
        assert_ne!(absent, broken);
    }

    #[test]
    fn an_absent_settings_file_is_absent_rather_than_unreadable() {
        let dir = TempDir::new().unwrap();
        assert_eq!(
            detect_language(&dir.path().join("settings.txt")),
            DetectedGameLanguage::SettingsAbsent
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_settings_file_the_operating_system_refuses_is_an_access_condition() {
        // A faithful proxy for the mapping, not evidence about macOS privacy: a chmod denial
        // is EACCES and a TCC denial is EPERM, and std maps both to PermissionDenied. What
        // this exercises is the arm both land in. The mode is restored before the assertions
        // so a failure cannot leave the file unreadable for TempDir's cleanup, and the file
        // rather than its directory is chmod'ed so the unlink still works.
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let settings = dir.path().join("settings.txt");
        fs::write(&settings, b"language=\"l_english\"\n").unwrap();
        fs::set_permissions(&settings, fs::Permissions::from_mode(0o000)).unwrap();

        if fs::read(&settings).is_ok() {
            // Running with privileges that ignore the mode bits; nothing to observe.
            fs::set_permissions(&settings, fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }
        // Staging actually denied the read. Asserted rather than assumed, because the early
        // return above is invisible in `cargo test` output and a test that skipped every run
        // would look identical to one that passed.
        assert_eq!(
            fs::read(&settings).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );

        let outcome = detect_language(&settings);
        fs::set_permissions(&settings, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(outcome, DetectedGameLanguage::AccessDenied { .. }));
    }
}
