//! The reachable set: which textures a sprite definition actually names.
//!
//! Exists because 33,118 files is the wrong denominator. A documentation tool converts the
//! textures its content references, and a coverage percentage over every `.dds` on disk is a
//! number about the filesystem rather than about the product. The reachable set is also where
//! `MissingBytes` turns out to occur naturally: vanilla names textures that are not there.
//!
//! This is a measurement aid, not the resolver. Which sprite a documented concept resolves to is
//! `analysis`'s question (`docs/technical-design.md:501`), and this module deliberately does not
//! answer it — it collects every path any sprite definition names, which is a superset.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Keys whose value is a texture path.
///
/// Enumerated from the corpus rather than assumed. An earlier three-key list missed `texturefile2`
/// — which progress-bar sprites use for their second layer — and only found it because a loose
/// substring match happened to catch it inside the longer key. A scan that is right by accident is
/// the failure mode the parser spike's span derivation was written about, so the keys are matched
/// exactly and the list is the measured one.
///
/// The 3D model-material keys — `texture_diffuse`, `texture_specular`, `texture_normal`,
/// `texture_wpo`, and the bare `texture`, `normal`, `specular`, `overlay` — are deliberately
/// excluded, and the exclusion is a measurement rather than a preference. Their values are
/// relative to the file that declares them (`1x1.dds`, `2x2.dds`), not to the content root the
/// sprite keys use. Resolving both against one base would report 2,310 references as missing that
/// are merely addressed differently, which is a number about two path conventions rather than
/// about absent content. They also describe ship and planet models, which this product never
/// renders.
const TEXTURE_KEYS: &[&str] = &[
    "texturefile",
    "texturefile1",
    "texturefile2",
    "animationtexturefile",
    "animationmaskfile",
    "masking_texture",
];

/// Files that can carry a sprite definition.
const DEFINITION_EXTENSIONS: &[&str] = &["gfx", "gui"];

/// Directories whose sprite definitions are game content.
///
/// An allowlist for the same reason `src-tauri/src/source/policy.rs` uses one: the install also
/// contains `pdx_launcher/`, `pdx_online_assets/`, `previewer_assets/`, and `tweakergui_assets/`,
/// whose `.gfx` files describe the Paradox launcher and internal developer tools. Scanning them
/// adds 83 references and 22 dangling paths that say nothing about game content — a number about
/// the launcher's own UI, counted as if it were a documentation gap.
const DEFINITION_DIRECTORIES: &[&str] = &["interface", "gfx"];

#[derive(Debug, Clone, Default)]
pub struct References {
    /// Every distinct `.dds` path named, normalized, sorted.
    pub referenced: BTreeSet<String>,
    /// Named paths with no file behind them: `MissingBytes`, occurring in shipped content.
    pub dangling: BTreeSet<String>,
    /// Paths that needed separator collapsing to resolve. A path-normalization concern for
    /// `analysis`, recorded here so it is not mistaken for an asset fault.
    pub doubled_separators: BTreeSet<String>,
}

/// Collapse repeated separators and normalize to `/`.
///
/// Vanilla writes `gfx//interface//musicplayer/...` in 14 places. Those files exist; the path is
/// merely written twice over. Treating them as missing would manufacture a failure.
pub fn normalize(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut previous_separator = false;
    for character in raw.chars() {
        let separator = character == '/' || character == '\\';
        if separator {
            if !previous_separator {
                out.push('/');
            }
        } else {
            out.push(character);
        }
        previous_separator = separator;
    }
    out
}

/// Scan `definitions` for sprite definitions and resolve what they name against `resolve`.
///
/// `resolve` is a list because a mod's sprite definition may name a vanilla texture: Stellaris
/// resolves a path against the merged content of every loaded source, so checking a mod's
/// references against the mod alone would report most of vanilla's texture set as missing. Which
/// source actually wins is the resolver's question (`docs/technical-design.md:287`); existence
/// anywhere is all this census needs.
pub fn scan(definitions: &Path, resolve: &[&Path]) -> std::io::Result<References> {
    let mut references = References::default();
    if !definitions.is_dir() {
        return Ok(references);
    }

    let mut stack: Vec<PathBuf> = DEFINITION_DIRECTORIES
        .iter()
        .map(|name| definitions.join(name))
        .filter(|path| path.is_dir())
        .collect();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            let is_definition = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    DEFINITION_EXTENSIONS
                        .iter()
                        .any(|known| extension.eq_ignore_ascii_case(known))
                });
            if !is_definition {
                continue;
            }
            // Read lossily: some definition files are not valid UTF-8, and a texture path is
            // ASCII in every observed case. Rejecting the file would lose its other references.
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            collect(&String::from_utf8_lossy(&bytes), &mut references);
        }
    }

    for raw in references.referenced.clone() {
        if !resolve.iter().any(|root| root.join(&raw).exists()) {
            references.dangling.insert(raw);
        }
    }
    Ok(references)
}

/// Pull every `key = "value"` texture path out of one definition file's text.
///
/// A deliberate scan rather than a parse. This module needs the quoted paths behind a known set of
/// keys, and the parser that would do it properly is the subject of another spike. The scan takes
/// the identifier immediately before each `=` and compares it whole, so `textureFile2` is its own
/// key rather than a prefix match on `textureFile`.
fn collect(text: &str, references: &mut References) {
    for line in text.lines() {
        for (index, _) in line.match_indices('=') {
            let Some(key) = identifier_before(line, index) else {
                continue;
            };
            if !TEXTURE_KEYS
                .iter()
                .any(|known| key.eq_ignore_ascii_case(known))
            {
                continue;
            }
            let rest = &line[index + 1..];
            let Some(open) = rest.find('"') else { continue };
            // A quote must be the next non-space thing after `=`; otherwise the quoted string
            // belongs to a later assignment on the same line.
            if rest[..open].bytes().any(|b| !b.is_ascii_whitespace()) {
                continue;
            }
            let value = &rest[open + 1..];
            let Some(close) = value.find('"') else { continue };
            let raw = &value[..close];
            if !raw.to_ascii_lowercase().ends_with(".dds") {
                continue;
            }
            let normalized = normalize(raw);
            if normalized != raw.replace('\\', "/") {
                references.doubled_separators.insert(raw.to_owned());
            }
            references.referenced.insert(normalized);
        }
    }
}

/// The identifier ending just before `index`, if there is one.
fn identifier_before(line: &str, index: usize) -> Option<&str> {
    let head = line[..index].trim_end();
    let start = head
        .char_indices()
        .rev()
        .take_while(|(_, character)| character.is_ascii_alphanumeric() || *character == '_')
        .map(|(offset, _)| offset)
        .last()?;
    Some(&head[start..])
}
