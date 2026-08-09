//! "Derived data is recomputed per call. `OrgDoc::code_blocks()`
//! allocates a `Vec` and sorts on every call. `code_block_name()` calls
//! it. `code_block_index_by_name()` loops calling `code_block_name()`
//! — so it's O(n² log n) with an allocation per iteration.
//! `has_drawer()` rescans the whole source. Given how good `scale.rs`
//! and `mutation_cost.rs` are at catching exactly this class of bug
//! elsewhere, these look like they slipped through because nothing
//! measures them."
//!
//! Reported 2026-08-04 in the Opus 5 review. So: something measures
//! them.
//!
//! These measure a *shape*, never a duration. The first version of
//! this file spent budgets in milliseconds — "generous enough that
//! machine speed never decides" — and machine speed decided: 2.19s on
//! a CI runner against a 2s budget, where the same test takes a
//! fraction of that here. An absolute budget is a bet that every
//! machine which will ever run the suite is at least as fast as the
//! one that wrote it.
//!
//! A ratio is not that bet. Quadratic work at four times the input
//! costs sixteen times as much; linear costs four. Both numbers move
//! together when the machine is slow, so the ratio says the same thing
//! on a laptop, a runner, and a Raspberry Pi.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// The smaller of the two sizes every scaling test compares.
///
/// Big enough that one pass is far above timer granularity, so the
/// ratio is signal rather than noise.
const N: usize = 1_000;

/// What separates linear from quadratic at four times the input.
///
/// Linear lands near 4, quadratic near 16. Anything under this is not
/// quadratic; the gap is wide enough that a noisy runner cannot cross
/// it, which is the whole point of measuring a ratio.
const NOT_QUADRATIC: f64 = 8.0;

/// `n` named source blocks under one headline.
fn blocks(n: usize) -> closure_org::OrgDoc {
    let mut src = String::from("* Blocks\n:PROPERTIES:\n:ID: 01DERIVEDAAAAAAAAAAAAAAAAA\n:END:\n");
    for i in 0..n {
        let _ = write!(
            src,
            "#+NAME: block-{i}\n#+BEGIN_SRC text\nbody {i}\n#+END_SRC\n"
        );
    }
    closure_org::parse(&src).expect("parses")
}

/// How long `f` takes, run enough times to be worth timing.
fn cost(f: impl Fn()) -> Duration {
    let t = Instant::now();
    for _ in 0..5 {
        f();
    }
    t.elapsed()
}

/// The factor by which the cost grew when the input grew four times.
fn growth(small: Duration, large: Duration) -> f64 {
    // A measurement of zero means the work vanished below the clock,
    // which is the answer we were hoping for.
    if small.as_nanos() == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    {
        large.as_nanos() as f64 / small.as_nanos() as f64
    }
}

#[test]
fn finding_a_block_by_name_does_not_rescan_per_block() {
    let (small, large) = (blocks(N), blocks(N * 4));
    let a = cost(|| {
        assert!(small.code_block_index_by_name("block-3").is_some());
    });
    let b = cost(|| {
        assert!(large.code_block_index_by_name("block-3").is_some());
    });
    let g = growth(a, b);
    assert!(
        g < NOT_QUADRATIC,
        "four times the blocks cost {g:.1} times as much \
         ({a:?} -> {b:?}) — that is a rescan per block"
    );
}

#[test]
fn a_name_that_is_not_there_costs_the_same_one_pass() {
    // The miss is the worst case: it visits every block.
    let (small, large) = (blocks(N), blocks(N * 4));
    let a = cost(|| assert!(small.code_block_index_by_name("no-such-block").is_none()));
    let b = cost(|| assert!(large.code_block_index_by_name("no-such-block").is_none()));
    let g = growth(a, b);
    assert!(
        g < NOT_QUADRATIC,
        "a miss cost {g:.1} times as much on four times the input \
         ({a:?} -> {b:?}) — that is a rescan per block"
    );
}

#[test]
fn asking_for_one_drawer_does_not_collect_them_all() {
    // Not a ratio over input size but over *where the answer is*: a
    // scan that stops at the first match answers about a drawer in the
    // first headline almost for free, and has to walk the whole
    // document to be sure one is absent. A scan that collects every
    // drawer before looking cannot tell the two apart — which is what
    // `has_drawer` did, under a comment claiming it did not.
    let mut front = String::from("* First\n:LOGBOOK:\nnote\n:END:\n");
    let mut absent = String::from("* First\n");
    for i in 0..N * 4 {
        let _ = write!(front, "* H{i}\n:PROPERTIES:\n:ID: 01DRAWER{i:018}\n:END:\n");
        let _ = write!(
            absent,
            "* H{i}\n:PROPERTIES:\n:ID: 01DRAWER{i:018}\n:END:\n"
        );
    }
    let front = closure_org::parse(&front).expect("parses");
    let absent = closure_org::parse(&absent).expect("parses");

    let hit = cost(|| assert!(front.has_drawer("LOGBOOK")));
    let miss = cost(|| assert!(!absent.has_drawer("LOGBOOK")));
    assert!(
        hit * 5 < miss,
        "finding a drawer in the first headline cost {hit:?} and being \
         sure one is absent cost {miss:?} — near enough the same, so \
         every drawer is being collected either way"
    );
}

#[test]
fn the_names_are_still_right() {
    // A faster wrong answer is not the goal.
    let d = blocks(50);
    assert_eq!(d.code_block_index_by_name("block-0"), Some(0));
    assert_eq!(d.code_block_index_by_name("block-49"), Some(49));
    assert_eq!(d.code_block_index_by_name("block-50"), None);
    assert!(d.has_drawer("PROPERTIES"));
    assert!(!d.has_drawer("LOGBOOK"));
}

#[test]
fn an_unnamed_block_has_no_name() {
    let d = closure_org::parse("* H\n#+BEGIN_SRC text\nx\n#+END_SRC\n").expect("parses");
    assert_eq!(d.code_block_index_by_name("anything"), None);
}
