//! X2a hermetic core: the parse/print/query surface the browser will
//! call, tested natively (no wasm toolchain). The wasm-bindgen wrappers
//! are a thin pass-through built for wasm32 by `just wasm-web`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_wasm::{headline_titles, headline_titles_joined, reformat};

#[test]
fn reformat_round_trips_valid_org_byte_exact() {
    let src = "* TODO Ship it\n:PROPERTIES:\n:ID: 01ABC\n:END:\nbody line\n** Sub\n";
    assert_eq!(reformat(src).expect("parses"), src, "I1 byte-exact");
}

#[test]
fn reformat_reports_nothing_to_panic_on_odd_input() {
    // Arbitrary text still parses as org (paragraphs); never panics.
    let r = reformat("just some text\n");
    assert!(r.is_ok());
}

#[test]
fn headline_titles_lists_in_document_order() {
    let src = "* First\n** Nested\n* Second\n";
    assert_eq!(headline_titles(src), vec!["First", "Nested", "Second"]);
    assert_eq!(headline_titles_joined(src), "First\nNested\nSecond");
}

#[test]
fn headline_titles_empty_for_no_headlines() {
    assert!(headline_titles("plain paragraph\n").is_empty());
}
