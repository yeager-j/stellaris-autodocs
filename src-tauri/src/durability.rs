//! Making a directory entry durable, and what it means when the platform does not
//! provide that operation (docs/decision-log.md, D-123).
//!
//! Not part of the technical design's deep-module row: like [`canonical`](crate::canonical),
//! this is a leaf primitive below it, and depending on it is not a peer edge. It is named in
//! the design's top-level module map. It exists because two protocols —
//! `state::replace`'s state-document replacement and `revisions`'s bundle publication —
//! both reach a commit point whose durability rests on the same platform fact, and a fact
//! with two implementations is a fact with two answers.
//!
//! # The rule
//!
//! Flushing a *file* is universal: `fsync` on POSIX, `FlushFileBuffers` on Windows, both
//! reached through [`std::fs::File::sync_all`]. Flushing a *directory* is not, and the
//! difference is not a gap in `std`:
//!
//! - On POSIX a newly created, renamed, or removed entry is durable only after its parent
//!   directory is itself flushed. `fsync` on a directory file descriptor is the documented
//!   way to do that, and skipping it is a real data-loss bug. A read-only descriptor is
//!   sufficient; POSIX imposes no access requirement on `fsync`.
//! - On Windows the two Win32 contracts involved are these, and only these:
//!   - `CreateFileW`, "Directories": *"To open a directory using **CreateFile**, specify
//!     the **FILE_FLAG_BACKUP_SEMANTICS** flag as part of dwFlagsAndAttributes. Appropriate
//!     security checks still apply when this flag is used without **SE_BACKUP_NAME** and
//!     **SE_RESTORE_NAME** privileges."
//!     (<https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew>).
//!     [`std::fs::File::open`] does not set that flag, so it cannot open a directory at all.
//!   - `FlushFileBuffers`, `hFile`: *"The file handle must have the **GENERIC_WRITE** access
//!     right."*
//!     (<https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers>).
//!     That is the page's one stated access requirement, and it says nothing about directory
//!     handles being unsupported.
//!
//! So this module asks for what the contract names: on Windows it opens the directory with
//! `FILE_FLAG_BACKUP_SEMANTICS` **and** write access, and only falls back to a read-only
//! handle when the write open is refused.
//!
//! **An earlier version of this module got that wrong**, and the correction is the reason
//! this section quotes rather than summarizes. It opened `.read(true)` only and then read
//! the resulting `ERROR_ACCESS_DENIED` as "Windows does not provide a directory flush". That
//! refusal was this code's own access mode: it had asked for `GENERIC_READ` and called an
//! operation documented to require `GENERIC_WRITE`. Worse, the misreading was unfalsifiable —
//! with every flush excused, [`sync_dir`] returned `Ok` for any openable path on Windows, so
//! `state`'s `CommittedDurabilityUncertain` and `revisions`'s `BundleDurabilityUnconfirmed`
//! were both unreachable there, which is precisely the distinction those two protocols claim
//! to be able to make.
//!
//! # What is actually unavailable, and where
//!
//! With a write-capable directory handle, the attested failure is narrow. The field report
//! this module's tolerated codes come from
//! (<https://github.com/hypermodeinc/badger/issues/699>) opened directories with
//! `GENERIC_READ | GENERIC_WRITE | FILE_FLAG_BACKUP_SEMANTICS` and observed
//! `FlushFileBuffers` **succeed on a local drive** and fail with `ERROR_INVALID_FUNCTION`
//! ("Incorrect function") only over an SMB redirector. So the tolerance here is for
//! redirectors and filesystems that answer "I do not implement this", not for Windows.
//!
//! `flush_is_unavailable` therefore tolerates `ERROR_INVALID_FUNCTION` and
//! `ERROR_NOT_SUPPORTED` on a write-capable handle, and tolerates `ERROR_ACCESS_DENIED`
//! *only* on the read-only fallback, where it is the documented consequence of the missing
//! `GENERIC_WRITE` and says nothing about the directory. On a write-capable handle an access
//! denial is a real denial and is reported.
//!
//! # The residual risk, stated rather than hidden
//!
//! Where the tolerance fires, this is a durability weakening and is recorded as one. It is
//! bounded by the filesystem rather than by the API:
//!
//! - **On local NTFS** the flush is expected to succeed and the question does not arise. The
//!   `windows-latest` CI job is what keeps that expectation honest — it executes
//!   `tests::a_write_capable_handle_opens_and_the_flush_is_not_refused` on a real Windows
//!   volume, and a red there is the report that the weakening below is not hypothetical.
//! - **On exFAT, FAT32, and SMB or other network shares** there is no metadata journal and
//!   the redirector may refuse outright, so nothing stands in for the flush. A removable
//!   drive or a redirected application-data directory is the realistic way a user reaches
//!   this.
//!
//! Both callers' worst case is the same and is recoverable rather than corrupting: a
//! directory entry that a crash erased leaves a state document or a published bundle that
//! is absent rather than damaged, and both callers already fail closed on absence — the
//! Revision identifier is a pure function of content, so the same rebuild republishes to
//! the same path and repairs it. What is lost is the *guarantee*, not the recovery.
//!
//! The Phase 12 Windows packaged smoke test is where this stops being a CI observation on a
//! runner's volume and becomes one on a user-shaped installation, alongside the
//! rename-semantics claims `state::replace` and `revisions` already record
//! (docs/technical-design.md, "Verification architecture").

use std::fs;
use std::io;
use std::path::Path;

/// Durably flush a directory's entries where the platform provides it.
///
/// An error means the flush was attempted and refused for a reason this platform does not
/// call "unavailable" — a caller may treat that as a genuine durability failure. A missing
/// or unopenable directory is always an error on every platform: the open is not part of
/// the tolerance, so a path that was never created is reported rather than excused.
pub fn sync_dir(path: &Path) -> io::Result<()> {
    let (directory, has_flush_right) = open_directory(path)?;
    match directory.sync_all() {
        Err(error) if flush_is_unavailable(&error, has_flush_right) => Ok(()),
        outcome => outcome,
    }
}

/// Open a directory for flushing, reporting whether the handle carries the access right
/// this platform's directory flush requires.
///
/// POSIX imposes no access requirement on `fsync`, so a read-only descriptor already
/// carries everything the call needs and the flag is unconditionally true.
#[cfg(not(windows))]
fn open_directory(path: &Path) -> io::Result<(fs::File, bool)> {
    Ok((fs::File::open(path)?, true))
}

/// Open a directory for flushing, reporting whether the handle carries the access right
/// this platform's directory flush requires.
///
/// `CreateFileW` refuses a directory without `FILE_FLAG_BACKUP_SEMANTICS`, which
/// `File::open` does not set, and `FlushFileBuffers` documents `GENERIC_WRITE` as its one
/// access requirement — so the first attempt asks for both. The read-only fallback exists
/// because write access to a directory can be denied by an ACL the process cannot change,
/// and a flush that is refused for *that* reason is better than no flush attempt at all;
/// the caller learns which handle it got so it can read the refusal correctly.
#[cfg(windows)]
fn open_directory(path: &Path) -> io::Result<(fs::File, bool)> {
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

    fn open(path: &Path, write: bool) -> io::Result<fs::File> {
        use std::os::windows::fs::OpenOptionsExt;

        fs::OpenOptions::new()
            .read(true)
            .write(write)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
    }

    match open(path, true) {
        Ok(directory) => Ok((directory, true)),
        // Deliberately falls back on *any* refusal of the write open rather than on a
        // classified subset: the fallback attempts strictly less and reports strictly less,
        // so misclassifying here can only cost precision, never safety. If the path is
        // simply absent the read-only open fails too, and that error is the one returned.
        Err(_) => open(path, false).map(|directory| (directory, false)),
    }
}

/// Whether an error from flushing a directory handle means "this platform does not provide
/// the operation" rather than "the operation failed".
///
/// **Never true off Windows**, deliberately: on POSIX the flush is provided, so every error
/// it returns is a durability failure a caller must see. Making the tolerance
/// platform-specific rather than error-code-specific is what keeps this from quietly
/// swallowing an `EIO` on the platform the project is developed on.
///
/// The gate is `cfg!` rather than `#[cfg]` so that it is one expression rather than two
/// bodies, and so that the table below stays compiled — and therefore testable — on every
/// platform. Off Windows this folds to a constant `false`.
fn flush_is_unavailable(error: &io::Error, has_flush_right: bool) -> bool {
    cfg!(windows) && windows_flush_is_unavailable(error, has_flush_right)
}

/// The Windows tolerance table, as a pure decision over a Win32 error code.
///
/// `ERROR_INVALID_FUNCTION` and `ERROR_NOT_SUPPORTED` are the "I do not implement this"
/// answers, attested for a write-capable directory handle over an SMB redirector (see the
/// module documentation). `ERROR_ACCESS_DENIED` is tolerated **only** on the read-only
/// fallback, where `FlushFileBuffers`'s documented `GENERIC_WRITE` requirement makes it a
/// statement about the handle rather than about the directory. On a write-capable handle it
/// is a real denial and is reported, which is what keeps the two callers' durability
/// outcomes reachable on Windows at all.
///
/// **Compiled on every platform on purpose.** The defect this table replaces was a Win32
/// contract misread in a `#[cfg(windows)]` branch that nothing on the development machine
/// could execute, so the citation was the only evidence there was. Separating the table from
/// the platform gate above makes the table itself falsifiable wherever the project is
/// developed; the `windows-latest` CI job then runs the same assertions where the codes are
/// real. Only `flush_is_unavailable` decides whether the table applies.
fn windows_flush_is_unavailable(error: &io::Error, has_flush_right: bool) -> bool {
    const ERROR_INVALID_FUNCTION: i32 = 1;
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_NOT_SUPPORTED: i32 = 50;

    match error.raw_os_error() {
        Some(ERROR_INVALID_FUNCTION | ERROR_NOT_SUPPORTED) => true,
        Some(ERROR_ACCESS_DENIED) => !has_flush_right,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn flushing_a_real_directory_succeeds() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("entry"), b"{}").unwrap();
        sync_dir(dir.path()).unwrap();
    }

    #[test]
    fn flushing_a_directory_that_does_not_exist_reports_the_failure() {
        // The open is deliberately outside the "platform does not provide this" tolerance:
        // both callers read this result at a commit point, so a path that was never created
        // must not be excused as an unavailable operation. On Windows this also covers the
        // read-only fallback, whose failure is the error `open_directory` returns.
        let dir = TempDir::new().unwrap();
        assert!(sync_dir(&dir.path().join("absent")).is_err());
    }

    #[test]
    fn every_directory_this_project_flushes_opens_with_the_right_the_flush_requires() {
        // The claim N1 turned on, asserted where it can be: an ordinary directory this
        // process owns yields a handle carrying the access right the flush needs, so the
        // tolerance below is never reached for the access mode's own sake. On Windows this
        // is the write open succeeding; off Windows `fsync` requires no access right and
        // the flag is true by construction.
        let dir = TempDir::new().unwrap();
        let (_directory, has_flush_right) = open_directory(dir.path()).unwrap();
        assert!(has_flush_right);
    }

    #[cfg(windows)]
    #[test]
    fn a_write_capable_handle_opens_and_the_flush_is_not_refused() {
        // Runs only on the `windows-latest` CI job, and is the empirical half of D-123: on
        // a local NTFS volume, `FlushFileBuffers` on a directory handle opened the way
        // `CreateFileW` and `FlushFileBuffers` document is expected to *succeed*, not to be
        // excused. A red here is not a product failure — `sync_dir` still returns `Ok` for
        // the tolerated codes — it is the report that the durability weakening D-123
        // records as bounded to redirectors and non-journalling volumes has been observed
        // somewhere else, and D-123 must be rewritten.
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("entry"), b"{}").unwrap();
        let (directory, has_flush_right) = open_directory(dir.path()).unwrap();
        assert!(
            has_flush_right,
            "the write open was refused for a directory this process just created"
        );
        directory
            .sync_all()
            .expect("FlushFileBuffers refused a write-capable directory handle on this volume");
    }

    #[test]
    fn an_access_denial_is_only_excused_for_the_handle_that_explains_it() {
        // The heart of the correction. `FlushFileBuffers` documents exactly one access
        // requirement — the handle must have `GENERIC_WRITE` — so `ERROR_ACCESS_DENIED` on
        // the read-only fallback is a statement about the handle and may be excused, while
        // the same code on a write-capable handle is a real denial the caller must see.
        // Excusing it on both is what made the tolerance unfalsifiable on Windows: every
        // openable path flushed "successfully", so neither `CommittedDurabilityUncertain`
        // nor `BundleDurabilityUnconfirmed` was reachable there at all.
        let denied = io::Error::from_raw_os_error(5);
        assert!(windows_flush_is_unavailable(&denied, false));
        assert!(!windows_flush_is_unavailable(&denied, true));

        // The other half: narrowing must not have narrowed to nothing. The two codes that
        // *are* attested for a write-capable handle are still excused on one.
        assert!(windows_flush_is_unavailable(
            &io::Error::from_raw_os_error(1),
            true
        ));
        assert!(windows_flush_is_unavailable(
            &io::Error::from_raw_os_error(50),
            true
        ));
    }

    #[test]
    fn the_windows_tolerance_is_exactly_three_codes_and_no_others() {
        // Every code in the tolerated set is exercised from both sides, including 1 and 50,
        // which an earlier version of this loop never reached: without them the expectation
        // was satisfied only by inputs that could not distinguish the arms it describes.
        for code in [1, 2, 5, 13, 22, 28, 50] {
            assert_eq!(
                windows_flush_is_unavailable(&io::Error::from_raw_os_error(code), true),
                matches!(code, 1 | 50),
                "write-capable handle, raw code {code}"
            );
            assert_eq!(
                windows_flush_is_unavailable(&io::Error::from_raw_os_error(code), false),
                matches!(code, 1 | 5 | 50),
                "read-only fallback, raw code {code}"
            );
        }
    }

    #[test]
    fn the_unavailable_classification_is_platform_specific_and_not_a_blanket_tolerance() {
        // The negative control for the platform gate, as distinct from the table it gates.
        // Raw code 5 is `ERROR_ACCESS_DENIED` on Windows and `EIO` on POSIX, a genuine
        // device failure a commit point must never treat as a successful flush. One code,
        // two meanings; only the platform tells them apart.
        for code in [1, 2, 5, 13, 22, 28, 50] {
            for has_flush_right in [true, false] {
                let error = io::Error::from_raw_os_error(code);
                assert_eq!(
                    flush_is_unavailable(&error, has_flush_right),
                    cfg!(windows) && windows_flush_is_unavailable(&error, has_flush_right),
                    "raw code {code}, write-capable {has_flush_right}"
                );
                assert!(
                    cfg!(windows) || !flush_is_unavailable(&error, has_flush_right),
                    "raw code {code} was excused off Windows"
                );
            }
        }
    }
}
