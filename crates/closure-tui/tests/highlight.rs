//! Coverage for `highlight_org_source`: the src-block highlight path is
//! the bulk of the function and was only touched once. Hermetic — pure
//! string -> styled-Line transform, no terminal.

#![allow(clippy::unwrap_used)]

use closure_tui::highlight_org_source;
use ratatui::style::{Color, Modifier};

fn all_text(lines: &[ratatui::text::Line<'static>]) -> String {
    lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn plain_text_passes_through_unstyled() {
    let out = highlight_org_source("just a line\nanother");
    assert_eq!(all_text(&out), "just a line\nanother");
    // No keyword styling on plain prose.
    let styled = out.iter().any(|l| {
        l.spans
            .iter()
            .any(|s| s.style.add_modifier.contains(Modifier::BOLD))
    });
    assert!(!styled);
}

#[test]
fn src_block_highlights_keywords() {
    let src = "#+BEGIN_SRC python\ndef f():\n    return 1\n#+END_SRC\n";
    let out = highlight_org_source(src);
    // Roundtrip: every original char survives the per-char span map.
    let text = all_text(&out);
    assert!(text.contains("def f():"), "content preserved: {text}");
    assert!(text.contains("#+BEGIN_SRC python") && text.contains("#+END_SRC"));
    // The 'def'/'return' keywords get yellow-bold styling.
    let kw = out.iter().any(|l| {
        l.spans.iter().any(|s| {
            s.style.fg == Some(Color::Yellow) && s.style.add_modifier.contains(Modifier::BOLD)
        })
    });
    assert!(kw, "expected a yellow-bold keyword span");
}

#[test]
fn src_block_without_lang_defaults_plain() {
    let src = "#+begin_src\nhello\n#+end_src\n";
    let out = highlight_org_source(src);
    assert!(all_text(&out).contains("hello"));
}

#[test]
fn empty_input_yields_no_lines() {
    assert!(highlight_org_source("").is_empty());
}
