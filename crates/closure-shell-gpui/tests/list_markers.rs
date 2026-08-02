//! "theme numbers and dashes in the body detail preview and the
//! editor"
//!
//! Everything else on an org line had a face by now — the keyword, the
//! priority, the drawer, the table, the link, the emphasis runs — and
//! the two marks that make a list a list did not. A `-` and a `1.`
//! were prose, the same grey as the words after them, so the shape of
//! a list was carried entirely by the indentation.
//!
//! They are structure, not content: org paints them from their own
//! face, and so does this. The marker takes the colour; the text after
//! it stays prose, because a list item whose *text* changed colour
//! would be a list you cannot skim.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::Theme;
use closure_shell_gpui::{BodySpan, highlight_body, span_color_of};

/// The spans of a single line.
fn spans(line: &str) -> Vec<(BodySpan, String)> {
    highlight_body(line).into_iter().next().expect("one line")
}

/// The kinds on a line, in order.
fn kinds(line: &str) -> Vec<BodySpan> {
    spans(line).into_iter().map(|(k, _)| k).collect()
}

#[test]
fn a_dash_bullet_is_its_own_span() {
    let s = spans("- a bullet");
    assert_eq!(s.first().map(|(k, _)| *k), Some(BodySpan::Bullet));
    assert_eq!(s[0].1, "- ", "the marker and its space: {s:?}");
}

#[test]
fn the_text_after_a_bullet_is_still_prose() {
    // Colouring the whole item would make a list unskimmable.
    let s = spans("- a bullet");
    assert!(
        s.iter()
            .any(|(k, t)| *k == BodySpan::Plain && t.contains("a bullet")),
        "{s:?}"
    );
}

#[test]
fn org_s_other_bullets_count_too() {
    for line in ["+ plus", "* star at column zero is a headline"] {
        let _ = line;
    }
    assert_eq!(kinds("+ plus").first(), Some(&BodySpan::Bullet));
    assert_eq!(
        kinds("  - indented").first(),
        Some(&BodySpan::Plain),
        "the indent comes first"
    );
    assert!(
        kinds("  - indented").contains(&BodySpan::Bullet),
        "{:?}",
        kinds("  - indented")
    );
}

#[test]
fn a_star_at_column_zero_is_a_headline_not_a_bullet() {
    // The one ambiguity in org's list syntax, and getting it backwards
    // would repaint every headline in the file as a list item.
    assert!(matches!(kinds("* Heading")[0], BodySpan::Headline(_)));
    assert_eq!(kinds("  * nested star").get(1), Some(&BodySpan::Bullet));
}

#[test]
fn a_number_is_its_own_span() {
    for line in ["1. a number", "2) another", "10. ten"] {
        let s = spans(line);
        assert_eq!(
            s.first().map(|(k, _)| *k),
            Some(BodySpan::Number),
            "{line}: {s:?}"
        );
    }
}

#[test]
fn a_sentence_that_merely_starts_with_a_digit_is_prose() {
    // "1984 was a year" is not a list, and a classifier that thought so
    // would recolour a paragraph on its first word.
    assert_eq!(kinds("1984 was a year")[0], BodySpan::Plain);
    assert_eq!(kinds("3.14 is pi")[0], BodySpan::Plain);
}

#[test]
fn a_checkbox_is_not_swallowed_by_the_bullet() {
    // `- [ ] thing` is a bullet, a checkbox and prose — three things,
    // and the checkbox is the one you look for.
    let s = spans("- [X] done thing");
    assert_eq!(s[0].0, BodySpan::Bullet);
    assert!(s.iter().any(|(k, _)| *k == BodySpan::Checkbox), "{s:?}");
}

#[test]
fn markers_do_not_read_as_prose() {
    // The point of the item: they must not be the same colour as the
    // words beside them.
    let theme = Theme::dark();
    let plain = span_color_of(&theme, BodySpan::Plain);
    assert_ne!(span_color_of(&theme, BodySpan::Bullet), plain);
    assert_ne!(span_color_of(&theme, BodySpan::Number), plain);
    assert_ne!(span_color_of(&theme, BodySpan::Checkbox), plain);
}

#[test]
fn a_marker_inside_a_source_block_stays_verbatim() {
    // `- x` in a shell script is a flag, not a bullet.
    let lines = highlight_body("#+begin_src sh\n- not a bullet\n#+end_src");
    assert!(
        !lines[1].iter().any(|(k, _)| *k == BodySpan::Bullet),
        "{:?}",
        lines[1]
    );
}
