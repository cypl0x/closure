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
    let kinds: Vec<BlockKind> = doc
        .blocks()
        .iter()
        .map(closure_markdown::Block::kind)
        .collect();
    assert!(
        kinds.iter().all(|k| *k == BlockKind::ListItem),
        "got {kinds:?}"
    );
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
    assert!(
        doc.blocks()
            .iter()
            .any(|b| b.kind() == BlockKind::Paragraph && b.source().contains("plain"))
    );
}

#[test]
fn roundtrip_with_lists_and_fences() {
    let md = "# Title\n\n- a\n- b\n\n```sh\necho hi\n```\n\npara\n";
    let doc = closure_markdown::parse(md).expect("parse");
    assert_eq!(closure_markdown::print(&doc), md, "I1 byte-exact");
}

// === Q4-M1: inline markup classification (gap-free, I1 untouched). ===

use closure_markdown::{InlineKind, inline_spans};

#[test]
fn inline_spans_classify_the_common_marks() {
    let spans = inline_spans("plain **strong** and *em* plus `code` end");
    let kinds: Vec<InlineKind> = spans.iter().map(|(k, _)| *k).collect();
    assert!(kinds.contains(&InlineKind::Strong));
    assert!(kinds.contains(&InlineKind::Emphasis));
    assert!(kinds.contains(&InlineKind::Code));
    let strong = spans
        .iter()
        .find(|(k, _)| *k == InlineKind::Strong)
        .expect("strong");
    assert_eq!(strong.1, "**strong**", "span keeps its markers verbatim");
}

#[test]
fn inline_spans_are_gap_free() {
    // The Highlighter coverage rule: concatenating the span texts
    // reproduces the input byte-exactly, always.
    for text in [
        "no markup at all",
        "**a** *b* `c` [d](e)",
        "unbalanced **never closes",
        "`tick` at *end*",
        "",
        "***",
    ] {
        let joined: String = inline_spans(text).iter().map(|(_, s)| *s).collect();
        assert_eq!(joined, text, "gap-free over {text:?}");
    }
}

#[test]
fn inline_spans_classify_links() {
    let spans = inline_spans("see [docs](https://x.y) here");
    let link = spans
        .iter()
        .find(|(k, _)| *k == InlineKind::Link)
        .expect("link");
    assert_eq!(link.1, "[docs](https://x.y)");
}

#[test]
fn unbalanced_markup_stays_plain() {
    let spans = inline_spans("dangling **half");
    assert!(
        spans.iter().all(|(k, _)| *k == InlineKind::Plain),
        "{spans:?}"
    );
}

// === Q4-M2: setext headings (two-line lookback, I1 by construction). ===

#[test]
fn setext_equals_is_a_level_one_heading() {
    let doc = parse("Title\n=====\n").expect("parse");
    let blocks = doc.blocks();
    assert_eq!(blocks.len(), 1, "{blocks:?}");
    assert_eq!(blocks[0].kind(), BlockKind::Heading);
    assert_eq!(blocks[0].heading_level(), Some(1));
    assert_eq!(
        blocks[0].source(),
        "Title\n=====\n",
        "span covers both lines"
    );
    assert_eq!(print(&doc), "Title\n=====\n", "roundtrip untouched");
}

#[test]
fn setext_dashes_is_a_level_two_heading_not_a_break() {
    let doc = parse("Sub\n---\n").expect("parse");
    let blocks = doc.blocks();
    assert_eq!(blocks.len(), 1, "{blocks:?}");
    assert_eq!(blocks[0].kind(), BlockKind::Heading);
    assert_eq!(blocks[0].heading_level(), Some(2));
}

#[test]
fn dashes_without_a_paragraph_stay_a_thematic_break() {
    let doc = parse("---\n").expect("parse");
    assert_eq!(doc.blocks()[0].kind(), BlockKind::ThematicBreak);
    let doc = parse("P\n\n---\n").expect("parse");
    let kinds: Vec<BlockKind> = doc
        .blocks()
        .iter()
        .map(closure_markdown::Block::kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            BlockKind::Paragraph,
            BlockKind::BlankLine,
            BlockKind::ThematicBreak
        ],
        "blank line breaks the setext attachment"
    );
}

#[test]
fn setext_takes_the_whole_pending_paragraph() {
    // CommonMark rule: the entire paragraph run becomes the heading.
    let doc = parse("one\ntwo\n===\n").expect("parse");
    let blocks = doc.blocks();
    assert_eq!(blocks.len(), 1, "{blocks:?}");
    assert_eq!(blocks[0].kind(), BlockKind::Heading);
    assert_eq!(blocks[0].source(), "one\ntwo\n===\n");
}

// === Q4-M3: link target extraction (md identity = path/slug, no :ID:). ===

#[test]
fn link_targets_finds_inline_and_wiki_links() {
    let md = "see [docs](notes.md) and [[wiki-page]] plus [x](https://e.g/#f)\n";
    let t = closure_markdown::link_targets(md);
    assert_eq!(
        t,
        vec![
            "notes.md".to_owned(),
            "wiki-page".to_owned(),
            "https://e.g/#f".to_owned()
        ]
    );
}

#[test]
fn link_targets_skips_code_fences() {
    let md = "```\n[not](a-link.md)\n```\nreal [yes](real.md)\n";
    assert_eq!(
        closure_markdown::link_targets(md),
        vec!["real.md".to_owned()]
    );
}
