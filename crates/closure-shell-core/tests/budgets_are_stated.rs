//! "'Almost no input lag' cannot be tested. State p99 keystroke->frame,
//! cold vault open, and a memory ceiling, each at a stated headline
//! count."
//!
//! Until this, performance could only ever be *regression*-tested: the
//! shape tests say four times the data must not cost four times as
//! much, which catches a change for the worse and says nothing about
//! whether the current speed is acceptable. Nobody could fail a build
//! for being too slow, because nothing said what too slow was.
//!
//! The budgets are per headline rather than per vault, and that is the
//! whole trick. A total is a hostage to the build profile and the
//! machine — opening 100,000 headlines takes 300 ms in release here and
//! 1.65 s in debug, and a CI runner is slower again, so any total
//! generous enough to be portable is too loose to catch anything. A
//! cost *per headline* is the same number in every one of those places
//! to within a small factor, which is what makes it both portable and
//! worth failing a build over.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{INVARIANTS, KEYSTROKE_BUDGET_US, manual_org};

fn spec() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root");
    std::fs::read_to_string(root.join("docs/spec.md")).expect("docs/spec.md")
}

#[test]
fn the_spec_has_an_invariant_about_speed() {
    assert!(
        spec().contains("### I11"),
        "there is still no invariant that can be too slow"
    );
}

#[test]
fn the_invariant_list_carries_it() {
    assert!(
        INVARIANTS.iter().any(|l| l.starts_with("I11")),
        "{INVARIANTS:?}"
    );
    assert!(manual_org(InputMode::Doom).contains("I11"));
}

#[test]
fn the_spec_and_the_code_agree_on_every_budget() {
    // The same rule as the target scale: a number in prose drifts from
    // the number in the tests inside one release, and these have tests
    // hanging off them.
    let s = spec();
    for (what, n) in [
        ("keystroke", u64::from(KEYSTROKE_BUDGET_US)),
        (
            "open",
            u64::from(closure_store::OPEN_BUDGET_US_PER_HEADLINE),
        ),
        (
            "resident",
            u64::from(closure_store::RESIDENT_BUDGET_KB_PER_HEADLINE),
        ),
    ] {
        assert!(
            s.contains(&n.to_string()),
            "the spec does not name the {what} budget ({n})"
        );
    }
}
