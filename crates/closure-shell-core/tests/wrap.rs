//! Soft wrap: one logical line, several visual ones.
//!
//! The editor clips long lines and scrolls sideways instead of
//! wrapping, and that was a deliberate decision — wrapping desyncs the
//! one-number gutter, the fixed row height and the arithmetic that
//! turns pane height into a line count. It is also not what people
//! want when they write prose in a note.
//!
//! So the wrapping itself lives here, as a pure function over the text
//! and a column count, and the painter consumes visual lines instead of
//! logical ones. A visual line remembers which logical line it came
//! from and where in it, which is what keeps the gutter honest and lets
//! the cursor be found again.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{VisualLine, wrap_body};

fn texts<'a>(body: &'a str, lines: &[VisualLine]) -> Vec<&'a str> {
    lines.iter().map(|l| &body[l.start..l.end]).collect()
}

#[test]
fn a_short_line_is_one_visual_line() {
    let body = "short";
    let lines = wrap_body(body, 20);
    assert_eq!(texts(body, &lines), vec!["short"]);
    assert_eq!(lines[0].logical, 0);
    assert!(lines[0].first, "and it is the first of its logical line");
}

#[test]
fn a_long_line_breaks_at_a_space() {
    let body = "the quick brown fox jumps";
    let lines = wrap_body(body, 10);
    // The break space stays on the row it broke, so the rows partition
    // the bytes exactly (see `wrap_body`).
    assert_eq!(
        texts(body, &lines),
        vec!["the quick ", "brown fox ", "jumps"]
    );
    assert!(lines.iter().all(|l| l.logical == 0), "all one logical line");
    assert!(lines[0].first);
    assert!(!lines[1].first, "continuations are not firsts");
}

#[test]
fn every_logical_line_is_numbered_from_its_own_start() {
    let body = "aaa\nthe quick brown fox\nbbb";
    let lines = wrap_body(body, 10);
    let firsts: Vec<usize> = lines
        .iter()
        .filter(|l| l.first)
        .map(|l| l.logical)
        .collect();
    assert_eq!(firsts, vec![0, 1, 2], "one gutter number per logical line");
}

#[test]
fn a_word_longer_than_the_width_is_broken_rather_than_lost() {
    // A URL is the normal case here.
    let body = "https://example.com/a/very/long/path";
    let lines = wrap_body(body, 10);
    assert!(lines.len() > 1, "{:?}", texts(body, &lines));
    let rejoined: String = texts(body, &lines).concat();
    assert_eq!(rejoined, body, "no byte is dropped");
}

#[test]
fn wrapping_never_drops_or_duplicates_a_byte() {
    let body = "alpha beta\n\ngamma delta epsilon zeta\nlast";
    for cols in [4, 7, 10, 40] {
        let lines = wrap_body(body, cols);
        let mut rebuilt = String::new();
        let mut prev: Option<usize> = None;
        for l in &lines {
            if prev.is_some_and(|p| p != l.logical) {
                rebuilt.push('\n');
            }
            rebuilt.push_str(&body[l.start..l.end]);
            prev = Some(l.logical);
        }
        assert_eq!(rebuilt, body, "cols={cols}");
    }
}

#[test]
fn an_empty_line_still_takes_a_row() {
    // Otherwise a blank line between paragraphs disappears and every
    // number after it is wrong.
    let body = "one\n\ntwo";
    let lines = wrap_body(body, 10);
    assert_eq!(lines.len(), 3);
    assert_eq!(texts(body, &lines)[1], "");
}

#[test]
fn multibyte_text_wraps_on_character_boundaries() {
    let body = "ähnlich größer wörter hier";
    let lines = wrap_body(body, 8);
    // Slicing at a bad boundary would panic; this asserts it does not,
    // and that the text survives.
    let rejoined: String = texts(body, &lines).concat();
    assert_eq!(rejoined.replace(' ', ""), body.replace(' ', ""));
}

#[test]
fn zero_columns_does_not_hang_or_panic() {
    let lines = wrap_body("anything at all", 0);
    assert!(!lines.is_empty(), "degenerate width still yields rows");
}

#[test]
fn the_visual_row_of_a_cursor_can_be_found() {
    let body = "the quick brown fox jumps";
    let lines = wrap_body(body, 10);
    // Byte 12 is inside "brown", on the second visual row.
    let row = lines
        .iter()
        .position(|l| (l.start..=l.end).contains(&12))
        .expect("a row holds it");
    assert_eq!(row, 1, "{:?}", texts(body, &lines));
}
