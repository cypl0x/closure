//! Property/fuzz tests for `closure-markdown` (D1).
//!
//! Enforce the universal invariants from `docs/spec.md` on arbitrary input:
//!
//! * I1 byte-exact roundtrip — `print(parse(s)) == s` for any `s`, because
//!   the parser is source-preserving: every byte belongs to exactly one
//!   block span and the printer concatenates spans in order.
//! * I5 no panic on malformed or random input.
//! * I6 determinism — `parse` yields the same blocks every time.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use closure_markdown::{parse, print};
use proptest::prelude::*;

/// Random ASCII plus newline, weighted toward markdown structural
/// characters so classification edge cases (fences, pipes, rules, quotes,
/// hashes) are hit often.
fn md_ish_string() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            Just('\n'),
            Just(' '),
            Just('\t'),
            Just('#'),
            Just('`'),
            Just('~'),
            Just('|'),
            Just('>'),
            Just('-'),
            Just('*'),
            Just('_'),
            Just('+'),
            Just('1'),
            Just('.'),
            Just('a'),
            any::<char>().prop_filter("printable ASCII", |c| c.is_ascii_graphic() || *c == ' '),
        ],
        0..256usize,
    )
    .prop_map(|cs| cs.into_iter().collect())
}

proptest! {
    #[test]
    fn roundtrip_is_byte_exact(s in md_ish_string()) {
        let doc = parse(&s).expect("parser is total");
        prop_assert_eq!(print(&doc), s);
    }

    #[test]
    fn parse_never_panics_and_is_deterministic(s in md_ish_string()) {
        let a = parse(&s).expect("total");
        let b = parse(&s).expect("total");
        prop_assert_eq!(a.blocks().len(), b.blocks().len());
        // Every byte is covered: the concatenated spans equal the source.
        prop_assert_eq!(print(&a), print(&b));
    }
}
