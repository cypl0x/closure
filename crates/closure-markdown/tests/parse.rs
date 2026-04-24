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
