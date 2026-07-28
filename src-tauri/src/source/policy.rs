//! The versioned enumeration policy: which files under a Mod Source root are
//! analysis-relevant (docs/technical-design.md, "Source module", "Source snapshot
//! consistency" step 1).
//!
//! The policy is an allowlist of top-level directories and exact lowercase extensions,
//! taken from the parser spike's corpus conventions (docs/spikes/parser-evaluation.md)
//! and re-verified against the local installation. A denylist would have to keep pace
//! with everything a game install and a Workshop mod happen to contain: licence text,
//! launcher payloads, sound banks, an application bundle, a mod author's stray `.git`
//! directory. All of those hold `.txt` files that were never script.
//!
//! `<install>/checksum_manifest.txt` is explicitly **not** the rule. It declares the
//! game's checksum scope — `common/**.txt`, `common/**.shader`, `common/**.csv`,
//! `events/**.txt`, `map/**.shader`, `map/**.txt` — which omits `interface/`, `gfx/`,
//! `prescripted_countries/` and every localization file. Documentation that ignored
//! those would be missing content the game loads.
//!
//! Two families, because they are two languages with two owners: Clausewitz script
//! belongs to the parser, and `.yml` localization belongs to `localization`
//! (docs/technical-design.md, "Localization module"). Feeding one to the other's reader
//! would manufacture failures that describe neither.
//!
//! Known deliberate exclusions, each a policy-version decision rather than an oversight:
//!
//! - Binary and non-script content under script directories (`.dds`, `.mesh`, `.anim`,
//!   `.shader`, fonts, audio). Referenced source assets are frozen and hashed lazily
//!   through asset requests; the build "does not hash unrelated large binary assets
//!   merely because they are present" (docs/technical-design.md, "Source snapshot
//!   consistency").
//! - `localisation_synced/`, a Paradox convention in other titles. No such directory
//!   exists in the local install or in any of the 30 installed Workshop mods, and the
//!   game's own logs never name one, so including it would be unevidenced.
//! - Case-variant extensions (`.TXT`). No uppercase script or localization extension
//!   occurs anywhere in the local corpora.
//! - `dlc/`. DLC archives supply visual assets, not a script layer.
//!
//! Changing any of this changes which bytes every fingerprint covers, so the change
//! protocol is: bump [`ENUMERATION_POLICY_VERSION`], then re-pin
//! `tests::pinned_policy_surface`. Never the re-pin alone.

use crate::canonical::path::LogicalPath;

/// The version of everything in this module that decides what a fingerprint covers: the
/// allowlists above and the source-fingerprint domain built over them.
///
/// Homed here rather than as a literal in `analysis::AnalysisVersionVector`, which reads
/// it as its `source_enumeration` component. The design's rule is that a semantic change to
/// a component changes its version, and a version that lived away from the policy it names
/// could be forgotten in the commit that changed the policy. This is the coupling
/// `pinned_policy_surface` could previously only ask for in prose.
///
/// - 1: initial policy (Phase 2A).
/// - 2: fingerprint domain `/v2` — each file framed as a nested two-item sequence.
/// - 3: fingerprint domain `/v3` — observation gaps join the content set.
pub const ENUMERATION_POLICY_VERSION: u32 = 3;

/// Which language a selected file is written in, decided once at enumeration so no
/// downstream stage re-derives it from a path (Meyer's Single Choice).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FileFamily {
    /// Clausewitz script, including the root `.mod` descriptor.
    Script,
    /// Stellaris localization YAML.
    Localization,
}

/// Top-level directories holding Clausewitz script. Sorted; kept in step with
/// [`enumerated_root_directories`].
pub const SCRIPT_DIRECTORIES: &[&str] = &[
    "common",
    "events",
    "gfx",
    "interface",
    "map",
    "prescripted_countries",
];

/// Extensions parsed as Clausewitz script inside [`SCRIPT_DIRECTORIES`].
///
/// The list is deliberately not a curation of *parseable* files: `common/` and
/// `interface/` still hold prose such as `common/HOW_TO_MAKE_NEW_SHIPS.txt`. How the
/// parser treats those is a real question about failure isolation, not a nuisance to
/// filter away here.
pub const SCRIPT_EXTENSIONS: &[&str] = &["asset", "gfx", "gui", "txt"];

/// The localization tree. British spelling, as the game ships it.
pub const LOCALIZATION_DIRECTORY: &str = "localisation";

pub const LOCALIZATION_EXTENSIONS: &[&str] = &["yml"];

/// Root-level descriptors only. The root descriptor is what declares `replace_path` and
/// supported versions; a `.mod` file buried inside content describes some other source.
pub const DESCRIPTOR_EXTENSION: &str = "mod";

/// The analysis-relevant family of a logical path, or `None` when the policy excludes it.
///
/// The single authority for the question. The filesystem walk uses
/// [`enumerated_root_directories`] to avoid descending trees that cannot contribute, but
/// nothing is *selected* except here.
pub fn family_for(path: &LogicalPath) -> Option<FileFamily> {
    let Some((first, rest)) = path.as_str().split_once('/') else {
        // A root-level file.
        return (extension_of(path.as_str()) == Some(DESCRIPTOR_EXTENSION))
            .then_some(FileFamily::Script);
    };
    let name = rest.rsplit_once('/').map_or(rest, |(_, name)| name);
    let extension = extension_of(name)?;
    if SCRIPT_DIRECTORIES.contains(&first) && SCRIPT_EXTENSIONS.contains(&extension) {
        return Some(FileFamily::Script);
    }
    if first == LOCALIZATION_DIRECTORY && LOCALIZATION_EXTENSIONS.contains(&extension) {
        return Some(FileFamily::Localization);
    }
    None
}

/// Whether a raw file name in `top_level` could be selected, judged without decoding or
/// normalizing the name. `None` means the source root itself.
///
/// The same decision as [`family_for`], taken one step earlier so the walk can skip the
/// tens of thousands of `.dds` and `.mesh` files under `gfx/` cheaply. It is family-aware
/// rather than a union of every extension, because a name that is *not* valid Unicode can
/// only ever be reported as a rejection, and a rejection makes the whole inventory
/// incomplete. A `.yml` in a script directory would be excluded by the policy anyway;
/// admitting it here would turn a silent exclusion into a completeness failure.
///
/// [`family_for`] remains the only selector: this answers what *could* be selected.
pub fn raw_name_may_be_enumerated(top_level: Option<&str>, raw_name: &[u8]) -> bool {
    let Some(extension) = raw_extension(raw_name) else {
        return false;
    };
    let permitted: &[&str] = match top_level {
        // Only the root descriptor; a `.mod` file inside content describes another source.
        None => std::slice::from_ref(&DESCRIPTOR_EXTENSION),
        Some(LOCALIZATION_DIRECTORY) => LOCALIZATION_EXTENSIONS,
        Some(directory) if SCRIPT_DIRECTORIES.contains(&directory) => SCRIPT_EXTENSIONS,
        // A directory the walk never descends.
        Some(_) => return false,
    };
    permitted
        .iter()
        .any(|candidate| candidate.as_bytes() == extension)
}

/// The bytes after the final `.`, matching [`extension_of`]'s rule on raw bytes.
fn raw_extension(raw_name: &[u8]) -> Option<&[u8]> {
    let dot = raw_name.iter().rposition(|byte| *byte == b'.')?;
    if dot == 0 || dot + 1 == raw_name.len() {
        return None;
    }
    Some(&raw_name[dot + 1..])
}

/// The top-level directories a source walk descends. Everything else under the root is
/// skipped without being read.
pub fn enumerated_root_directories() -> Vec<&'static str> {
    let mut roots = SCRIPT_DIRECTORIES.to_vec();
    roots.push(LOCALIZATION_DIRECTORY);
    roots
}

/// Membership test for [`enumerated_root_directories`], allocation-free because the walk
/// asks it for every entry at the root.
pub fn is_enumerated_root(name: &str) -> bool {
    SCRIPT_DIRECTORIES.contains(&name) || name == LOCALIZATION_DIRECTORY
}

/// The text after the final `.`, or `None` when the name has no dot or ends in one.
/// Dotfiles such as `.gitignore` have no extension, matching `Path::extension`.
fn extension_of(name: &str) -> Option<&str> {
    let (stem, extension) = name.rsplit_once('.')?;
    (!stem.is_empty() && !extension.is_empty()).then_some(extension)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn family(raw: &str) -> Option<FileFamily> {
        family_for(&LogicalPath::parse(raw).unwrap())
    }

    #[test]
    fn script_directories_and_extensions_are_included() {
        assert_eq!(
            family("common/technology/00_phys_tech.txt"),
            Some(FileFamily::Script)
        );
        assert_eq!(family("events/acot_events.txt"), Some(FileFamily::Script));
        assert_eq!(family("interface/topbar.gui"), Some(FileFamily::Script));
        assert_eq!(
            family("gfx/models/ships/corvette.asset"),
            Some(FileFamily::Script)
        );
        assert_eq!(
            family("gfx/interface/icons/icons.gfx"),
            Some(FileFamily::Script)
        );
        assert_eq!(
            family("map/setup_scenarios/a.txt"),
            Some(FileFamily::Script)
        );
        assert_eq!(
            family("prescripted_countries/humans.txt"),
            Some(FileFamily::Script)
        );
    }

    #[test]
    fn localization_files_are_their_own_family() {
        assert_eq!(
            family("localisation/english/l_english.yml"),
            Some(FileFamily::Localization)
        );
        assert_eq!(
            family("localisation/languages.yml"),
            Some(FileFamily::Localization)
        );
        // A `.txt` readme inside localisation is not localization content and is not
        // Clausewitz script either: vanilla ships 99_README_GRAMMAR.txt there.
        assert_eq!(family("localisation/99_README_GRAMMAR.txt"), None);
    }

    #[test]
    fn root_level_descriptors_are_script_and_nested_mod_files_are_not() {
        assert_eq!(family("descriptor.mod"), Some(FileFamily::Script));
        // Only the root descriptor declares `replace_path` and supported versions; a
        // `.mod` file buried in content is not a descriptor of this source.
        assert_eq!(family("common/deep/other.mod"), None);
        assert_eq!(family("stray.txt"), None);
    }

    #[test]
    fn assets_and_non_script_trees_are_excluded() {
        // Binary assets are resolved lazily through asset requests, never hashed merely
        // for being present (docs/technical-design.md, "Source snapshot consistency").
        assert_eq!(family("gfx/models/ships/corvette.dds"), None);
        assert_eq!(family("gfx/models/ships/corvette.mesh"), None);
        assert_eq!(family("gfx/FX/pdxmesh.shader"), None);
        // Directories outside the allowlist: prose and third-party text that were never
        // script (licenses/, pdx_launcher/, sound/), plus repository leftovers mods ship.
        assert_eq!(family("licenses/font_license.txt"), None);
        assert_eq!(family("sound/soundeffects.txt"), None);
        assert_eq!(family("dlc/dlc001_symbols_of_domination/desc.txt"), None);
        assert_eq!(family(".git/COMMIT_EDITMSG"), None);
        assert_eq!(family("README.md"), None);
    }

    #[test]
    fn extension_matching_is_exact_lowercase() {
        // No uppercase script extension occurs in the local corpora (vanilla plus the
        // four pinned Workshop mods); accepting case variants would be an unevidenced
        // widening, and widening is a policy-version change rather than a tweak.
        assert_eq!(family("common/technology/A.TXT"), None);
        assert_eq!(family("common/technology/a.txt"), Some(FileFamily::Script));
    }

    #[test]
    fn enumerated_roots_are_the_directories_the_walk_descends() {
        let roots = enumerated_root_directories();
        assert!(roots.contains(&"common"));
        assert!(roots.contains(&"localisation"));
        assert!(!roots.contains(&"sound"));
        // The allocation-free membership test the walk uses must agree with the list.
        for root in &roots {
            assert!(is_enumerated_root(root), "{root}");
        }
        assert!(!is_enumerated_root("sound"));
        assert!(!is_enumerated_root("dlc"));
    }

    #[test]
    fn the_walk_prefilter_mirrors_the_policy_for_decodable_names() {
        // The prefilter exists so the walk can skip tens of thousands of `.dds` files
        // without decoding their names. It must agree with `family_for` exactly, or it
        // would either hide a selectable file from the authority or admit one the policy
        // excludes.
        let extensions = ["txt", "asset", "gfx", "gui", "yml", "mod", "dds", "md"];
        for directory in enumerated_root_directories() {
            for extension in extensions {
                let name = format!("name.{extension}");
                let logical = LogicalPath::parse(&format!("{directory}/sub/{name}")).unwrap();
                assert_eq!(
                    raw_name_may_be_enumerated(Some(directory), name.as_bytes()),
                    family_for(&logical).is_some(),
                    "{directory}/{name}"
                );
            }
        }
        for extension in extensions {
            let name = format!("name.{extension}");
            let logical = LogicalPath::parse(&name).unwrap();
            assert_eq!(
                raw_name_may_be_enumerated(None, name.as_bytes()),
                family_for(&logical).is_some(),
                "root {name}"
            );
        }
        // A directory the walk never descends selects nothing.
        assert!(!raw_name_may_be_enumerated(Some("sound"), b"effects.txt"));
        assert!(!raw_name_may_be_enumerated(Some("common"), b"LICENSE"));
        assert!(!raw_name_may_be_enumerated(Some("common"), b".DS_Store"));
        assert!(!raw_name_may_be_enumerated(Some("common"), b"trailing."));
    }

    #[test]
    fn the_walk_prefilter_is_family_aware_for_undecodable_names() {
        // The pure seam for the one case a filesystem here cannot stage. A name that is
        // not valid Unicode can never become an identity, so it can only be reported as a
        // rejection — and a rejection makes the whole inventory incomplete. That must not
        // happen for a file the policy would have excluded anyway: policy exclusions are
        // silent by design, and that rule has to apply before identity rejection.
        //
        // `.yml` in a script directory and `.txt` in the localization directory are the
        // shapes a family-blind extension union would wrongly admit.
        assert!(!raw_name_may_be_enumerated(
            Some("common"),
            b"bad\xffname.yml"
        ));
        assert!(!raw_name_may_be_enumerated(
            Some("localisation"),
            b"bad\xffname.txt"
        ));
        assert!(!raw_name_may_be_enumerated(None, b"bad\xffname.txt"));
        // Selectable in context: still admitted, so it is still rejected visibly rather
        // than dropped.
        assert!(raw_name_may_be_enumerated(
            Some("common"),
            b"bad\xffname.txt"
        ));
        assert!(raw_name_may_be_enumerated(
            Some("localisation"),
            b"bad\xffname.yml"
        ));
        assert!(raw_name_may_be_enumerated(None, b"bad\xffname.mod"));
    }

    #[test]
    fn pinned_policy_surface() {
        // A two-sided tripwire, not a derivation. It pins the allowlists and the version
        // side by side so an edit to either fails a test whose comment states the protocol;
        // it cannot *make* the bump happen, and a developer who edits an allowlist can
        // satisfy it by re-pinning the allowlist alone. A semantic policy change that
        // touches no constant here — `family_for` starting to accept nested `.mod` files,
        // say — does not fail this test at all.
        //
        // What Phase 2B did make mechanical is narrower and worth stating exactly: the
        // version has one home, so `analysis` and `source` cannot drift apart (Meyer's
        // Single Choice). Phase 2A kept a literal in `analysis` that nothing tied to this
        // module.
        //
        // Bumping the version re-pins `AnalysisVersionVector`'s digest, which is the review
        // moment the version vector exists to force. Re-pinning either side alone silently
        // changes what every fingerprint covers.
        //
        // Grounding (docs/spikes/parser-evaluation.md, verified against the local
        // install): an allowlist, not a denylist, because the install also contains
        // licenses/, pdx_launcher/, sound/ and an application bundle full of .txt files
        // that were never script. The checksum manifest is explicitly NOT the rule: it
        // declares a narrower checksum scope (common/**.txt, common/**.shader,
        // common/**.csv, events/**.txt, map/**.shader, map/**.txt) that omits
        // interface/, gfx/, prescripted_countries/ and all localization.
        assert_eq!(
            SCRIPT_DIRECTORIES,
            &[
                "common",
                "events",
                "gfx",
                "interface",
                "map",
                "prescripted_countries",
            ]
        );
        assert_eq!(SCRIPT_EXTENSIONS, &["asset", "gfx", "gui", "txt"]);
        assert_eq!(LOCALIZATION_DIRECTORY, "localisation");
        assert_eq!(LOCALIZATION_EXTENSIONS, &["yml"]);
        assert_eq!(DESCRIPTOR_EXTENSION, "mod");
        assert_eq!(ENUMERATION_POLICY_VERSION, 3);
    }
}
