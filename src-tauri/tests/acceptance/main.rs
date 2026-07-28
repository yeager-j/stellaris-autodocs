//! The acceptance harness: the shape the five golden cases keep.
//!
//! ```text
//! fixture Source Snapshots
//!     -> application build use case
//!     -> published Documentation Revision
//!     -> desktop read
//! ```
//!
//! It boots through `composition::open_stores` over a `testsupport::TempAppData` directory —
//! the same sequence the composition root runs — publishes through
//! `DocumentationHost::publish_documentation`, and reads back through
//! `DocumentationHost::entry_list`. Those are the two methods the `build_documentation` and
//! `get_entry_list` Tauri commands call. There is no test-private route around publication or
//! around the reader: everything below [`harness::AcceptanceThread::boot`] is production code —
//! a real bundle staged and validated, a real atomic move, a real compare-and-swap of the state
//! publication reference, and a read that resolves the revision from that pointer alone.
//! Headless throughout: no Steam installation, no host-specific paths, no live traversal
//! (docs/technical-design.md, "Verification architecture").
//!
//! # What a fixture corpus's *bytes* supply, and what they do not
//!
//! **They reach a published revision as exactly two values: the Target Mod and Vanilla Content
//! fingerprints a revision records as its `RevisionInputs`.** Nothing parses them. No documented
//! content is derived from them — the entries a case asserts on were written into the corpus
//! definition by hand and published unchanged, because this build performs no analysis and
//! `analysis` stays empty until Phase 6. A reader who assumed the entries came from the fixture
//! files would be wrong about every case in this target, which is why
//! `published_thread::the_fixture_bytes_reach_the_revision_and_nothing_a_reader_can_see` asserts
//! the gap instead of leaving this paragraph as the only place it is stated.
//!
//! The claim is about the bytes, and a later phase widening this must not read it as a claim
//! about the corpus. A corpus also carries the identity inputs a build is derived through — its
//! `location_path` and `mod_root` are what `boot` turns into a Mod Installation identifier — and
//! the `documentation_typed_by_hand` that is published. Those are corpus contributions today and
//! stay contributions afterwards; what Phase 6 changes is that the bytes stop being inert.
//!
//! # Where it stops, and why that is not a shortcut
//!
//! At `DocumentationHost`, the value `composition::run` manages and hands to every transport.
//! Above it a Tauri command adds `spawn_blocking` and the Result-envelope projection; neither is
//! constructible without a Tauri `App`, and `transport::tauri` says so. The consequence is worth
//! naming rather than leaving to be discovered: **after Phase 3 closes, no Tauri command is
//! invoked end to end by anything.** STE-19 carries the same narrowing from the frontend side —
//! its window exercises the read while the `seed-skeleton` example drives publication, so the
//! two never meet in one run. The envelope and DTO shapes are pinned separately by
//! `transport`'s contract vectors; packaged smoke tests in Phase 12 are what finally close it.
//!
//! # Where later phases widen it
//!
//! Every future consumer must be a module *inside* this directory. A sibling `tests/*.rs` file
//! is a separate crate and cannot see [`harness`] — that constraint is the whole reason the
//! target is laid out this way.
//!
//! - **A second corpus:** one named constructor in [`corpora`], passed to `boot`. Demonstrated
//!   rather than promised — this target already runs the same thread over four of them. Phase 4
//!   task 8's drift-checked run over an installed Vanilla and ACOT arrives the same way, through
//!   `source::snapshot::establish`; the constructor is also where its drift record and its
//!   skip-when-not-installed decision live.
//! - **A language** (Phase 5 generates `display_name` rather than having it typed):
//!   [`harness::AcceptanceThread::boot`], in the window before the `StateStore` is moved into
//!   the host, because only the explicit override is durable state. It belongs there and not on
//!   a corpus: a corpus answers "what is the source", a language answers "who is reading", and
//!   keeping them apart is what lets one corpus serve golden case 1's multi-language assertions
//!   without a corpus per language.
//! - **Phase 6's deletion of the candidate seam:** `AcceptanceCorpus`'s
//!   `documentation_typed_by_hand` field, `harness::HandAuthoredCandidate`, and the two places
//!   that construct one — a step of `boot` and a line of `rebuild_over`. No case names any of
//!   them, so no case changes.
//!
//! One forward obligation this shape depends on, recorded here because nothing else records it:
//! a memory-backed fixture corpus has no path, and `source::fixture` will never grow a
//! `from_directory` loader. Fixtures enter through the candidate seam today and Phase 6 deletes
//! it, so Phase 9's coordinator has to split into *establish from a Discovery Location* and
//! *build from established snapshots* — the harness calls the second half. If it only ever
//! offers the first, fixture corpora fork off the production path and this stops being the
//! acceptance harness.

mod corpora;
mod harness;
mod published_thread;
