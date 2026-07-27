//! The body-editor text seam.
//!
//! The editor pane used to paint one element per *character* so that a
//! click could land on the exact glyph — ~3000 elements per frame for
//! a 40-line viewport, which is where the reference shell's input lag
//! lived. The replacement paints one `StyledText` per line and
//! hit-tests through gpui's own `TextLayout`, which answers in *byte*
//! offsets into the line while [`closure_shell_core::BodyEditor`]
//! addresses positions in *char* columns.
//!
//! Everything that has to be right at that boundary is pure, and this
//! is where it is pinned: converting highlight spans into the byte
//! ranges `with_highlights` wants, and converting between byte offsets
//! and char columns without ever splitting a multi-byte char.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{
    BodySpan, Emphasis, byte_for_col, col_for_byte, selection_in_line, span_ranges, styled_runs,
};

/// gpui's `compute_runs` walks the highlight list assuming it is
/// ascending and non-overlapping, and debug-asserts every bound is a
/// char boundary. Any run list we hand it must satisfy all three plus
/// cover the line exactly — otherwise the pane silently drops text.
fn assert_well_formed(runs: &[closure_shell_gpui::StyledRun], line: &str) {
    let mut at = 0usize;
    for (range, _, _) in runs {
        assert_eq!(range.start, at, "runs must be contiguous: {runs:?}");
        assert!(range.start < range.end, "no empty runs: {runs:?}");
        assert!(
            line.is_char_boundary(range.start),
            "start on a char boundary"
        );
        assert!(line.is_char_boundary(range.end), "end on a char boundary");
        at = range.end;
    }
    assert_eq!(at, line.len(), "runs must cover the whole line: {runs:?}");
}

// === span_ranges: highlight spans -> byte ranges over the line ===

#[test]
fn span_ranges_cover_the_line_contiguously() {
    let spans = vec![
        (BodySpan::Keyword, "let".to_owned()),
        (BodySpan::Plain, " x = ".to_owned()),
        (BodySpan::Literal, "\"s\"".to_owned()),
    ];
    let ranges = span_ranges(&spans);
    assert_eq!(
        ranges,
        vec![
            (0..3, BodySpan::Keyword),
            (3..8, BodySpan::Plain),
            (8..11, BodySpan::Literal),
        ]
    );
    // Contiguity is what gpui's debug assertions demand: no gaps, no
    // overlaps, ending exactly at the line length.
    let line: String = spans.iter().map(|(_, s)| s.as_str()).collect();
    assert_eq!(ranges.last().unwrap().0.end, line.len());
    for pair in ranges.windows(2) {
        assert_eq!(pair[0].0.end, pair[1].0.start, "no gap between runs");
    }
}

#[test]
fn span_ranges_count_bytes_not_chars() {
    // 'ä' is two bytes; a range in chars would split it and trip
    // gpui's is_char_boundary assertion.
    let spans = vec![
        (BodySpan::Plain, "\u{e4}\u{e4}".to_owned()),
        (BodySpan::Comment, "# hi".to_owned()),
    ];
    assert_eq!(
        span_ranges(&spans),
        vec![(0..4, BodySpan::Plain), (4..8, BodySpan::Comment)]
    );
}

#[test]
fn span_ranges_skips_empty_spans() {
    let spans = vec![
        (BodySpan::Plain, String::new()),
        (BodySpan::Meta, "#+TITLE:".to_owned()),
    ];
    assert_eq!(
        span_ranges(&spans),
        vec![(0..8, BodySpan::Meta)],
        "an empty range is a no-op run; emitting it only costs work"
    );
}

#[test]
fn span_ranges_of_an_empty_line_is_empty() {
    assert_eq!(span_ranges(&[]), vec![]);
}

// === col_for_byte / byte_for_col: the mouse <-> cursor conversion ===

#[test]
fn col_for_byte_is_the_char_count_before_the_offset() {
    assert_eq!(col_for_byte("abc", 0), 0);
    assert_eq!(col_for_byte("abc", 2), 2);
    assert_eq!(col_for_byte("abc", 3), 3, "end of line");
}

#[test]
fn col_for_byte_counts_chars_over_multibyte_text() {
    let line = "\u{e4}b\u{20ac}c"; // ä(2) b(1) €(3) c(1)
    assert_eq!(col_for_byte(line, 0), 0);
    assert_eq!(col_for_byte(line, 2), 1, "past ä");
    assert_eq!(col_for_byte(line, 3), 2, "past b");
    assert_eq!(col_for_byte(line, 6), 3, "past €");
    assert_eq!(col_for_byte(line, 7), 4, "end");
}

#[test]
fn col_for_byte_clamps_past_the_end() {
    // gpui hit-tests a click in the empty tail of a line to an index
    // at or past the end; that must park the cursor at the line end,
    // never panic or wrap.
    assert_eq!(col_for_byte("abc", 99), 3);
    assert_eq!(col_for_byte("", 5), 0);
}

#[test]
fn col_for_byte_snaps_an_interior_byte_down_to_its_char() {
    // Defensive: byte 1 is inside 'ä'. Round down rather than split.
    assert_eq!(col_for_byte("\u{e4}b", 1), 0);
}

#[test]
fn byte_for_col_is_the_inverse_of_col_for_byte() {
    for line in ["", "abc", "\u{e4}b\u{20ac}c", "  indented"] {
        for col in 0..=line.chars().count() {
            let byte = byte_for_col(line, col);
            assert!(line.is_char_boundary(byte), "{line:?} col {col} -> {byte}");
            assert_eq!(col_for_byte(line, byte), col, "round trip {line:?} @{col}");
        }
    }
}

#[test]
fn byte_for_col_clamps_past_the_end() {
    assert_eq!(byte_for_col("abc", 99), 3);
    assert_eq!(byte_for_col("\u{e4}", 99), 2, "clamps to the byte length");
}

// === selection_in_line: the global VISUAL range, per line ===

#[test]
fn selection_clipped_to_a_line_it_fully_covers() {
    // Line occupying bytes 10..20 of the buffer, selection 0..100.
    assert_eq!(selection_in_line(10, 10, (0, 100)), Some(0..10));
}

#[test]
fn selection_clipped_to_a_partial_overlap() {
    // Line at bytes 10..20, selection 15..30 -> local 5..10.
    assert_eq!(selection_in_line(10, 10, (15, 30)), Some(5..10));
    // Selection 5..13 -> local 0..3.
    assert_eq!(selection_in_line(10, 10, (5, 13)), Some(0..3));
}

#[test]
fn a_selection_that_misses_the_line_yields_none() {
    assert_eq!(
        selection_in_line(10, 10, (0, 10)),
        None,
        "ends at our start"
    );
    assert_eq!(
        selection_in_line(10, 10, (20, 30)),
        None,
        "starts at our end"
    );
    assert_eq!(selection_in_line(10, 10, (30, 40)), None, "far below");
}

#[test]
fn an_empty_selection_yields_none() {
    assert_eq!(
        selection_in_line(10, 10, (15, 15)),
        None,
        "a zero-width range paints nothing"
    );
}

#[test]
fn a_reversed_selection_is_normalised() {
    // BodyEditor always hands over (lo, hi), but a caller inverting
    // them must not produce a panicking range.
    assert_eq!(selection_in_line(10, 10, (18, 12)), Some(2..8));
}

// === styled_runs: colour spans merged with the background marks ===
//
// A line carries two independent stylings — the syntax colour per span
// and a background for the VISUAL selection or the block caret. gpui
// takes a single ordered run list, so they have to be merged by
// splitting the colour spans at the highlight's edges.

fn spans() -> Vec<(BodySpan, String)> {
    vec![
        (BodySpan::Keyword, "let".to_owned()),
        (BodySpan::Plain, " x".to_owned()),
    ]
}

#[test]
fn without_a_highlight_the_runs_are_just_the_spans() {
    let runs = styled_runs(&spans(), &[]);
    assert_eq!(
        runs,
        vec![
            (0..3, BodySpan::Keyword, None),
            (3..5, BodySpan::Plain, None),
        ]
    );
    assert_well_formed(&runs, "let x");
}

#[test]
fn a_highlight_inside_one_span_splits_it_in_three() {
    // Block caret on the 'e' of `let`.
    let runs = styled_runs(&spans(), &[(1..2, Emphasis::Cursor)]);
    assert_eq!(
        runs,
        vec![
            (0..1, BodySpan::Keyword, None),
            (1..2, BodySpan::Keyword, Some(Emphasis::Cursor)),
            (2..3, BodySpan::Keyword, None),
            (3..5, BodySpan::Plain, None),
        ]
    );
    assert_well_formed(&runs, "let x");
}

#[test]
fn a_highlight_spanning_a_boundary_marks_both_sides() {
    // Selection over "t x" — crosses the Keyword/Plain seam, and each
    // side must keep its own colour while sharing the background.
    let runs = styled_runs(&spans(), &[(2..5, Emphasis::Cursor)]);
    assert_eq!(
        runs,
        vec![
            (0..2, BodySpan::Keyword, None),
            (2..3, BodySpan::Keyword, Some(Emphasis::Cursor)),
            (3..5, BodySpan::Plain, Some(Emphasis::Cursor)),
        ]
    );
    assert_well_formed(&runs, "let x");
}

#[test]
fn a_highlight_covering_everything_marks_every_run() {
    let runs = styled_runs(&spans(), &[(0..5, Emphasis::Cursor)]);
    assert!(runs.iter().all(|(_, _, m)| m.is_some()), "{runs:?}");
    assert_well_formed(&runs, "let x");
}

#[test]
fn a_highlight_outside_the_line_changes_nothing() {
    assert_eq!(
        styled_runs(&spans(), &[(9..12, Emphasis::Cursor)]),
        styled_runs(&spans(), &[])
    );
    assert_eq!(
        styled_runs(&spans(), &[(3..3, Emphasis::Cursor)]),
        styled_runs(&spans(), &[])
    );
}

#[test]
fn runs_stay_on_char_boundaries_over_multibyte_text() {
    let spans = vec![
        (BodySpan::Plain, "\u{e4}\u{20ac}".to_owned()), // ä(2) €(3)
        (BodySpan::Comment, "#".to_owned()),
    ];
    let line = "\u{e4}\u{20ac}#";
    // Highlight the € only: bytes 2..5.
    let runs = styled_runs(&spans, &[(2..5, Emphasis::Cursor)]);
    assert_well_formed(&runs, line);
    assert_eq!(
        runs,
        vec![
            (0..2, BodySpan::Plain, None),
            (2..5, BodySpan::Plain, Some(Emphasis::Cursor)),
            (5..6, BodySpan::Comment, None),
        ]
    );
}

#[test]
fn an_empty_line_produces_no_runs() {
    assert_eq!(styled_runs(&[], &[]), vec![]);
    assert_eq!(styled_runs(&[], &[(0..1, Emphasis::Cursor)]), vec![]);
}

// === split_runs: the INSERT caret bar ===
//
// NORMAL/VISUAL show the cursor as a background block, which is just
// another highlight range. INSERT shows a thin bar *between* two
// glyphs, so the cursor line is painted as two `StyledText` halves
// with a 2px div between them — which means the run list has to be cut
// at the caret and the tail rebased to start at 0.

use closure_shell_gpui::split_runs;

#[test]
fn splitting_between_runs_keeps_them_whole() {
    let runs = styled_runs(&spans(), &[]);
    let (head, tail) = split_runs(&runs, 3);
    assert_eq!(head, vec![(0..3, BodySpan::Keyword, None)]);
    assert_eq!(
        tail,
        vec![(0..2, BodySpan::Plain, None)],
        "tail rebased to 0"
    );
}

#[test]
fn splitting_inside_a_run_cuts_it_in_two() {
    let runs = styled_runs(&spans(), &[]);
    let (head, tail) = split_runs(&runs, 1);
    assert_eq!(head, vec![(0..1, BodySpan::Keyword, None)]);
    assert_eq!(
        tail,
        vec![
            (0..2, BodySpan::Keyword, None),
            (2..4, BodySpan::Plain, None)
        ],
        "the cut run keeps its kind on both sides"
    );
    assert_well_formed(&head, "l");
    assert_well_formed(&tail, "et x");
}

#[test]
fn splitting_at_zero_puts_everything_in_the_tail() {
    let runs = styled_runs(&spans(), &[]);
    let (head, tail) = split_runs(&runs, 0);
    assert!(head.is_empty(), "a caret at column 0 has no prefix");
    assert_eq!(tail, runs, "and the tail is the untouched line");
}

#[test]
fn splitting_at_the_end_puts_everything_in_the_head() {
    let runs = styled_runs(&spans(), &[]);
    let (head, tail) = split_runs(&runs, 5);
    assert_eq!(head, runs);
    assert!(tail.is_empty(), "a caret at end of line has no suffix");
}

#[test]
fn splitting_past_the_end_is_clamped_not_panicking() {
    let runs = styled_runs(&spans(), &[]);
    let (head, tail) = split_runs(&runs, 999);
    assert_eq!(head, runs);
    assert!(tail.is_empty());
}

#[test]
fn splitting_preserves_the_highlight_flags() {
    // Selection over "t x", caret inside it at byte 4.
    let runs = styled_runs(&spans(), &[(2..5, Emphasis::Cursor)]);
    let (head, tail) = split_runs(&runs, 4);
    assert_eq!(
        head,
        vec![
            (0..2, BodySpan::Keyword, None),
            (2..3, BodySpan::Keyword, Some(Emphasis::Cursor)),
            (3..4, BodySpan::Plain, Some(Emphasis::Cursor)),
        ]
    );
    assert_eq!(tail, vec![(0..1, BodySpan::Plain, Some(Emphasis::Cursor))]);
}

#[test]
fn splitting_an_empty_run_list_gives_two_empty_halves() {
    let (head, tail) = split_runs(&[], 3);
    assert!(head.is_empty() && tail.is_empty());
}

// === the cursor on empty lines ===
//
// The block cursor is drawn by inverting the glyph under it. On an
// empty line — or with the cursor parked past the last glyph — there
// is no glyph to invert, so the cursor simply vanished. Those cases
// need a block element of their own.

use closure_shell_gpui::needs_trailing_caret;

#[test]
fn a_cursor_over_a_glyph_needs_no_extra_caret() {
    assert!(!needs_trailing_caret("abc", true, 0));
    assert!(!needs_trailing_caret("abc", true, 2), "on the last glyph");
}

#[test]
fn a_cursor_past_the_last_glyph_needs_one() {
    assert!(
        needs_trailing_caret("abc", true, 3),
        "at end of line there is nothing to invert"
    );
}

#[test]
fn an_empty_line_always_needs_one() {
    assert!(needs_trailing_caret("", true, 0));
}

#[test]
fn other_lines_never_need_one() {
    assert!(!needs_trailing_caret("", false, 0), "not the cursor line");
    assert!(!needs_trailing_caret("abc", false, 9));
}

#[test]
fn a_column_past_the_end_still_counts_as_the_end() {
    // Vertical motion can leave the column beyond a shorter line.
    assert!(needs_trailing_caret("ab", true, 99));
}

#[test]
fn multibyte_glyphs_are_counted_as_glyphs() {
    // 3 chars, 7 bytes: column 2 is over a glyph, column 3 is past it.
    let line = "\u{e4}\u{20ac}c";
    assert!(!needs_trailing_caret(line, true, 2));
    assert!(needs_trailing_caret(line, true, 3));
}
