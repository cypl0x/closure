//! I11: opening a vault costs no more than
//! [`closure_store::OPEN_BUDGET_US_PER_HEADLINE`] per headline.
//!
//! Per headline rather than per vault, because a total is a hostage to
//! the build profile: the same 100,000 headlines open in 300 ms in
//! release and 1.65 s in debug on this machine, and a CI runner is
//! slower again. The per-headline cost barely moves between those, so
//! it is a number a build can be failed on.
//!
//! Loose on purpose. Measured at ~2.9 µs release and ~17 µs debug
//! against a ceiling of 100, this fails when opening a vault starts
//! doing something *per headline that is not constant* — an accidental
//! quadratic, a re-index per file, a second parse — rather than when
//! the machine is busy.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::time::Instant;

use closure_store::{OPEN_BUDGET_US_PER_HEADLINE, Vault};

/// `n` headlines spread over files of 2,000, the way a vault that size
/// actually looks.
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
                "{level} Headline {k}\n:PROPERTIES:\n:ID: 01BUDGET{k:018}\n:END:\n\
                 some body text for headline {k}, a sentence long enough to be realistic\n"
            );
        }
        std::fs::write(dir.path().join(format!("notes{f:03}.org")), src).unwrap();
    }
    dir
}

#[allow(clippy::cast_precision_loss)]
#[test]
fn opening_a_vault_stays_inside_its_budget() {
    const N: usize = 10_000;
    let dir = vault_of(N);
    let t = Instant::now();
    let vault = Vault::open(dir.path()).unwrap();
    let elapsed = t.elapsed();
    // The vault really did parse what it was given — otherwise this
    // measures how fast closure can do nothing.
    assert_eq!(vault.headline_count(), N, "the fixture did not load");
    let per = elapsed.as_micros() as f64 / N as f64;
    assert!(
        per <= f64::from(OPEN_BUDGET_US_PER_HEADLINE),
        "opening cost {per:.1} µs per headline, budget is \
         {OPEN_BUDGET_US_PER_HEADLINE} (I11) — {elapsed:?} for {N} headlines"
    );
}

#[allow(clippy::cast_precision_loss)]
#[test]
fn the_cost_of_opening_is_linear_in_headlines() {
    // The shape beside the budget. Four times the vault may cost four
    // times as much and no worse — this is what an accidental quadratic
    // trips over long before the per-headline ceiling does.
    let small = vault_of(2_000);
    let large = vault_of(8_000);
    // Warm the page cache for both before timing either.
    let _ = Vault::open(small.path()).unwrap();
    let _ = Vault::open(large.path()).unwrap();
    let t = Instant::now();
    let _ = Vault::open(small.path()).unwrap();
    let a = t.elapsed();
    let t = Instant::now();
    let _ = Vault::open(large.path()).unwrap();
    let b = t.elapsed();
    let growth = b.as_nanos() as f64 / a.as_nanos().max(1) as f64;
    assert!(
        growth < 8.0,
        "four times the vault took {growth:.1} times as long to open \
         ({a:?} -> {b:?}) — opening is worse than linear"
    );
}
