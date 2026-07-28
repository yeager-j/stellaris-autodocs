//! Which published bundles are being read right now, and the permission a retention sweep
//! must hold before it may delete one (docs/technical-design.md, "Revision retention";
//! docs/decision-log.md, D-066, D-125, D-126).
//!
//! # A pin is a count, never an open file
//!
//! On Windows a rename or delete of a directory tree is refused with a sharing violation
//! while a handle inside it is open, rather than proceeding as it does under POSIX
//! (`revisions/mod.rs`, "Platform caveat: an open handle can refuse a directory rename").
//! A reader that kept a `File` open would therefore obstruct the publication and the
//! cleanup this pin exists to *coordinate with*, and it would do so through the filesystem,
//! where neither of them can see it coming or say why they failed. Every read a
//! [`RevisionReader`](crate::revisions::RevisionReader) performs is read-and-close, and
//! the whole of the hold is an entry in the map below.
//!
//! The consequence to state plainly: **a pin excludes nothing outside this process.**
//! Another process, or a person with a shell, can delete a pinned bundle. That one Desktop
//! Host owns one application-data directory (D-065) is the compensating control, and it is
//! a control over the writer rather than a lock on the directory.
//!
//! # Why pinning and retiring share one map
//!
//! Retention's question is not "is this pinned" but "may I delete this". Those differ by a
//! window: between a `bool` answer and the `remove_dir_all` that acts on it, one
//! [`open_revision`](crate::revisions::RevisionStore::open_revision) can complete, and the
//! reader that wins that race holds a pin on a directory being deleted — the contract
//! false at exactly the moment it is load-bearing. On Windows the deletion would even
//! *succeed* in that window, precisely because no handle is held.
//!
//! So the two states are one enum in one map, and each refuses the other. A sweep takes a
//! [`RetirementClaim`] and holds it across its deletion; while it is outstanding, opening
//! that revision is refused. Neither operation holds the map's lock across filesystem I/O,
//! so a sweep's deletion cannot stall a read of an unrelated revision.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::revisions::manifest::RevisionIdentity;

/// Which revisions this process is reading, and which it is retiring.
pub(super) struct PinRegistry {
    entries: Mutex<BTreeMap<RevisionIdentity, PinState>>,
}

/// What one revision is currently doing, as one value rather than two flags.
///
/// **The exclusivity is the atomicity claim.** A pin is granted only when no claim is
/// outstanding and a claim only when no pin is held, and both decisions happen under one
/// lock against one entry, so there is no window between asking and acting. Two
/// independent booleans, or a count beside a flag, would each be a state where "readable"
/// and "being deleted" are simultaneously true.
///
/// A revision with no entry is neither: absent means unpinned and claimable, so no
/// zero-valued state is representable.
enum PinState {
    Held(NonZeroUsize),
    Retiring,
}

impl PinRegistry {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(BTreeMap::new()),
        })
    }

    /// Takes a hold on `revision`, refusing while it is being retired.
    ///
    /// Performs no I/O, which is what lets
    /// [`open_revision`](crate::revisions::RevisionStore::open_revision) call it *before*
    /// validating the directory: the alternative order leaves a window in which a claim
    /// could be granted between proving the bundle intact and taking a hold on it.
    pub(super) fn pin(self: &Arc<Self>, revision: RevisionIdentity) -> Result<BundlePin, Retiring> {
        match self.lock().entry(revision) {
            Entry::Vacant(vacant) => {
                vacant.insert(PinState::Held(NonZeroUsize::MIN));
            }
            Entry::Occupied(mut occupied) => match occupied.get() {
                PinState::Retiring => return Err(Retiring),
                // Saturating rather than wrapping: the count needs `usize::MAX` live
                // readers of one revision to reach it, and if it somehow did, a saturated
                // count leaks a pin — which fails toward never deleting rather than toward
                // deleting something being read.
                PinState::Held(held) => {
                    occupied.insert(PinState::Held(held.saturating_add(1)));
                }
            },
        }
        Ok(BundlePin {
            registry: Arc::clone(self),
            revision,
        })
    }

    /// Claims `revision` for deletion, refusing while any reader holds it or another claim
    /// is outstanding.
    pub(super) fn claim(
        self: &Arc<Self>,
        revision: RevisionIdentity,
    ) -> Result<RetirementClaim, RetirementRefused> {
        match self.lock().entry(revision) {
            Entry::Vacant(vacant) => {
                vacant.insert(PinState::Retiring);
            }
            Entry::Occupied(occupied) => {
                return Err(match occupied.get() {
                    PinState::Held(held) => RetirementRefused::Pinned {
                        handles: held.get(),
                    },
                    PinState::Retiring => RetirementRefused::AlreadyClaimed,
                });
            }
        }
        Ok(RetirementClaim {
            registry: Arc::clone(self),
            revision,
        })
    }

    /// How many readers hold `revision`. Test-only: production code asks
    /// [`claim`](PinRegistry::claim), which answers the question that has a consequence.
    #[cfg(test)]
    pub(super) fn handles(&self, revision: RevisionIdentity) -> usize {
        match self.lock().get(&revision) {
            Some(PinState::Held(held)) => held.get(),
            Some(PinState::Retiring) | None => 0,
        }
    }

    /// Drops one hold, removing the entry with the last one.
    ///
    /// A release of a revision this registry is not holding is a defect in the pairing
    /// rather than a caller's input, and it is ignored rather than asserted: this runs
    /// inside [`BundlePin::drop`], where a panic during unwinding aborts the process.
    fn release(&self, revision: RevisionIdentity) {
        let mut entries = self.lock();
        let Entry::Occupied(mut occupied) = entries.entry(revision) else {
            return;
        };
        match occupied.get() {
            PinState::Held(held) => match NonZeroUsize::new(held.get() - 1) {
                Some(remaining) => {
                    occupied.insert(PinState::Held(remaining));
                }
                None => {
                    occupied.remove();
                }
            },
            PinState::Retiring => {}
        }
    }

    /// Ends a claim, whether or not the holder deleted anything.
    ///
    /// **Abandoning a claim is always safe**, which is why it needs no outcome argument:
    /// the entry is a permission, not a record of work done, so a sweep that deleted the
    /// directory and one that gave up leave the same map. What is true on disk is
    /// re-observed by the next open, never remembered here.
    fn abandon_claim(&self, revision: RevisionIdentity) {
        let mut entries = self.lock();
        if matches!(entries.get(&revision), Some(PinState::Retiring)) {
            entries.remove(&revision);
        }
    }

    /// Recovers from poisoning rather than propagating it, for the reason
    /// [`RevisionStore::lock`](crate::revisions::RevisionStore) records and one more that
    /// is specific to this map: every mutation here is a single `entry` update with no
    /// fallible step between reading a state and writing its successor, so no unwind can
    /// leave a count half-applied. **An edit that made one operation two would end that**,
    /// and the argument would have to be remade rather than inherited.
    ///
    /// The second reason is that [`BundlePin::drop`] takes this lock. A panicking `Drop`
    /// during an unwind aborts the process, so propagating poison here would turn one
    /// unrelated panic into an abort — and, short of that, into documentation that is
    /// permanently unreadable for the life of the process.
    fn lock(&self) -> MutexGuard<'_, BTreeMap<RevisionIdentity, PinState>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One reader's hold on one bundle.
///
/// Not `pub`: it leaves this module only inside a
/// [`RevisionReader`](crate::revisions::RevisionReader), because a hold that could be
/// taken without opening anything would be a second way to block retention, answerable to
/// nothing.
pub(super) struct BundlePin {
    registry: Arc<PinRegistry>,
    revision: RevisionIdentity,
}

impl BundlePin {
    pub(super) fn revision(&self) -> RevisionIdentity {
        self.revision
    }
}

/// **The crate's first `Drop`, and it is load-bearing rather than tidy.** "Eligibility
/// begins only after the last handle releases" has to hold on every path a reader's owner
/// can leave by, including an early `?` in a caller and an unwinding panic. An explicit
/// `release()` would be true only by discipline, and the symptom of the first path that
/// forgets it — retention silently never deleting a bundle — is invisible until a disk
/// fills.
///
/// Release builds set `panic = "abort"`, so an aborting panic runs no destructor. That
/// costs nothing here: it also ends the process, and this map is process memory that dies
/// with it. There is no durable state to leak.
impl Drop for BundlePin {
    fn drop(&mut self) {
        self.registry.release(self.revision);
    }
}

/// A revision cannot be pinned because it is being retired.
///
/// Private: [`open_revision`](crate::revisions::RevisionStore::open_revision) is the only
/// caller and reports it as its own outcome, so this never crosses the module boundary as
/// itself.
#[derive(Debug)]
pub(super) struct Retiring;

/// The right to delete one bundle: granted only when no reader holds it, and refusing every
/// [`open_revision`](crate::revisions::RevisionStore::open_revision) of that revision while
/// it is outstanding.
///
/// **Two-phase deliberately.** The alternative shape — running the deletion inside a
/// closure while this module's lock is held — would put `remove_dir_all` of a whole
/// revision inside a mutex that every documentation read must take, so cleanup would stall
/// reads of unrelated revisions. Holding the *claim* across the deletion instead closes
/// the same window and holds no lock at all.
///
/// **No caller yet, and that is accurate rather than aspirational.** Phase 9's retention
/// sweep is the consumer (docs/implementation-plan.md, Phase 9 item 4). It is `pub` for the
/// reason [`is_staging_attempt_name`](crate::revisions::is_staging_attempt_name) is:
/// demoting it makes it dead code under `-D warnings`, which would say accurately that the
/// sweep does not exist, and a test asserting the rule inline would become a second
/// authority over it. It grants nothing a caller did not already have — a directory under
/// `bundles/` was always deletable by anything with the path — and adds one refusal to it.
/// Demote it to `pub(super)` when the sweep lands inside this module and calls it.
#[must_use = "dropping a claim ends it without retiring anything"]
pub struct RetirementClaim {
    registry: Arc<PinRegistry>,
    revision: RevisionIdentity,
}

impl RetirementClaim {
    /// Which revision this claim permits deleting. The path is not exposed: the sweep that
    /// deletes it lives inside this module (docs/decision-log.md, D-087).
    pub fn revision(&self) -> RevisionIdentity {
        self.revision
    }
}

impl fmt::Debug for RetirementClaim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RetirementClaim({})", self.revision)
    }
}

impl Drop for RetirementClaim {
    fn drop(&mut self) {
        self.registry.abandon_claim(self.revision);
    }
}

/// Why a bundle may not be retired right now. Both arms are "later", never "never".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetirementRefused {
    /// Readers hold this bundle.
    ///
    /// The count travels rather than a bare "pinned", for two reasons. A sweep's log wants
    /// to say what it is waiting on, and — the one that decides the type — a boolean would
    /// let an implementation that tracked *whether* rather than *how many* pass a
    /// last-handle test: with two readers open and one dropped, a flag would already read
    /// as released.
    Pinned { handles: usize },
    /// Another claim is outstanding, so something else is already retiring it.
    AlreadyClaimed,
}

impl fmt::Display for RetirementRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pinned { handles } => write!(
                f,
                "this revision is still being read by {handles} open reader(s), so it \
                 cannot be retired yet"
            ),
            Self::AlreadyClaimed => f.write_str(
                "this revision is already claimed for retirement by something else, so it \
                 cannot be claimed again",
            ),
        }
    }
}

impl std::error::Error for RetirementRefused {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two identifiers that are not each other, built without staging anything: this
    /// module decides nothing about what a bundle is, so its tests need no bundle.
    fn revision(byte: u8) -> RevisionIdentity {
        RevisionIdentity::parse(&format!("{byte:02x}").repeat(32)).unwrap()
    }

    #[test]
    fn a_revision_nothing_holds_is_immediately_claimable() {
        // The positive control the refusal tests rest on: without it, a `claim` that
        // refused unconditionally would pass every one of them.
        let registry = PinRegistry::new();
        let claim = registry.claim(revision(1)).unwrap();
        assert_eq!(claim.revision(), revision(1));
    }

    #[test]
    fn a_revision_with_an_open_pin_cannot_be_claimed_for_retirement() {
        let registry = PinRegistry::new();
        let _pin = registry.pin(revision(1)).unwrap();

        assert_eq!(
            registry.claim(revision(1)).unwrap_err(),
            RetirementRefused::Pinned { handles: 1 }
        );
        // Another revision is unaffected: the refusal is per-revision and not a global
        // "something is being read" flag.
        assert!(registry.claim(revision(2)).is_ok());
    }

    #[test]
    fn retirement_eligibility_begins_only_after_the_last_pin_releases() {
        // **The negative control against a flag-shaped implementation.** With two holds
        // taken and one dropped, anything that tracked *whether* rather than *how many*
        // would already report the revision free, and a sweep would delete a bundle a
        // live reader is serving from.
        let registry = PinRegistry::new();
        let first = registry.pin(revision(1)).unwrap();
        let second = registry.pin(revision(1)).unwrap();
        assert_eq!(registry.handles(revision(1)), 2);

        drop(first);
        assert_eq!(
            registry.claim(revision(1)).unwrap_err(),
            RetirementRefused::Pinned { handles: 1 }
        );

        drop(second);
        assert_eq!(registry.handles(revision(1)), 0);
        assert!(registry.claim(revision(1)).is_ok());
    }

    #[test]
    fn an_outstanding_claim_refuses_every_pin_so_a_deletion_cannot_race_a_reader() {
        // The whole reason the two states share one map. A sweep holds its claim across
        // `remove_dir_all`; anything opening that revision meanwhile is refused rather
        // than handed a reader onto a directory that is going away.
        let registry = PinRegistry::new();
        let claim = registry.claim(revision(1)).unwrap();

        assert!(registry.pin(revision(1)).is_err());

        drop(claim);
        assert!(registry.pin(revision(1)).is_ok());
    }

    #[test]
    fn two_claims_for_one_revision_cannot_be_outstanding_at_once() {
        let registry = PinRegistry::new();
        let _claim = registry.claim(revision(1)).unwrap();

        assert_eq!(
            registry.claim(revision(1)).unwrap_err(),
            RetirementRefused::AlreadyClaimed
        );
    }

    #[test]
    fn abandoning_a_claim_leaves_the_registry_as_it_found_it() {
        // A claim is a permission and not a record of work: a sweep that deleted the
        // directory and one that gave up must leave the same map, or the next open would
        // be answering from memory instead of from disk.
        let registry = PinRegistry::new();
        drop(registry.claim(revision(1)).unwrap());
        assert_eq!(registry.handles(revision(1)), 0);
        assert!(registry.claim(revision(1)).is_ok());
    }

    #[test]
    fn a_pin_dropped_while_unwinding_still_releases_its_revision() {
        // Why release is a `Drop` and not a method. The path that skips an explicit
        // `release()` is not the one somebody forgets to write — it is the one no code
        // was written for at all.
        let registry = PinRegistry::new();
        let held = Arc::clone(&registry);
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _pin = held.pin(revision(1)).unwrap();
            panic!("a reader's owner panicked mid-read");
        }));

        assert!(unwound.is_err());
        assert_eq!(registry.handles(revision(1)), 0);
        assert!(registry.claim(revision(1)).is_ok());
    }

    #[test]
    fn a_pin_and_a_claim_for_one_revision_never_coexist_under_contention() {
        // The atomicity `PinState` encodes, observed rather than argued. Threads race
        // `pin` against `claim` on one revision; every iteration asserts that whichever
        // won, the other was refused. A check-then-act implementation — read a count,
        // then write a flag — fails this.
        let registry = PinRegistry::new();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let registry = Arc::clone(&registry);
                scope.spawn(move || {
                    for _ in 0..500 {
                        if let Ok(claim) = registry.claim(revision(1)) {
                            assert!(
                                registry.pin(revision(1)).is_err(),
                                "a claim was granted while a pin could still be taken"
                            );
                            drop(claim);
                        } else if let Ok(pin) = registry.pin(revision(1)) {
                            assert!(
                                registry.claim(revision(1)).is_err(),
                                "a pin was held while a claim could still be granted"
                            );
                            drop(pin);
                        }
                    }
                });
            }
        });

        // Every hold and claim released, so the map is empty rather than merely quiet.
        assert_eq!(registry.handles(revision(1)), 0);
        assert!(registry.claim(revision(1)).is_ok());
    }

    #[test]
    fn retirement_refusals_display_and_implement_std_error() {
        // Sweeps will `?` and log these; both need `Display`, and `std::error::Error` is
        // what makes `?` conversion available (state/mutations.rs:690's rationale). No
        // rendering leaks a debug variant name, because these reach a person.
        let refusals = [
            RetirementRefused::Pinned { handles: 3 },
            RetirementRefused::AlreadyClaimed,
        ];
        for refusal in refusals {
            let rendered = refusal.to_string();
            assert!(!rendered.is_empty());
            assert!(
                !rendered.contains("Pinned") && !rendered.contains("AlreadyClaimed"),
                "the variant name leaked into {rendered:?}"
            );
            // Neither arm wraps a cause: the finding is the message, and the values a
            // reader needs are already in it.
            let error: &dyn std::error::Error = &refusal;
            assert!(error.source().is_none());
        }
        assert!(
            RetirementRefused::Pinned { handles: 3 }
                .to_string()
                .contains('3'),
            "the count a sweep waits on must reach the message"
        );
    }
}
