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

// === inline markup ===
//
// closure-org has parsed `*bold*` / `/italic/` / `=code=` / `~verb~` /
// `+strike+` / `_under_` since the parser existed, and the reference
// shell rendered every one of them as flat prose — in a note-taking
// tool whose text is the product. The runs are classified here and
// carry their own decoration, so a shell can draw weight and slant
// rather than only a colour.

use closure_shell_gpui::{span_decoration, span_ranges};

/// The kinds on one line, in order.
fn kinds(body: &str) -> Vec<BodySpan> {
    highlight_body(body)[0].iter().map(|(k, _)| *k).collect()
}

#[test]
fn every_emphasis_marker_is_classified() {
    assert_eq!(kinds("*b*"), vec![BodySpan::Bold]);
    assert_eq!(kinds("/i/"), vec![BodySpan::Italic]);
    assert_eq!(kinds("=c="), vec![BodySpan::InlineCode]);
    assert_eq!(kinds("~v~"), vec![BodySpan::Verbatim]);
    assert_eq!(kinds("+s+"), vec![BodySpan::Strike]);
    assert_eq!(kinds("_u_"), vec![BodySpan::Underline]);
}

#[test]
fn prose_around_emphasis_stays_prose() {
    assert_eq!(
        kinds("a *b* c"),
        vec![BodySpan::Plain, BodySpan::Bold, BodySpan::Plain]
    );
    assert_verbatim("a *b* c");
}

#[test]
fn emphasis_and_links_share_a_line() {
    let ks = kinds("see *this* [[id:1][note]] now");
    assert!(ks.contains(&BodySpan::Bold));
    assert!(ks.contains(&BodySpan::Link));
    assert_verbatim("see *this* [[id:1][note]] now");
}

#[test]
fn a_link_wins_over_markup_inside_it() {
    // `[[https://x/a_b_c]]` must stay one link, not a link with an
    // underline run chewed out of its middle.
    let ks = kinds("[[https://x/a_b_c][l]]");
    assert_eq!(ks, vec![BodySpan::Link]);
    assert_verbatim("[[https://x/a_b_c][l]]");
}

#[test]
fn markup_never_reaches_a_headline_lookalike() {
    // `*bold*` is markup; the escape and the parser both agree, and so
    // must the renderer — a starred line is prose here, not a heading.
    assert_verbatim("*bold* opening the line");
}

#[test]
fn emphasis_carries_its_decoration() {
    assert!(span_decoration(BodySpan::Bold).bold);
    assert!(span_decoration(BodySpan::Italic).italic);
    assert!(span_decoration(BodySpan::Strike).strike);
    assert!(span_decoration(BodySpan::Underline).underline);
    let plain = span_decoration(BodySpan::Plain);
    assert!(!plain.bold && !plain.italic && !plain.strike && !plain.underline);
}

#[test]
fn emphasis_spans_are_contiguous_and_char_aligned() {
    let body = "ä *fett* ö /kursiv/";
    let line = &highlight_body(body)[0];
    let ranges = span_ranges(line);
    let mut at = 0usize;
    for (range, _) in &ranges {
        assert_eq!(range.start, at, "contiguous");
        assert!(body.is_char_boundary(range.start));
        assert!(body.is_char_boundary(range.end));
        at = range.end;
    }
    assert_eq!(at, body.len(), "covers the line");
}

// === block content ===
//
// Only `#+BEGIN_SRC` got its content classified. A quote, an example
// or an export block read as undifferentiated prose, so the one thing
// those blocks exist to say — this text is not mine / not prose — was
// exactly what the shell did not show.

#[test]
fn quote_block_content_is_marked_as_quoted() {
    let lines = highlight_body("#+BEGIN_QUOTE\nsaid someone\n#+END_QUOTE");
    assert_eq!(lines[0][0].0, BodySpan::Meta, "the delimiter is syntax");
    assert_eq!(lines[1][0].0, BodySpan::Quote);
    assert_eq!(lines[2][0].0, BodySpan::Meta);
}

#[test]
fn example_and_export_content_is_verbatim() {
    for (open, close) in [
        ("#+BEGIN_EXAMPLE", "#+END_EXAMPLE"),
        ("#+BEGIN_EXPORT html", "#+END_EXPORT"),
        ("#+BEGIN_COMMENT", "#+END_COMMENT"),
    ] {
        let body = format!("{open}\ncontent\n{close}");
        assert_eq!(
            highlight_body(&body)[1][0].0,
            BodySpan::Example,
            "{open} content"
        );
    }
}

#[test]
fn verse_and_center_read_as_quoted_prose() {
    for open in ["#+BEGIN_VERSE", "#+BEGIN_CENTER"] {
        let name = open.trim_start_matches("#+BEGIN_");
        let body = format!("{open}\nline\n#+END_{name}");
        assert_eq!(highlight_body(&body)[1][0].0, BodySpan::Quote, "{open}");
    }
}

#[test]
fn markup_inside_a_block_is_left_alone() {
    // The block's content is verbatim: `*x*` in an example block is
    // two stars and an x, not an emphasis run.
    let lines = highlight_body("#+BEGIN_EXAMPLE\n*x* /y/\n#+END_EXAMPLE");
    assert_eq!(lines[1].len(), 1, "one span, unsplit: {:?}", lines[1]);
    assert_verbatim("#+BEGIN_EXAMPLE\n*x* /y/\n#+END_EXAMPLE");
}

#[test]
fn a_block_only_closes_on_its_own_end() {
    let lines = highlight_body("#+BEGIN_QUOTE\na\n#+END_EXAMPLE\nb\n#+END_QUOTE");
    assert_eq!(lines[3][0].0, BodySpan::Quote, "still inside the quote");
}

#[test]
fn a_src_block_still_gets_its_language_highlighting() {
    let lines = highlight_body("#+BEGIN_SRC rust\nfn x() {}\n#+END_SRC");
    assert!(
        lines[1].iter().any(|(k, _)| *k == BodySpan::Keyword),
        "the keyword tier still runs: {:?}",
        lines[1]
    );
}

#[test]
fn an_unclosed_block_still_classifies_its_content() {
    // Half-typed is the normal state while writing one.
    let lines = highlight_body("#+BEGIN_QUOTE\nstill quoted");
    assert_eq!(lines[1][0].0, BodySpan::Quote);
}
