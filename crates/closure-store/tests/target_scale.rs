//! "The store parses the whole vault into memory. That is a
//! load-bearing decision made by accident. Say the number the kernel
//! promises to hold."
//!
//! Every performance question closure could be asked — is opening fast
//! enough, is a keystroke cheap enough, is the memory reasonable — is
//! unanswerable without a scale to ask it at. So the number is stated
//! once, in code, and the spec is held to the same number rather than
//! to a sentence somebody wrote a year ago.
//!
//! Measured here on 2026-08-11, release build: 10,000 headlines
//! (1.4 MB of org) open in 32 ms and cost 27 MB resident; 100,000
//! (14.2 MB) open in 285 ms and cost 196 MB. Roughly 2.9 µs and 2 KB
//! per headline, both linear. A million headlines would be ~2 GB and
//! ~3 s, which is where the parse-it-all design stops being the right
//! one — so the promise stops below it, deliberately.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

/// The spec, read from the repo.
fn spec() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root");
    std::fs::read_to_string(root.join("docs/spec.md")).expect("docs/spec.md")
}

#[test]
fn the_spec_states_a_target_scale() {
    let s = spec().to_lowercase();
    assert!(
        s.contains("target scale"),
        "the spec still does not say what the kernel promises to hold"
    );
}

#[test]
fn the_spec_and_the_code_agree_on_the_number() {
    // A number in prose drifts from the number in the tests within a
    // release. This is the one place it is written down.
    let n = closure_store::TARGET_SCALE_HEADLINES;
    assert_eq!(n, 100_000, "the promise changed; say why in the spec");
    let pretty = "100,000";
    assert!(
        spec().contains(pretty),
        "the spec does not name {pretty} headlines"
    );
}
