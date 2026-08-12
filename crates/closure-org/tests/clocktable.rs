//! `#+BEGIN: clocktable` — the time you clocked, where you clocked it.
//!
//! Every part of this existed and none of them were connected: `CLOCK:`
//! lines parse, `Vault::clock_minutes` sums them, the CLI has
//! `clock-report`, and the one place a reader would actually look — a
//! block in the file the work is written in — stayed empty.
//!
//! Org fills the block in by rewriting the file. closure does not, and
//! that is the load-bearing difference (I12): a document whose bytes
//! depend on when it was last refreshed is not a document you can
//! diff, sync or roundtrip. So this parses what the block *asks for*
//! and hands the answer to whoever is rendering, and the source keeps
//! its own bytes (I1).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{Scope, clocktable_params, render_clocktable};

#[test]
fn a_block_is_recognised() {
    let p = clocktable_params("#+BEGIN: clocktable :maxlevel 2").expect("params");
    assert_eq!(p.maxlevel, Some(2));
}

#[test]
fn a_block_with_no_options_is_still_a_block() {
    let p = clocktable_params("#+BEGIN: clocktable").expect("params");
    assert_eq!(p.maxlevel, None);
    // The default is the file it sits in, which is what org does and
    // what someone writing a weekly note means.
    assert_eq!(p.scope, Scope::File);
}

#[test]
fn the_scope_can_be_the_whole_vault() {
    let p = clocktable_params("#+BEGIN: clocktable :scope vault").expect("params");
    assert_eq!(p.scope, Scope::Vault);
}

#[test]
fn some_other_dynamic_block_is_not_a_clocktable() {
    // `#+BEGIN: closure-widget` is one of these too, and reading it as
    // a clocktable would replace somebody's template with a time report.
    assert!(clocktable_params("#+BEGIN: closure-widget :name card").is_none());
    assert!(clocktable_params("#+BEGIN_SRC sh").is_none());
    assert!(clocktable_params("just a line").is_none());
}

#[test]
fn the_table_is_org_and_totals() {
    let rows = vec![("Writing".to_owned(), 90_u64), ("Reading".to_owned(), 30)];
    let out = render_clocktable(&rows);
    assert!(out.contains("| Headline | Time |"), "{out}");
    assert!(out.contains("| Writing | 1:30 |"), "{out}");
    assert!(out.contains("| Reading | 0:30 |"), "{out}");
    assert!(out.contains("*Total time*"), "{out}");
    assert!(out.contains("2:00"), "{out}");
}

#[test]
fn nothing_clocked_says_so_rather_than_showing_an_empty_table() {
    let out = render_clocktable(&[]);
    assert!(out.contains("no clocked time"), "{out}");
    assert!(!out.contains("| Headline"), "{out}");
}

#[test]
fn minutes_are_hours_and_minutes_the_way_org_writes_them() {
    // `=> 1:05`, not `65m` and not `1.08h`: this is the format org's
    // own clocktable uses, and a file read by both must agree.
    let out = render_clocktable(&[("A".to_owned(), 65)]);
    assert!(out.contains("1:05"), "{out}");
    let out = render_clocktable(&[("A".to_owned(), 600)]);
    assert!(out.contains("10:00"), "{out}");
}
