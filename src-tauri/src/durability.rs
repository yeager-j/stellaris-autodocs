//! Making a directory entry durable, and what it means when the platform does not
//! provide that operation (docs/decision-log.md, D-123).
//!
//! Not part of the technical design's named module map: like [`canonical`](crate::canonical),
//! this is a leaf primitive below the deep-module row, and depending on it is not a peer
//! edge. It exists because two protocols — `state::replace`'s state-document replacement
//! and `revisions`'s bundle publication — both reach a commit point whose durability rests
//! on the same platform fact, and a fact with two implementations is a fact with two
//! answers.
//!
//! # The rule
//!
//! Flushing a *file* is universal: `fsync` on POSIX, `FlushFileBuffers` on Windows, both
//! reached through [`std::fs::File::sync_all`]. Flushing a *directory* is not, and the
//! difference is not a gap in `std`:
//!
//! - On POSIX a newly created, renamed, or removed entry is durable only after its parent
//!   directory is itself flushed. `fsync` on a directory file descriptor is the documented
//!   way to do that, and skipping it is a real data-loss bug.
//! - On Windows a directory cannot even be opened without `FILE_FLAG_BACKUP_SEMANTICS`
//!   (`CreateFileW` returns `ERROR_ACCESS_DENIED` otherwise), and `FlushFileBuffers` on the
//!   resulting handle is not a supported operation — it refuses rather than flushing
//!   metadata. There is no Win32 call that means "flush this directory's entries".
//!
//! NTFS is why that is not simply a missing feature. NTFS is a metadata-journalling
//! filesystem: creating, renaming, and removing a directory entry are logged transactions
//! in `$LogFile`, and the log record for an entry is ordered ahead of the volume changes it
//! describes. A crash therefore replays the journal and either has the entry or does not —
//! it cannot produce the POSIX shape this flush exists to prevent, a directory entry that
//! names a file whose creation never reached disk. The operation is absent because the
//! ordering it buys is already in the filesystem.
//!
//! # The residual risk, stated rather than hidden
//!
//! This *is* a durability weakening on Windows, and it is bounded by the filesystem rather
//! than by the API:
//!
//! - **On NTFS**, the journal's ordering stands in for the flush, but the application
//!   cannot force the journal to reach the platter at a chosen moment the way `fsync` can.
//!   A crash inside the journal's own write-back window can lose a recently created entry
//!   that POSIX would have made durable on demand.
//! - **On exFAT, FAT32, and SMB or other network shares**, there is no metadata journal at
//!   all, so nothing stands in for the flush. A removable drive or a redirected
//!   application-data directory is the realistic way a user reaches this.
//!
//! Both callers' worst case is the same and is recoverable rather than corrupting: a
//! directory entry that a crash erased leaves a state document or a published bundle that
//! is absent rather than damaged, and both callers already fail closed on absence — the
//! Revision identifier is a pure function of content, so the same rebuild republishes to
//! the same path and repairs it. What is lost is the *guarantee*, not the recovery.
//!
//! The Phase 12 Windows packaged smoke test is where this stops being an API-contract claim
//! and becomes an observation on a real machine, alongside the rename-semantics claims
//! `state::replace` and `revisions` already record (docs/technical-design.md,
//! "Verification architecture").

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
    let directory = open_directory(path)?;
    match directory.sync_all() {
        Err(error) if flush_is_unavailable(&error) => Ok(()),
        outcome => outcome,
    }
}

#[cfg(not(windows))]
fn open_directory(path: &Path) -> io::Result<fs::File> {
    fs::File::open(path)
}

/// `CreateFileW` refuses a directory unless `FILE_FLAG_BACKUP_SEMANTICS` is set, which
/// `File::open` does not set. Without this the flush below is never even reached.
#[cfg(windows)]
fn open_directory(path: &Path) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

    fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
}

/// Whether an error from flushing a directory handle means "this platform does not provide
/// the operation" rather than "the operation failed".
///
/// **Never true off Windows**, deliberately: on POSIX the flush is provided, so every error
/// it returns is a durability failure a caller must see. Making the tolerance
/// platform-specific rather than error-code-specific is what keeps this from quietly
/// swallowing an `EIO` on the platform the project is developed on.
#[cfg(not(windows))]
fn flush_is_unavailable(_error: &io::Error) -> bool {
    false
}

/// `FlushFileBuffers` on a directory handle refuses rather than flushing. Windows reports
/// that refusal as `ERROR_ACCESS_DENIED` on NTFS and has been observed to report
/// `ERROR_INVALID_FUNCTION` or `ERROR_NOT_SUPPORTED` on other filesystems and redirectors,
/// so all three are read as "not provided". Anything else — a disconnected volume, a device
/// error — is still a failure.
#[cfg(windows)]
fn flush_is_unavailable(error: &io::Error) -> bool {
    const ERROR_INVALID_FUNCTION: i32 = 1;
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_NOT_SUPPORTED: i32 = 50;

    matches!(
        error.raw_os_error(),
        Some(ERROR_INVALID_FUNCTION | ERROR_ACCESS_DENIED | ERROR_NOT_SUPPORTED)
    )
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
        // must not be excused as an unavailable operation.
        let dir = TempDir::new().unwrap();
        assert!(sync_dir(&dir.path().join("absent")).is_err());
    }

    #[test]
    fn the_unavailable_classification_is_platform_specific_and_not_a_blanket_tolerance() {
        // The negative control for the tolerance itself, and the reason it is written over
        // the platform rather than over the error. Raw code 5 is `ERROR_ACCESS_DENIED` on
        // Windows — the refusal `FlushFileBuffers` returns for a directory handle — and
        // `EIO` on POSIX, a genuine device failure a commit point must never treat as a
        // successful flush. One code, two meanings; only the platform tells them apart.
        assert_eq!(
            flush_is_unavailable(&io::Error::from_raw_os_error(5)),
            cfg!(windows)
        );

        // Nothing this project's own platform can produce is tolerated. On Windows the set
        // is exactly the three refusal codes above; everywhere else it is empty.
        for code in [2, 5, 13, 22, 28] {
            assert_eq!(
                flush_is_unavailable(&io::Error::from_raw_os_error(code)),
                cfg!(windows) && matches!(code, 1 | 5 | 50)
            );
        }
    }
}
