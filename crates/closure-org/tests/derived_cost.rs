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
//! them. Budgets in `scale.rs`' style — generous enough that machine
//! speed never decides, far below what a quadratic path costs.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fmt::Write as _;
use std::time::Instant;

/// Measured on the dev machine, debug build, before the fix:
///
///     n=500   7.8ms
///     n=1000  26ms
///     n=2000  99ms
///     n=4000  490ms
///
/// Four times the blocks, sixteen times the cost — quadratic, and the
/// reason the budget below sits at 4,000 rather than somewhere more
/// comfortable. At 2,000 the old code came in at 99ms and would have
/// slipped under any budget loose enough not to flake.
///
/// After: 262us / 495us / 1.5ms / 2.5ms — linear, and 190 times
/// cheaper at 4,000.
const BLOCKS: usize = 4_000;

/// A document with `BLOCKS` named source blocks under one headline.
fn doc() -> closure_org::OrgDoc {
    let mut src = String::from("* Blocks\n:PROPERTIES:\n:ID: 01DERIVEDAAAAAAAAAAAAAAAAA\n:END:\n");
    for i in 0..BLOCKS {
        let _ = write!(
            src,
            "#+NAME: block-{i}\n#+BEGIN_SRC shell\necho {i}\n#+END_SRC\n"
        );
    }
    closure_org::parse(&src).expect("parses")
}

#[test]
fn finding_a_block_by_name_does_not_rescan_per_block() {
    let d = doc();
    let t = Instant::now();
    let found = d.code_block_index_by_name(&format!("block-{}", BLOCKS - 1));
    let elapsed = t.elapsed();
    assert_eq!(found, Some(BLOCKS - 1), "the last block, by name");
    // Quadratic here is 2000 collect+sort passes over 2000 nodes, which
    // is seconds even in release. Linear is well under a millisecond.
    assert!(
        elapsed.as_millis() < 50,
        "code_block_index_by_name is still rescanning per block: {elapsed:?}"
    );
}

#[test]
fn a_name_that_is_not_there_costs_the_same_one_pass() {
    // The miss is the worst case: it visits every block.
    let d = doc();
    let t = Instant::now();
    assert_eq!(d.code_block_index_by_name("no-such-block"), None);
    let elapsed = t.elapsed();
    assert!(elapsed.as_millis() < 50, "a miss rescans: {elapsed:?}");
}

#[test]
fn asking_for_one_drawer_does_not_collect_them_all() {
    let mut src = String::new();
    for i in 0..BLOCKS {
        let _ = write!(
            src,
            "* H{i}\n:PROPERTIES:\n:ID: 01DRAWER{i:018}\n:END:\n:LOGBOOK:\nnote\n:END:\n"
        );
    }
    let d = closure_org::parse(&src).expect("parses");
    let t = Instant::now();
    for _ in 0..200 {
        assert!(d.has_drawer("LOGBOOK"));
    }
    let elapsed = t.elapsed();
    assert!(
        elapsed.as_millis() < 2000,
        "has_drawer collects every drawer to answer about one: {elapsed:?}"
    );
}

#[test]
fn the_names_are_still_right() {
    // A faster wrong answer is not the goal.
    let d = doc();
    assert_eq!(d.code_block_name(0).as_deref(), Some("block-0"));
    assert_eq!(d.code_block_name(7).as_deref(), Some("block-7"));
    assert_eq!(d.code_block_index_by_name("block-3"), Some(3));
    assert_eq!(d.code_block_index_by_name("block-99999"), None);
    assert_eq!(d.code_blocks().len(), BLOCKS);
}

#[test]
fn an_unnamed_block_has_no_name() {
    let d = closure_org::parse("#+BEGIN_SRC shell\necho hi\n#+END_SRC\n").expect("parses");
    assert_eq!(d.code_block_name(0), None);
    assert_eq!(d.code_block_index_by_name("anything"), None);
}
