//! D1: `CommonMark` + GFM block depth — blockquotes, GFM tables, and
//! thematic breaks — classified per line so the byte-exact roundtrip (I1)
//! is preserved by construction.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_markdown::{Block, BlockKind, parse, print};

fn kinds(md: &str) -> Vec<BlockKind> {
    parse(md)
        .expect("parse")
        .blocks()
        .iter()
        .map(Block::kind)
        .collect()
}

#[test]
fn classifies_blockquote() {
    let md = "> quoted line\n> second\n";
    assert!(
        kinds(md).iter().all(|k| *k == BlockKind::Blockquote),
        "got {:?}",
        kinds(md)
    );
}

#[test]
fn classifies_gfm_table() {
    let md = "| a | b |\n|---|---|\n| 1 | 2 |\n";
    assert!(
        kinds(md).iter().all(|k| *k == BlockKind::Table),
        "got {:?}",
        kinds(md)
    );
}

#[test]
fn classifies_thematic_break() {
    for br in ["---\n", "***\n", "___\n", "- - -\n"] {
        let k = kinds(br);
        assert_eq!(k, vec![BlockKind::ThematicBreak], "for {br:?}");
    }
}

#[test]
fn dashes_after_text_are_not_a_break_inside_a_list() {
    // `- item` is still a list item, not a thematic break.
    assert_eq!(kinds("- item\n"), vec![BlockKind::ListItem]);
}

#[test]
fn mixed_doc_roundtrips_byte_exact() {
    let md = "# Title\n\n> a quote\n\n| h1 | h2 |\n|----|----|\n| x | y |\n\n---\n\npara\n";
    let doc = parse(md).expect("parse");
    assert_eq!(print(&doc), md, "I1 byte-exact with GFM blocks");
    // And the new kinds are all represented.
    let ks = kinds(md);
    assert!(ks.contains(&BlockKind::Blockquote));
    assert!(ks.contains(&BlockKind::Table));
    assert!(ks.contains(&BlockKind::ThematicBreak));
}
