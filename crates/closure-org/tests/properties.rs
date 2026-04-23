//! Property tests for `closure-org`.
//!
//! Enforce the universal invariants from `docs/spec.md`:
//!
//! * I1 byte-exact roundtrip — holds for arbitrary input because the parser
//!   is source-preserving: every byte is owned by some span and the printer
//!   concatenates slices in order.
//! * I5 no panic on malformed or random input.
//! * I6 determinism — `parse` returns the same `OrgDoc` every time.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use proptest::prelude::*;

/// Random ASCII plus newline. Intentionally small alphabet, high density of
/// structural characters (`#`, `+`, `:`, `*`, ` `, `\n`) to maximise the
/// chance of hitting classification edge cases.
fn org_ish_string() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            Just('\n'),
            Just(' '),
            Just('\t'),
            Just('#'),
            Just('+'),
            Just(':'),
            Just('*'),
            Just('-'),
            Just('_'),
            Just('a'),
            Just('A'),
            Just('1'),
            any::<char>().prop_filter("printable ASCII", |c| c.is_ascii_graphic() || *c == ' '),
        ],
        0..256usize,
    )
    .prop_map(|v| v.into_iter().collect())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 2048,
        .. ProptestConfig::default()
    })]

    /// I1 byte-exact roundtrip on arbitrary input. If this ever fails, the
    /// parser has introduced a transformation the printer can't reverse.
    #[test]
    fn roundtrip_is_byte_exact_on_random_input(src in org_ish_string()) {
        let doc = closure_org::parse(&src).expect("parse is infallible");
        prop_assert_eq!(closure_org::print(&doc), src);
    }

    /// I6 determinism.
    #[test]
    fn parse_is_deterministic(src in org_ish_string()) {
        let a = closure_org::parse(&src).expect("parse is infallible");
        let b = closure_org::parse(&src).expect("parse is infallible");
        prop_assert_eq!(a, b);
    }

    /// I5 no panic on any input. Reaching this assertion for every input the
    /// proptest explores is the guarantee.
    #[test]
    fn parse_never_panics(src in org_ish_string()) {
        let _ = closure_org::parse(&src);
    }
}
