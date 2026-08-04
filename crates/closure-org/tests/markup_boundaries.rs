//! "link content get's italized (sometimes) … Just happens with the
//! melpa.org. Strange behavior."
//!
//! `https://melpa.org/#/evil-ghostel` came out with `melpa.org` in
//! italics, and `https://github.com/dakra/ghostel` on the line above
//! did not. Nothing to do with melpa: the emphasis scanner opened a
//! run at the second `/` of `https://` and closed it at the `/` after
//! `.org`, because it accepted *any* non-word byte in front of a
//! marker. The github URL has an odd number of slashes in the wrong
//! places to pair up, so it escaped — which is what makes the bug look
//! like it is about one site.
//!
//! Org's own rule is narrower. `org-emphasis-regexp-components`
//! defaults to
//!
//!     pre:  whitespace, and one of  - – — ( ' " {
//!     post: whitespace, and one of  - – — . , : ! ? ; ' " ) } [
//!
//! so a marker after a `/` or a `:` or a letter is not a marker at all.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{MarkupKind, markup_spans};

fn kinds(text: &str) -> Vec<(String, MarkupKind)> {
    markup_spans(text)
        .into_iter()
        .map(|(r, k)| (text[r].to_owned(), k))
        .collect()
}

#[test]
fn a_url_is_not_emphasis() {
    assert_eq!(kinds("https://melpa.org/#/evil-ghostel"), vec![]);
    assert_eq!(kinds("https://github.com/dakra/ghostel"), vec![]);
    assert_eq!(kinds("see https://melpa.org/#/evil-ghostel today"), vec![]);
}

#[test]
fn a_path_is_not_emphasis_either() {
    assert_eq!(kinds("/usr/share/doc"), vec![]);
    assert_eq!(kinds("run ~/.config/doom/config.el"), vec![]);
}

#[test]
fn org_emphasis_still_works() {
    assert_eq!(
        kinds("this is *bold* here"),
        vec![("*bold*".to_owned(), MarkupKind::Bold)]
    );
    assert_eq!(
        kinds("/italic/ at the start"),
        vec![("/italic/".to_owned(), MarkupKind::Italic)]
    );
    assert_eq!(
        kinds("ends with =code="),
        vec![("=code=".to_owned(), MarkupKind::Code)]
    );
}

#[test]
fn the_pre_and_post_characters_org_allows() {
    // Brackets and quotes around emphasis are the common case, and the
    // ones a stricter rule would break if it only allowed whitespace.
    for text in [
        "(/italic/)",
        "\"*bold*\"",
        "{~verbatim~}",
        "a -*bold*-",
        "*bold*, then",
        "*bold*: then",
        "*bold*; then",
        "*bold*!",
    ] {
        assert_eq!(markup_spans(text).len(), 1, "{text}");
    }
}

#[test]
fn a_marker_glued_to_a_word_is_not_a_marker() {
    // Org's border rule: `a*b*c` is arithmetic, not emphasis.
    assert_eq!(kinds("a*b*c"), vec![]);
    assert_eq!(kinds("2*3*4"), vec![]);
}

#[test]
fn the_whitespace_border_still_holds() {
    // `* not emphasis *` — a marker with a space just inside it opens
    // nothing, which is also what keeps a headline's stars alone.
    assert_eq!(kinds("* not emphasis *"), vec![]);
    assert_eq!(kinds("half open *text"), vec![]);
}
