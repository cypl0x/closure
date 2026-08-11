//! I11: a loaded vault costs no more than
//! [`closure_store::RESIDENT_BUDGET_KB_PER_HEADLINE`] per headline.
//!
//! The one budget that is about the design rather than the code. A
//! vault is parsed whole into memory — every file, every span, the id
//! index and the inverted backlink index — which is what makes
//! byte-exact printing and backlinks simple, and what puts a ceiling on
//! how big a vault can be. Measured at ~2 KB per headline for org that
//! averages ~150 bytes, so most of what is held is not the text.
//!
//! Measured by asking the kernel, which is the only honest source: an
//! allocator's own accounting does not see what it has returned to the
//! OS, and a difference of two `Vault`s does not see the page the
//! parser touched and freed. Resident set is what the machine actually
//! has to find room for.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_store::{RESIDENT_BUDGET_KB_PER_HEADLINE, Vault};

/// Resident set size in KB, from `/proc/self/statm`.
///
/// `None` where there is no procfs — this budget is checked where it
/// can be measured rather than guessed at elsewhere.
fn resident_kb() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4)
}

/// `n` headlines spread over files of 2,000.
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

#[test]
fn a_loaded_vault_stays_inside_its_memory_budget() {
    const N: usize = 10_000;
    let Some(before) = resident_kb() else {
        // No procfs. Say so rather than pass quietly: a budget that
        // silently checks nothing is worse than no budget.
        eprintln!("no /proc/self/statm — memory budget not checked here");
        return;
    };
    let dir = vault_of(N);
    let vault = Vault::open(dir.path()).unwrap();
    assert_eq!(vault.headline_count(), N, "the fixture did not load");
    let after = resident_kb().expect("procfs was there a moment ago");
    let per = after.saturating_sub(before) / N as u64;
    // Keep the vault alive across the measurement, or the thing being
    // measured may already have been dropped.
    drop(vault);
    assert!(
        per <= u64::from(RESIDENT_BUDGET_KB_PER_HEADLINE),
        "a loaded vault costs {per} KB per headline, budget is \
         {RESIDENT_BUDGET_KB_PER_HEADLINE} (I11)"
    );
}
