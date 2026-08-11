//! Entities and LaTeX fragments: `\alpha`, `$x^2$`, `\(a\)`, `\[b\]`.
//!
//! Both were preserved and neither was understood — the bytes came
//! back and nothing knew they were anything. A pane rendering a note
//! about `\alpha` showed the backslash, a search for "α" found
//! nothing, and an exporter had no way to ask.
//!
//! Two constructs, one commit, because they are the same question
//! asked twice: which run of characters in this line is maths or a
//! named character rather than prose. What closure does with the
//! answer — render the glyph, hand it to an exporter — is a shell's
//! business; knowing where they are is the parser's.
//!
//! Deliberately not an evaluator. `$x^2$` is a fragment with a body,
//! not a formula to compute, and org does not compute it either.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{Fragment, FragmentKind, entity_char, fragments};

#[test]
fn a_named_entity_is_found_and_named() {
    let found = fragments("the \\alpha particle");
    assert_eq!(
        found,
        vec![Fragment {
            range: 4..10,
            kind: FragmentKind::Entity("alpha".to_owned()),
        }]
    );
}

#[test]
fn a_known_entity_has_a_character_behind_it() {
    assert_eq!(entity_char("alpha"), Some("α"));
    assert_eq!(entity_char("Alpha"), Some("Α"));
    assert_eq!(entity_char("hellip"), Some("…"));
    assert_eq!(entity_char("rightarrow"), Some("→"));
}

#[test]
fn an_entity_nobody_has_heard_of_is_still_a_fragment() {
    // Org has hundreds and closure will not carry all of them. Finding
    // it and having no glyph is a better answer than not finding it:
    // the shell can leave the source alone and say why.
    let found = fragments("\\nosuchentity here");
    assert_eq!(found.len(), 1);
    assert_eq!(entity_char("nosuchentity"), None);
}

#[test]
fn dollar_maths_is_a_fragment() {
    let found = fragments("before $x^2$ after");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, FragmentKind::Latex("x^2".to_owned()));
    assert_eq!(&"before $x^2$ after"[found[0].range.clone()], "$x^2$");
}

#[test]
fn both_bracket_forms_are_fragments() {
    let inline = fragments(r"see \(a + b\) here");
    assert_eq!(inline.len(), 1, "{inline:?}");
    assert_eq!(inline[0].kind, FragmentKind::Latex("a + b".to_owned()));
    let display = fragments(r"\[E = mc^2\]");
    assert_eq!(display.len(), 1, "{display:?}");
    assert_eq!(display[0].kind, FragmentKind::Latex("E = mc^2".to_owned()));
}

#[test]
fn a_lone_dollar_is_not_maths() {
    // "it cost $5" is prose, and treating it as an unterminated
    // fragment would swallow the rest of the line.
    assert!(fragments("it cost $5 and change").is_empty());
}

#[test]
fn a_price_pair_is_not_maths_either() {
    // `$5 and $6` has two dollars and is still prose. Org's own rule is
    // that the character after the opening `$` may not be whitespace
    // and the one before the closing `$` may not be either — which is
    // exactly what tells maths from money.
    let found = fragments("between $5 and $6");
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_fragment_inside_a_word_is_not_one() {
    // `a$b$c` is not maths in org, and a parser that says it is will
    // find fragments in every URL with a dollar in it.
    assert!(fragments("a$b$c").is_empty());
}

#[test]
fn several_in_one_line_come_back_in_order() {
    let line = r"\alpha then $y$ then \(z\)";
    let found = fragments(line);
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found.windows(2).all(|w| w[0].range.end <= w[1].range.start));
}

#[test]
fn finding_them_does_not_change_the_line() {
    // I1's shape: this reads, it never rewrites. Whatever a shell does
    // with a fragment, the file still says what it said.
    let line = r"the \alpha and $x^2$";
    let before = line.to_owned();
    let _ = fragments(line);
    assert_eq!(line, before);
}
