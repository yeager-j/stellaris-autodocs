//! Final live-source verification: does the tree a build read still hold what the build
//! read? (docs/technical-design.md, "Source snapshot consistency" steps 6-7.)
//!
//! Only a [`LiveSource`] can be asked. The check recomputes the authoritative live
//! fingerprint by re-scanning the root — hash-only, buffering nothing, which is the second
//! read the design accepts as correctness-first — re-reads every frozen asset capture, and
//! re-observes every referenced asset the build recorded as absent.
//!
//! **All three parts are always computed.** A verification that returned at the first
//! difference would report the edited script and stay silent about the asset that also
//! moved, and a partial answer to "did anything change" is the silent success this check
//! exists to prevent.
//!
//! The absence check is the one part that catches something nothing else can. Referenced
//! assets are outside the fingerprint by design, and an *absent* one is outside the
//! revision manifest's referenced-asset set as well, so a placeholder rendered for evidence
//! that arrived mid-build would otherwise survive every later freshness check until some
//! unrelated script edit happened to move the fingerprint.
//!
//! A source that moved is a result, not a failure: a vanished root, a file that became
//! unreadable, a changed byte, a rename, a deletion are all [`LiveVerification::Changed`].
//! The `Unexpected` channel carries only a genuine contract violation, and is the grounds
//! Phase 9 turns into an expected `SourceChangedDuringBuild` build outcome.

use crate::canonical::path::LogicalPath;
use crate::error::Unexpected;
use crate::source::enumerate::RootError;
use crate::source::fingerprint::{ContentHash, SourceFingerprint};
use crate::source::snapshot::{AssetAbsence, LiveSource, read_live_asset};
use crate::source::{ScanError, scan};
use std::io;

#[derive(Debug)]
pub enum LiveVerification {
    Unchanged,
    Changed(SourceChange),
}

/// Everything that stopped matching, never only the first thing.
///
/// Three orthogonal facts, each named for what happened, rather than one list a consumer
/// would have to re-classify: enumerated content moved, a frozen capture moved, or evidence
/// the build recorded as missing turned up.
#[derive(Debug)]
pub struct SourceChange {
    /// `None` when the enumerated content and the observation gaps still fingerprint the
    /// same. Referenced assets are absent from a fingerprint by design, so an asset-only
    /// change leaves this `None` and populates `assets` or `appeared`.
    pub fingerprint: Option<FingerprintMismatch>,
    /// One entry per frozen capture that no longer reads back as it was, in canonical
    /// logical-path order.
    pub assets: Vec<AssetChange>,
    /// One entry per referenced asset the build recorded as absent that now has readable
    /// bytes, in canonical logical-path order.
    ///
    /// Kept separate from `assets` because the two invalidate different things: an
    /// `AssetChange` invalidates a captured input the manifest quotes, while an appearance
    /// invalidates an "evidence absent" Analysis Issue and the placeholder rendered for it.
    pub appeared: Vec<AppearedAsset>,
}

/// The fingerprint half of the comparison.
///
/// Named for the usual case, but `observed` also carries "the source could not be scanned at
/// all", where nothing mismatched because nothing could be read. See [`ObservedFingerprint`];
/// the enclosing `Option` on [`SourceChange::fingerprint`] is what carries "these agree".
#[derive(Debug)]
pub struct FingerprintMismatch {
    pub expected: SourceFingerprint,
    pub observed: ObservedFingerprint,
}

#[derive(Debug)]
pub enum ObservedFingerprint {
    Computed(SourceFingerprint),
    /// The live source could no longer be scanned at all. Still a change, not an error: a
    /// root that went away under a build is exactly what verification is for.
    Unscannable(UnscannableSource),
}

/// Why a re-scan produced no fingerprint.
///
/// [`ScanError`] with its documented-unreachable `Duplicate` arm removed — that one is a
/// contract violation and leaves through `Unexpected`, so by the time a value of this type
/// exists the impossible case has already been discharged.
#[derive(Debug)]
pub enum UnscannableSource {
    Root(RootError),
    Read {
        logical: LogicalPath,
        kind: io::ErrorKind,
        detail: String,
    },
}

#[derive(Debug)]
pub struct AssetChange {
    pub logical: LogicalPath,
    /// The hash frozen at first read — what the revision manifest quotes.
    pub expected: ContentHash,
    pub observed: ObservedAsset,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ObservedAsset {
    Hash(ContentHash),
    Absent(AssetAbsence),
}

/// A referenced asset the build looked for, did not find, and which now exists.
#[derive(Debug)]
pub struct AppearedAsset {
    pub logical: LogicalPath,
    /// The absence this build froze, and attached "evidence absent" to.
    pub expected: AssetAbsence,
    /// The bytes that are there now.
    pub observed: ContentHash,
}

impl LiveSource {
    /// Compares the live tree with this snapshot, immediately before publication.
    ///
    /// **`Unchanged` means no change was observed, not that none happened.** The re-scan
    /// walks and hashes sequentially, so a file edited after this pass hashed it — the
    /// vanilla pass takes about half a second — is not seen, and the recomputed fingerprint
    /// can equal the snapshot's while the live tree no longer does. The window does not
    /// close at the last hash either: anything may change between the final comparison and
    /// the publication that follows it.
    ///
    /// That residual window is the protocol, not an oversight in it. The design calls this
    /// an *optimistic* snapshot protocol and specifies exactly one mechanism — recompute and
    /// compare (docs/technical-design.md, "Source snapshot consistency" steps 6-7) — whose
    /// stated guarantee runs one way: "A mismatch means the source changed during analysis."
    /// It never claims the converse. Closing the window would take a filesystem change
    /// generation or a watch held across the whole interval, which is a platform-specific
    /// mechanism no design section calls for; the cost of the miss is a revision pinned to
    /// bytes that were real when read, which the next Ensure re-checks and rebuilds.
    pub fn verify(&self) -> Result<LiveVerification, Unexpected> {
        let root = self.live_root();
        let snapshot = self.snapshot();

        let observed = match scan(root.path()) {
            Ok(scanned) => ObservedFingerprint::Computed(scanned.fingerprint),
            Err(ScanError::Root(error)) => {
                ObservedFingerprint::Unscannable(UnscannableSource::Root(error))
            }
            Err(ScanError::Read {
                logical,
                kind,
                detail,
            }) => ObservedFingerprint::Unscannable(UnscannableSource::Read {
                logical,
                kind,
                detail,
            }),
            // `classify_entries` emits one file per logical path, so a scan cannot offer a
            // duplicate. Typed rather than asserted, because nothing panics across a module
            // boundary (src/error.rs).
            Err(ScanError::Duplicate(error)) => {
                return Err(Unexpected::new(format!(
                    "source re-scan produced a duplicate logical path: {error}"
                )));
            }
        };
        let expected = snapshot.fingerprint();
        let fingerprint = match observed {
            ObservedFingerprint::Computed(live) if live == expected => None,
            observed => Some(FingerprintMismatch { expected, observed }),
        };

        // Fresh reads, deliberately not `read_asset`: that is the memo this check exists to
        // compare *against*, and asking it would compare the snapshot with itself.
        let mut assets = Vec::new();
        for captured in snapshot.captured_assets() {
            let live_path = snapshot.live_path(root, &captured.logical);
            let observed = match read_live_asset(root, &live_path) {
                Ok(bytes) => ObservedAsset::Hash(ContentHash::of(&bytes)),
                Err(absence) => ObservedAsset::Absent(absence),
            };
            if observed == ObservedAsset::Hash(captured.hash) {
                continue;
            }
            assets.push(AssetChange {
                logical: captured.logical,
                expected: captured.hash,
                observed,
            });
        }

        // Design step 7 is "publish only when the current paths and contents still match the
        // candidate's snapshot", and an absent path that now has bytes no longer matches.
        // Nothing else can catch it: assets are outside the fingerprint by design, and an
        // absence is outside the manifest's referenced-asset set too, so the "evidence
        // absent" placeholder would be permanent until some unrelated script edit happened
        // to move the fingerprint.
        //
        // Only absent-to-present counts. One absence turning into another — `NotFound` to
        // `Unreadable`, a containment refusal to a missing file — still yields no bytes, and
        // treating it as a change would make publication depend on host permission state: a
        // transient permission blip would abort an otherwise valid build.
        //
        // Dependency worth naming, because it is an assumption about a module that does not
        // exist yet: this holds only while Analysis Issue text does not distinguish the
        // absence kinds. `AssetAbsence`'s variants are deliberately *not* interchangeable —
        // `OutsideSourceRoot` exists precisely so a containment refusal is never read as
        // "the mod didn't ship it" — so if Phase 4 renders the kinds differently, a
        // `NotFound -> Unreadable` transition freezes an issue saying the mod did not ship a
        // file it demonstrably ships. That is the day to revisit this rule, not to inherit
        // it.
        let mut appeared = Vec::new();
        for (logical, expected) in snapshot.absent_assets() {
            if !expected.is_re_observable() {
                continue;
            }
            let live_path = snapshot.live_path(root, &logical);
            if let Ok(bytes) = read_live_asset(root, &live_path) {
                appeared.push(AppearedAsset {
                    logical,
                    expected,
                    observed: ContentHash::of(&bytes),
                });
            }
        }

        if fingerprint.is_none() && assets.is_empty() && appeared.is_empty() {
            return Ok(LiveVerification::Unchanged);
        }
        Ok(LiveVerification::Changed(SourceChange {
            fingerprint,
            assets,
            appeared,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::path::LogicalPath;
    use crate::source::snapshot::{AssetRead, Established, SourceKind, establish};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn write(root: &Path, relative: &str, contents: &[u8]) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn staged_source(root: &Path) {
        write(root, "descriptor.mod", b"name=\"Fixture\"\n");
        write(root, "common/technology/00_tech.txt", b"tech_a = {}\n");
        write(root, "common/technology/01_tech.txt", b"tech_b = {}\n");
        write(root, "localisation/english/l_english.yml", b"l_english:\n");
        write(root, "gfx/models/ship.dds", b"binary-ish");
    }

    fn path(raw: &str) -> LogicalPath {
        LogicalPath::parse(raw).unwrap()
    }

    fn established(root: &Path) -> LiveSource {
        match establish(SourceKind::TargetMod, root).unwrap() {
            Established::Complete(source) | Established::Incomplete(source) => source,
        }
    }

    /// A source whose one asset has already been captured, which is the state a build is in
    /// when it reaches the pre-publication check.
    fn established_with_captured_asset(root: &Path) -> LiveSource {
        let source = established(root);
        assert!(matches!(
            source.snapshot().read_asset(&path("gfx/models/ship.dds")),
            AssetRead::Captured(_)
        ));
        source
    }

    fn changed(source: &LiveSource) -> SourceChange {
        match source.verify().unwrap() {
            LiveVerification::Changed(change) => change,
            LiveVerification::Unchanged => panic!("expected a change"),
        }
    }

    #[test]
    fn an_untouched_tree_verifies_unchanged() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let source = established_with_captured_asset(dir.path());
        assert!(matches!(
            source.verify().unwrap(),
            LiveVerification::Unchanged
        ));
    }

    #[test]
    fn editing_a_script_file_is_a_fingerprint_change() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let source = established_with_captured_asset(dir.path());

        write(
            dir.path(),
            "common/technology/00_tech.txt",
            b"tech_a = { cost = 1 }\n",
        );

        let change = changed(&source);
        let mismatch = change.fingerprint.expect("the content moved");
        assert_eq!(mismatch.expected, source.snapshot().fingerprint());
        assert!(matches!(
            mismatch.observed,
            ObservedFingerprint::Computed(_)
        ));
        assert!(change.assets.is_empty());
    }

    #[test]
    fn deleting_a_script_file_is_a_fingerprint_change() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let source = established_with_captured_asset(dir.path());

        fs::remove_file(dir.path().join("common/technology/01_tech.txt")).unwrap();

        assert!(changed(&source).fingerprint.is_some());
    }

    #[test]
    fn renaming_a_script_file_is_a_fingerprint_change() {
        // The bytes are all still there; only a logical path moved.
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let source = established_with_captured_asset(dir.path());

        fs::rename(
            dir.path().join("common/technology/01_tech.txt"),
            dir.path().join("common/technology/09_tech.txt"),
        )
        .unwrap();

        let change = changed(&source);
        assert!(change.fingerprint.is_some());
        assert!(change.assets.is_empty());
    }

    #[test]
    fn editing_a_captured_asset_is_an_asset_change_only() {
        // Referenced assets are outside the fingerprint by design, so this is the case an
        // asset-blind check would publish as unchanged
        // (docs/technical-design.md, "Source snapshot consistency").
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let source = established_with_captured_asset(dir.path());

        write(dir.path(), "gfx/models/ship.dds", b"repainted");

        let change = changed(&source);
        assert!(change.fingerprint.is_none());
        assert_eq!(change.assets.len(), 1);
        assert_eq!(change.assets[0].logical, path("gfx/models/ship.dds"));
        assert_eq!(change.assets[0].expected, ContentHash::of(b"binary-ish"));
        assert_eq!(
            change.assets[0].observed,
            ObservedAsset::Hash(ContentHash::of(b"repainted"))
        );
    }

    #[test]
    fn deleting_a_captured_asset_is_an_asset_change() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let source = established_with_captured_asset(dir.path());

        fs::remove_file(dir.path().join("gfx/models/ship.dds")).unwrap();

        let change = changed(&source);
        assert!(change.fingerprint.is_none());
        assert_eq!(
            change.assets[0].observed,
            ObservedAsset::Absent(AssetAbsence::NotFound)
        );
    }

    #[test]
    fn renaming_a_captured_asset_is_an_asset_change() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let source = established_with_captured_asset(dir.path());

        fs::rename(
            dir.path().join("gfx/models/ship.dds"),
            dir.path().join("gfx/models/cruiser.dds"),
        )
        .unwrap();

        let change = changed(&source);
        assert_eq!(change.assets.len(), 1);
        assert_eq!(change.assets[0].logical, path("gfx/models/ship.dds"));
        assert_eq!(
            change.assets[0].observed,
            ObservedAsset::Absent(AssetAbsence::NotFound)
        );
    }

    #[test]
    fn both_halves_are_reported_never_the_first_one() {
        // The AC's "silent success" is a check that stops at the script edit and never
        // looks at the asset. Both must be present in one answer.
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let source = established_with_captured_asset(dir.path());

        write(dir.path(), "common/technology/00_tech.txt", b"edited\n");
        write(dir.path(), "gfx/models/ship.dds", b"repainted");

        let change = changed(&source);
        assert!(change.fingerprint.is_some());
        assert_eq!(change.assets.len(), 1);
    }

    #[test]
    fn a_root_that_vanished_is_a_change_not_an_error() {
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let source = established_with_captured_asset(dir.path());

        fs::remove_dir_all(dir.path()).unwrap();

        let change = changed(&source);
        let mismatch = change.fingerprint.expect("nothing could be scanned");
        assert!(matches!(
            mismatch.observed,
            ObservedFingerprint::Unscannable(UnscannableSource::Root(RootError::Unreadable {
                kind: io::ErrorKind::NotFound,
                ..
            }))
        ));
        // The captured asset is gone with it, and is reported rather than skipped.
        assert_eq!(
            change.assets[0].observed,
            ObservedAsset::Absent(AssetAbsence::NotFound)
        );
    }

    #[test]
    fn an_absence_that_became_present_is_a_change() {
        // Nothing else in the system could catch this: assets are outside the fingerprint by
        // design, and an absence is outside the manifest's referenced-asset set too, so the
        // "evidence absent" placeholder would be permanent. Design step 7: publish only when
        // the current paths still match the candidate's snapshot.
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let source = established(dir.path());
        assert!(matches!(
            source.snapshot().read_asset(&path("gfx/models/late.dds")),
            AssetRead::Absent(AssetAbsence::NotFound)
        ));

        write(dir.path(), "gfx/models/late.dds", b"arrived late");

        let change = changed(&source);
        assert!(change.fingerprint.is_none());
        assert!(change.assets.is_empty());
        assert_eq!(change.appeared.len(), 1);
        assert_eq!(change.appeared[0].logical, path("gfx/models/late.dds"));
        assert_eq!(change.appeared[0].expected, AssetAbsence::NotFound);
        assert_eq!(
            change.appeared[0].observed,
            ContentHash::of(b"arrived late")
        );

        // The freeze rule is unchanged inside the build: only `verify` re-observes, so the
        // stage that already attached "evidence absent" still sees an absence.
        assert!(matches!(
            source.snapshot().read_asset(&path("gfx/models/late.dds")),
            AssetRead::Absent(AssetAbsence::NotFound)
        ));
    }

    #[test]
    fn an_absence_that_is_still_absent_is_not_a_change() {
        // The other half of the rule, and what keeps it from being a permanent `Changed`:
        // a path that was missing and is still missing matches the snapshot.
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let source = established_with_captured_asset(dir.path());
        assert!(matches!(
            source.snapshot().read_asset(&path("gfx/models/late.dds")),
            AssetRead::Absent(AssetAbsence::NotFound)
        ));

        assert!(matches!(
            source.verify().unwrap(),
            LiveVerification::Unchanged
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_containment_refusal_that_became_readable_is_an_appearance() {
        // An escape is not "the mod didn't ship it", but repointing the link inside the root
        // does turn a refusal into evidence, and the placeholder must not survive it.
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("foreign.dds"), b"foreign").unwrap();
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let link = dir.path().join("gfx/models/escaped.dds");
        std::os::unix::fs::symlink(outside.path().join("foreign.dds"), &link).unwrap();
        let source = established(dir.path());
        assert!(matches!(
            source
                .snapshot()
                .read_asset(&path("gfx/models/escaped.dds")),
            AssetRead::Absent(AssetAbsence::OutsideSourceRoot)
        ));

        fs::remove_file(&link).unwrap();
        write(dir.path(), "gfx/models/escaped.dds", b"now shipped");

        let change = changed(&source);
        assert_eq!(change.appeared.len(), 1);
        assert_eq!(change.appeared[0].expected, AssetAbsence::OutsideSourceRoot);
    }

    #[cfg(unix)]
    #[test]
    fn removing_a_dangling_symlink_is_a_change_though_no_file_moved() {
        // The end-to-end form of the /v3 regression: the build observed a broken source and
        // attached "evidence absent" to it. Repairing the source before publication must not
        // publish a revision that claims to describe the repaired one.
        let dir = TempDir::new().unwrap();
        staged_source(dir.path());
        let link = dir.path().join("common/dangling.txt");
        std::os::unix::fs::symlink("nowhere.txt", &link).unwrap();
        let source = established_with_captured_asset(dir.path());
        assert!(!source.snapshot().gaps().is_empty());

        fs::remove_file(&link).unwrap();

        let change = changed(&source);
        assert!(change.fingerprint.is_some());
        assert!(change.assets.is_empty());
    }
}
