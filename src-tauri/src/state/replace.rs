//! Crash-safe replacement of the state document (docs/technical-design.md, "Mutable
//! state storage").
//!
//! The normative commit point is the atomic rename onto the state path, and it is
//! structural: once the rename returns Ok the new state was visible, so every later
//! failure floors at [`ReplaceOutcome::CommittedDurabilityUncertain`] and nothing may
//! downgrade it to a claim that the old file survived.
//!
//! A rename that reports failure is instead ambiguous evidence — the rename may have
//! completed before the error surfaced. That case alone is decided by reopening and
//! comparing the authoritative path against the known prior and next bytes, never
//! inferred from the error alone.

use crate::durability::DirectoryFlush;
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
    /// Durably flush the directory entry change, reporting which of
    /// [`DirectoryFlush`]'s two performed-or-not answers occurred; an error means the flush
    /// was attempted and refused. The platform question is
    /// [`durability::sync_dir`](crate::durability::sync_dir)'s and this seam never has to
    /// know which platform it is on — but it must not flatten the answer, because
    /// [`replace_state`] reads it to decide between [`ReplaceOutcome::Committed`] and
    /// [`ReplaceOutcome::CommittedDurabilityUncertain`].
    fn sync_dir(&mut self, dir: &Path) -> io::Result<DirectoryFlush>;
}

/// The temporary-file naming scheme. Named once here because two operations depend on
/// it: creating one replacement's temporary file, and recognizing temporaries abandoned
/// by an earlier run so [`sweep_stale_temps`] can remove them.
const TEMP_PREFIX: &str = ".state-";
const TEMP_SUFFIX: &str = ".tmp";

/// Remove temporary files abandoned by a crashed or failed earlier run. Called at open,
/// when no replacement of this store is in flight, so every `.state-*.tmp` present is
/// unreachable: its name is a fresh UUID no later replacement will ever choose.
pub(super) fn sweep_stale_temps(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(TEMP_PREFIX) && name.ends_with(TEMP_SUFFIX) {
            // Best effort: a temporary that cannot be removed is junk, never a
            // correctness problem, and must not fail opening the store.
            let _ = fs::remove_file(entry.path());
        }
    }
}

pub struct RealIo;

impl RealIo {
    /// Create a uniquely named temporary file in `dir`, `fill` it, and flush it durably,
    /// removing the file if any step after creation fails. A partial temporary is never
    /// named again, so leaving one behind accumulates junk in the user's application
    /// data directory on every ENOSPC.
    ///
    /// `fill` is a parameter only so that guarantee is observable: a failure between
    /// create and sync cannot be provoked on a healthy filesystem.
    fn write_temp_in(
        dir: &Path,
        fill: impl FnOnce(&mut fs::File) -> io::Result<()>,
    ) -> io::Result<PathBuf> {
        let path = dir.join(format!(
            "{TEMP_PREFIX}{}{TEMP_SUFFIX}",
            uuid::Uuid::new_v4()
        ));
        let attempt = fs::File::create(&path).and_then(|mut file| {
            fill(&mut file)?;
            file.sync_all()
        });
        match attempt {
            Ok(()) => Ok(path),
            Err(error) => {
                let _ = fs::remove_file(&path);
                Err(error)
            }
        }
    }
}

impl ReplacementIo for RealIo {
    fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
        Self::write_temp_in(dir, |file| file.write_all(bytes))
    }

    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        // `std::fs::rename` replaces an existing destination on every supported
        // platform: POSIX rename on Unix; on Windows, SetFileInformationByHandle
        // with FileRenameInfoEx (POSIX semantics, Windows 10 1607+) falling back to
        // MoveFileExW with replace semantics. The fallback's atomicity is weaker
        // than POSIX rename; the Phase 12 Windows packaged smoke test exercises
        // real-machine replacement semantics before that platform inherits the
        // release claim (docs/technical-design.md, "Verification architecture").
        fs::rename(from, to)
    }

    /// Delegated rather than spelled here: whether a platform provides a directory flush
    /// at all is one fact, and this module and `revisions::publish` both reach a commit
    /// point that rests on it (docs/decision-log.md, D-123). Two spellings would be two
    /// answers, and the one that mattered would be the one nobody read.
    fn sync_dir(&mut self, dir: &Path) -> io::Result<DirectoryFlush> {
        crate::durability::sync_dir(dir)
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
        return classify_reported_rename_failure(
            state_path,
            next_bytes,
            prior_bytes,
            &format!("rename onto state path failed: {error}"),
        );
    }
    match io_seam.sync_dir(dir) {
        Ok(DirectoryFlush::Flushed) => ReplaceOutcome::Committed,
        // Past the commit point. A re-read could only ever confirm this outcome, so it
        // is not performed: it can find the file already replaced by another writer or
        // fail with EIO, and either reading would misreport a committed replacement as
        // a retained prior or as corruption.
        //
        // Both arms floor here, and `NotProvided` belongs in this one rather than beside
        // `Flushed`: a flush the platform never performed confirmed nothing, so claiming
        // `Committed` would assert a durability nothing observed. It needs no arm of its
        // own — "visible, durability unconfirmed" is exactly what this outcome already
        // means, and the protocol's consumers (`state::mutations`, and through it
        // retention) already read it that way (docs/decision-log.md, D-123).
        Ok(DirectoryFlush::NotProvided) | Err(_) => ReplaceOutcome::CommittedDurabilityUncertain,
    }
}

/// The only ambiguous step: a reported rename failure may still have replaced the path.
/// The authoritative bytes decide, never the error.
fn classify_reported_rename_failure(
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
        fn sync_dir(&mut self, dir: &Path) -> io::Result<DirectoryFlush> {
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
            fn sync_dir(&mut self, _dir: &Path) -> io::Result<DirectoryFlush> {
                Ok(DirectoryFlush::Flushed)
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
    fn a_flush_this_platform_does_not_provide_is_not_a_committed_durability() {
        // The distinction `DirectoryFlush` exists to keep, asserted where the policy lives.
        // On an exFAT, FAT32, or SMB volume the directory flush is never performed at all,
        // and while that was reported as `Ok(())` this outcome was `Committed` — a claim
        // that a rename a crash can still erase had reached disk. It is
        // `CommittedDurabilityUncertain`, which is literally accurate and is an outcome the
        // protocol already carries (docs/decision-log.md, D-123).
        //
        // Platform-independent by construction: the seam returns the outcome the Windows
        // tolerance produces, so the arm no macOS filesystem can reach through
        // `durability::sync_dir` is still exercised here.
        struct UnavailableFlush;
        impl ReplacementIo for UnavailableFlush {
            fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
                RealIo.write_temp(dir, bytes)
            }
            fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
                RealIo.rename(from, to)
            }
            fn sync_dir(&mut self, _dir: &Path) -> io::Result<DirectoryFlush> {
                Ok(DirectoryFlush::NotProvided)
            }
        }
        let (_dir, state_path) = seeded_dir();
        let outcome = replace_state(&mut UnavailableFlush, &state_path, NEXT, Some(PRIOR));
        assert_eq!(outcome, ReplaceOutcome::CommittedDurabilityUncertain);
        // Visible either way: the replacement happened, and only the guarantee is withheld.
        assert_eq!(fs::read(&state_path).unwrap(), NEXT);
    }

    #[test]
    fn post_rename_failure_floors_at_uncertain_even_if_the_file_was_disturbed() {
        // The commit point is structural: once `rename` returns Ok the new state was
        // visible, so no later observation may downgrade the outcome to a claim about
        // the prior file (docs/technical-design.md, "Mutable state storage"). Here an
        // external writer replaces the file before the post-commit re-read would see it.
        struct DisturbingSync;
        impl ReplacementIo for DisturbingSync {
            fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
                RealIo.write_temp(dir, bytes)
            }
            fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
                fs::rename(from, to)
            }
            fn sync_dir(&mut self, dir: &Path) -> io::Result<DirectoryFlush> {
                fs::write(dir.join("state.json"), b"disturbed by another writer").unwrap();
                Err(io::Error::other("injected sync_dir failure"))
            }
        }
        let (_dir, state_path) = seeded_dir();
        let outcome = replace_state(&mut DisturbingSync, &state_path, NEXT, Some(PRIOR));
        assert_eq!(outcome, ReplaceOutcome::CommittedDurabilityUncertain);
    }

    #[test]
    fn post_rename_failure_with_the_state_file_gone_is_still_uncertain() {
        // NotFound plus no prior would classify as PriorRetained on the pre-commit path;
        // after the commit point that reading is unavailable.
        struct DeletingSync;
        impl ReplacementIo for DeletingSync {
            fn write_temp(&mut self, dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
                RealIo.write_temp(dir, bytes)
            }
            fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
                fs::rename(from, to)
            }
            fn sync_dir(&mut self, dir: &Path) -> io::Result<DirectoryFlush> {
                fs::remove_file(dir.join("state.json")).unwrap();
                Err(io::Error::other("injected sync_dir failure"))
            }
        }
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        let outcome = replace_state(&mut DeletingSync, &state_path, NEXT, None);
        assert_eq!(outcome, ReplaceOutcome::CommittedDurabilityUncertain);
    }

    #[test]
    fn write_temp_removes_its_partial_file_when_the_write_fails() {
        // An orphaned `.state-*.tmp` is never named again: leaving one behind on every
        // ENOSPC accumulates junk in the user's application-data directory forever.
        let dir = TempDir::new().unwrap();
        let error = RealIo::write_temp_in(dir.path(), |_| {
            Err(io::Error::other("injected mid-write failure"))
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "injected mid-write failure");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn sweeping_removes_stale_temporaries_and_nothing_else() {
        let (dir, state_path) = seeded_dir();
        let stale = dir.path().join(".state-0e2a.tmp");
        fs::write(&stale, b"abandoned").unwrap();
        let quarantine = dir.path().join("state.json.quarantine-1-deadbeef");
        fs::write(&quarantine, b"preserved").unwrap();

        sweep_stale_temps(dir.path());

        assert!(!stale.exists());
        assert_eq!(fs::read(&state_path).unwrap(), PRIOR);
        assert_eq!(fs::read(&quarantine).unwrap(), b"preserved");
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
            fn sync_dir(&mut self, _dir: &Path) -> io::Result<DirectoryFlush> {
                Ok(DirectoryFlush::Flushed)
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
