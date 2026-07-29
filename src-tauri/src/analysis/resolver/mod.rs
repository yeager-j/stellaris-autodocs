//! The two-contributor resolver: which definition does the game actually use, and where did
//! every fact in it come from.
//!
//! # The contract
//!
//! `analysis` hands the resolver one Vanilla Content and one Target Mod Source Snapshot and
//! asks a declared registry for its effective content. The answer is a [`ResolvedRegistry`]
//! — definitions addressed by key, with per-field provenance — or a typed [`Refusal`].
//! There is no third outcome. A registry nobody has declared, and a policy cell no oracle
//! record settles, both refuse; neither borrows a neighbouring row's rule, falls back to a
//! generic merge, or picks a universal first- or last-wins answer (D-098).
//!
//! # Two contributors are not two layers
//!
//! This is the finding the whole module is shaped around
//! (`docs/spikes/resolver-evaluation.md`, "There is no layer precedence. There is one path
//! order."). `r10-loadorder` gave one mod an early-sorting filename and applied it to a
//! first-wins registry and a last-wins registry at once. Under a layer model the mod
//! registers after Vanilla in both cases and therefore wins the last-wins registry and loses
//! the first-wins one. The exact opposite happened: the mod won the *event* and lost the
//! *technology*.
//!
//! So the model is:
//!
//! ```text
//! 1. Resolve same-path file collisions   (Target Mod's file replaces Vanilla's)
//! 2. Enumerate every surviving file in one global normalized-path order
//! 3. Within a file, take definitions in source order
//! 4. On a repeat registration, apply the registry's rule: replace, or reject
//! ```
//!
//! Only step 1 mentions a layer, and only because identical paths cannot be ordered against
//! each other. [`SourceKind`] appears downstream of it in provenance and in the one
//! cross-source cell a row must declare — never as a tiebreak. Cross-source precedence is a
//! *consequence* of steps 2 and 4, which is why no row states a bare precedence.
//!
//! # Module shape
//!
//! - [`selection`] — step 1: `replace_path` exclusion and exact-path collision.
//! - [`stream`] — step 2: per-family semantic streams. Script and sprite share one global
//!   path order; localization has its own Vanilla → mod → `replace/` stream.
//! - [`registry`] — steps 3 and 4, plus the eight-cell row vocabulary and the refusals.
//! - [`constants`] and [`inline_scripts`] — the two substitution mechanisms a consuming row
//!   may declare: a scripted-constant environment looked up per symbol, and a path-addressed
//!   library of fragments spliced into the effective fields of the rows that include them.
//! - [`resolved`] — what leaves: effective definitions, the five provenance kinds, and the
//!   references a row detected without resolving.
//! - [`profile`] — the profile's version, its pinned game build, and its declared rows.
//! - `oracle` (tests) — the captured oracle records, consumed as machine-checked
//!   expectations, with the drift gate that blocks a silent game-build change.
//!
//! # What does not leave
//!
//! Parser-library types stop in `super::parser` (its own gate). The *file-level* parse model
//! stops here: nothing in [`resolved`] names a whole-file type, its faults, or its per-file
//! evidence bookkeeping, so a consumer cannot come to depend on how many files were read or
//! which recovered from a syntax fault. Definition bodies remain the application-owned
//! parsed value model, because re-modelling a Clausewitz value here would be a second
//! authority on what one is.

mod constants;
mod inline_scripts;
mod profile;
mod registry;
mod resolved;
mod selection;
mod sprites;
mod stream;

#[cfg(test)]
mod oracle;
#[cfg(test)]
mod trial;

pub(in crate::analysis) use profile::RESOLUTION_PROFILE_VERSION;
pub(in crate::analysis) use registry::Refusal;
// Re-exported as the module's product surface, ahead of the Phase 6 consumer: these are the
// types a documentation generator reads, and naming them here is what makes "resolved
// registries are application-owned types" checkable at the module boundary rather than by
// following imports into submodules.
#[allow(unused_imports)]
pub(in crate::analysis) use resolved::{
    ConstantFact, ConstantOutcome, FactKind, FactProvenance, FactSite, InlineOutcome,
    InlineScriptFact, LocalizationFile, LocalizationFileStream, ReferenceFact, ReferenceKind,
    ResolvedDefinition, ResolvedRegistry, ResolvedSpriteTexture, ShadowedLocalizationFile,
    SpriteReferenceEdge, SpriteResolution, SpriteTextureOutcome, StreamPosition,
    UnresolvedConstant, UnresolvedInline,
};

use crate::source::{SourceKind, SourceSnapshot};

use selection::FileSelection;
use stream::StreamEntry;

use super::parser::{self, ParsedFile, SourceIdentity};

/// One build's two contributors, with common file selection already applied.
///
/// Built once per build: file selection is family-independent and every registry reads the
/// same surviving set, so deriving it per registry would be the same answer computed many
/// times from the same inputs.
pub(in crate::analysis) struct Resolution<'a> {
    vanilla: &'a SourceSnapshot,
    target: &'a SourceSnapshot,
    selection: FileSelection,
}

pub(in crate::analysis) fn resolve<'a>(
    vanilla: &'a SourceSnapshot,
    target: &'a SourceSnapshot,
) -> Resolution<'a> {
    debug_assert_eq!(vanilla.kind(), SourceKind::VanillaContent);
    debug_assert_eq!(target.kind(), SourceKind::TargetMod);
    Resolution {
        selection: selection::select(vanilla, target),
        vanilla,
        target,
    }
}

impl Resolution<'_> {
    /// Localization files in the exact order Phase 5 must ingest them.
    ///
    /// This stops before `.yml` interpretation. In particular it does not merge keys, resolve
    /// `$key$` references, or infer which keys a shadowed file lost; those decisions belong to
    /// the localization module that consumes this byte stream.
    pub fn localization_files(&self) -> Result<LocalizationFileStream, Refusal> {
        if let Some(declaration) = self.selection.unusable_declarations().first() {
            return Err(Refusal::UnusableReplacePath {
                declaration: declaration.clone(),
            });
        }

        let entries = stream::build(
            &self.selection,
            stream::ContentFamily::Localization,
            stream::LOCALIZATION_SCOPE,
        );
        let files = entries
            .into_iter()
            .map(|entry| {
                let bytes = self
                    .read(&entry)
                    .expect("a selected file remains readable from its immutable snapshot");
                LocalizationFile {
                    order: entry.order,
                    source: entry.source,
                    logical: entry.logical,
                    bytes,
                }
            })
            .collect();
        let shadowed_files = stream::shadowed_files(&self.selection, stream::LOCALIZATION_SCOPE)
            .into_iter()
            .map(|provenance| {
                let FactSite::RemovedBySelection {
                    source, logical, ..
                } = &provenance.site
                else {
                    unreachable!("the file-shadow projection produced a non-file fact");
                };
                let bytes = self
                    .read_source(*source, logical)
                    .expect("a removed file remains readable from its immutable snapshot");
                ShadowedLocalizationFile { provenance, bytes }
            })
            .collect();

        Ok(LocalizationFileStream {
            files,
            shadowed_files,
        })
    }

    /// The effective content of one declared registry.
    ///
    /// Files are parsed on demand, per call. Deliberately not cached: no consumer asks for
    /// the same registry twice today, and a cache would be a second authority on what a file
    /// says for the sake of a cost nothing has measured. Phase 4M's whole-corpus run is what
    /// would justify one.
    pub fn registry(&self, name: &str) -> Result<ResolvedRegistry, Refusal> {
        let Some(policy) = profile::declared(name) else {
            return Err(Refusal::UndeclaredRegistry {
                registry: name.to_owned(),
            });
        };
        self.resolve_row(policy)
    }

    fn resolve_row(&self, policy: &registry::RegistryPolicy) -> Result<ResolvedRegistry, Refusal> {
        registry::resolve(policy, &self.selection, |entry| self.parse(entry))
    }

    fn read(&self, entry: &StreamEntry) -> Option<crate::source::SourceBytes> {
        self.read_source(entry.source, &entry.logical)
    }

    fn read_source(
        &self,
        source: SourceKind,
        logical: &crate::canonical::path::LogicalPath,
    ) -> Option<crate::source::SourceBytes> {
        let snapshot = match source {
            SourceKind::VanillaContent => self.vanilla,
            SourceKind::TargetMod => self.target,
        };
        snapshot.read(logical)
    }

    fn parse(&self, entry: &StreamEntry) -> Option<ParsedFile> {
        let bytes = self.read(entry)?;
        let identity = SourceIdentity::new(entry.source, entry.logical.clone());
        Some(parser::parse(identity, bytes.as_slice()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::fixture::FixtureCorpus;

    #[test]
    fn an_undeclared_registry_refuses_by_name() {
        // Phase 4J declares localization file selection, not the `.yml` key registry Phase 5
        // will build from it. Asking for a registry must therefore keep refusing rather than
        // turning the file-level support into a false per-key claim.
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .with_file("common/technology/00_vanilla.txt", b"tech_a = {}")
            .build()
            .expect("a fixture corpus");
        let target = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"name=\"m\"")
            .build()
            .expect("a fixture corpus");

        let resolution = resolve(&vanilla, &target);
        assert_eq!(
            resolution.registry("localization"),
            Err(Refusal::UndeclaredRegistry {
                registry: "localization".to_owned()
            })
        );
    }

    #[test]
    fn localization_files_refuse_an_unusable_replace_path() {
        let vanilla = FixtureCorpus::new(SourceKind::VanillaContent)
            .with_file(
                "localisation/english/base_l_english.yml",
                b"l_english:\n base:0 \"Base\"",
            )
            .build()
            .expect("a fixture corpus");
        let target = FixtureCorpus::new(SourceKind::TargetMod)
            .with_file("descriptor.mod", b"replace_path = { invalid }")
            .with_file(
                "localisation/english/mod_l_english.yml",
                b"l_english:\n mod:0 \"Mod\"",
            )
            .build()
            .expect("a fixture corpus");

        assert!(matches!(
            resolve(&vanilla, &target).localization_files(),
            Err(Refusal::UnusableReplacePath { .. })
        ));
    }
}
