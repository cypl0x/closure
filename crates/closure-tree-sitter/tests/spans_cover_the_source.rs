//! The invariant every highlighter promises, checked on real inputs.
//!
//! The trait states it: the returned spans "must be non-overlapping and
//! cover `[0, source.len())` without gaps so shells can fold them into
//! a string-buffer renderer without re-scanning". A shell trusts that
//! completely — it concatenates the spans and paints the result — so a
//! gap silently deletes characters from the screen and an overlap
//! duplicates them.
//!
//! Most of the code guarding this is marked "should not happen", which
//! is exactly the kind of code that is never tested and never removed.
//! Rather than reach for each defensive branch, these assert the
//! *promise* over a spread of real source, which is what those branches
//! exist to keep.
//!
//! `pick_highlighter` is here because it had no caller in a test.
//! Its own doc records why it exists: the function lived twice, and the
//! gpui shell — the one the user looks at all day — reached straight
//! for `KeywordHighlighter`, so twenty grammars would have been
//! compiled and never consulted.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_tree_sitter::pick_highlighter;

/// Every span, in order, with no gap and no overlap, covering the whole
/// input.
fn assert_covers(lang: &str, source: &str) {
    let h = pick_highlighter(lang);
    let spans = h.highlight(source);

    if source.is_empty() {
        assert!(spans.is_empty(), "{lang}: empty source produced {spans:?}");
        return;
    }

    assert!(
        !spans.is_empty(),
        "{lang}: non-empty source produced no spans: {source:?}"
    );
    assert_eq!(spans[0].start, 0, "{lang}: does not start at 0: {spans:?}");
    assert_eq!(
        spans.last().expect("non-empty").end,
        source.len(),
        "{lang}: does not reach the end of {source:?}: {spans:?}"
    );
    for pair in spans.windows(2) {
        assert_eq!(
            pair[0].end, pair[1].start,
            "{lang}: gap or overlap between {:?} and {:?} in {source:?}",
            pair[0], pair[1]
        );
    }
    for s in &spans {
        assert!(s.start < s.end, "{lang}: empty span {s:?} in {source:?}");
    }
}

#[test]
fn shell_and_python_comments_do_not_break_the_covering() {
    // The `#` branch: in these languages it opens a comment, so the
    // scanner must stop grouping punctuation there. Getting that wrong
    // is how a comment marker ends up inside a punctuation span.
    for lang in ["shell", "python"] {
        assert_covers(lang, "x = 1  # a trailing comment\n");
        assert_covers(lang, "# a whole-line comment\n");
        assert_covers(lang, "#");
        assert_covers(lang, "###\n");
    }
}

#[test]
fn rust_line_comments_do_not_break_the_covering() {
    // The `//` branch, which needs two bytes of lookahead — and the
    // lookahead has to stop at the end of the buffer.
    assert_covers("rust", "let x = 1; // a comment\n");
    assert_covers("rust", "// leading comment\nfn main() {}\n");
    // A single slash at the very end: the lookahead would read past
    // the buffer if it did not check.
    assert_covers("rust", "let x = 1 /");
    assert_covers("rust", "/");
    assert_covers("rust", "a / b");
}

#[test]
fn strings_and_numbers_and_keywords_still_tile_the_source() {
    assert_covers("rust", r#"fn main() { let s = "hi"; let n = 42; }"#);
    assert_covers("python", "def f(x):\n    return \"a\" + 'b' + str(3)\n");
    assert_covers("shell", "echo \"$HOME\" | grep -c '' # count\n");
}

#[test]
fn punctuation_runs_and_whitespace_tile_the_source() {
    // The run-grouping loop, including the "if we advanced 0, force at
    // least one byte" guard — without it a byte that satisfies none of
    // the conditions loops forever.
    for src in ["((((", "    ", "\t\n\t", "!!!???", "a  ,  b", "===>"] {
        assert_covers("rust", src);
    }
}

#[test]
fn an_unterminated_string_still_tiles_the_source() {
    // Truncated input, which is what a half-typed line is — and the
    // editor highlights on every keystroke, so this is not rare.
    assert_covers("rust", r#"let s = "unterminated"#);
    assert_covers("python", "s = 'unterminated");
    assert_covers("shell", "echo \"unterminated");
}

#[test]
fn a_language_nobody_bundles_still_tiles_the_source() {
    // `pick_highlighter` falls back to the keyword tier, and the
    // fallback owes the same promise as everything else.
    for lang in ["", "cobol", "brainfuck", "not a language"] {
        assert_covers(lang, "some text 123 \"quoted\" # hash\n");
    }
}

#[test]
fn an_empty_source_produces_no_spans_rather_than_one_empty_span() {
    // A zero-length span would make a renderer emit nothing and a
    // naive `windows(2)` check pass — worth pinning either way.
    for lang in ["rust", "python", "shell", "unknown"] {
        assert!(pick_highlighter(lang).highlight("").is_empty());
    }
}

#[test]
fn non_ascii_source_tiles_by_bytes_without_splitting_the_promise() {
    // Spans are byte offsets. A multi-byte character must not produce
    // a gap, whatever the scanner decides it is.
    assert_covers("rust", "let s = \"héllo wörld\";\n");
    assert_covers("python", "# comment with émoji 🎉\n");
    assert_covers("shell", "echo 'ünïcode'\n");
}

#[test]
fn pick_highlighter_names_the_language_it_was_asked_for() {
    // The fact that made this function worth having: a shell asks for
    // a language and gets a highlighter that agrees it is that
    // language, rather than one silently answering for something else.
    for lang in ["rust", "python", "shell"] {
        assert_eq!(pick_highlighter(lang).language(), lang);
    }
}
