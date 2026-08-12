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
const FLOOR: u32 = 93;

/// How many constructs are understood, as a count rather than a ratio.
///
/// The ratio alone is the wrong ratchet and 2026-08-12 is when that
/// showed. Declining `#+ATTR_*` and the diary sexp took the list to
/// "everything closure attempts, it does", `the_matrix_is_worth_having`
/// fired at exactly that, and looking harder at org turned up five more
/// constructs it does not do — `#+PROPERTY:`, `#+LINK:`, description
/// lists, `[@3]` counters, timestamp delays. Being honest about those
/// pushed the ratio from 95% to 88%.
///
/// A ratio that falls when you admit to more of org punishes the one
/// behaviour this matrix exists to encourage. A *count* cannot be
/// gamed that way: it goes up only by making something work, and
/// growing the list never moves it. So the ratio floor may be lowered
/// when — and only when — the list grows, and this number never falls
/// for any reason at all.
const UNDERSTOOD_FLOOR: usize = 42;

#[test]
fn the_number_understood_never_falls() {
    let n = CONFORMANCE
        .iter()
        .filter(|c| c.support == Support::Understood)
        .count();
    assert!(
        n >= UNDERSTOOD_FLOOR,
        "understood constructs fell to {n}, floor is {UNDERSTOOD_FLOOR} — \
         something that used to work stopped working"
    );
}

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
        "a matrix where everything is understood — or understood and \
         declined — is a matrix that is not looking at what org has. \
         Declining a construct must not be a way to empty this list: \
         `Declined` says closure will not do it, `Preserved` says it \
         has not yet, and org is large enough that there is always \
         something in the second column"
    );
}

/// The floor for what a shell actually offers. Raise it the same way.
///
/// 2026-08-12: 100% of the listed constructs, understood and offered.
/// Which makes the list the thing to grow now — nothing
/// parsed and invisible. The gap is the point of two numbers: it was 91
/// and 70 before the unwired parsers were wired up, and nothing about
/// the first number would have shown that.
const OFFERED_FLOOR: u32 = 93;

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

#[test]
fn a_construct_that_is_not_understood_says_why_or_says_what_is_missing() {
    // `Preserved` conflates two different states: "not built yet" and
    // "looked at, decided against, here is the reason". A matrix that
    // cannot tell them apart reports a settled decision as a gap
    // forever, and the number never finishes going up even when the
    // work is done.
    for c in CONFORMANCE {
        match c.support {
            Support::Understood => {}
            Support::Preserved => assert!(
                !c.missing.is_empty(),
                "`{}` is preserved and does not say what is missing",
                c.name
            ),
            Support::Declined { reason } => assert!(
                reason.len() > 30,
                "`{}` is declined with a reason too short to be one: {reason:?}",
                c.name
            ),
        }
    }
}

#[test]
fn the_two_constructs_closure_will_not_do_are_recorded_as_decisions() {
    // `#+ATTR_*` applies to export back-ends closure does not have, and
    // a diary sexp is elisp — an evaluator for one language inside a
    // notebook that is deliberately language-agnostic. Both are the
    // right answer, and both used to read as unfinished work.
    for name in ["#+ATTR_* export attributes", "diary sexp timestamp"] {
        let c = CONFORMANCE
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("`{name}` is not in the matrix"));
        assert!(
            matches!(c.support, Support::Declined { .. }),
            "`{name}` is still listed as an unfinished gap"
        );
    }
}

#[test]
fn a_declined_construct_is_not_counted_against_the_rate() {
    // The number should mean "of what closure set out to do", so a
    // decision neither flatters it nor drags it down forever.
    let declined = CONFORMANCE
        .iter()
        .filter(|c| matches!(c.support, Support::Declined { .. }))
        .count();
    assert!(declined > 0, "nothing is declined, so this proves nothing");
    let attempted = CONFORMANCE.len() - declined;
    let understood = CONFORMANCE
        .iter()
        .filter(|c| c.support == Support::Understood)
        .count();
    assert_eq!(
        conformance_rate(),
        u32::try_from(understood * 100 / attempted).unwrap_or(0),
        "the rate still counts declined constructs as outstanding"
    );
}
