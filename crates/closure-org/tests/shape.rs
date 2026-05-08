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

#[test]
fn link_with_description_parsed() {
    let links = closure_org::find_links("Visit [[https://example.com][example]]!");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, "https://example.com");
    assert_eq!(links[0].description, Some("example"));
}

#[test]
fn link_without_description_parsed() {
    let links = closure_org::find_links("See [[wiki:Home]] soon.");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, "wiki:Home");
    assert_eq!(links[0].description, None);
}

#[test]
fn multiple_links_in_text() {
    let links = closure_org::find_links("[[a]] and [[b][B]] and [[c]]");
    assert_eq!(links.len(), 3);
    assert_eq!(links[0].target, "a");
    assert_eq!(links[1].target, "b");
    assert_eq!(links[1].description, Some("B"));
    assert_eq!(links[2].target, "c");
}

#[test]
fn unterminated_link_is_ignored() {
    let links = closure_org::find_links("broken [[unfinished text");
    assert!(links.is_empty());
}

#[test]
fn attach_results_to_first_code_block() {
    let src = "#+BEGIN_SRC sh\necho hi\n#+END_SRC\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_attach_results_to_code_block(&doc, 0, "hi\n").expect("attach");
    let out = closure_org::print(&new);
    assert!(out.contains("#+RESULTS:"));
    assert!(out.contains(": hi"));
}

#[test]
fn cookies_count_and_percent() {
    let cookies = closure_org::find_cookies("Tasks [1/3] and progress [50%]");
    assert_eq!(cookies.len(), 2);
    assert_eq!(
        cookies[0],
        closure_org::CookieView::Count { done: 1, total: 3 }
    );
    assert_eq!(cookies[1], closure_org::CookieView::Percent(50));
}

#[test]
fn cookies_ignore_non_numeric_brackets() {
    let cookies = closure_org::find_cookies("[#A] is priority, not a cookie");
    assert!(cookies.is_empty());
}

#[test]
fn rewrite_code_block_content_swaps_body() {
    let src = "#+BEGIN_SRC sh\necho old\n#+END_SRC\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_code_block_content(&doc, 0, "echo new\n").expect("rewrite");
    let out = closure_org::print(&new);
    assert!(out.contains("echo new"));
    assert!(!out.contains("echo old"));
    assert!(out.contains("#+BEGIN_SRC sh"));
    assert!(out.contains("#+END_SRC"));
}

#[test]
fn attach_results_replaces_existing_block() {
    let src = "#+BEGIN_SRC sh\necho hi\n#+END_SRC\n#+RESULTS:\n: old\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_attach_results_to_code_block(&doc, 0, "new\n").expect("attach");
    let out = closure_org::print(&new);
    assert!(out.contains(": new"));
    assert!(!out.contains(": old"));
}

#[test]
fn unordered_list_items_classified() {
    let src = "- first\n- second\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.preamble().len(), 2);
    assert_eq!(doc.preamble()[0].kind(), NodeKind::ListItem);
    let li = doc.preamble()[0].as_list_item().expect("list item");
    assert_eq!(li.marker, closure_org::ListMarker::Dash);
    assert_eq!(li.content, "first");
    assert!(li.checkbox.is_none());
}

#[test]
fn plus_marker_list() {
    let doc = parse("+ hi\n").expect("parse");
    let li = doc.preamble()[0].as_list_item().expect("list item");
    assert_eq!(li.marker, closure_org::ListMarker::Plus);
    assert_eq!(li.content, "hi");
}

#[test]
fn ordered_list_dot_marker() {
    let doc = parse("1. a\n2. b\n").expect("parse");
    let li = doc.preamble()[0].as_list_item().expect("list item");
    assert_eq!(li.marker, closure_org::ListMarker::OrderedDot);
    assert_eq!(li.content, "a");
}

#[test]
fn ordered_list_paren_marker() {
    let doc = parse("1) a\n").expect("parse");
    let li = doc.preamble()[0].as_list_item().expect("list item");
    assert_eq!(li.marker, closure_org::ListMarker::OrderedParen);
}

#[test]
fn checkbox_unchecked_checked_partial() {
    let doc = parse("- [ ] todo\n- [X] done\n- [-] partial\n").expect("parse");
    assert_eq!(
        doc.preamble()[0].as_list_item().expect("li").checkbox,
        Some(closure_org::Checkbox::Unchecked)
    );
    assert_eq!(
        doc.preamble()[1].as_list_item().expect("li").checkbox,
        Some(closure_org::Checkbox::Checked)
    );
    assert_eq!(
        doc.preamble()[2].as_list_item().expect("li").checkbox,
        Some(closure_org::Checkbox::Partial)
    );
    assert_eq!(
        doc.preamble()[0].as_list_item().expect("li").content,
        "todo"
    );
}

#[test]
fn indented_list_item_tracks_indent() {
    let doc = parse("  - nested\n").expect("parse");
    let li = doc.preamble()[0].as_list_item().expect("li");
    assert_eq!(li.indent, 2);
    assert_eq!(li.content, "nested");
}

#[test]
fn non_list_dash_not_classified() {
    let doc = parse("-not a list\n").expect("parse");
    assert_eq!(doc.preamble()[0].kind(), NodeKind::Paragraph);
}

#[test]
fn table_row_classified() {
    let doc = parse("| a | b |\n| c | d |\n").expect("parse");
    assert_eq!(doc.preamble().len(), 2);
    assert_eq!(doc.preamble()[0].kind(), NodeKind::TableRow);
    assert_eq!(doc.preamble()[1].kind(), NodeKind::TableRow);
    assert_eq!(doc.preamble()[0].source(), "| a | b |\n");
}

#[test]
fn table_separator_row_is_also_table_row() {
    let doc = parse("|---|---|\n").expect("parse");
    assert_eq!(doc.preamble()[0].kind(), NodeKind::TableRow);
}

#[test]
fn active_timestamp_found() {
    let ts = closure_org::find_timestamps("Meeting at <2026-05-01 Fri 14:30>.");
    assert_eq!(ts.len(), 1);
    assert!(ts[0].active);
    assert_eq!(ts[0].content, "2026-05-01 Fri 14:30");
}

#[test]
fn inactive_timestamp_found() {
    let ts = closure_org::find_timestamps("Due [2026-05-15].");
    assert_eq!(ts.len(), 1);
    assert!(!ts[0].active);
    assert_eq!(ts[0].content, "2026-05-15");
}

#[test]
fn timestamp_range() {
    let ts = closure_org::find_timestamps("<2026-06-01>--<2026-06-10>");
    assert_eq!(ts.len(), 2);
    assert_eq!(ts[0].content, "2026-06-01");
    assert_eq!(ts[1].content, "2026-06-10");
}

#[test]
fn non_date_brackets_ignored() {
    let ts = closure_org::find_timestamps("Link [[target]] not a timestamp.");
    assert!(ts.is_empty());
}

#[test]
fn bold_italic_code_markup_found() {
    let m = closure_org::find_markup("*bold* /italic/ =code=");
    assert_eq!(m.len(), 3);
    assert_eq!(m[0].kind, closure_org::MarkupKind::Bold);
    assert_eq!(m[0].content, "bold");
    assert_eq!(m[1].kind, closure_org::MarkupKind::Italic);
    assert_eq!(m[1].content, "italic");
    assert_eq!(m[2].kind, closure_org::MarkupKind::Code);
    assert_eq!(m[2].content, "code");
}

#[test]
fn verbatim_strike_under_markup() {
    let m = closure_org::find_markup("~verbatim~ +strike+ _under_");
    assert_eq!(m.len(), 3);
    assert_eq!(m[0].kind, closure_org::MarkupKind::Verbatim);
    assert_eq!(m[1].kind, closure_org::MarkupKind::Strikethrough);
    assert_eq!(m[2].kind, closure_org::MarkupKind::Underline);
}

#[test]
fn markup_inside_word_ignored() {
    // `foo*bar*baz` — left boundary fails (alphanum before *).
    let m = closure_org::find_markup("foo*bar*baz");
    assert!(m.is_empty());
}

#[test]
fn markup_with_space_after_open_marker_ignored() {
    let m = closure_org::find_markup("* not bold*");
    assert!(m.is_empty());
}

#[test]
fn set_planning_inserts_scheduled_line() {
    let src = "* TODO Task\nbody\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_headline_set_planning(
        &doc,
        &[0],
        Some("<2026-04-25 Sat>"),
        None,
        None,
    )
    .expect("rewrite");
    let out = closure_org::print(&new);
    assert!(out.contains("SCHEDULED: <2026-04-25 Sat>"), "got: {out:?}");
    assert!(out.contains("body"));
}

#[test]
fn set_planning_replaces_existing_line() {
    let src = "* TODO Task\nSCHEDULED: <2026-04-01 Wed>\nbody\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_headline_set_planning(
        &doc,
        &[0],
        Some("<2026-05-01 Fri>"),
        Some("<2026-05-15 Fri>"),
        None,
    )
    .expect("rewrite");
    let out = closure_org::print(&new);
    assert!(out.contains("SCHEDULED: <2026-05-01 Fri>"));
    assert!(out.contains("DEADLINE: <2026-05-15 Fri>"));
    assert!(!out.contains("2026-04-01"));
}

#[test]
fn set_planning_clear_removes_line() {
    let src = "* TODO Task\nSCHEDULED: <2026-04-01 Wed>\nbody\n";
    let doc = parse(src).expect("parse");
    let new =
        closure_org::rewrite_headline_set_planning(&doc, &[0], None, None, None).expect("rewrite");
    let out = closure_org::print(&new);
    assert!(!out.contains("SCHEDULED"));
    assert!(out.contains("body"));
}

#[test]
fn toggle_comment_adds_prefix() {
    let src = "* Task\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_headline_toggle_comment(&doc, &[0]).expect("toggle");
    let h = &new.roots()[0];
    assert!(h.is_comment());
    assert_eq!(h.title(), "COMMENT Task");
}

#[test]
fn toggle_comment_removes_prefix() {
    let src = "* COMMENT Task\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_headline_toggle_comment(&doc, &[0]).expect("toggle");
    let h = &new.roots()[0];
    assert!(!h.is_comment());
    assert_eq!(h.title(), "Task");
}

#[test]
fn set_property_adds_to_existing_drawer() {
    let src = "* Task\n:PROPERTIES:\n:ID: x\n:END:\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_headline_set_property(&doc, &[0], "EFFORT", "2h").expect("set");
    let h = &new.roots()[0];
    let p = h.properties().expect("props");
    assert_eq!(p.get("EFFORT"), Some("2h"));
    assert_eq!(p.get("ID"), Some("x"));
}

#[test]
fn set_property_replaces_existing_value() {
    let src = "* Task\n:PROPERTIES:\n:EFFORT: 1h\n:END:\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_headline_set_property(&doc, &[0], "EFFORT", "3h").expect("set");
    assert_eq!(
        new.roots()[0].properties().expect("p").get("EFFORT"),
        Some("3h")
    );
}

#[test]
fn set_property_creates_drawer_if_absent() {
    let src = "* Task\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_headline_set_property(&doc, &[0], "EFFORT", "30m").expect("set");
    assert_eq!(
        new.roots()[0].properties().expect("p").get("EFFORT"),
        Some("30m")
    );
}

#[test]
fn toggle_archive_adds_tag() {
    let src = "* TODO Old task :work:\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_headline_toggle_archive(&doc, &[0]).expect("toggle");
    let h = &new.roots()[0];
    assert!(h.tags().contains(&"ARCHIVE"));
    assert!(h.tags().contains(&"work"));
}

#[test]
fn toggle_archive_removes_tag_when_present() {
    let src = "* TODO Old task :work:ARCHIVE:\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_headline_toggle_archive(&doc, &[0]).expect("toggle");
    let h = &new.roots()[0];
    assert!(!h.tags().contains(&"ARCHIVE"));
    assert!(h.tags().contains(&"work"));
}

#[test]
fn macro_simple_invocation() {
    let m = closure_org::find_macros("Hello {{{name}}}!");
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].name, "name");
    assert!(m[0].args.is_empty());
}

#[test]
fn macro_with_arguments() {
    let m = closure_org::find_macros("{{{greet(world,42)}}}");
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].name, "greet");
    assert_eq!(m[0].args, vec!["world", "42"]);
}

#[test]
fn macro_unclosed_ignored() {
    let m = closure_org::find_macros("nothing here {{{ open");
    assert!(m.is_empty());
}

#[test]
fn footnote_ref_detected() {
    let f = closure_org::find_footnotes("See [fn:1] for details.");
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].name, "1");
    assert!(f[0].definition.is_none());
}

#[test]
fn footnote_inline_definition_detected() {
    let f = closure_org::find_footnotes("Note [fn:foo: inline body] there.");
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].name, "foo");
    assert_eq!(f[0].definition, Some("inline body"));
}

#[test]
fn footnote_anonymous_skipped() {
    let f = closure_org::find_footnotes("Just [fn::body] anonymous.");
    assert!(f.is_empty());
}

#[test]
fn all_keywords_lists_in_source_order() {
    let src = "#+TITLE: A\n#+AUTHOR: B\n#+CUSTOM: c\n* H\n";
    let doc = parse(src).expect("parse");
    let kws = doc.all_keywords();
    assert_eq!(kws.len(), 3);
    assert_eq!(kws[0], ("TITLE", "A"));
    assert_eq!(kws[1], ("AUTHOR", "B"));
    assert_eq!(kws[2], ("CUSTOM", "c"));
}

#[test]
fn doc_keywords_extracted_from_preamble() {
    let src = "#+TITLE: My Notes\n#+AUTHOR: Alice\n#+DATE: 2026-04-24\n#+FILETAGS: :work:home:\n* First\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.title(), Some("My Notes"));
    assert_eq!(doc.author(), Some("Alice"));
    assert_eq!(doc.date(), Some("2026-04-24"));
    assert_eq!(doc.filetags(), vec!["work", "home"]);
}

#[test]
fn doc_keywords_absent_returns_none() {
    let doc = parse("* Just a heading\n").expect("parse");
    assert_eq!(doc.title(), None);
    assert_eq!(doc.author(), None);
    assert_eq!(doc.date(), None);
    assert!(doc.filetags().is_empty());
}

#[test]
fn doc_keywords_case_insensitive() {
    let doc = parse("#+title: lowercase\n").expect("parse");
    assert_eq!(doc.title(), Some("lowercase"));
}

#[test]
fn planning_view_reads_scheduled_and_deadline() {
    let src = "* TODO Task\nSCHEDULED: <2026-04-25 Sat> DEADLINE: <2026-05-01 Fri>\nbody\n";
    let doc = parse(src).expect("parse");
    let p = doc.roots()[0].planning().expect("planning");
    assert_eq!(p.scheduled, Some("<2026-04-25 Sat>"));
    assert_eq!(p.deadline, Some("<2026-05-01 Fri>"));
    assert!(p.closed.is_none());
}
