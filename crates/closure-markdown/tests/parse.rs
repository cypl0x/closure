#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::{fs, path::PathBuf};

use closure_markdown::{Block, BlockKind, parse, print};

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap()
        .join("fixtures")
        .join("md")
}

#[test]
fn roundtrip_exact_on_golden_corpus() {
    let mut files: Vec<PathBuf> = fs::read_dir(corpus_dir())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect();
    files.sort();
    assert!(!files.is_empty());
    for path in files {
        let src = fs::read_to_string(&path).unwrap();
        let doc = parse(&src).unwrap();
        assert_eq!(print(&doc), src, "roundtrip {}", path.display());
    }
}

#[test]
fn heading_recognised() {
    let doc = parse("# Hello\n").unwrap();
    assert_eq!(doc.blocks().len(), 1);
    assert_eq!(doc.blocks()[0].kind(), BlockKind::Heading);
    assert_eq!(doc.blocks()[0].heading_level(), Some(1));
}

#[test]
fn nested_heading_level() {
    let doc = parse("# A\n## B\n### C\n").unwrap();
    let levels: Vec<_> = doc
        .blocks()
        .iter()
        .filter_map(Block::heading_level)
        .collect();
    assert_eq!(levels, vec![1, 2, 3]);
}

#[test]
fn hash_without_space_is_paragraph() {
    let doc = parse("#foo\n").unwrap();
    assert_eq!(doc.blocks()[0].kind(), BlockKind::Paragraph);
}

#[test]
fn classifies_list_items() {
    use closure_markdown::BlockKind;
    let doc = closure_markdown::parse("- one\n- two\n* star\n1. num\n").expect("parse");
    let kinds: Vec<BlockKind> = doc.blocks().iter().map(closure_markdown::Block::kind).collect();
    assert!(kinds.iter().all(|k| *k == BlockKind::ListItem), "got {kinds:?}");
}

#[test]
fn classifies_fenced_code_block() {
    use closure_markdown::BlockKind;
    let md = "para\n```rust\nfn main() {}\n```\nafter\n";
    let doc = closure_markdown::parse(md).expect("parse");
    let fences: Vec<&closure_markdown::Block> = doc
        .blocks()
        .iter()
        .filter(|b| b.kind() == BlockKind::CodeFence)
        .collect();
    assert_eq!(fences.len(), 1, "one code fence block");
    assert!(fences[0].source().contains("fn main()"));
    assert!(fences[0].source().starts_with("```rust"));
}

#[test]
fn fence_does_not_swallow_following_text() {
    use closure_markdown::BlockKind;
    let doc = closure_markdown::parse("```\ncode\n```\nplain\n").expect("parse");
    assert!(doc.blocks().iter().any(|b| b.kind() == BlockKind::Paragraph
        && b.source().contains("plain")));
}

#[test]
fn roundtrip_with_lists_and_fences() {
    let md = "# Title\n\n- a\n- b\n\n```sh\necho hi\n```\n\npara\n";
    let doc = closure_markdown::parse(md).expect("parse");
    assert_eq!(closure_markdown::print(&doc), md, "I1 byte-exact");
}
