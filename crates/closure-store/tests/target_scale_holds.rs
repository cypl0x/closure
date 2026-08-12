//! The promise, at the size it was made about.
//!
//! `budget_open.rs` and `budget_memory.rs` measure the per-headline
//! cost at 10,000, which is fast enough to run on every build and is
//! where a change of kind shows up. Neither of them proves the thing
//! the spec actually claims: that a vault of
//! [`closure_store::TARGET_SCALE_HEADLINES`] opens at all, holds
//! together, and answers questions.
//!
//! A per-headline cost measured at a tenth of the scale is an
//! extrapolation, and extrapolating is how a promise about 100,000
//! headlines gets made by someone who has only ever opened 10,000.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::time::Instant;

use closure_core::BlockId;
use closure_store::{
    OPEN_BUDGET_US_PER_HEADLINE, RESIDENT_BUDGET_KB_PER_HEADLINE, TARGET_SCALE_HEADLINES, Vault,
};

fn resident_kb() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    Some(statm.split_whitespace().nth(1)?.parse::<u64>().ok()? * 4)
}

/// A vault of `n` headlines over files of 2,000, with links so the
/// backlink index has something to build.
fn vault_of(n: usize) -> tempfile::TempDir {
    use std::fmt::Write as _;
    let dir = tempfile::tempdir().unwrap();
    let per = 2_000;
    for f in 0..n.div_ceil(per) {
        let mut src = String::new();
        for i in 0..per.min(n - f * per) {
            let k = f * per + i;
            let level = if k % 20 == 0 { "*" } else { "**" };
            let _ = write!(
                src,
                "{level} Headline {k}\n:PROPERTIES:\n:ID: 01SCALE{k:018}\n:END:\n\
                 some body text for headline {k}, a sentence long enough to be realistic\n"
            );
            // Every tenth headline points at its predecessor, so the
            // inverted index is doing work rather than staying empty.
            if k % 10 == 0 && k > 0 {
                let _ = writeln!(src, "see [[id:01SCALE{:018}][the one before]]", k - 1);
            }
        }
        std::fs::write(dir.path().join(format!("notes{f:03}.org")), src).unwrap();
    }
    dir
}

#[allow(clippy::cast_precision_loss)]
#[test]
fn a_vault_of_the_promised_size_opens_inside_its_budgets() {
    const N: usize = TARGET_SCALE_HEADLINES;
    let dir = vault_of(N);
    let before = resident_kb();
    let t = Instant::now();
    let vault = Vault::open(dir.path()).unwrap();
    let elapsed = t.elapsed();
    assert_eq!(vault.headline_count(), N, "the fixture did not load");

    let per_us = elapsed.as_micros() as f64 / N as f64;
    assert!(
        per_us <= f64::from(OPEN_BUDGET_US_PER_HEADLINE),
        "opening the promised {N} headlines cost {per_us:.1} µs each, \
         budget is {OPEN_BUDGET_US_PER_HEADLINE} (I11) — {elapsed:?} in total"
    );

    if let (Some(a), Some(b)) = (before, resident_kb()) {
        let per_kb = b.saturating_sub(a) / N as u64;
        assert!(
            per_kb <= u64::from(RESIDENT_BUDGET_KB_PER_HEADLINE),
            "a vault of {N} costs {per_kb} KB per headline, budget is \
             {RESIDENT_BUDGET_KB_PER_HEADLINE} (I11)"
        );
    }
    drop(vault);
}

#[test]
fn it_still_answers_questions_at_that_size() {
    // Opening is half the promise. A vault that opens and then cannot
    // find anything has not held.
    let dir = vault_of(TARGET_SCALE_HEADLINES);
    let vault = Vault::open(dir.path()).unwrap();

    // By id, from the far end of the vault.
    let last = TARGET_SCALE_HEADLINES - 1;
    let id = BlockId::from_existing(&format!("01SCALE{last:018}"));
    let (h, _path) = vault.find_by_id(&id).expect("the last headline by id");
    assert_eq!(h.title(), format!("Headline {last}"));

    // And the backlink index really was built across all of it.
    let target = BlockId::from_existing(&format!("01SCALE{:018}", 89_999));
    let back = closure_query::backlinks(&vault, &target);
    assert!(!back.is_empty(), "no backlinks that deep into the vault");
}
