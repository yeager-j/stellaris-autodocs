//! Test-only helpers, compiled solely under the `test-support` feature. Production
//! builds never enable the feature. Phase 2 adds source-owned fixture-corpus support
//! here alongside the source module's own test seam.

use std::path::Path;
use tempfile::TempDir;

/// An isolated, disposable application-data directory for one test.
///
/// Every constructed value owns a distinct directory that disappears on drop. This is
/// the caller-precondition isolation the single-instance design demands of tests and
/// development tooling (docs/technical-design.md, "Single-instance ownership").
pub struct TempAppData {
    root: TempDir,
}

impl TempAppData {
    pub fn new() -> Self {
        Self {
            root: TempDir::new().expect("create temporary application-data directory"),
        }
    }

    pub fn path(&self) -> &Path {
        self.root.path()
    }
}

impl Default for TempAppData {
    fn default() -> Self {
        Self::new()
    }
}
