//! Named product use cases and coordination that genuinely spans deep modules:
//! documentation reads and the Ensure, Rebuild, and Validate workflows
//! (docs/technical-design.md, "Documentation build use cases"). Populated from Phase 3.
//!
//! Phase 3 populates the *shape* of that, not the workflows themselves. What is here is the
//! smallest honest thread: a coordinator that publishes a Revision Candidate obtained through
//! a seam ([`publish`]), the desktop read that resolves a published revision and decodes one
//! document ([`read`]), the two values those depend on ([`target`], [`candidates`]), and the
//! single object a transport is given ([`host`]). Ensure, Rebuild, and Validate arrive in
//! Phase 9 and replace [`publish::publish_provided_candidate`] rather than wrapping it.
//!
//! This module accepts and returns application-owned types and **imports no framework**. That
//! is not a convention here — it is the edge the design fixes, and `transport` is where Tauri
//! types stop.

pub mod candidates;
pub mod host;
pub mod publish;
pub mod read;
pub mod target;

pub use candidates::{CandidateUnavailable, NoAnalysisSource, RevisionCandidateSource};
pub use host::DocumentationHost;
pub use publish::{
    AnalysisFailure, BuildGuard, BundleEntryDurability, DocumentationPublished,
    PublicationRecordDurability, PublishDocumentationError, PublishedDurability, StorageFailure,
    publish_provided_candidate,
};
pub use read::{
    DocumentationEntries, DocumentationEntry, ReadEntryListError, UnavailableReason,
    read_entry_list,
};
pub use target::BuildTarget;

#[cfg(test)]
mod tests {
    /// The dependency edge the design fixes, as a gate rather than a convention: "application
    /// modules accept and return application-owned types and do not import Tauri or the HTTP
    /// framework". A violation compiles perfectly and is invisible until somebody reads for it.
    ///
    /// A text gate, and it says so: it reads `use` statements, so a fully-qualified `tauri::`
    /// path inside a function body would slip past. What it catches is the plausible mistake —
    /// reaching for `tauri::State` or `AppHandle` while writing a use case.
    #[test]
    fn no_application_module_imports_the_framework() {
        fn uses_of(dir: &std::path::Path, found: &mut Vec<(String, String)>) {
            for entry in std::fs::read_dir(dir).expect("read a directory").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    uses_of(&path, found);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    for line in std::fs::read_to_string(&path)
                        .expect("read a source file")
                        .lines()
                    {
                        let line = line.trim_start();
                        if line.starts_with("use ") {
                            found.push((path.display().to_string(), line.to_owned()));
                        }
                    }
                }
            }
        }

        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let framework = |line: &str| line.contains("tauri") || line.contains("http");

        let mut application = Vec::new();
        uses_of(&src.join("application"), &mut application);
        assert!(!application.is_empty(), "the scan found no imports to read");
        for (path, line) in &application {
            assert!(!framework(line), "{path} imports the framework: {line}");
        }

        // Negative control: the composition root is where framework types are allowed to live,
        // so the same reader must find one there. Without it, a passing assertion above could
        // mean the scanner sees nothing at all.
        let mut composition = Vec::new();
        uses_of(&src.join("composition"), &mut composition);
        assert!(composition.iter().any(|(_, line)| framework(line)));
    }
}
