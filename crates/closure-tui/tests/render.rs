//! Unit tests for the pure render/translation helpers of the shell.

use closure_core::Document;
use closure_tui::{headline_lines, stroke_of};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn stroke_of_plain_char() {
    assert_eq!(stroke_of(&key(KeyCode::Char('j'))), Some("j".to_owned()));
}

#[test]
fn stroke_of_uppercase_char_keeps_case() {
    assert_eq!(stroke_of(&key(KeyCode::Char('G'))), Some("G".to_owned()));
}

#[test]
fn stroke_of_space_is_spc() {
    assert_eq!(stroke_of(&key(KeyCode::Char(' '))), Some("SPC".to_owned()));
}

#[test]
fn stroke_of_named_keys() {
    assert_eq!(stroke_of(&key(KeyCode::Esc)), Some("ESC".to_owned()));
    assert_eq!(stroke_of(&key(KeyCode::Enter)), Some("RET".to_owned()));
    assert_eq!(stroke_of(&key(KeyCode::Tab)), Some("TAB".to_owned()));
    assert_eq!(stroke_of(&key(KeyCode::Backspace)), Some("DEL".to_owned()));
    assert_eq!(stroke_of(&key(KeyCode::Up)), Some("<up>".to_owned()));
    assert_eq!(stroke_of(&key(KeyCode::Down)), Some("<down>".to_owned()));
}

#[test]
fn stroke_of_ctrl_char_is_emacs_notation() {
    let ev = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(stroke_of(&ev), Some("C-c".to_owned()));
}

#[test]
fn stroke_of_meta_char_is_emacs_notation() {
    let ev = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT);
    assert_eq!(stroke_of(&ev), Some("M-x".to_owned()));
}

#[test]
fn stroke_of_unhandled_key_is_none() {
    assert_eq!(stroke_of(&key(KeyCode::F(5))), None);
}

#[test]
fn headline_lines_renders_tree_with_todo_priority_tags() {
    let src = "* TODO [#A] Root :work:\n** Child\n";
    let doc = Document::load_str(src);
    let rendered = doc.as_ref().map(headline_lines);
    let text = rendered.unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(
        lines[0].starts_with("* TODO [#A] Root :work:"),
        "got {:?}",
        lines[0]
    );
    assert!(lines[1].starts_with("  * Child"), "got {:?}", lines[1]);
}

#[test]
fn headline_lines_empty_doc_is_empty() {
    let doc = Document::load_str("just text, no headline\n");
    let rendered = doc.as_ref().map(headline_lines);
    assert_eq!(rendered.unwrap_or_default(), String::new());
}

// New test written first (TDD) for the tree-sitter TUI integration cycle.
// It exercises the helper that produces ratatui-styled lines for a source
// containing a code block (using the pluggable KeywordHighlighter from
// closure-tree-sitter). The FileView render will be wired to use it.
#[test]
fn file_view_with_code_block_produces_styled_spans() {
    // Small self-contained block content (equivalent to the rust fixture inner).
    // Avoids fragile relative include_str from the tests/ subdirectory.
    let src = "#+BEGIN_SRC rust\nfn main() {\n    println!(\"hi\");\n}\n#+END_SRC\n";
    let lines = closure_tui::highlight_org_source(src);
    // Must produce lines, and the helper must have applied styling to
    // keywords inside the detected block content (e.g. "fn").
    assert!(!lines.is_empty());
    let has_styled_keyword = lines.iter().any(|line| {
        line.spans.iter().any(|sp| {
            (sp.style.add_modifier != ratatui::style::Modifier::empty() || sp.style.fg.is_some())
                && sp.content.contains("fn")
        })
    });
    assert!(has_styled_keyword, "expected at least one styled keyword span (e.g. 'fn') from the highlighter in the rendered lines");
}
