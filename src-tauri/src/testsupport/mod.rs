//! Test-only helpers, compiled solely under the `test-support` feature. Production
//! builds never enable the feature.
//!
//! Fixture Source Snapshots are **not** here: they live in
//! [`source::fixture`](crate::source::fixture), because a fixture corpus must be built by
//! the same construction and fingerprint path a live snapshot is, and the enumeration
//! policy that decides what one may contain is source-owned. What lives here is helper
//! state with a process or test lifetime rather than a deep module's knowledge.

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
