//! The which-key panel misreads its own chords.
//!
//! Seen on :1 in Emacs mode with `C-c` pending. The chord column was a
//! fixed `w(px(56.0))`, so a chord wider than that wrapped *inside its
//! own box* while the description stayed beside the first line.
//! `C-c <down>` came out as
//!
//! ```text
//! C-c        priority-down
//! <down>
//! ```
//!
//! which reads as "C-c is priority-down" — a binding that does not
//! exist, for a command bound to something else. `C-c <up>` filled the
//! box exactly and so ran into its description with no gap at all:
//! `C-c <up>priority-up`.
//!
//! A panel whose whole job is to say what a key does must not print a
//! key it does not mean.
//!
//! These test the width itself rather than the painted result. The
//! gpui test harness measures a stub font — it reports 15px for `C-c k`
//! and 55px for `A` — so nothing painted through it can witness a wrap
//! that depends on real glyph advances. The decision the defect lives
//! in is the width, and the width is arithmetic.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::which_key_chord_w;

/// Roughly the advance of the panel's 11px monospace.
const ADVANCE: f32 = 7.2;

fn group(chords: &[&str]) -> Vec<(String, Vec<(String, String)>)> {
    vec![(
        "Edit".to_owned(),
        chords
            .iter()
            .map(|c| ((*c).to_owned(), "command".to_owned()))
            .collect(),
    )]
}

#[test]
fn a_long_chord_gets_a_column_wide_enough_to_hold_it() {
    // The reported case. `C-c <down>` is ten characters; at 7.2px it
    // needs ~72px and the column gave it 56.
    let groups = group(&["C-c <down>", "C-c t"]);
    let width = which_key_chord_w(&groups, ADVANCE);
    let needed = 10.0 * ADVANCE;
    assert!(
        width > needed,
        "`C-c <down>` needs {needed}px and the column is {width}px — it wraps"
    );
}

#[test]
fn the_column_answers_to_the_chords_not_to_a_constant() {
    // The defect in one line: a panel of short chords and a panel of
    // long ones came out identically wide, which is what proves the
    // width had never looked at a chord.
    let short = which_key_chord_w(&group(&["?", "g", "i"]), ADVANCE);
    let long = which_key_chord_w(&group(&["C-c C-x C-l", "C-c f c"]), ADVANCE);
    assert!(
        long > short,
        "long chords got {long}px and short ones {short}px"
    );
}

#[test]
fn the_widest_chord_sets_it_for_everyone() {
    // One width for the whole panel, not one per cell: the
    // descriptions line up into a column, and that column is what
    // makes the panel scannable rather than a ragged list.
    let mixed = which_key_chord_w(&group(&["?", "C-c C-x C-l", "g"]), ADVANCE);
    let alone = which_key_chord_w(&group(&["C-c C-x C-l"]), ADVANCE);
    assert!(
        (mixed - alone).abs() < f32::EPSILON,
        "a short chord narrowed the column under a long one: {mixed} vs {alone}"
    );
}

#[test]
fn a_panel_of_short_chords_pulls_its_descriptions_in() {
    // Written first as "a terse keymap keeps the old 56px", on the
    // theory that the previous look was worth preserving. The user
    // asked for the opposite on 2026-08-03: on the `g` prefix, where
    // the widest chord is three characters, 56px left every
    // description marooned a third of the panel away from its key —
    // "the alignment between the keybind and the command looks a bit
    // off", and far enough that `g O`, `g l`, `g y` and `g r` read as
    // unbound. They are all bound. There is no floor now, so this
    // asserts the tight pairing instead of the old padding.
    let width = which_key_chord_w(&group(&["g O", "g !"]), ADVANCE);
    assert!(
        width < 56.0,
        "a three-character chord still reserves {width}px"
    );
    assert!(
        width > 3.0 * ADVANCE,
        "{width}px does not fit `g O` and a gap"
    );
}

#[test]
fn an_empty_panel_still_has_a_gap() {
    // `which_key_filter` can narrow to nothing while a chord is
    // pending. Nothing is painted then, but a zero width would be a
    // layout with no gap in it at all rather than an empty panel.
    assert!(which_key_chord_w(&[], ADVANCE) > 0.0);
}

#[test]
fn the_column_follows_the_zoom() {
    // The advance passed in is what the window measured, so a zoomed
    // font moves the column with it instead of clipping into it.
    let normal = which_key_chord_w(&group(&["C-c C-x C-l"]), ADVANCE);
    let zoomed = which_key_chord_w(&group(&["C-c C-x C-l"]), ADVANCE * 2.0);
    assert!(zoomed > normal, "{zoomed} vs {normal}");
}
