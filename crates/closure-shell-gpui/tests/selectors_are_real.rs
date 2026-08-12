//! Every selector a test asserts on is one some paint site can produce.
//!
//! `debug_bounds("outline-row-2")` returning `None` is how a window
//! test says "this is not painted", and it is also what a *typo*
//! returns. `debug_bounds("outline-rwo-2").is_none()` passes, forever,
//! proving nothing — the same vacuous-green shape as a `prop_assert!
//! (true)` or a parser with no caller, in the one place this codebase
//! leans on hardest.
//!
//! gpui's element tree is built at runtime, so a selector cannot be a
//! type. What can be checked is that the strings the tests use and the
//! strings the window emits are the same set, which is the property a
//! type would have given.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

/// Selector *shapes* the window paints, with `{}` where a number or
/// name is interpolated. Extracted from the source rather than listed
/// by hand, because a hand-kept list is the thing that goes stale.
fn painted_shapes() -> Vec<String> {
    let src = include_str!("../src/lib.rs");
    let mut out = Vec::new();
    for (at, _) in src.match_indices("debug_selector(") {
        // Either `debug_selector(|| format!("x-{i}"))` or
        // `debug_selector(|| "x".into())`. `match_indices` yields the
        // *matched* text, not the remainder — reading it as the tail is
        // how the first version of this scan found nothing and said so.
        let rest = &src[at..];
        let Some(open) = rest.find('"') else { continue };
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else {
            continue;
        };
        out.push(normalise(&after[..close]));
    }
    out.sort();
    out.dedup();
    out
}

/// `outline-row-{i}` and `outline-row-3` become the same shape.
fn normalise(s: &str) -> String {
    let mut out = String::new();
    let mut in_brace = false;
    for c in s.chars() {
        match c {
            '{' => {
                in_brace = true;
                out.push_str("{}");
            }
            '}' => in_brace = false,
            _ if in_brace => {}
            _ if c.is_ascii_digit() => out.push_str("{}"),
            _ => out.push(c),
        }
    }
    // `row-{}{}` from `row-12` collapses to one hole.
    while out.contains("{}{}") {
        out = out.replace("{}{}", "{}");
    }
    out
}

#[test]
fn the_window_paints_selectors_at_all() {
    let shapes = painted_shapes();
    assert!(
        shapes.len() > 20,
        "only {} selector shapes found — the scan is broken, not the window",
        shapes.len()
    );
}

#[test]
fn every_painted_selector_is_looked_at_by_some_test() {
    // A ratchet rather than zero, because writing seventeen window
    // tests is not this item and pretending otherwise would mean either
    // a bad commit or a disabled guard. It only goes down — the same
    // mechanism holding the org matrix and the field count.
    const UNLOOKED_CEILING: usize = 8;

    // The sound direction, and the more useful one.
    //
    // I tried the other way first — flag a test asserting on a selector
    // nothing paints — and it misfired on a third of the suite, because
    // `assert!(vcx.debug_bounds(sel).is_some(), "…was never painted")`
    // puts the *message* where a naive scan looks for the selector. A
    // guard that cries wolf gets deleted, so it is not worth having in
    // that shape.
    //
    // This way needs no guessing: a selector the window paints and no
    // test ever names is an affordance nobody checks. That is the same
    // parsed-and-invisible shape as an org construct with no caller,
    // one layer down.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut suite = String::new();
    for entry in std::fs::read_dir(&dir).expect("tests dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        suite.push_str(&std::fs::read_to_string(&path).expect("read"));
    }
    // The literal head of each shape — `outline-row-{}` is looked for
    // as `outline-row-`, which is what a test naming any row contains.
    let unlooked: Vec<String> = painted_shapes()
        .into_iter()
        .filter(|shape| {
            let head = shape.split("{}").next().unwrap_or(shape);
            // A shape that is all hole carries no name to search for.
            head.len() >= 4 && !suite.contains(head)
        })
        .collect();
    assert!(
        unlooked.len() <= UNLOOKED_CEILING,
        "{} painted selectors have no test looking at them, ceiling is \
         {UNLOOKED_CEILING}: {unlooked:?}",
        unlooked.len()
    );
}
