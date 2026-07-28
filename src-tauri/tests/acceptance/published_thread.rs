//! The Phase 3 acceptance cases: the walking skeleton's thread, and the controls that make it
//! mean something.

use stellaris_docs_lib::application::{DocumentationEntry, ReadEntryListError, UnavailableReason};
use stellaris_docs_lib::error::Failure;

use crate::corpora;
use crate::harness::AcceptanceThread;

#[test]
fn a_build_publishes_entries_that_the_read_serves_back() {
    let thread = AcceptanceThread::boot(corpora::trivial());

    thread.build().expect("the trivial corpus publishes");
    let served = thread
        .desktop_entries()
        .expect("the published revision serves its entry list");

    assert_eq!(served.installation, thread.installation());
    assert_eq!(
        served.entries,
        vec![DocumentationEntry {
            category: "technology".to_owned(),
            identifier: "tech_a".to_owned(),
            display_name: Some("Fixture Technology".to_owned()),
        }],
    );
}

/// The causation control. Without it, the case above is equally consistent with a read that
/// serves whatever the harness happens to be holding.
#[test]
fn reading_before_any_build_reports_that_nothing_is_published() {
    let thread = AcceptanceThread::boot(corpora::trivial());

    let refusal = thread.desktop_entries().unwrap_err();

    assert!(matches!(
        refusal,
        Failure::Expected(ReadEntryListError::NoPublishedRevision)
    ));
}

/// Its meaning comes from `reading_before_any_build_reports_that_nothing_is_published` sitting
/// beside it: an empty list is also what a build that silently did nothing would leave behind,
/// and that control is what rules the second reading out.
#[test]
fn a_revision_that_documents_nothing_is_a_success_with_no_entries() {
    let thread = AcceptanceThread::boot(corpora::documents_nothing());

    thread.build().expect("the empty corpus publishes");
    let served = thread
        .desktop_entries()
        .expect("a revision documenting nothing is still a revision");

    assert!(served.entries.is_empty());
}

/// The empty-versus-absent line, end to end. A candidate with no documents publishes cleanly —
/// `revisions` has no rule against it — and its manifest then names no entry-list entry, which
/// the reader reports as absence rather than as an empty list. Conflating the two here would
/// erase a distinction two layers below.
#[test]
fn a_revision_carrying_no_entry_list_is_a_different_answer_from_one_that_documents_nothing() {
    let thread = AcceptanceThread::boot(corpora::carries_no_entry_list());

    thread
        .build()
        .expect("a revision carrying no documents publishes");
    let refusal = thread.desktop_entries().unwrap_err();

    assert!(matches!(
        refusal,
        Failure::Expected(ReadEntryListError::DocumentationUnavailable {
            reason: UnavailableReason::RevisionCarriesNoEntryList
        })
    ));
}

/// Durability as a semantic fact rather than as `PublishedDurability`, which reports
/// `NotProvidedByPlatform` on a volume that cannot flush a directory entry (D-123) and would
/// therefore go red for a developer whose `TMPDIR` is a network mount — a failure about the
/// machine, not about the product.
#[test]
fn a_published_revision_survives_a_restart() {
    let thread = AcceptanceThread::boot(corpora::trivial());
    thread.build().expect("the trivial corpus publishes");

    let restarted = thread.reopen();
    let served = restarted
        .desktop_entries()
        .expect("the revision published before the restart is still the published revision");

    assert_eq!(served.installation, restarted.installation());
    assert_eq!(
        served.entries,
        vec![DocumentationEntry {
            category: "technology".to_owned(),
            identifier: "tech_a".to_owned(),
            display_name: Some("Fixture Technology".to_owned()),
        }],
    );
    // The restarted host was given `NoAnalysisSource`, so nothing in this process could have
    // republished what the read just served: it came off disk.
    assert!(
        restarted.build().is_err(),
        "a restarted production-shaped host cannot publish",
    );
}

/// The honesty control, and the only case here that observes the fixture bytes at all: without
/// it, a harness that ignored its corpus entirely would pass every other case in this target.
///
/// One installation, rebuilt over a second corpus carrying identical hand-authored
/// documentation and different bytes. Both halves of the claim are asserted, and both are
/// asserted the only way that is unconfounded — by holding the installation fixed, because two
/// separately booted threads mint two Discovery Location identifiers and would publish
/// differing revisions whatever their corpora contributed:
///
/// - **The corpus reaches the published revision.** The two revision identifiers differ, and
///   the only thing that differed between the builds is the fixture bytes.
/// - **The corpus reaches nothing a reader can see.** The entries served before and after are
///   identical, because no analysis exists and the documented content came from the corpus
///   definition rather than from its bytes.
///
/// **Phase 6 must make the second half go red**, and replace it with the assertion that two
/// corpora document *different* entries. Leaving it green there would mean `analysis` ran and
/// changed nothing.
#[test]
fn the_fixture_bytes_reach_the_revision_and_nothing_a_reader_can_see() {
    let thread = AcceptanceThread::boot(corpora::trivial());
    let changed = corpora::trivial_over_different_bytes();
    // The precondition that keeps the comparison below from being vacuous: the second corpus
    // really is a different observation, on both sides.
    assert_ne!(
        thread.corpus().target_mod().fingerprint(),
        changed.target_mod().fingerprint(),
    );
    assert_ne!(
        thread.corpus().vanilla_content().fingerprint(),
        changed.vanilla_content().fingerprint(),
    );

    let first = thread.build().expect("the first corpus publishes");
    let first_entries = thread.desktop_entries().unwrap().entries;

    let rebuilt = thread.rebuild_over(changed);
    let second = rebuilt
        .build()
        .expect("the rebuild over new bytes publishes");

    assert_ne!(first.revision, second.revision);
    assert_eq!(second.installation, first.installation);
    assert_eq!(rebuilt.desktop_entries().unwrap().entries, first_entries);
}
