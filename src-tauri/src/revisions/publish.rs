//! The filesystem steps of one revision publication, as a seam, plus the production
//! implementation (docs/technical-design.md, "Documentation revision publication").
//!
//! Publication has two normative commit points — the durable move of a validated staging
//! directory onto its final path, and the state publication-reference replacement — and
//! the acceptance criterion is that a crash before or after either one leaves readers on
//! the prior complete revision or on the replacement, never on staging state. That claim
//! cannot be established on a healthy filesystem by argument, so every step a crash can
//! land between is named here and can be failed individually by a test double.
//!
//! **Reads are deliberately not in this seam**, for the reason `state::replace`
//! (`src/state/replace.rs:20`) records for its own: a read is not a commit point, and a
//! corruption test is more honest when it corrupts real bytes on a real disk than when it
//! asks a double to lie about what a file contains. [`stage::validate`] therefore reads
//! with plain `std::fs`, and the crash matrix injects failures only where an interrupted
//! write can actually leave the tree in an intermediate shape.
//!
//! [`stage::validate`]: crate::revisions::stage::validate

use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// The externally observable I/O steps of one publication, in protocol order.
/// Production is [`RealPublicationIo`].
pub trait PublicationIo {
    /// Create a directory and every missing parent. Idempotent, and deliberately so: the
    /// per-attempt staging name is a fresh UUID, so "already exists" cannot mean another
    /// attempt's directory, while `staging/` and `bundles/` are shared and long-lived.
    fn create_dir(&mut self, path: &Path) -> io::Result<()>;
    /// Create, write, and durably flush one bundle file, creating parents.
    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    /// Durably flush a directory's entries where the platform provides it.
    fn sync_dir(&mut self, path: &Path) -> io::Result<()>;
    /// Atomically move a validated staging directory onto its final path. Commit point 1.
    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()>;
    /// Best-effort removal of an abandoned staging directory.
    fn remove_dir_all(&mut self, path: &Path) -> io::Result<()>;
}

pub struct RealPublicationIo;

impl PublicationIo for RealPublicationIo {
    fn create_dir(&mut self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    /// No partial file is removed when a write fails, and that is the difference from
    /// [`ReplacementIo::write_temp`](crate::state::replace::ReplacementIo::write_temp):
    /// there the temporary file is the unit of abandonment, here the whole staging
    /// directory is. A half-written entry inside a staging directory that is never
    /// renamed is removed with that directory by the retention sweep, which recognizes it
    /// by name; removing the file alone would leave the directory behind anyway.
    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(path)?;
        file.write_all(bytes)?;
        file.sync_all()
    }

    fn sync_dir(&mut self, path: &Path) -> io::Result<()> {
        fs::File::open(path)?.sync_all()
    }

    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        // Directory rename, not file replacement: POSIX `rename` refuses a non-empty
        // destination directory with ENOTEMPTY rather than replacing it, so this is not
        // the "replace whatever is there" operation `state::replace` performs. The
        // protocol (Task 6) therefore never renames onto an occupied final path; an
        // already-present bundle is adopted after validation instead.
        fs::rename(from, to)
    }

    fn remove_dir_all(&mut self, path: &Path) -> io::Result<()> {
        fs::remove_dir_all(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_written_file_lands_with_its_parents_created() {
        // `stage_bundle` writes `documents/entry-list.json` into a directory that holds
        // only itself, so the seam has to create the intermediate directory; a caller that
        // had to pre-create each one would be restating bundle layout outside `stage`.
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("documents").join("entry-list.json");
        RealPublicationIo.write_file(&nested, b"{}").unwrap();
        assert_eq!(fs::read(&nested).unwrap(), b"{}");
    }

    #[test]
    fn writing_over_an_existing_file_replaces_its_whole_content() {
        // `fs::File::create` truncates. Without that, rewriting a shorter document into a
        // reused path would leave the previous document's tail behind and the bundle would
        // hash as `changed` for reasons no reader could explain.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("entry.json");
        RealPublicationIo
            .write_file(&path, b"a much longer previous document")
            .unwrap();
        RealPublicationIo.write_file(&path, b"{}").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{}");
    }

    #[test]
    fn creating_a_directory_that_already_exists_succeeds() {
        // Documented as idempotent because `staging/` and `bundles/` are shared and
        // long-lived: the first publication creates them and every later one must not
        // fail because it was not the first.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("staging");
        RealPublicationIo.create_dir(&path).unwrap();
        RealPublicationIo.create_dir(&path).unwrap();
        assert!(path.is_dir());
    }

    #[test]
    fn a_directory_rename_moves_the_whole_subtree_and_refuses_an_occupied_destination() {
        // The move step's real semantics, pinned here rather than assumed by the protocol:
        // the subtree arrives intact, and an occupied destination is an error rather than
        // a silent replacement. Task 6 depends on both — the first is what makes the
        // bundle complete at one instant, the second is why an already-present bundle is
        // adopted after validation instead of overwritten.
        let dir = TempDir::new().unwrap();
        let from = dir.path().join("staging").join("attempt");
        RealPublicationIo
            .write_file(&from.join("documents").join("entry-list.json"), b"{}")
            .unwrap();
        let to = dir.path().join("bundles").join("abc");
        RealPublicationIo.create_dir(to.parent().unwrap()).unwrap();
        RealPublicationIo.rename(&from, &to).unwrap();
        assert!(!from.exists());
        assert_eq!(
            fs::read(to.join("documents").join("entry-list.json")).unwrap(),
            b"{}"
        );

        let second = dir.path().join("staging").join("other-attempt");
        RealPublicationIo
            .write_file(&second.join("manifest.json"), b"{}")
            .unwrap();
        assert!(RealPublicationIo.rename(&second, &to).is_err());
        assert!(second.exists());
    }

    #[test]
    fn removing_an_abandoned_staging_directory_removes_its_contents() {
        let dir = TempDir::new().unwrap();
        let abandoned = dir.path().join("attempt");
        RealPublicationIo
            .write_file(&abandoned.join("documents").join("half.json"), b"{")
            .unwrap();
        RealPublicationIo.remove_dir_all(&abandoned).unwrap();
        assert!(!abandoned.exists());
    }

    #[test]
    fn syncing_a_directory_that_does_not_exist_reports_the_failure() {
        // `sync_dir` is a durability step, not a best-effort one: the protocol reads its
        // result, so it must not swallow a path that was never created.
        let dir = TempDir::new().unwrap();
        assert!(
            RealPublicationIo
                .sync_dir(&dir.path().join("absent"))
                .is_err()
        );
    }
}
