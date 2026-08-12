//! "Publish a pass rate against a real org corpus as an enforced
//! invariant, then raise it."
//!
//! The obvious measure is the wrong one. closure's parser is
//! span-preserving: anything it does not recognise becomes a paragraph
//! and prints back verbatim, so a file carrying `#+TBLFM:`, a timestamp
//! repeater, a LaTeX fragment and an entity roundtrips byte-exact
//! today — checked, not assumed. A roundtrip rate over real org would
//! read close to 100% while none of those four did anything.
//!
//! What "org compatible subset" has to mean is *understood*: the
//! construct has semantics closure can act on, and a test that proves
//! it. So conformance is a matrix of constructs, the rate is the
//! fraction understood, and every claim in it names the test that
//! backs it — a matrix nobody checks is a marketing document.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{CONFORMANCE, Support, conformance_rate, offered_rate};

/// The floor. Raise it when a construct moves up; never lower it.
///
/// 2026-08-11: 61% of 34 constructs. Then repeaters, entities, LaTeX,
/// #+INCLUDE, inline tasks, column view, #+TBLFM, noweb and #+CALL took
/// it to 91, and org macros, #+SETUPFILE and radio targets to 100.
///
/// At which point `the_matrix_is_worth_having` fired, exactly as it was
/// written to: a matrix where everything is understood has stopped
/// looking at what org has. Eight constructs closure genuinely does not
/// handle went in — statistics cookies, #+STARTUP:, clocktables, export
/// attributes, time ranges, diary sexps, sub/superscripts, column
/// groups — and the honest number over the longer list is 80.
///
/// Which is the number worth ratcheting. 100% of a list somebody chose
/// is a claim about the list.
const FLOOR: u32 = 95;

#[test]
fn the_rate_never_falls() {
    let rate = conformance_rate();
    assert!(
        rate >= FLOOR,
        "org conformance fell to {rate}%, floor is {FLOOR}% — a construct \
         stopped being understood, or one was added without support"
    );
}

#[test]
fn every_construct_that_claims_support_names_a_test() {
    // A matrix nobody checks is a marketing document.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root");
    for c in CONFORMANCE {
        if c.support == Support::Understood {
            assert!(
                !c.evidence.is_empty(),
                "`{}` claims to be understood and names no test",
                c.name
            );
            assert!(
                root.join(c.evidence).exists(),
                "`{}` names `{}`, which does not exist",
                c.name,
                c.evidence
            );
        }
    }
}

#[test]
fn nothing_is_listed_twice() {
    let mut names: Vec<&str> = CONFORMANCE.iter().map(|c| c.name).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(before, names.len(), "a construct is listed more than once");
}

#[test]
fn the_matrix_is_worth_having() {
    // Small enough to be a lie by omission is the other way to get a
    // flattering number.
    assert!(
        CONFORMANCE.len() >= 30,
        "only {} constructs listed",
        CONFORMANCE.len()
    );
    assert!(
        CONFORMANCE.iter().any(|c| c.support == Support::Preserved),
        "a matrix where everything is understood is a matrix that is \
         not looking at what org has"
    );
}

/// The floor for what a shell actually offers. Raise it the same way.
///
/// 2026-08-12: 100% of the listed constructs, understood and offered.
/// Which makes the list the thing to grow now — nothing
/// parsed and invisible. The gap is the point of two numbers: it was 91
/// and 70 before the unwired parsers were wired up, and nothing about
/// the first number would have shown that.
const OFFERED_FLOOR: u32 = 95;

#[test]
fn what_a_shell_offers_never_falls_either() {
    let rate = offered_rate();
    assert!(
        rate >= OFFERED_FLOOR,
        "org constructs a shell offers fell to {rate}%, floor is {OFFERED_FLOOR}%"
    );
}

#[test]
fn nothing_claims_to_be_offered_by_a_test_that_does_not_exist() {
    // The same rule the spec's citations follow: a pointer into a file
    // nobody has reads as evidence and is not.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root");
    for c in CONFORMANCE {
        if !c.offered.is_empty() {
            assert!(
                root.join(c.offered).exists(),
                "`{}` claims to be offered by `{}`, which does not exist",
                c.name,
                c.offered
            );
        }
    }
}

#[test]
fn nothing_is_offered_that_is_not_understood() {
    // A shell cannot show what the parser cannot read, so this
    // combination means somebody wrote a row wrong.
    for c in CONFORMANCE {
        if !c.offered.is_empty() {
            assert_eq!(
                c.support,
                Support::Understood,
                "`{}` is offered and not understood",
                c.name
            );
        }
    }
}
