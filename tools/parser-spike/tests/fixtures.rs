//! The semantic checks, run as ordinary tests.
//!
//! Same functions the `semantics` binary captures into `p6-semantics`, so a green test run
//! and a green record cannot disagree about what passed.

use parser_spike::checks;

#[test]
fn every_semantic_check_passes() {
    let checks = checks::run();
    assert!(!checks.is_empty(), "no checks ran");

    let failures: Vec<_> = checks
        .iter()
        .filter(|check| !check.passed())
        .map(|check| check.line())
        .collect();

    assert!(
        failures.is_empty(),
        "{} of {} semantic checks failed:\n{}",
        failures.len(),
        checks.len(),
        failures.join("\n")
    );
}

#[test]
fn every_evidence_requirement_named_by_the_checks_is_covered() {
    // A guard against a check being deleted or renamed into irrelevance: the requirements
    // this file claims to answer are listed here, and a missing one fails rather than
    // quietly reducing coverage.
    let checks = checks::run();
    for requirement in [2u8, 5, 8, 9, 10, 11] {
        assert!(
            checks.iter().any(|check| check.requirement == requirement),
            "no check bears on evidence requirement {requirement}"
        );
    }
}
