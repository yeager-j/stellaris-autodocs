//! Crash-safe replacement of the state document (docs/technical-design.md, "Mutable
//! state storage").
//!
//! The normative commit point is the atomic rename onto the state path. Failure before
//! it leaves the prior file authoritative. A failure report at or after it is ambiguous
//! evidence: the outcome is decided by reopening and comparing the authoritative path
//! against the known prior and next bytes, never inferred from the error alone.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The externally observable I/O steps of one replacement, in protocol order.
/// A seam so tests can fail each step; production is [`RealIo`].
pub trait ReplacementIo {
    /// Create, write, and durably flush a uniquely named temporary file in `dir`.
    fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf>;
    /// Atomically rename `from` onto `to`. The commit point.
    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()>;
    /// Durably flush the directory entry change where the platform provides it.
    fn sync_dir(&mut self, dir: &Path) -> io::Result<()>;
}

pub struct RealIo;

impl ReplacementIo for RealIo {
    fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
        let path = dir.join(format!(".state-{}.tmp", uuid::Uuid::new_v4()));
        let mut file = fs::File::create(&path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(path)
    }

    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn sync_dir(&mut self, dir: &Path) -> io::Result<()> {
        fs::File::open(dir)?.sync_all()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReplaceOutcome {
    Committed,
    /// The new state is visible on the authoritative path but its durability could not
    /// be confirmed.
    CommittedDurabilityUncertain,
    /// Failure before the commit point; the prior file remains authoritative.
    PriorRetained {
        detail: String,
    },
    /// The authoritative path matches neither prior nor next; mutation must stop and
    /// state recovery begins.
    RecoveryRequired {
        detail: String,
    },
}

pub fn replace_state(
    io_seam: &mut dyn ReplacementIo,
    state_path: &Path,
    next_bytes: &[u8],
    prior_bytes: Option<&[u8]>,
) -> ReplaceOutcome {
    let Some(dir) = state_path.parent() else {
        return ReplaceOutcome::PriorRetained {
            detail: "state path has no parent directory".to_owned(),
        };
    };
    let temp = match io_seam.write_temp(dir, next_bytes) {
        Ok(temp) => temp,
        Err(error) => {
            return ReplaceOutcome::PriorRetained {
                detail: format!("writing temporary state: {error}"),
            };
        }
    };
    if let Err(error) = io_seam.rename(&temp, state_path) {
        let _ = fs::remove_file(&temp);
        return classify_by_reread(
            state_path,
            next_bytes,
            prior_bytes,
            &format!("rename onto state path failed: {error}"),
        );
    }
    match io_seam.sync_dir(dir) {
        Ok(()) => ReplaceOutcome::Committed,
        Err(error) => classify_by_reread(
            state_path,
            next_bytes,
            prior_bytes,
            &format!("directory sync failed: {error}"),
        ),
    }
}

fn classify_by_reread(
    state_path: &Path,
    next_bytes: &[u8],
    prior_bytes: Option<&[u8]>,
    detail: &str,
) -> ReplaceOutcome {
    match fs::read(state_path) {
        Ok(found) if found == next_bytes => ReplaceOutcome::CommittedDurabilityUncertain,
        Ok(found) if prior_bytes == Some(found.as_slice()) => ReplaceOutcome::PriorRetained {
            detail: detail.to_owned(),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound && prior_bytes.is_none() => {
            ReplaceOutcome::PriorRetained {
                detail: detail.to_owned(),
            }
        }
        _ => ReplaceOutcome::RecoveryRequired {
            detail: detail.to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    const PRIOR: &[u8] = br#"{"schema":1,"marker":"prior"}"#;
    const NEXT: &[u8] = br#"{"schema":1,"marker":"next"}"#;

    fn seeded_dir() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        fs::write(&state_path, PRIOR).unwrap();
        (dir, state_path)
    }

    /// Fails exactly one protocol step; every other step is real.
    struct FailAt {
        step: &'static str,
        inner: RealIo,
    }

    impl ReplacementIo for FailAt {
        fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
            if self.step == "write_temp" {
                return Err(io::Error::other("injected write_temp failure"));
            }
            self.inner.write_temp(dir, bytes)
        }
        fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
            if self.step == "rename" {
                return Err(io::Error::other("injected rename failure"));
            }
            self.inner.rename(from, to)
        }
        fn sync_dir(&mut self, dir: &Path) -> io::Result<()> {
            if self.step == "sync_dir" {
                return Err(io::Error::other("injected sync_dir failure"));
            }
            self.inner.sync_dir(dir)
        }
    }

    #[test]
    fn success_commits_and_replaces_the_file() {
        let (_dir, state_path) = seeded_dir();
        let outcome = replace_state(&mut RealIo, &state_path, NEXT, Some(PRIOR));
        assert_eq!(outcome, ReplaceOutcome::Committed);
        assert_eq!(fs::read(&state_path).unwrap(), NEXT);
    }

    #[test]
    fn failure_before_the_commit_point_retains_the_prior_file() {
        let (_dir, state_path) = seeded_dir();
        let mut io_seam = FailAt {
            step: "write_temp",
            inner: RealIo,
        };
        let outcome = replace_state(&mut io_seam, &state_path, NEXT, Some(PRIOR));
        assert!(matches!(outcome, ReplaceOutcome::PriorRetained { .. }));
        assert_eq!(fs::read(&state_path).unwrap(), PRIOR);
    }

    #[test]
    fn rename_error_with_prior_still_on_disk_retains_prior() {
        let (_dir, state_path) = seeded_dir();
        let mut io_seam = FailAt {
            step: "rename",
            inner: RealIo,
        };
        let outcome = replace_state(&mut io_seam, &state_path, NEXT, Some(PRIOR));
        assert!(matches!(outcome, ReplaceOutcome::PriorRetained { .. }));
        assert_eq!(fs::read(&state_path).unwrap(), PRIOR);
    }

    #[test]
    fn rename_that_succeeded_but_reported_failure_is_committed_uncertain() {
        // The authoritative path, not the error, decides
        // (docs/technical-design.md, "Mutable state storage").
        struct LyingRename;
        impl ReplacementIo for LyingRename {
            fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
                RealIo.write_temp(dir, bytes)
            }
            fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
                fs::rename(from, to)?;
                Err(io::Error::other("injected post-rename failure report"))
            }
            fn sync_dir(&mut self, _dir: &Path) -> io::Result<()> {
                Ok(())
            }
        }
        let (_dir, state_path) = seeded_dir();
        let outcome = replace_state(&mut LyingRename, &state_path, NEXT, Some(PRIOR));
        assert_eq!(outcome, ReplaceOutcome::CommittedDurabilityUncertain);
        assert_eq!(fs::read(&state_path).unwrap(), NEXT);
    }

    #[test]
    fn sync_dir_failure_after_rename_is_committed_uncertain() {
        let (_dir, state_path) = seeded_dir();
        let mut io_seam = FailAt {
            step: "sync_dir",
            inner: RealIo,
        };
        let outcome = replace_state(&mut io_seam, &state_path, NEXT, Some(PRIOR));
        assert_eq!(outcome, ReplaceOutcome::CommittedDurabilityUncertain);
        assert_eq!(fs::read(&state_path).unwrap(), NEXT);
    }

    #[test]
    fn unrecognizable_authoritative_content_requires_recovery() {
        // Rename fails AND the authoritative file no longer matches prior or next:
        // callers must stop mutating rather than guess.
        struct CorruptingRename;
        impl ReplacementIo for CorruptingRename {
            fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
                RealIo.write_temp(dir, bytes)
            }
            fn rename(&mut self, _from: &Path, to: &Path) -> io::Result<()> {
                fs::write(to, b"neither prior nor next").unwrap();
                Err(io::Error::other("injected rename failure"))
            }
            fn sync_dir(&mut self, _dir: &Path) -> io::Result<()> {
                Ok(())
            }
        }
        let (_dir, state_path) = seeded_dir();
        let outcome = replace_state(&mut CorruptingRename, &state_path, NEXT, Some(PRIOR));
        assert!(matches!(outcome, ReplaceOutcome::RecoveryRequired { .. }));
    }

    #[test]
    fn first_write_with_no_prior_file_commits() {
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        let outcome = replace_state(&mut RealIo, &state_path, NEXT, None);
        assert_eq!(outcome, ReplaceOutcome::Committed);
        assert_eq!(fs::read(&state_path).unwrap(), NEXT);
    }
}
