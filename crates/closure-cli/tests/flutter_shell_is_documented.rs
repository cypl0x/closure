//! The Flutter shell sits outside the hermetic gate, and the spec has
//! to say so.
//!
//! I10 says every gate is hermetic and reproducible. `nix flake check`
//! is green without ever building the Dart, so somebody who trusts the
//! green will believe this shell is covered by it. That is not a
//! documentation nicety: it is the difference between "the gate passed"
//! and "the thing works", and the two are not the same for exactly one
//! shell in this repo.
//!
//! Asserted rather than trusted because a section that stops being true
//! is worse than one that was never written — it is the same one fact,
//! two owners shape as the rest of this codebase, with a document as
//! the second owner.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

fn spec() -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec.md"),
    )
    .expect("docs/spec.md")
}

fn justfile() -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../justfile"))
        .expect("justfile")
}

#[test]
fn the_spec_has_a_flutter_section() {
    assert_eq!(
        spec()
            .lines()
            .filter(|l| *l == "## The Flutter shell")
            .count(),
        1
    );
}

#[test]
fn the_section_names_the_recipe_that_builds_it() {
    // The whole point of the section is telling somebody what to run
    // instead of `nix flake check`. A section that says "outside the
    // gate" without naming the way in is half a sentence.
    let s = spec();
    let start = s.find("## The Flutter shell").expect("the section");
    let body = &s[start..];
    let end = body[4..].find("\n## ").map_or(body.len(), |i| i + 4);
    let section = &body[..end];

    assert!(section.contains("just flutter"), "no recipe named");
    assert!(
        justfile().contains("\nflutter:"),
        "the section names a recipe the justfile does not have"
    );
}

#[test]
fn the_section_says_what_does_not_work() {
    // The gpui window has a startup notice about the rasteriser and an
    // honest sentence about held keys. This shell's equivalent is that
    // it needs GL the way GDK3 wants it, and that the display this
    // project is demonstrated on does not provide it. Written from what
    // was measured, so it names the mechanism.
    let s = spec();
    let start = s.find("## The Flutter shell").expect("the section");
    let body = &s[start..];
    let end = body[4..].find("\n## ").map_or(body.len(), |i| i + 4);
    let section = &body[..end].to_lowercase();

    for word in ["glx", "gtk3", "wayland", "browse"] {
        assert!(section.contains(word), "the section never mentions {word}");
    }
}

#[test]
fn the_dart_side_reaches_nothing_but_the_abi() {
    // The claim the section makes, checked against the source rather
    // than believed. A Dart file that parsed org would be a second
    // owner of every rule in closure-org, on the unreproducible side of
    // the boundary where no gate can see it.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../flutter/lib");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).expect("flutter/lib") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|e| e != "dart") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read");
        checked += 1;
        for smell in ["RegExp(r'^\\*+", "startsWith('*')", "split('\\n* ')"] {
            assert!(
                !src.contains(smell),
                "{} looks like it parses org: {smell}",
                path.display()
            );
        }
    }
    assert!(checked >= 2, "only {checked} dart files scanned");
}
