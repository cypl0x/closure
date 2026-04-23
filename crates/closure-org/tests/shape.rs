//! AST shape tests for headline parsing.
//!
//! These assert structural properties of the parse tree that the
//! roundtrip test cannot: that a line starting with `* ` yields a
//! [`Headline`] in `roots`, not a paragraph in `preamble`. Byte-exact
//! roundtrip is checked in `roundtrip.rs`; this file checks semantic
//! classification.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_org::{NodeKind, parse};

#[test]
fn simple_heading_is_a_headline_not_a_paragraph() {
    let doc = parse("* Hello\n").expect("parse");
    assert!(
        doc.preamble().is_empty(),
        "heading must not land in preamble"
    );
    assert_eq!(doc.roots().len(), 1);
    let h = &doc.roots()[0];
    assert_eq!(h.level(), 1);
    assert_eq!(h.title(), "Hello");
}

#[test]
fn sibling_headings_all_become_roots() {
    let doc = parse("* First\n* Second\n* Third\n").expect("parse");
    assert_eq!(doc.roots().len(), 3);
    assert_eq!(doc.roots()[0].title(), "First");
    assert_eq!(doc.roots()[1].title(), "Second");
    assert_eq!(doc.roots()[2].title(), "Third");
}

#[test]
fn empty_title_heading_stars_plus_newline() {
    let doc = parse("*\n").expect("parse");
    assert_eq!(doc.roots().len(), 1);
    assert_eq!(doc.roots()[0].level(), 1);
    assert_eq!(doc.roots()[0].title(), "");
}

#[test]
fn empty_title_level_two() {
    let doc = parse("**\n").expect("parse");
    assert_eq!(doc.roots().len(), 1);
    assert_eq!(doc.roots()[0].level(), 2);
    assert_eq!(doc.roots()[0].title(), "");
}

#[test]
fn stars_followed_by_non_space_non_eol_is_paragraph() {
    let doc = parse("**foo\n").expect("parse");
    assert!(doc.roots().is_empty());
    assert_eq!(doc.preamble().len(), 1);
    assert_eq!(doc.preamble()[0].kind(), NodeKind::Paragraph);
}

#[test]
fn preamble_before_first_heading_preserved() {
    let doc = parse("#+TITLE: x\n\n* Heading\n").expect("parse");
    assert_eq!(doc.preamble().len(), 2);
    assert_eq!(doc.preamble()[0].kind(), NodeKind::Keyword);
    assert_eq!(doc.preamble()[1].kind(), NodeKind::BlankLine);
    assert_eq!(doc.roots().len(), 1);
    assert_eq!(doc.roots()[0].title(), "Heading");
}

#[test]
fn body_lines_after_heading_become_headline_body() {
    let doc = parse("* Hello\nbody line\n").expect("parse");
    assert_eq!(doc.roots().len(), 1);
    let h = &doc.roots()[0];
    assert_eq!(h.title(), "Hello");
    assert_eq!(h.body().len(), 1);
    assert_eq!(h.body()[0].kind(), NodeKind::Paragraph);
}

#[test]
fn headline_title_excludes_stars_and_leading_space() {
    let doc = parse("*** Three stars title\n").expect("parse");
    assert_eq!(doc.roots().len(), 1);
    let h = &doc.roots()[0];
    assert_eq!(h.level(), 3);
    assert_eq!(h.title(), "Three stars title");
}

#[test]
fn nested_heading_becomes_child() {
    let doc = parse("* Parent\n** Child\n").expect("parse");
    assert_eq!(doc.roots().len(), 1);
    let parent = &doc.roots()[0];
    assert_eq!(parent.title(), "Parent");
    assert_eq!(parent.children().len(), 1);
    assert_eq!(parent.children()[0].title(), "Child");
    assert_eq!(parent.children()[0].level(), 2);
}

#[test]
fn three_level_deep_nesting() {
    let doc = parse("* Parent\n** Child\n*** Grandchild\n").expect("parse");
    assert_eq!(doc.roots().len(), 1);
    let p = &doc.roots()[0];
    assert_eq!(p.children().len(), 1);
    let c = &p.children()[0];
    assert_eq!(c.children().len(), 1);
    assert_eq!(c.children()[0].title(), "Grandchild");
    assert_eq!(c.children()[0].level(), 3);
}

#[test]
fn mixed_levels_build_correct_tree() {
    let src = "* A\n** A.1\n** A.2\n*** A.2.a\n* B\n** B.1\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.roots().len(), 2);
    let a = &doc.roots()[0];
    assert_eq!(a.title(), "A");
    assert_eq!(a.children().len(), 2);
    assert_eq!(a.children()[0].title(), "A.1");
    assert_eq!(a.children()[1].title(), "A.2");
    assert_eq!(a.children()[1].children().len(), 1);
    assert_eq!(a.children()[1].children()[0].title(), "A.2.a");
    let b = &doc.roots()[1];
    assert_eq!(b.title(), "B");
    assert_eq!(b.children().len(), 1);
    assert_eq!(b.children()[0].title(), "B.1");
}

#[test]
fn level_jump_descends_to_nearest_parent() {
    // * One → root
    // *** Three skipping two → child of One (nearest lower-level ancestor)
    // ** Back to two → sibling of Three under One
    let src = "* One\n*** Three skipping two\n** Back to two\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.roots().len(), 1);
    let one = &doc.roots()[0];
    assert_eq!(one.children().len(), 2);
    assert_eq!(one.children()[0].title(), "Three skipping two");
    assert_eq!(one.children()[0].level(), 3);
    assert_eq!(one.children()[1].title(), "Back to two");
    assert_eq!(one.children()[1].level(), 2);
}

#[test]
fn sibling_at_same_level_does_not_nest() {
    let doc = parse("* First\n* Second\n").expect("parse");
    assert_eq!(doc.roots().len(), 2);
    assert!(doc.roots()[0].children().is_empty());
    assert!(doc.roots()[1].children().is_empty());
}

#[test]
fn body_lines_attach_to_preceding_heading_even_under_nesting() {
    let src = "* Parent\nparent body\n** Child\nchild body\n";
    let doc = parse(src).expect("parse");
    let parent = &doc.roots()[0];
    assert_eq!(parent.body().len(), 1);
    assert_eq!(parent.body()[0].source(), "parent body\n");
    let child = &parent.children()[0];
    assert_eq!(child.body().len(), 1);
    assert_eq!(child.body()[0].source(), "child body\n");
}

#[test]
fn todo_keyword_extracted() {
    let doc = parse("* TODO Fix bug\n").expect("parse");
    let h = &doc.roots()[0];
    assert_eq!(h.todo(), Some("TODO"));
    assert_eq!(h.title(), "Fix bug");
}

#[test]
fn done_keyword_extracted() {
    let doc = parse("* DONE Ship feature\n").expect("parse");
    let h = &doc.roots()[0];
    assert_eq!(h.todo(), Some("DONE"));
    assert_eq!(h.title(), "Ship feature");
}

#[test]
fn non_keyword_starting_word_is_title() {
    let doc = parse("* Hello World\n").expect("parse");
    let h = &doc.roots()[0];
    assert_eq!(h.todo(), None);
    assert_eq!(h.title(), "Hello World");
}

#[test]
fn priority_extracted() {
    let doc = parse("* [#A] Urgent\n").expect("parse");
    let h = &doc.roots()[0];
    assert_eq!(h.priority(), Some('A'));
    assert_eq!(h.title(), "Urgent");
}

#[test]
fn todo_and_priority_together() {
    let doc = parse("* TODO [#B] Review PR\n").expect("parse");
    let h = &doc.roots()[0];
    assert_eq!(h.todo(), Some("TODO"));
    assert_eq!(h.priority(), Some('B'));
    assert_eq!(h.title(), "Review PR");
}

#[test]
fn tags_extracted() {
    let doc = parse("* Task :work:urgent:\n").expect("parse");
    let h = &doc.roots()[0];
    assert_eq!(h.tags(), vec!["work", "urgent"]);
    assert_eq!(h.title(), "Task");
}

#[test]
fn todo_priority_title_and_tags_all_together() {
    let doc = parse("* TODO [#A] Urgent task :work:urgent:\n").expect("parse");
    let h = &doc.roots()[0];
    assert_eq!(h.todo(), Some("TODO"));
    assert_eq!(h.priority(), Some('A'));
    assert_eq!(h.title(), "Urgent task");
    assert_eq!(h.tags(), vec!["work", "urgent"]);
}

#[test]
fn priority_without_todo_with_tags() {
    let doc = parse("* [#A] Urgent task :work:urgent:\n").expect("parse");
    let h = &doc.roots()[0];
    assert_eq!(h.todo(), None);
    assert_eq!(h.priority(), Some('A'));
    assert_eq!(h.title(), "Urgent task");
    assert_eq!(h.tags(), vec!["work", "urgent"]);
}

#[test]
fn no_tags_is_empty_vec() {
    let doc = parse("* Hello\n").expect("parse");
    assert!(doc.roots()[0].tags().is_empty());
}

#[test]
fn property_drawer_parsed() {
    let src = "* H\n:PROPERTIES:\n:ID: abc123\n:CUSTOM: xyz\n:END:\n";
    let doc = parse(src).expect("parse");
    let h = &doc.roots()[0];
    let p = h.properties().expect("properties present");
    assert_eq!(p.get("ID"), Some("abc123"));
    assert_eq!(p.get("CUSTOM"), Some("xyz"));
    assert_eq!(p.id(), Some("abc123"));
    assert_eq!(p.len(), 2);
}

#[test]
fn no_property_drawer_returns_none() {
    let doc = parse("* H\nbody\n").expect("parse");
    assert!(doc.roots()[0].properties().is_none());
}

#[test]
fn property_drawer_not_in_headline_body() {
    // The drawer lines must not also appear in body nodes.
    let src = "* H\n:PROPERTIES:\n:ID: x\n:END:\nbody\n";
    let doc = parse(src).expect("parse");
    let h = &doc.roots()[0];
    assert!(h.properties().is_some());
    // Body should only contain the "body\n" paragraph, not the drawer.
    let body_sources: String = h.body().iter().map(closure_org::Node::source).collect();
    assert_eq!(body_sources, "body\n");
}

#[test]
fn code_block_recognised_as_single_node() {
    let src = "#+BEGIN_SRC rust\nfn main() {}\n#+END_SRC\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.preamble().len(), 1);
    let n = &doc.preamble()[0];
    assert_eq!(n.kind(), NodeKind::CodeBlock);
    let cb = n.as_code_block().expect("code block");
    assert_eq!(cb.language, Some("rust"));
    assert_eq!(cb.content, "fn main() {}\n");
}

#[test]
fn code_block_lowercase_directives_accepted() {
    let src = "#+begin_src python :results output\nprint(\"hi\")\n#+end_src\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.preamble().len(), 1);
    let cb = doc.preamble()[0].as_code_block().expect("code block");
    assert_eq!(cb.language, Some("python"));
    assert_eq!(cb.args, Some(":results output"));
    assert_eq!(cb.content, "print(\"hi\")\n");
}

#[test]
fn code_block_no_language() {
    let src = "#+BEGIN_SRC\nline one\nline two\n#+END_SRC\n";
    let doc = parse(src).expect("parse");
    let cb = doc.preamble()[0].as_code_block().expect("code block");
    assert_eq!(cb.language, None);
    assert_eq!(cb.args, None);
    assert_eq!(cb.content, "line one\nline two\n");
}

#[test]
fn code_block_content_preserves_internals_verbatim() {
    // Inside a code block, `* stars` must not be parsed as a heading.
    let src = "#+BEGIN_SRC org\n* not a heading\n#+END_SRC\n";
    let doc = parse(src).expect("parse");
    assert!(doc.roots().is_empty());
    assert_eq!(doc.preamble().len(), 1);
    let cb = doc.preamble()[0].as_code_block().expect("code block");
    assert_eq!(cb.content, "* not a heading\n");
}

#[test]
fn unclosed_code_block_falls_back_to_keyword_line() {
    // No #+END_SRC anywhere. The #+BEGIN_SRC line is just a keyword
    // and the remainder are paragraph lines.
    let src = "#+BEGIN_SRC rust\nfn main() {}\n";
    let doc = parse(src).expect("parse");
    assert!(
        doc.preamble()
            .iter()
            .all(|n| n.kind() != NodeKind::CodeBlock),
        "no CodeBlock when unclosed"
    );
}
