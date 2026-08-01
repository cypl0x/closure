//! Keeping a cursor, a long line and a capped list on screen.
//!
//! Four separate ways the window could show you something other than
//! where you were:
//!
//!  * the graph pane painted the first 20 hubs and 50 orphans while the
//!    keyboard cursor ran across the whole list, so past the cap `j`
//!    moved a cursor that was not drawn and nothing scrolled after it;
//!  * the sniffer and conflict panes put a button row above their
//!    rows, so a row index was not a child index and neither pane
//!    could reveal its own cursor at all;
//!  * a body line longer than the pane wrapped, which desynced the
//!    one-number gutter, the fixed row height and the line arithmetic
//!    that turns pane height into a viewport;
//!  * and a `/` search moved the cursor to a match it never marked.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{
    BodySpan, Emphasis, ModalSurface, body_text_px, h_scroll_start, line_matches, outline_text_px,
    palette_list_height, scaled_text_px, side_reveal_offset, styled_runs, visible_window,
};

// === a capped list still shows its cursor ===

#[test]
fn a_short_list_is_shown_whole() {
    assert_eq!(visible_window(0, 5, 20), 0..5);
    assert_eq!(visible_window(4, 5, 20), 0..5);
}

#[test]
fn a_long_list_starts_at_the_top_while_the_cursor_is_there() {
    assert_eq!(visible_window(0, 100, 20), 0..20);
    assert_eq!(visible_window(19, 100, 20), 0..20);
}

#[test]
fn the_window_follows_the_cursor_past_the_cap() {
    // The bug: a cursor at 25 with a 20-row cap was simply not drawn.
    let w = visible_window(25, 100, 20);
    assert!(w.contains(&25), "the cursor is in it: {w:?}");
    assert_eq!(w.len(), 20, "still only a capful");
}

#[test]
fn the_window_stops_at_the_end_of_the_list() {
    let w = visible_window(99, 100, 20);
    assert_eq!(w, 80..100);
}

#[test]
fn a_cursor_outside_the_list_does_not_panic() {
    assert_eq!(visible_window(500, 10, 20), 0..10);
    assert_eq!(visible_window(0, 0, 20), 0..0);
    assert_eq!(visible_window(0, 5, 0), 0..0);
}

// === which panes can reveal their own cursor, and where its row is ===

#[test]
fn a_flat_list_reveals_row_zero_at_child_zero() {
    for surface in [
        ModalSurface::Headlines,
        ModalSurface::BodySearch,
        ModalSurface::Backlinks,
        ModalSurface::Journal,
        ModalSurface::Cron,
        ModalSurface::UndoHistory,
    ] {
        assert_eq!(side_reveal_offset(surface), Some(0), "{surface:?}");
    }
}

#[test]
fn a_pane_with_a_button_row_offsets_by_it() {
    // Sniffer and Conflicts paint allow/block and ours/theirs above
    // their rows, so row 0 is child 1 — which is why they used to
    // reveal nothing at all rather than reveal the wrong thing.
    assert_eq!(side_reveal_offset(ModalSurface::Sniffer), Some(1));
    assert_eq!(side_reveal_offset(ModalSurface::Conflicts), Some(1));
}

#[test]
fn a_sectioned_pane_still_reveals_nothing() {
    // Graph groups its rows under headers, so a child index is not a
    // row index there; it keeps its cursor visible by windowing
    // instead ([`visible_window`]).
    assert_eq!(side_reveal_offset(ModalSurface::Graph), None);
    assert_eq!(side_reveal_offset(ModalSurface::Browse), None);
    assert_eq!(side_reveal_offset(ModalSurface::EditBody), None);
}

// === a long line does not wrap, so it has to scroll ===

#[test]
fn a_line_that_fits_is_not_scrolled() {
    assert_eq!(h_scroll_start(0, 80), 0);
    assert_eq!(h_scroll_start(79, 80), 0);
}

#[test]
fn the_cursor_pulls_the_line_left_when_it_runs_off_the_edge() {
    // Column 100 in an 80-column pane: the last visible column is the
    // cursor's, the way the vertical viewport keeps the cursor line.
    assert_eq!(h_scroll_start(80, 80), 1);
    assert_eq!(h_scroll_start(100, 80), 21);
}

#[test]
fn a_degenerate_pane_width_never_divides_by_it() {
    assert_eq!(h_scroll_start(50, 0), 0);
}

// === search matches are marked ===

#[test]
fn every_match_on_the_line_is_found() {
    assert_eq!(line_matches("ab cd ab", "ab"), vec![0..2, 6..8]);
}

#[test]
fn a_match_is_case_sensitive_like_the_search_itself() {
    // `BodyEditor`'s `/` uses `str::find`, so a case-insensitive mark
    // would light up a word `n` refuses to jump to.
    assert_eq!(line_matches("Alpha alpha", "alpha"), vec![6..11]);
    assert!(line_matches("Alpha", "ALPHA").is_empty());
}

#[test]
fn no_pattern_and_no_match_mark_nothing() {
    assert!(line_matches("abc", "").is_empty());
    assert!(line_matches("abc", "zz").is_empty());
    assert!(line_matches("", "a").is_empty());
}

#[test]
fn overlapping_matches_do_not_overlap_in_the_result() {
    // `aa` in `aaaa` is two runs, not three: the marks are handed to a
    // renderer that assumes disjoint, ascending ranges.
    assert_eq!(line_matches("aaaa", "aa"), vec![0..2, 2..4]);
}

#[test]
fn matches_land_on_char_boundaries() {
    let text = "ä match ö match";
    for range in line_matches(text, "match") {
        assert!(text.is_char_boundary(range.start));
        assert!(text.is_char_boundary(range.end));
    }
}

// === several marks on one line, layered in order ===

fn spans() -> Vec<(BodySpan, String)> {
    vec![
        (BodySpan::Plain, "abc".to_owned()),
        (BodySpan::Link, "defgh".to_owned()),
    ]
}

#[test]
fn a_line_with_no_marks_is_one_run_per_span() {
    let runs = styled_runs(&spans(), &[]);
    assert_eq!(
        runs,
        vec![(0..3, BodySpan::Plain, None), (3..8, BodySpan::Link, None)]
    );
}

#[test]
fn a_search_mark_and_the_cursor_coexist() {
    // The cursor is on the `d`, a search matched `bc`. Both are
    // background ranges and both must survive — the cursor used to be
    // the only one the renderer could carry.
    let runs = styled_runs(
        &spans(),
        &[(1..3, Emphasis::Search), (3..4, Emphasis::Cursor)],
    );
    let marks: Vec<Option<Emphasis>> = runs.iter().map(|(_, _, m)| *m).collect();
    assert!(marks.contains(&Some(Emphasis::Search)));
    assert!(marks.contains(&Some(Emphasis::Cursor)));
}

#[test]
fn marked_runs_stay_contiguous_and_ordered() {
    let runs = styled_runs(
        &spans(),
        &[(1..2, Emphasis::Search), (4..6, Emphasis::Selection)],
    );
    let mut at = 0usize;
    for (range, _, _) in &runs {
        assert_eq!(range.start, at, "contiguous: {runs:?}");
        at = range.end;
    }
    assert_eq!(at, 8, "covers the line");
}

#[test]
fn a_later_mark_wins_where_two_overlap() {
    // The cursor is drawn over a search hit it happens to sit on:
    // whichever mark comes last in the list is on top.
    let runs = styled_runs(
        &spans(),
        &[(0..4, Emphasis::Search), (1..2, Emphasis::Cursor)],
    );
    let at_one = runs
        .iter()
        .find(|(r, _, _)| r.start == 1)
        .expect("a run starting at 1");
    assert_eq!(at_one.2, Some(Emphasis::Cursor));
}

#[test]
fn an_empty_mark_marks_nothing() {
    assert_eq!(
        styled_runs(&spans(), &[(2..2, Emphasis::Cursor)]),
        styled_runs(&spans(), &[])
    );
}

#[test]
fn marks_outside_the_line_are_ignored() {
    assert_eq!(
        styled_runs(&spans(), &[(20..30, Emphasis::Search)]),
        styled_runs(&spans(), &[])
    );
}

// === zoom reaches the text the outline is made of ===

#[test]
fn zoom_scales_the_outline_and_the_body_together() {
    // The scale was applied to the body pane alone, so `C-+` pressed in
    // the outline — where a reader spends most of the session — changed
    // nothing they could see.
    assert!((outline_text_px(1.0) - 14.0).abs() < f32::EPSILON);
    assert!((body_text_px(1.0) - 13.0).abs() < f32::EPSILON);
    assert!(outline_text_px(2.0) > outline_text_px(1.0));
    assert!(body_text_px(2.0) > body_text_px(1.0));
    // The ratio is the zoom's, not a per-pane invention.
    assert!((outline_text_px(2.0) / outline_text_px(1.0) - 2.0).abs() < 0.001);
    assert!((body_text_px(0.5) / body_text_px(1.0) - 0.5).abs() < 0.001);
}

#[test]
fn every_pane_scales_from_the_one_number() {
    // Zoom reached two panes; the other sixty-odd text sizes in the
    // window were literals, so a picker, the block output, the agenda
    // and the status line all stayed at 11px under a 3x body. One
    // window, one scale: the sizes differ, the ratio does not.
    for base in [10.0_f32, 11.0, 12.0, 13.0, 14.0, 15.0] {
        assert!((scaled_text_px(base, 1.0) - base).abs() < f32::EPSILON);
        assert!((scaled_text_px(base, 2.0) / base - 2.0).abs() < 0.001);
        assert!((scaled_text_px(base, 0.5) / base - 0.5).abs() < 0.001);
    }
    // The two named panes are the same function underneath.
    assert!((outline_text_px(1.7) - scaled_text_px(14.0, 1.7)).abs() < f32::EPSILON);
    assert!((body_text_px(1.7) - scaled_text_px(13.0, 1.7)).abs() < f32::EPSILON);
}

// === the floating palette has to be given a height ===

#[test]
fn the_palette_list_is_as_tall_as_its_matches() {
    // A `uniform_list` fills the space it is given and asks for none:
    // inside a panel that sizes to its content, "grow" had nothing to
    // grow into, so the palette painted its query line, its footer and
    // a list exactly zero pixels tall.
    let one = palette_list_height(1, 1.0);
    assert!(one > 0.0, "one match is still a row: {one}");
    // Five rows are five times one *row* — the container's own padding
    // is added once, not per row. Asserting `5 × one` was what pinned
    // the padding out of the budget and clipped the last match.
    let row = closure_shell_gpui::palette_row_height(1.0);
    assert!(
        4.0f32
            .mul_add(
                -row,
                palette_list_height(5, 1.0) - palette_list_height(1, 1.0)
            )
            .abs()
            < 0.001,
        "four more rows are four more row heights"
    );
    // Empty is still a row's worth — the "no matches" line lives there.
    assert!((palette_list_height(0, 1.0) - one).abs() < f32::EPSILON);
    // Long lists stop growing rather than running off the window.
    assert!(
        (palette_list_height(500, 1.0) - palette_list_height(12, 1.0)).abs() < f32::EPSILON,
        "capped"
    );
    // And it scales with the window's zoom like everything else.
    assert!((palette_list_height(3, 2.0) / palette_list_height(3, 1.0) - 2.0).abs() < 0.001);
}

// === The palette box is tall enough for the rows it promises ===
//
// Reported 2026-08-02: "the box is always too small so that the last
// item in the list will just be the half of it shown in the height.
// Especially when there is just a single item because of the filter it
// looks weird."
//
// The height budgeted for `n` rows was `n × row height` exactly, and
// the list container adds its own padding above and below. So every
// palette was short by that padding and the last row was clipped —
// most visibly with one match, where the clipped row is the only row.

#[test]
fn the_list_is_taller_than_the_rows_it_holds() {
    let rows = 3.0 * closure_shell_gpui::palette_row_height(1.0);
    assert!(
        palette_list_height(3, 1.0) > rows,
        "{} must clear {rows} to leave room for its own padding",
        palette_list_height(3, 1.0)
    );
}

#[test]
fn a_single_match_gets_a_whole_row() {
    let one = closure_shell_gpui::palette_row_height(1.0);
    assert!(
        palette_list_height(1, 1.0) >= one,
        "one row plus padding, not a clipped one"
    );
}

#[test]
fn the_padding_does_not_grow_with_the_row_count() {
    // Otherwise a long list drifts steadily taller than its contents.
    let slack = |n: usize| {
        #[expect(clippy::cast_precision_loss, reason = "small counts")]
        let rows = n as f32 * closure_shell_gpui::palette_row_height(1.0);
        palette_list_height(n, 1.0) - rows
    };
    assert!((slack(1) - slack(9)).abs() < 0.001, "constant slack");
}
