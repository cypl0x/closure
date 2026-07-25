//! Links and tables in the body.
//!
//! `highlight_body` classified `#+…` keywords, drawers and the inside
//! of src blocks, and nothing else. So the two constructs org uses
//! most for structure — `[[target][label]]` links and `| a | b |`
//! tables — rendered as undifferentiated prose, in a shell whose whole
//! premise is that the outline and its links are the point.
//!
//! Two rules here, and they are both about not lying to the reader:
//!
//!  * the spans still concatenate back to the line verbatim, because
//!    everything downstream (the byte ranges, the cursor, the mouse
//!    hit-testing) assumes it (I1);
//!  * a link's *target* is carried separately rather than hidden, so a
//!    click can follow it — a rendered label with no way back to the
//!    id would be prettier and useless.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{BodySpan, highlight_body, line_links};

/// Spans must reconstruct the line exactly, whatever they classify.
fn assert_verbatim(body: &str) {
    let joined: Vec<String> = highlight_body(body)
        .iter()
        .map(|l| l.iter().map(|(_, s)| s.as_str()).collect::<String>())
        .collect();
    assert_eq!(joined.join("\n"), body, "spans must cover every byte (I1)");
}

// === links ===

#[test]
fn a_labelled_link_is_classified() {
    let body = "see [[id:01HQ][the note]] for more";
    let lines = highlight_body(body);
    assert!(
        lines[0].iter().any(|(k, _)| *k == BodySpan::Link),
        "a link span exists: {:?}",
        lines[0]
    );
    assert_verbatim(body);
}

#[test]
fn a_bare_link_is_classified_too() {
    let body = "see [[https://example.com]] ok";
    assert!(
        highlight_body(body)[0]
            .iter()
            .any(|(k, _)| *k == BodySpan::Link)
    );
    assert_verbatim(body);
}

#[test]
fn prose_around_a_link_stays_prose() {
    let lines = highlight_body("a [[x][y]] b");
    let plain: String = lines[0]
        .iter()
        .filter(|(k, _)| *k == BodySpan::Plain)
        .map(|(_, s)| s.as_str())
        .collect();
    assert_eq!(plain, "a  b", "only the link itself is a link");
}

#[test]
fn two_links_on_one_line_are_both_found() {
    let body = "[[a][one]] and [[b][two]]";
    let links = line_links(body);
    assert_eq!(links.len(), 2, "{links:?}");
    assert_eq!(links[0].target, "a");
    assert_eq!(links[1].target, "b");
    assert_verbatim(body);
}

#[test]
fn a_link_carries_its_target_and_its_byte_range() {
    // The range is what lets a click on the rendered label resolve
    // back to the id it points at.
    let body = "go [[id:01HQ][there]] now";
    let links = line_links(body);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, "id:01HQ");
    assert_eq!(links[0].label, "there");
    assert_eq!(&body[links[0].range.clone()], "[[id:01HQ][there]]");
}

#[test]
fn a_bare_link_labels_itself() {
    let links = line_links("[[https://example.com]]");
    assert_eq!(links[0].target, "https://example.com");
    assert_eq!(
        links[0].label, "https://example.com",
        "with no label, the target is the label"
    );
}

#[test]
fn unclosed_brackets_are_not_links() {
    // Half-typed syntax must not swallow the rest of the line.
    assert!(line_links("[[unclosed").is_empty());
    assert!(line_links("a [ b ] c").is_empty());
    assert_verbatim("[[unclosed");
}

#[test]
fn links_inside_a_src_block_stay_code() {
    // `[[` is valid in plenty of languages; inside a fence it is code.
    let body = "#+BEGIN_SRC sh\ntest [[ -f x ]] && echo\n#+END_SRC";
    let lines = highlight_body(body);
    assert!(
        !lines[1].iter().any(|(k, _)| *k == BodySpan::Link),
        "no link spans inside a block: {:?}",
        lines[1]
    );
    assert_verbatim(body);
}

// === tables ===

#[test]
fn a_table_row_is_classified() {
    let body = "| name | qty |";
    let lines = highlight_body(body);
    assert!(
        lines[0].iter().all(|(k, _)| *k == BodySpan::Table),
        "the whole row is table: {:?}",
        lines[0]
    );
    assert_verbatim(body);
}

#[test]
fn a_table_rule_is_a_table_row() {
    assert_eq!(highlight_body("|------+-----|")[0][0].0, BodySpan::Table);
}

#[test]
fn an_indented_table_still_counts() {
    assert_eq!(highlight_body("   | a | b |")[0][0].0, BodySpan::Table);
}

#[test]
fn a_pipe_in_prose_is_not_a_table() {
    let lines = highlight_body("a | b");
    assert!(
        lines[0].iter().all(|(k, _)| *k != BodySpan::Table),
        "{:?}",
        lines[0]
    );
}

#[test]
fn a_table_inside_a_src_block_stays_code() {
    let body = "#+BEGIN_SRC sh\n| a | b |\n#+END_SRC";
    assert!(
        !highlight_body(body)[1]
            .iter()
            .any(|(k, _)| *k == BodySpan::Table)
    );
}

// === the whole thing still round-trips ===

#[test]
fn a_mixed_body_reconstructs_verbatim() {
    assert_verbatim(
        "* not a headline here\n\
         prose with [[id:1][a link]] in it\n\
         | col | col |\n\
         |-----+-----|\n\
         | a   | b   |\n\
         :PROPERTIES:\n\
         :ID: x\n\
         :END:\n\
         #+BEGIN_SRC rust\n\
         let x = \"[[not a link]]\";\n\
         #+END_SRC\n\
         tail\n",
    );
}
