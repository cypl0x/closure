//! C4a: scale budgets on a real-world-sized vault (50k headlines).
//!
//! Plain `Instant`-timed `#[test]`s (no criterion — same approach as the
//! TUI input-lag guard). Budgets are generous so the gate tracks
//! *algorithmic* regressions (an O(n²) path blows them) rather than raw
//! machine speed. Marked `#[ignore]`? No — kept in the normal run so a
//! quadratic regression fails CI; the build is hermetic.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fmt::Write as _;
use std::time::Instant;

use closure_store::Vault;
use tempfile::TempDir;

const FILES: usize = 500;
const PER_FILE: usize = 100; // 500 * 100 = 50_000 headlines
const HUB: &str = "01HUBHUBHUBHUBHUBHUBHUBHUB";

/// Write a 50k-headline vault. Every file's first headline links to a
/// shared HUB id, so `backlinks_of(HUB)` resolves `FILES` sources.
fn big_vault() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for f in 0..FILES {
        let mut s = String::with_capacity(PER_FILE * 96);
        for i in 0..PER_FILE {
            let _ = write!(
                s,
                "* Headline {f}_{i} about assorted things\n\
                 :PROPERTIES:\n:ID: 01ID{f:04}{i:04}PADPADPADPAD\n:END:\n"
            );
            if i == 0 {
                let _ = writeln!(s, "see [[id:{HUB}]] for context");
            } else {
                let _ = writeln!(s, "body line for {f}_{i}");
            }
        }
        std::fs::write(dir.path().join(format!("f{f}.org")), s).expect("write");
    }
    // A file that actually owns the HUB id (link target).
    std::fs::write(
        dir.path().join("hub.org"),
        format!("* Hub\n:PROPERTIES:\n:ID: {HUB}\n:END:\nthe hub\n"),
    )
    .expect("write hub");
    dir
}

#[test]
fn fifty_k_headlines_load_search_backlink_reload_within_budget() {
    let td = big_vault();

    let t = Instant::now();
    let mut vault = Vault::open(td.path()).expect("open");
    let load = t.elapsed();

    // Sanity: we really built ~50k headlines.
    let titles: Vec<String> = vault
        .iter()
        .flat_map(|(_, doc)| doc.all_headlines().map(|h| h.title().to_owned()))
        .collect();
    assert!(titles.len() >= 50_000, "got {} headlines", titles.len());

    // Fuzzy filter over every title.
    let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
    let t = Instant::now();
    let hits = closure_query::fuzzy_filter("assorted", &refs);
    let fuzzy = t.elapsed();
    assert!(!hits.is_empty());

    // Backlink resolve for the shared hub.
    let t = Instant::now();
    let back = vault.backlinks_of(HUB);
    let backlink = t.elapsed();
    assert_eq!(back.len(), FILES, "every file links the hub");

    // Incremental reload of an unchanged vault: zero reparses, fast.
    let t = Instant::now();
    let reparsed = vault.reload_incremental().expect("reload");
    let reload = t.elapsed();
    assert_eq!(reparsed, 0, "unchanged vault triggers no reparse");

    // Budgets (generous; catch quadratic blow-ups, not machine speed).
    // Debug-run observed (dev machine): load ~0.6s, fuzzy ~0.4s,
    // backlink ~7µs, reload ~0.07s. Budgets sit well above with CI
    // headroom but far below any O(n²) regression (which is seconds+).
    assert!(load.as_secs() < 20, "load 50k: {load:?}");
    assert!(fuzzy.as_millis() < 2000, "fuzzy over 50k titles: {fuzzy:?}");
    assert!(backlink.as_millis() < 50, "backlink resolve: {backlink:?}");
    assert!(reload.as_secs() < 10, "incremental reload (unchanged): {reload:?}");

    println!(
        "scale 50k: load={load:?} fuzzy={fuzzy:?} backlink={backlink:?} reload={reload:?}"
    );
}
