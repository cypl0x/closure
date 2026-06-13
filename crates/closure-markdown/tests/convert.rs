//! org <-> markdown conversion (line-level subset).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_markdown::{from_org, to_org};

#[test]
fn org_headlines_become_atx_headings() {
    let (md, _warn) = from_org("* One\n** Two\n*** Three\n");
    assert_eq!(md, "# One\n## Two\n### Three\n");
}

#[test]
fn org_src_block_becomes_fence() {
    let (md, _w) = from_org("#+BEGIN_SRC rust\nfn main() {}\n#+END_SRC\n");
    assert_eq!(md, "```rust\nfn main() {}\n```\n");
}

#[test]
fn org_drawers_and_planning_are_dropped_and_warned() {
    let (md, warn) = from_org(
        "* Task\nSCHEDULED: <2026-06-13 Fri>\n:PROPERTIES:\n:ID: x\n:END:\nbody\n",
    );
    assert!(md.contains("# Task"));
    assert!(md.contains("body"));
    assert!(!md.contains(":PROPERTIES:"));
    assert!(!md.contains("SCHEDULED:"));
    assert!(!warn.is_empty(), "lossy parts reported");
}

#[test]
fn md_headings_become_org_headlines() {
    assert_eq!(to_org("# A\n## B\n"), "* A\n** B\n");
}

#[test]
fn md_fence_becomes_org_src_block() {
    assert_eq!(
        to_org("```python\nprint(1)\n```\n"),
        "#+BEGIN_SRC python\nprint(1)\n#+END_SRC\n"
    );
}

#[test]
fn md_fence_without_language_uses_example() {
    assert_eq!(to_org("```\nplain\n```\n"), "#+BEGIN_SRC\nplain\n#+END_SRC\n");
}

#[test]
fn plain_paragraphs_survive_both_directions() {
    let (md, _w) = from_org("just text\nmore text\n");
    assert_eq!(md, "just text\nmore text\n");
    assert_eq!(to_org("just text\n"), "just text\n");
}
