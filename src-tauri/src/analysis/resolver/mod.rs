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
    InlineScriptFact, ReferenceFact, ReferenceKind, ResolvedDefinition, ResolvedRegistry,
    StreamPosition, UnresolvedConstant, UnresolvedInline,
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

    fn parse(&self, entry: &StreamEntry) -> Option<ParsedFile> {
        let snapshot = match entry.source {
            SourceKind::VanillaContent => self.vanilla,
            SourceKind::TargetMod => self.target,
        };
        let bytes = snapshot.read(&entry.logical)?;
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
        // Megastructures, because the row is genuinely undeclared and will stay that way for
        // a while: its field cell is *inconclusive* rather than open — the registry's
        // diagnostics cannot detect field inheritance — so no ticket can close it without new
        // evidence. "A content type may be claimed as supported only when every policy it
        // requires is explicit and oracle-backed."
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
            resolution.registry("megastructures"),
            Err(Refusal::UndeclaredRegistry {
                registry: "megastructures".to_owned()
            })
        );
    }
}
