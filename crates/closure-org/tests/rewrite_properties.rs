//! "The `rewrite_*` family isn't fuzzed. ~30 functions doing span
//! arithmetic on the source, covered only by example tests. `parse`
//! gets 2048 proptest cases plus a full-byte-range fuzzer; the
//! rewrites — where the actual byte-offset math lives — get none. My
//! manual UTF-8 probe passed, which is encouraging, but that's five
//! inputs against a combinatorial space. Also worth noting: the
//! proptest alphabet is ASCII-only (`is_ascii_graphic`), so multibyte
//! only gets explored via `fuzz_replay`."
//!
//! Reported 2026-08-04 in the Opus 5 review. So the rewrites get the
//! same treatment the parser has, over an alphabet that is *not*
//! ASCII-only: German umlauts, Japanese, emoji, a combining diacritic
//! and a zero-width space, because a byte offset that lands mid-rune
//! panics and only multibyte input can find it.
//!
//! What every rewrite has to hold, whatever it is handed:
//!
//! * it does not panic (I5);
//! * what comes back is org that parses (I1's precondition);
//! * and it round-trips byte-exact, so the next rewrite starts from
//!   something the printer can reproduce.

#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use closure_org::{
    parse, print, rewrite_add_child_with_id, rewrite_add_sibling_after_with_id,
    rewrite_add_sibling_before_with_id, rewrite_headline_demote, rewrite_headline_ensure_id,
    rewrite_headline_promote, rewrite_headline_set_body, rewrite_headline_set_priority,
    rewrite_headline_set_property, rewrite_headline_set_tags, rewrite_headline_set_todo,
    rewrite_headline_title, rewrite_remove_subtree,
};
use proptest::prelude::*;

/// Characters chosen to break byte arithmetic: two-byte, three-byte and
/// four-byte runes, a combining mark that follows another character, a
/// zero-width space, plus org's own structural bytes.
fn wild_char() -> impl Strategy<Value = char> {
    prop_oneof![
        Just('\n'),
        Just(' '),
        Just('*'),
        Just(':'),
        Just('#'),
        Just('+'),
        Just('['),
        Just(']'),
        Just('a'),
        Just('ä'),        // 2 bytes
        Just('ß'),        // 2 bytes
        Just('日'),       // 3 bytes
        Just('本'),       // 3 bytes
        Just('🦀'),       // 4 bytes
        Just('\u{0301}'), // combining acute, follows the previous char
        Just('\u{200b}'), // zero-width space
    ]
}

fn wild_string(max: usize) -> impl Strategy<Value = String> {
    proptest::collection::vec(wild_char(), 0..max).prop_map(|v| v.into_iter().collect())
}

/// A document with at least one headline, so the rewrites have
/// something to address, and arbitrary bytes around it.
fn wild_doc() -> impl Strategy<Value = String> {
    (wild_string(48), wild_string(48), wild_string(48)).prop_map(|(pre, title, body)| {
        format!("{pre}\n* {}\n{body}\n** child\n", title.replace('\n', " "))
    })
}

/// Every path in `src`, so a property can address a headline that is
/// really there.
fn paths(src: &str) -> Vec<Vec<usize>> {
    let doc = parse(src).expect("parse is infallible");
    let mut out = Vec::new();
    for (i, root) in doc.roots().iter().enumerate() {
        out.push(vec![i]);
        for j in 0..root.children().len() {
            out.push(vec![i, j]);
        }
    }
    out
}

/// What every rewrite owes: org back, and org that survives a round
/// trip.
fn holds(before: &str, after: &closure_org::OrgDoc) {
    let printed = print(after);
    let reparsed = parse(&printed).expect("what a rewrite returns is still org");
    assert_eq!(
        print(&reparsed),
        printed,
        "a rewrite produced source that does not round-trip.\nbefore:\n{before:?}"
    );
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, .. ProptestConfig::default() })]

    #[test]
    fn setting_a_title_holds(src in wild_doc(), title in wild_string(24)) {
        let doc = parse(&src).expect("parse");
        let title = title.replace('\n', " ");
        for path in paths(&src) {
            if let Ok(out) = rewrite_headline_title(&doc, &path, &title) {
                holds(&src, &out);
            }
        }
    }

    #[test]
    fn setting_a_body_holds(src in wild_doc(), body in wild_string(48)) {
        let doc = parse(&src).expect("parse");
        for path in paths(&src) {
            if let Ok(out) = rewrite_headline_set_body(&doc, &path, &body) {
                holds(&src, &out);
            }
        }
    }

    #[test]
    fn keywords_tags_and_priorities_hold(
        src in wild_doc(),
        todo in wild_string(8),
        tag in wild_string(8),
    ) {
        let doc = parse(&src).expect("parse");
        let todo = todo.replace(['\n', ' '], "");
        let tag = tag.replace(['\n', ' ', ':'], "");
        for path in paths(&src) {
            if let Ok(out) = rewrite_headline_set_todo(&doc, &path, Some(&todo)) {
                holds(&src, &out);
            }
            if let Ok(out) = rewrite_headline_set_todo(&doc, &path, None) {
                holds(&src, &out);
            }
            if let Ok(out) = rewrite_headline_set_tags(&doc, &path, &[tag.as_str()]) {
                holds(&src, &out);
            }
            if let Ok(out) = rewrite_headline_set_priority(&doc, &path, Some('A')) {
                holds(&src, &out);
            }
        }
    }

    #[test]
    fn properties_and_ids_hold(src in wild_doc(), key in wild_string(8), value in wild_string(16)) {
        let doc = parse(&src).expect("parse");
        let key = key.replace(['\n', ':', ' '], "");
        let value = value.replace('\n', " ");
        for path in paths(&src) {
            if !key.is_empty()
                && let Ok(out) = rewrite_headline_set_property(&doc, &path, &key, &value) {
                holds(&src, &out);
            }
            if let Ok(out) = rewrite_headline_ensure_id(&doc, &path, "01PROP00000000000000000000") {
                holds(&src, &out);
            }
        }
    }

    #[test]
    fn the_structural_rewrites_hold(src in wild_doc(), title in wild_string(16)) {
        let doc = parse(&src).expect("parse");
        let title = title.replace('\n', " ");
        for path in paths(&src) {
            if let Ok(out) = rewrite_headline_promote(&doc, &path) {
                holds(&src, &out);
            }
            if let Ok(out) = rewrite_headline_demote(&doc, &path) {
                holds(&src, &out);
            }
            if let Ok(out) = rewrite_remove_subtree(&doc, &path) {
                holds(&src, &out);
            }
            let id = "01ADD00000000000000000000A";
            if let Ok(out) = rewrite_add_sibling_after_with_id(&doc, &path, &title, id) {
                holds(&src, &out);
            }
            if let Ok(out) = rewrite_add_sibling_before_with_id(&doc, &path, &title, id) {
                holds(&src, &out);
            }
            if let Ok(out) = rewrite_add_child_with_id(&doc, &path, &title, id) {
                holds(&src, &out);
            }
        }
    }

    /// Rewrites compose: the output of one is the input of the next,
    /// and nothing in the chain may panic or stop being org. This is
    /// the case example tests never reach, because they each start
    /// from a hand-written document.
    #[test]
    fn a_chain_of_rewrites_holds(src in wild_doc(), title in wild_string(16)) {
        let mut doc = parse(&src).expect("parse");
        let title = title.replace('\n', " ");
        for _ in 0..4 {
            let Some(path) = paths(&print(&doc)).into_iter().next() else {
                break;
            };
            if let Ok(next) = rewrite_headline_title(&doc, &path, &title) {
                doc = next;
            }
            if let Ok(next) = rewrite_headline_set_todo(&doc, &path, Some("TODO")) {
                doc = next;
            }
            if let Ok(next) = rewrite_headline_demote(&doc, &path) {
                doc = next;
            }
            holds(&src, &doc);
        }
    }
}

#[test]
fn a_hash_plus_line_of_multibyte_text_does_not_panic_the_parser() {
    // The first thing the properties above found, reduced. `#+` was
    // followed by an unchecked nine-byte slice looking for `begin_src`,
    // and nine bytes of `ä[🦀本` ends inside the `本`. Any org file with
    // a `#+` line starting in non-ASCII took the parser down — I5, in
    // the one crate the whole system is built on.
    for src in [
        "#+ä[🦀本\n",
        "#+日本語のキーワード\n",
        "#+🦀🦀🦀🦀🦀\n",
        "#+ä\n",
        "#+\n",
        "#+begin_src shell\necho ok\n#+end_src\n",
    ] {
        let doc = parse(src).expect("parse is infallible");
        assert_eq!(print(&doc), src, "roundtrip: {src:?}");
    }
}
