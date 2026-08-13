//! Documents that are legal org and awkward to parse.
//!
//! `closure-org` had 985 unexecuted lines, and a parser's uncovered
//! lines are its unusual branches — which are exactly the ones a real
//! vault eventually contains. Every document here roundtrips byte-exact
//! (I1) and is walked by every accessor that takes the whole document,
//! because I5 says the kernel does not panic and an accessor is where
//! that gets tested cheaply.
//!
//! Each entry is something somebody actually writes, not fuzz. A
//! parser that only meets tidy input is a parser that meets untidy
//! input in production.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::parse;

/// The corpus. Named so a failure says which shape broke.
const AWKWARD: &[(&str, &str)] = &[
    ("empty", ""),
    ("just a newline", "\n"),
    ("no trailing newline", "* A headline"),
    ("crlf", "* A headline\r\nbody\r\n"),
    ("bom", "\u{feff}* After a byte order mark\n"),
    ("tabs for indent", "* Head\n\t- an item\n\t\t- nested\n"),
    (
        "deep nesting",
        "* 1\n** 2\n*** 3\n**** 4\n***** 5\n****** 6\n******* 7\n",
    ),
    (
        "stars in a body",
        "* Head\nnot a headline: *bold* and 2 * 3\n",
    ),
    ("escaped headline", "* Head\n,* not a headline\n"),
    (
        "drawer never closed",
        "* Head\n:PROPERTIES:\n:ID: 01AWKWARD0000000001\n",
    ),
    ("block never closed", "* Head\n#+BEGIN_SRC sh\necho hi\n"),
    ("end without begin", "* Head\n#+END_SRC\n"),
    (
        "nested blocks",
        "* Head\n#+BEGIN_QUOTE\n#+BEGIN_SRC sh\necho\n#+END_SRC\n#+END_QUOTE\n",
    ),
    ("empty drawer", "* Head\n:PROPERTIES:\n:END:\n"),
    (
        "property with no value",
        "* Head\n:PROPERTIES:\n:KEY:\n:END:\n",
    ),
    (
        "colon in a value",
        "* Head\n:PROPERTIES:\n:URL: https://example.com:8080/x\n:END:\n",
    ),
    ("tags only", "* :justtags:\n"),
    ("todo with no title", "* TODO\n"),
    ("priority with no title", "* [#A]\n"),
    (
        "everything on one headline",
        "* TODO [#A] Title :a:b:\nSCHEDULED: <2026-01-01 Thu>\n",
    ),
    ("unicode title", "* Ünïcödé — 日本語 — emoji 🎉\n"),
    ("rtl text", "* עברית and العربية in one line\n"),
    ("very long line", "* Head\n"),
    ("table with no rows", "* Head\n|\n"),
    ("ragged table", "* Head\n| a | b |\n| c |\n| d | e | f |\n"),
    ("formula with no table", "* Head\n#+TBLFM: $2=$1*2\n"),
    ("list markers mixed", "* Head\n- a\n+ b\n* c\n1. d\n2) e\n"),
    ("checkbox states", "* Head\n- [ ] a\n- [X] b\n- [-] c\n"),
    ("footnote with no definition", "* Head\ntext[fn:nope]\n"),
    (
        "definition with no reference",
        "* Head\n[fn:orphan] the note\n",
    ),
    (
        "link with no description",
        "* Head\n[[https://example.com]]\n",
    ),
    (
        "link with brackets inside",
        "* Head\n[[https://example.com/a[b]c][desc]]\n",
    ),
    (
        "timestamp range and repeater",
        "* Head\nSCHEDULED: <2026-01-01 Thu 09:00-10:00 +1w -2d>\n",
    ),
    ("keyword with no value", "* Head\n#+TITLE:\n"),
    ("comment lines", "# a comment\n* Head\n# another\n"),
    ("only keywords", "#+TITLE: T\n#+AUTHOR: A\n"),
    (
        "windows path in a property",
        "* Head\n:PROPERTIES:\n:P: C:\\Users\\x\n:END:\n",
    ),
];

#[test]
fn every_awkward_document_roundtrips_byte_exact() {
    for (name, src) in AWKWARD {
        let doc = parse(src).unwrap_or_else(|e| panic!("`{name}` did not parse: {e}"));
        assert_eq!(doc.source(), *src, "`{name}` did not roundtrip");
    }
}

#[test]
fn a_very_long_line_roundtrips() {
    // Kept out of the table so the table stays readable.
    let src = format!("* Head\n{}\n", "x".repeat(100_000));
    let doc = parse(&src).expect("parses");
    assert_eq!(doc.source(), src);
}

#[test]
fn every_document_survives_being_walked() {
    // I5: no panics in the kernel. These are the accessors a shell
    // calls on whatever it just opened, so every one of them meets
    // every one of these shapes eventually.
    for (name, src) in AWKWARD {
        let doc = parse(src).unwrap_or_else(|e| panic!("`{name}`: {e}"));
        let _ = doc.headline_count();
        let _ = doc.link_target_count();
        let _ = doc.property_key_counts();
        let _ = doc.property_key_len_counts();
        let _ = doc.property_value_len_counts();
        let _ = doc.timestamp_count_counts();
        let _ = doc.preamble_kind_counts();
        let _ = doc.link_target_counts();
        let _ = doc.timestamped_pct();
        let _ = doc.property_pct();
        let _ = doc.property_key_diversity_pct();
        let _ = doc.source_byte_len();
        let _ = doc.source_line_count();
        let _ = doc.source_word_count();
        let _ = doc.unfinished_checkboxes();
        for h in doc.iter_headlines() {
            let _ = h.title();
            let _ = h.level();
            let _ = h.tags();
            let _ = h.todo();
            let _ = h.priority();
            let _ = h.properties();
        }
        // …and the free functions a shell runs over a body, on *this*
        // document. An earlier version looped over the whole corpus
        // here and broke after one iteration, so every one of these saw
        // only the empty string — coverage that looks broad and is not.
        {
            let s = src;
            let _ = closure_org::find_links(s);
            let _ = closure_org::find_timestamps(s);
            let _ = closure_org::find_footnotes(s);
            let _ = closure_org::find_cookies(s);
            let _ = closure_org::find_drawers(s);
            let _ = closure_org::find_macros(s);
            let _ = closure_org::find_markup(s);
            let _ = closure_org::fragments(s);
            let _ = closure_org::scripts(s);
            let _ = closure_org::description_items(s);
            let _ = closure_org::list_counter(s);
            let _ = closure_org::column_alignments(s);
            let _ = closure_org::clocktable_params(s);
            let _ = closure_org::document_properties(s);
            let _ = closure_org::link_abbreviations(s);
            let _ = closure_org::startup_of(s);
            let _ = closure_org::radio_targets(s);
            let _ = closure_org::table_formulas(s);
            let _ = closure_org::clock_entries(s);
        }
    }
}

#[test]
fn a_parsed_document_prints_back_through_the_public_printer() {
    // `print` is the other half of I1 and is not the same code path as
    // `source()` — one re-serialises, the other returns what was read.
    for (name, src) in AWKWARD {
        let doc = parse(src).unwrap_or_else(|e| panic!("`{name}`: {e}"));
        assert_eq!(
            closure_org::print(&doc),
            *src,
            "`{name}` printed differently"
        );
    }
}
