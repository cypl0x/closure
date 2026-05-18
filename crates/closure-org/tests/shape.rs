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
fn rewrite_clear_drawer_removes_named_drawer() {
    let src = "* Task\n:LOGBOOK:\n- old entry\n:END:\nbody\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_headline_clear_drawer(&doc, &[0], "LOGBOOK")
        .expect("clear");
    let out = closure_org::print(&new);
    assert!(!out.contains(":LOGBOOK:"));
    assert!(!out.contains("old entry"));
    assert!(out.contains("body"));
}

#[test]
fn rewrite_clear_drawer_no_op_if_absent() {
    let src = "* Task\nbody\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_headline_clear_drawer(&doc, &[0], "LOGBOOK")
        .expect("clear");
    assert_eq!(closure_org::print(&new), src);
}

#[test]
fn rewrite_append_logbook_entry_creates_drawer() {
    let src = "* Task\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_headline_append_logbook(
        &doc,
        &[0],
        "- State \"DONE\" from \"TODO\" [2026-04-25]",
    )
    .expect("append");
    let out = closure_org::print(&new);
    assert!(out.contains(":LOGBOOK:"));
    assert!(out.contains("State \"DONE\" from \"TODO\""));
    assert!(out.contains(":END:"));
}

#[test]
fn rewrite_append_logbook_extends_existing_drawer() {
    let src = "* Task\n:LOGBOOK:\n- earlier entry\n:END:\n";
    let doc = parse(src).expect("parse");
    let new =
        closure_org::rewrite_headline_append_logbook(&doc, &[0], "- new entry").expect("append");
    let out = closure_org::print(&new);
    assert!(out.contains("- earlier entry"));
    assert!(out.contains("- new entry"));
}

#[test]
fn parse_logbook_state_change() {
    let body = "- State \"DONE\" from \"TODO\" [2026-04-25]\n- State \"TODO\" from \"DOING\" [2026-04-24]\n";
    let entries = closure_org::parse_logbook(body);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].new_state, Some("DONE"));
    assert_eq!(entries[0].old_state, Some("TODO"));
    assert_eq!(entries[0].when, Some("2026-04-25"));
    assert_eq!(entries[1].new_state, Some("TODO"));
}

#[test]
fn parse_logbook_clock_in_out() {
    let body = "CLOCK: [2026-04-25 Sat 09:00]--[2026-04-25 Sat 10:30]\n";
    let entries = closure_org::parse_logbook(body);
    assert_eq!(entries.len(), 1);
    assert!(entries[0].kind == closure_org::LogbookKind::Clock);
}

#[test]
fn parse_block_args_simple() {
    let args = closure_org::parse_block_args(":results output :wrap example");
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], (":results", "output"));
    assert_eq!(args[1], (":wrap", "example"));
}

#[test]
fn parse_block_args_no_value_drops_key() {
    let args = closure_org::parse_block_args(":noeval");
    assert!(args.is_empty());
}

#[test]
fn all_tags_aggregates_recursively() {
    let doc = parse("* A :work:\n** B :urgent:\n* C :work:\n").expect("parse");
    let tags = doc.all_tags();
    assert_eq!(tags, vec!["urgent", "work"]);
}

#[test]
fn all_todos_aggregates_recursively() {
    let doc = parse("* TODO A\n** DONE B\n* TODO C\n").expect("parse");
    let todos = doc.all_todos();
    assert_eq!(todos, vec!["DONE", "TODO"]);
}

#[test]
fn max_depth_picks_deepest_branch() {
    let doc = parse("* A\n** B\n*** C\n* D\n").expect("parse");
    assert_eq!(doc.max_depth(), 3);
}

#[test]
fn max_depth_zero_when_no_headlines() {
    let doc = parse("just paragraph\n").expect("parse");
    assert_eq!(doc.max_depth(), 0);
}

#[test]
fn preamble_kind_counts_histogram() {
    let doc = parse("hello\n\n# comment\n#+TITLE: t\n").expect("parse");
    let counts = doc.preamble_kind_counts();
    let by_kind: std::collections::HashMap<closure_org::NodeKind, usize> =
        counts.into_iter().collect();
    assert_eq!(by_kind.get(&closure_org::NodeKind::Paragraph), Some(&1));
    assert_eq!(by_kind.get(&closure_org::NodeKind::Comment), Some(&1));
    assert_eq!(by_kind.get(&closure_org::NodeKind::Keyword), Some(&1));
    assert_eq!(by_kind.get(&closure_org::NodeKind::BlankLine), Some(&1));
}

#[test]
fn parse_path_reads_from_disk() {
    let dir = tempfile::tempdir().expect("tempdir");
    let p = dir.path().join("a.org");
    std::fs::write(&p, "* Hello\n").expect("write");
    let doc = closure_org::parse_path(&p).expect("parse_path");
    assert_eq!(doc.roots()[0].title(), "Hello");
}

#[test]
fn is_empty_true_for_empty_source() {
    let doc = parse("").expect("parse");
    assert!(doc.is_empty());
    let doc2 = parse("* hi\n").expect("parse");
    assert!(!doc2.is_empty());
}

#[test]
fn headline_count_walks_recursively() {
    let doc = parse("* A\n** B\n** C\n*** D\n").expect("parse");
    assert_eq!(doc.headline_count(), 4);
}

#[test]
fn source_hash_is_deterministic() {
    let a = parse("* Hello\n").expect("parse");
    let b = parse("* Hello\n").expect("parse");
    assert_eq!(a.source_hash(), b.source_hash());
}

#[test]
fn source_hash_changes_with_source() {
    let a = parse("* A\n").expect("parse");
    let b = parse("* B\n").expect("parse");
    assert_ne!(a.source_hash(), b.source_hash());
}

#[test]
fn format_property_drawer_emits_full_block() {
    let s = closure_org::format_property_drawer([("ID", "01ABC"), ("EFFORT", "2h")]);
    assert!(s.starts_with(":PROPERTIES:\n"));
    assert!(s.contains(":ID: 01ABC\n"));
    assert!(s.contains(":EFFORT: 2h\n"));
    assert!(s.ends_with(":END:\n"));
}

#[test]
fn format_property_drawer_empty_returns_empty() {
    let s = closure_org::format_property_drawer::<_, &str, &str>(std::iter::empty());
    assert!(s.is_empty());
}

#[test]
fn format_link_with_description() {
    let s = closure_org::format_link("id:01ABC", Some("Target"));
    assert_eq!(s, "[[id:01ABC][Target]]");
}

#[test]
fn format_link_bare() {
    let s = closure_org::format_link("https://example.com", None);
    assert_eq!(s, "[[https://example.com]]");
}

#[test]
fn format_active_timestamp() {
    let s = closure_org::format_timestamp("2026-04-25 Sat", true);
    assert_eq!(s, "<2026-04-25 Sat>");
}

#[test]
fn format_inactive_timestamp() {
    let s = closure_org::format_timestamp("2026-04-25", false);
    assert_eq!(s, "[2026-04-25]");
}

#[test]
fn find_anchor_targets_regular_and_radio() {
    let text = "See <<topic-a>> and <<<radio-target>>> here.";
    let a = closure_org::find_anchor_targets(text);
    assert_eq!(a.len(), 2);
    assert_eq!(a[0].name, "topic-a");
    assert!(!a[0].is_radio);
    assert_eq!(a[1].name, "radio-target");
    assert!(a[1].is_radio);
}

#[test]
fn find_named_blocks_returns_all() {
    let text = "#+BEGIN_QUOTE\nhello\n#+END_QUOTE\n#+BEGIN_EXAMPLE\nworld\n#+END_EXAMPLE\n";
    let blocks = closure_org::find_named_blocks(text);
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].name, "QUOTE");
    assert_eq!(blocks[0].content.trim(), "hello");
    assert_eq!(blocks[1].name, "EXAMPLE");
}

#[test]
fn lists_group_consecutive_items() {
    let src = "- a\n- b\n- c\n\n- d\n";
    let doc = parse(src).expect("parse");
    let lists = doc.lists();
    assert_eq!(lists.len(), 2);
    assert_eq!(lists[0].items.len(), 3);
    assert_eq!(lists[1].items.len(), 1);
}

#[test]
fn lists_group_mixed_markers_continue() {
    let src = "- a\n+ b\n";
    let doc = parse(src).expect("parse");
    let lists = doc.lists();
    assert_eq!(lists.len(), 1);
    assert_eq!(lists[0].items.len(), 2);
}

#[test]
fn tables_group_consecutive_rows() {
    let src = "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n";
    let doc = parse(src).expect("parse");
    let tables = doc.tables();
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].rows.len(), 4);
    assert!(!tables[0].rows[0].is_separator);
    assert!(tables[0].rows[1].is_separator);
    assert_eq!(tables[0].rows[0].cells, vec!["a", "b"]);
    assert_eq!(tables[0].rows[2].cells, vec!["1", "2"]);
}

#[test]
fn tables_blank_line_breaks_table() {
    let src = "| a |\n\n| b |\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.tables().len(), 2);
}

#[test]
fn find_drawers_logbook_in_text() {
    let text = ":LOGBOOK:\n- State \"DONE\" from \"TODO\" [2026-04-25]\n:END:\n";
    let d = closure_org::find_drawers(text);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].name, "LOGBOOK");
    assert!(d[0].content.contains("State"));
}

#[test]
fn find_drawers_skips_properties() {
    // PROPERTIES drawer should still match — it's a regular drawer.
    let text = ":PROPERTIES:\n:ID: x\n:END:\n";
    let d = closure_org::find_drawers(text);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].name, "PROPERTIES");
}

#[test]
fn find_drawers_unclosed_ignored() {
    let text = ":LOGBOOK:\nstuff\nno end\n";
    let d = closure_org::find_drawers(text);
    assert!(d.is_empty());
}

#[test]
fn unfinished_checkboxes_filters_correctly() {
    let doc = parse("- [ ] todo\n- [X] done\n- [-] partial\n").expect("parse");
    let pending = doc.unfinished_checkboxes();
    assert_eq!(pending.len(), 2);
}

#[test]
fn toggle_checkbox_unchecked_to_checked() {
    let src = "- [ ] task\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_toggle_checkbox(&doc, 0).expect("toggle");
    let li = new.preamble()[0].as_list_item().expect("li");
    assert_eq!(li.checkbox, Some(closure_org::Checkbox::Checked));
}

#[test]
fn toggle_checkbox_checked_to_unchecked() {
    let src = "- [X] task\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_toggle_checkbox(&doc, 0).expect("toggle");
    let li = new.preamble()[0].as_list_item().expect("li");
    assert_eq!(li.checkbox, Some(closure_org::Checkbox::Unchecked));
}

#[test]
fn toggle_checkbox_partial_to_checked() {
    let src = "- [-] task\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_toggle_checkbox(&doc, 0).expect("toggle");
    let li = new.preamble()[0].as_list_item().expect("li");
    assert_eq!(li.checkbox, Some(closure_org::Checkbox::Checked));
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
fn find_link_sources_returns_linkers() {
    let src = "* Source\nlink to [[id:01TGT]]\n* Other\n";
    let doc = parse(src).expect("parse");
    let hits = doc.find_link_sources("01TGT");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title(), "Source");
}

#[test]
fn headline_link_targets_collects_title_and_body() {
    let src = "* See [[id:01TGT]]\nbody [[https://x.com][X]]\n";
    let doc = parse(src).expect("parse");
    let targets = doc.roots()[0].link_targets();
    assert_eq!(targets.len(), 2);
    assert!(targets.contains(&"id:01TGT".to_owned()));
    assert!(targets.contains(&"https://x.com".to_owned()));
}

#[test]
fn body_word_count_counts_body_words() {
    let doc = parse("* H\nfour words go here\n").expect("parse");
    assert_eq!(doc.roots()[0].body_word_count(), 4);
}

#[test]
fn headline_by_id_walks_tree() {
    let src = "* Outer\n** Inner\n:PROPERTIES:\n:ID: 01HXTAR0000000000000000000\n:END:\n";
    let doc = parse(src).expect("parse");
    let h = doc
        .headline_by_id("01HXTAR0000000000000000000")
        .expect("found");
    assert_eq!(h.title(), "Inner");
}

#[test]
fn flatten_returns_self_then_descendants() {
    let doc = parse("* A\n** B\n*** C\n** D\n").expect("parse");
    let flat = doc.roots()[0].flatten();
    assert_eq!(flat.len(), 4);
    assert_eq!(flat[0].title(), "A");
    assert_eq!(flat[1].title(), "B");
    assert_eq!(flat[2].title(), "C");
    assert_eq!(flat[3].title(), "D");
}

#[test]
fn subtree_source_includes_children() {
    let src = "* Parent\nbody\n** Child\nchild body\n* Sibling\n";
    let doc = parse(src).expect("parse");
    let parent = &doc.roots()[0];
    let sub = parent.subtree_source();
    assert!(sub.contains("Parent"));
    assert!(sub.contains("Child"));
    assert!(!sub.contains("Sibling"));
}

#[test]
fn max_depth_walks_tree() {
    let doc = parse("* A\n** B\n*** C\n* D\n").expect("parse");
    assert_eq!(doc.max_depth(), 3);
    assert_eq!(doc.min_level(), 1);
}

#[test]
fn headline_count_includes_descendants() {
    let doc = parse("* A\n** B\n*** C\n* D\n").expect("parse");
    assert_eq!(doc.headline_count(), 4);
    assert_eq!(doc.root_count(), 2);
    assert_eq!(doc.count_leaves(), 2);
}

#[test]
fn count_todos_walks_tree() {
    let doc = parse("* TODO A\n** DONE B\n* C\n").expect("parse");
    assert_eq!(doc.count_todos(), 2);
}

#[test]
fn count_tagged_filters_by_tag() {
    let doc = parse("* A :work:\n* B :home:\n* C :work:\n").expect("parse");
    assert_eq!(doc.count_tagged("work"), 2);
    assert_eq!(doc.count_tagged("home"), 1);
}

#[test]
fn distinct_tag_count_counts_distinct_tags() {
    let doc = parse("* A :x:y:\n* B :x:z:\n").expect("parse");
    assert_eq!(doc.distinct_tag_count(), 3);
}

#[test]
fn modal_tag_picks_most_common() {
    let doc = parse("* A :x:\n* B :x:\n* C :y:\n").expect("parse");
    let (tag, n) = doc.modal_tag().expect("modal");
    assert_eq!(tag, "x");
    assert_eq!(n, 2);
}

#[test]
fn modal_level_picks_most_common() {
    let doc = parse("* A\n* B\n** C\n").expect("parse");
    let (lvl, n) = doc.modal_level().expect("modal");
    assert_eq!(lvl, 1);
    assert_eq!(n, 2);
}

#[test]
fn id_set_is_distinct() {
    let src = "* A\n:PROPERTIES:\n:ID: x\n:END:\n* B\n:PROPERTIES:\n:ID: x\n:END:\n* C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.id_set().len(), 1);
    assert_eq!(doc.id_count(), 2);
    assert!(doc.has_duplicate_ids());
}

#[test]
fn linking_count_counts_only_linkers() {
    let doc = parse("* A [[id:01TGT]]\n* B\n").expect("parse");
    assert_eq!(doc.linking_count(), 1);
}

#[test]
fn empty_title_count_walks_tree() {
    let doc = parse("*\n* B\n*\n").expect("parse");
    assert_eq!(doc.empty_title_count(), 2);
}

#[test]
fn id_edges_returns_id_links() {
    let src = "* A\n:PROPERTIES:\n:ID: SRC\n:END:\nLink [[id:TGT]]\n* B\n:PROPERTIES:\n:ID: TGT\n:END:\n";
    let doc = parse(src).expect("parse");
    let edges = doc.id_edges();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].0, "SRC");
    assert_eq!(edges[0].1, "TGT");
    assert_eq!(doc.id_edge_count(), 1);
    assert_eq!(doc.resolved_edge_count(), 1);
    assert_eq!(doc.dead_edge_count(), 0);
}

#[test]
fn dead_edge_count_when_target_missing() {
    let src = "* A\n:PROPERTIES:\n:ID: SRC\n:END:\nLink [[id:GHOST]]\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.dead_edge_count(), 1);
    assert_eq!(doc.resolved_edge_count(), 0);
}

#[test]
fn self_loop_count_picks_self_link() {
    let src = "* A\n:PROPERTIES:\n:ID: X\n:END:\nLink [[id:X]]\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.self_loop_count(), 1);
}

#[test]
fn isolated_id_count_when_no_edges() {
    let src = "* A\n:PROPERTIES:\n:ID: X\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.isolated_id_count(), 1);
}

#[test]
fn hub_count_when_id_both_source_and_sink() {
    let src = "* A\n:PROPERTIES:\n:ID: HUB\n:END:\nLink [[id:OTHER]]\n* B\n:PROPERTIES:\n:ID: OTHER\n:END:\nLink [[id:HUB]]\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.hub_count(), 2);
}

#[test]
fn most_referenced_picks_top() {
    let src = "* A\n:PROPERTIES:\n:ID: TGT\n:END:\n* B\n:PROPERTIES:\n:ID: B\n:END:\nLink [[id:TGT]]\n* C\n:PROPERTIES:\n:ID: C\n:END:\nLink [[id:TGT]]\n";
    let doc = parse(src).expect("parse");
    let (id, n) = doc.most_referenced().expect("found");
    assert_eq!(id, "TGT");
    assert_eq!(n, 2);
}

#[test]
fn link_balance_outgoing_minus_incoming() {
    let src = "* A\n:PROPERTIES:\n:ID: SRC\n:END:\nLink [[id:T1]]\nLink [[id:T2]]\n* B\n:PROPERTIES:\n:ID: T1\n:END:\nLink [[id:SRC]]\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.link_balance("SRC"), Some(2 - 1));
    assert_eq!(doc.link_balance("T1"), Some(1 - 1));
}

#[test]
fn density_pct_returns_percent() {
    let doc = parse("* TODO A\n* TODO B\n* C\n* D\n").expect("parse");
    assert_eq!(doc.todo_density_pct(), 50);
    assert_eq!(doc.tag_density_pct(), 0);
}

#[test]
fn id_pct_returns_percent() {
    let src = "* A\n:PROPERTIES:\n:ID: x\n:END:\n* B\n* C\n* D\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.id_pct(), 25);
}

#[test]
fn longest_title_picks_widest() {
    let doc = parse("* short\n* a much longer title here\n* mid\n").expect("parse");
    assert_eq!(doc.longest_title().expect("h").title(), "a much longer title here");
}

#[test]
fn shortest_title_picks_narrowest() {
    let doc = parse("* longer\n* xx\n* mid\n").expect("parse");
    assert_eq!(doc.shortest_title().expect("h").title(), "xx");
}

#[test]
fn deepest_leaf_picks_max_level_leaf() {
    let doc = parse("* A\n** B\n*** C\n* D\n").expect("parse");
    assert_eq!(doc.deepest_leaf().expect("h").title(), "C");
}

#[test]
fn largest_subtree_picks_most_descendants() {
    let doc = parse("* A\n** B\n** C\n** D\n* E\n** F\n").expect("parse");
    assert_eq!(doc.largest_subtree().expect("h").title(), "A");
}

#[test]
fn longest_body_picks_max_words() {
    let doc = parse("* A\nfew\n* B\none two three four\n").expect("parse");
    assert_eq!(doc.longest_body().expect("h").title(), "B");
}

#[test]
fn distinct_tags_sorted_unique() {
    let doc = parse("* A :work:home:\n* B :work:urgent:\n").expect("parse");
    assert_eq!(doc.distinct_tags(), vec!["home", "urgent", "work"]);
}

#[test]
fn distinct_todos_sorted_unique() {
    let doc = parse("* TODO A\n* DONE B\n* TODO C\n").expect("parse");
    assert_eq!(doc.distinct_todos(), vec!["DONE", "TODO"]);
}

#[test]
fn distinct_priorities_sorted_unique() {
    let doc = parse("* [#A] x\n* [#B] y\n* [#A] z\n").expect("parse");
    assert_eq!(doc.distinct_priorities(), vec!['A', 'B']);
}

#[test]
fn distinct_levels_sorted_unique() {
    let doc = parse("* A\n** B\n*** C\n** D\n").expect("parse");
    assert_eq!(doc.distinct_levels(), vec![1, 2, 3]);
}

#[test]
fn rename_first_root_via_path() {
    let src = "* Old\n";
    let doc = parse(src).expect("parse");
    let new = closure_org::rewrite_headline_title(&doc, &[0], "New").expect("rewrite");
    assert_eq!(new.roots()[0].title(), "New");
}

#[test]
fn count_with_id_walks_tree() {
    let src = "* A\n:PROPERTIES:\n:ID: x\n:END:\n* B\n** C\n:PROPERTIES:\n:ID: y\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.count_with_id(), 2);
}

#[test]
fn first_todo_returns_first_match() {
    let doc = parse("* A\n* TODO B\n* TODO C\n").expect("parse");
    assert_eq!(doc.first_todo().expect("h").title(), "B");
}

#[test]
fn first_archived_returns_first_match() {
    let doc = parse("* A\n* B :ARCHIVE:\n").expect("parse");
    assert_eq!(doc.first_archived().expect("h").title(), "B");
}

#[test]
fn first_with_priority_returns_match() {
    let doc = parse("* A\n* [#A] B\n* [#B] C\n").expect("parse");
    assert_eq!(doc.first_with_priority('A').expect("h").title(), "B");
    assert_eq!(doc.first_with_priority('B').expect("h").title(), "C");
}

#[test]
fn first_with_property_returns_match() {
    let src = "* A\n* B\n:PROPERTIES:\n:EFFORT: 2h\n:END:\n* C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(
        doc.first_with_property("EFFORT").expect("h").title(),
        "B"
    );
}

#[test]
fn count_at_level_filters() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    assert_eq!(doc.count_at_level(1), 2);
    assert_eq!(doc.count_at_level(2), 2);
    assert_eq!(doc.count_at_level(3), 0);
}

#[test]
fn count_title_contains_substring() {
    let doc = parse("* hello world\n* hello there\n* bye\n").expect("parse");
    assert_eq!(doc.count_title_contains("hello"), 2);
    assert_eq!(doc.count_title_contains("bye"), 1);
}

#[test]
fn count_descendant_todos_recursive() {
    let src = "* Parent\n** TODO A\n*** TODO B\n** C\n";
    let doc = parse(src).expect("parse");
    let parent = &doc.roots()[0];
    assert_eq!(parent.count_descendant_todos("TODO"), 2);
}

#[test]
fn count_descendant_tagged_recursive() {
    let src = "* Parent\n** A :work:\n*** B :work:\n** C :home:\n";
    let doc = parse(src).expect("parse");
    let parent = &doc.roots()[0];
    assert_eq!(parent.count_descendant_tagged("work"), 2);
}

#[test]
fn count_descendants_at_level_filters() {
    let src = "* A\n** B\n*** C\n*** D\n** E\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert_eq!(root.count_descendants_at_level(2), 2);
    assert_eq!(root.count_descendants_at_level(3), 2);
}

#[test]
fn count_descendants_with_property_filters() {
    let src = "* A\n** B\n:PROPERTIES:\n:EFFORT: 1h\n:END:\n** C\n*** D\n:PROPERTIES:\n:EFFORT: 2h\n:END:\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert_eq!(root.count_descendants_with_property("EFFORT"), 2);
    assert_eq!(root.count_descendants_with_property("ID"), 0);
}

#[test]
fn count_descendant_leaves_only_terminal() {
    let src = "* A\n** B\n*** C\n** D\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert_eq!(root.count_descendant_leaves(), 2);
}

#[test]
fn count_descendants_with_id_filters() {
    let src = "* Root\n** A\n:PROPERTIES:\n:ID: x\n:END:\n** B\n*** C\n:PROPERTIES:\n:ID: y\n:END:\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert_eq!(root.count_descendants_with_id(), 2);
}

#[test]
fn descendants_at_level_returns_matching() {
    let src = "* A\n** B\n*** C\n*** D\n** E\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let l3: Vec<&str> = root
        .descendants_at_level(3)
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(l3, vec!["C", "D"]);
}

#[test]
fn descendant_leaves_returns_matching() {
    let src = "* A\n** B\n*** C\n** D\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let leaves: Vec<&str> = root
        .descendant_leaves()
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(leaves, vec!["C", "D"]);
}

#[test]
fn descendant_todos_returns_todo_subtree() {
    let src = "* Root\n** TODO A\n*** B\n** DONE C\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let titles: Vec<&str> = root
        .descendant_todos()
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(titles, vec!["A", "C"]);
}

#[test]
fn descendant_with_tag_returns_subtree() {
    let src = "* Root\n** A :work:\n*** B :work:\n** C :home:\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let titles: Vec<&str> = root
        .descendants_with_tag("work")
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(titles, vec!["A", "B"]);
}

#[test]
fn descendants_with_id_returns_subtree() {
    let src = "* R\n** A\n:PROPERTIES:\n:ID: x\n:END:\n** B\n*** C\n:PROPERTIES:\n:ID: y\n:END:\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let titles: Vec<&str> = root
        .descendants_with_id()
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(titles, vec!["A", "C"]);
}

#[test]
fn descendants_with_planning_returns_subtree() {
    let src = "* R\n** A\nSCHEDULED: <2026-04-25 Sat>\n** B\n*** C\nDEADLINE: <2026-05-01 Fri>\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let titles: Vec<&str> = root
        .descendants_with_planning()
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(titles, vec!["A", "C"]);
}

#[test]
fn descendants_with_property_returns_subtree() {
    let src = "* R\n** A\n:PROPERTIES:\n:EFFORT: 1h\n:END:\n** B\n*** C\n:PROPERTIES:\n:EFFORT: 2h\n:END:\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let titles: Vec<&str> = root
        .descendants_with_property("EFFORT")
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(titles, vec!["A", "C"]);
}

#[test]
fn descendants_archived_returns_subtree() {
    let src = "* R\n** A :ARCHIVE:\n** B\n*** C :ARCHIVE:\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let titles: Vec<&str> = root
        .descendants_archived()
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(titles, vec!["A", "C"]);
}

#[test]
fn descendants_commented_returns_subtree() {
    let src = "* R\n** COMMENT A\n** B\n*** COMMENT C\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let titles: Vec<&str> = root
        .descendants_commented()
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(titles, vec!["COMMENT A", "COMMENT C"]);
}

#[test]
fn descendant_priority_returns_subtree() {
    let src = "* R\n** [#A] A\n** B\n*** [#A] C\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let titles: Vec<&str> = root
        .descendants_with_priority('A')
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(titles, vec!["A", "C"]);
}

#[test]
fn descendants_with_todo_returns_subtree() {
    let src = "* R\n** TODO A\n** DONE B\n*** TODO C\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let titles: Vec<&str> = root
        .descendants_with_todo("TODO")
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(titles, vec!["A", "C"]);
}

#[test]
fn count_descendants_archived() {
    let src = "* R\n** A :ARCHIVE:\n** B\n*** C :ARCHIVE:\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert_eq!(root.count_descendants_archived(), 2);
}

#[test]
fn count_descendants_commented() {
    let src = "* R\n** COMMENT A\n** B\n*** COMMENT C\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert_eq!(root.count_descendants_commented(), 2);
}

#[test]
fn count_descendants_with_planning() {
    let src = "* R\n** A\nSCHEDULED: <2026-04-25 Sat>\n** B\n*** C\nDEADLINE: <2026-05-01 Fri>\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert_eq!(root.count_descendants_with_planning(), 2);
}

#[test]
fn count_descendants_with_priority() {
    let src = "* R\n** [#A] A\n** B\n*** [#A] C\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert_eq!(root.count_descendants_with_priority('A'), 2);
}

#[test]
fn descendant_with_property_value_returns_subtree() {
    let src = "* R\n** A\n:PROPERTIES:\n:EFFORT: 2h\n:END:\n** B\n*** C\n:PROPERTIES:\n:EFFORT: 1h\n:END:\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let titles: Vec<&str> = root
        .descendants_with_property_value("EFFORT", "2h")
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(titles, vec!["A"]);
}

#[test]
fn descendants_with_title_substring_matches() {
    let src = "* Project\n** kangaroo report\n** B\n*** kangaroo notes\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let titles: Vec<&str> = root
        .descendants_with_title_substring("kangaroo")
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(titles, vec!["kangaroo report", "kangaroo notes"]);
}

#[test]
fn descendants_filter_walks_with_predicate() {
    let src = "* R\n** A :work:\n*** B :work:\n** C\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let titles: Vec<&str> = root
        .filter_descendants(|h| h.has_tag("work"))
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(titles, vec!["A", "B"]);
}

#[test]
fn count_filter_descendants_returns_count() {
    let src = "* R\n** A :work:\n*** B :work:\n** C\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert_eq!(root.count_filter_descendants(|h| h.has_tag("work")), 2);
}

#[test]
fn find_descendant_returns_first() {
    let src = "* R\n** A\n** B :match:\n*** C :match:\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let h = root.find_descendant(|h| h.has_tag("match")).expect("found");
    assert_eq!(h.title(), "B");
}

#[test]
fn flatten_yields_self_then_dfs() {
    let src = "* A\n** B\n*** C\n** D\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let titles: Vec<&str> = root.flatten().iter().map(|h| h.title()).collect();
    assert_eq!(titles, vec!["A", "B", "C", "D"]);
}

#[test]
fn descendants_yields_dfs_without_self() {
    let src = "* A\n** B\n*** C\n** D\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let titles: Vec<&str> = root.descendants().iter().map(|h| h.title()).collect();
    assert_eq!(titles, vec!["B", "C", "D"]);
}

#[test]
fn descendants_at_depth_below_self() {
    let src = "* A\n** B\n*** C\n*** D\n** E\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let titles: Vec<&str> = root
        .descendants_at_depth(2)
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(titles, vec!["C", "D"]);
}

#[test]
fn count_descendants_at_depth_returns_count() {
    let src = "* A\n** B\n*** C\n*** D\n** E\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert_eq!(root.count_descendants_at_depth(1), 2);
    assert_eq!(root.count_descendants_at_depth(2), 2);
}

#[test]
fn child_titles_returns_immediate_titles() {
    let src = "* R\n** A\n** B\n** C\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert_eq!(root.child_titles(), vec!["A", "B", "C"]);
}

#[test]
fn descendant_titles_dfs() {
    let src = "* R\n** A\n*** A1\n** B\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert_eq!(root.descendant_titles(), vec!["A", "A1", "B"]);
}

#[test]
fn descendant_ids_returns_drawer_ids() {
    let src = "* R\n** A\n:PROPERTIES:\n:ID: x\n:END:\n** B\n*** C\n:PROPERTIES:\n:ID: y\n:END:\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert_eq!(root.descendant_ids(), vec!["x", "y"]);
}

#[test]
fn descendant_tags_collected() {
    let src = "* R\n** A :work:\n*** B :urgent:work:\n** C :home:\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let mut tags: Vec<&str> = root.descendant_tags();
    tags.sort_unstable();
    tags.dedup();
    assert_eq!(tags, vec!["home", "urgent", "work"]);
}

#[test]
fn descendant_todos_collected() {
    let src = "* R\n** TODO A\n** DONE B\n*** TODO C\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let mut todos: Vec<&str> = root.descendant_todo_keywords();
    todos.sort_unstable();
    todos.dedup();
    assert_eq!(todos, vec!["DONE", "TODO"]);
}

#[test]
fn descendant_priorities_collected() {
    let src = "* R\n** [#A] A\n** [#B] B\n*** [#A] C\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let mut p: Vec<char> = root.descendant_priorities();
    p.sort_unstable();
    p.dedup();
    assert_eq!(p, vec!['A', 'B']);
}

#[test]
fn descendant_levels_collected() {
    let src = "* R\n** A\n*** A1\n** B\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert_eq!(root.descendant_levels(), vec![2, 3, 2]);
}

#[test]
fn doc_distinct_priorities_test() {
    let doc = parse("* [#A] x\n* [#A] y\n* [#C] z\n").expect("parse");
    let mut p: Vec<char> = doc
        .iter_headlines()
        .into_iter()
        .filter_map(closure_org::Headline::priority)
        .collect();
    p.sort_unstable();
    p.dedup();
    assert_eq!(p, vec!['A', 'C']);
}

#[test]
fn descendants_filter_by_level_range() {
    let src = "* R\n** A\n*** A1\n*** A2\n** B\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let titles: Vec<&str> = root
        .descendants_in_level_range(3, 3)
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(titles, vec!["A1", "A2"]);
}

#[test]
fn count_descendants_in_level_range_count() {
    let src = "* R\n** A\n*** A1\n*** A2\n**** A2a\n** B\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert_eq!(root.count_descendants_in_level_range(3, 4), 3);
}

#[test]
fn map_descendants_returns_mapped() {
    let src = "* R\n** A\n** B\n** C\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let upper: Vec<String> = root.map_descendants(|h| h.title().to_uppercase());
    assert_eq!(upper, vec!["A", "B", "C"]);
}

#[test]
fn fold_descendants_aggregates() {
    let src = "* R\n** A\n** B\n*** C\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    let total = root.fold_descendants(0usize, |acc, h| acc + h.title().len());
    assert_eq!(total, 3);
}

#[test]
fn any_descendant_predicate() {
    let src = "* R\n** A\n** B :match:\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert!(root.any_descendant(|h| h.has_tag("match")));
    assert!(!root.any_descendant(|h| h.has_tag("none")));
}

#[test]
fn all_descendants_predicate() {
    let src = "* R\n** A :work:\n** B :work:\n";
    let doc = parse(src).expect("parse");
    let root = &doc.roots()[0];
    assert!(root.all_descendants(|h| h.has_tag("work")));
    assert!(!root.all_descendants(|h| h.title() == "A"));
}

#[test]
fn doc_any_headline_predicate() {
    let doc = parse("* A\n* B :match:\n").expect("parse");
    assert!(doc.any_headline(|h| h.has_tag("match")));
    assert!(!doc.any_headline(|h| h.has_tag("none")));
}

#[test]
fn doc_all_headlines_predicate() {
    let doc = parse("* A :work:\n* B :work:\n").expect("parse");
    assert!(doc.all_headlines_match(|h| h.has_tag("work")));
}

#[test]
fn doc_map_headlines_returns_mapped() {
    let doc = parse("* A\n* B\n* C\n").expect("parse");
    let titles: Vec<String> = doc.map_headlines(|h| h.title().to_owned());
    assert_eq!(titles, vec!["A", "B", "C"]);
}

#[test]
fn doc_fold_headlines_aggregates() {
    let doc = parse("* abc\n* d\n").expect("parse");
    let total: usize = doc.fold_headlines(0, |acc, h| acc + h.title().len());
    assert_eq!(total, 4);
}

#[test]
fn doc_titles_returns_all() {
    let doc = parse("* A\n** B\n* C\n").expect("parse");
    assert_eq!(doc.titles(), vec!["A", "B", "C"]);
}

#[test]
fn doc_levels_returns_all() {
    let doc = parse("* A\n** B\n* C\n").expect("parse");
    assert_eq!(doc.levels(), vec![1, 2, 1]);
}

#[test]
fn doc_root_titles_returns_only_roots() {
    let doc = parse("* A\n** B\n* C\n").expect("parse");
    assert_eq!(doc.root_titles(), vec!["A", "C"]);
}

#[test]
fn doc_all_links_returns_targets() {
    let src = "* A [[id:T1]]\n* B\nbody [[https://x]]\n";
    let doc = parse(src).expect("parse");
    let mut targets = doc.all_link_targets();
    targets.sort();
    assert_eq!(targets, vec!["https://x", "id:T1"]);
}

#[test]
fn doc_distinct_link_targets_unique() {
    let src = "* A [[id:T1]]\n* B [[id:T1]]\n* C [[id:T2]]\n";
    let doc = parse(src).expect("parse");
    let mut targets = doc.distinct_link_targets();
    targets.sort();
    assert_eq!(targets, vec!["id:T1", "id:T2"]);
}

#[test]
fn doc_distinct_link_target_count_returns_count() {
    let src = "* A [[id:T1]]\n* B [[id:T1]]\n* C [[id:T2]]\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.distinct_link_target_count(), 2);
}

#[test]
fn doc_root_at_returns_root() {
    let doc = parse("* A\n* B\n* C\n").expect("parse");
    assert_eq!(doc.root_at(1).expect("h").title(), "B");
    assert!(doc.root_at(99).is_none());
}

#[test]
fn doc_first_root_returns_first() {
    let doc = parse("* A\n* B\n").expect("parse");
    assert_eq!(doc.first_root().expect("h").title(), "A");
}

#[test]
fn doc_last_root_returns_last() {
    let doc = parse("* A\n* B\n").expect("parse");
    assert_eq!(doc.last_root().expect("h").title(), "B");
}

#[test]
fn doc_root_titles_iter_match() {
    let doc = parse("* A\n* B\n* C\n").expect("parse");
    let titles: Vec<&str> = doc
        .iter_roots()
        .map(closure_org::Headline::title)
        .collect();
    assert_eq!(titles, vec!["A", "B", "C"]);
}

#[test]
fn doc_first_headline_at_dfs() {
    let doc = parse("* A\n** B\n* C\n").expect("parse");
    assert_eq!(doc.headline_at_index(1).expect("h").title(), "B");
    assert_eq!(doc.headline_at_index(2).expect("h").title(), "C");
}

#[test]
fn doc_headline_index_of_returns_position() {
    let doc = parse("* A\n** B\n* C\n").expect("parse");
    let bid = closure_org::find_links("");
    let _ = bid;
    let idx = doc.headline_index_of(|h| h.title() == "C");
    assert_eq!(idx, Some(2));
}

#[test]
fn doc_position_of_id_returns_dfs_index() {
    let src = "* A\n** B\n:PROPERTIES:\n:ID: x\n:END:\n* C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.position_of_id("x"), Some(1));
    assert_eq!(doc.position_of_id("missing"), None);
}

#[test]
fn doc_position_of_title_returns_position() {
    let doc = parse("* A\n** B\n* C\n").expect("parse");
    assert_eq!(doc.position_of_title("C"), Some(2));
    assert_eq!(doc.position_of_title("Missing"), None);
}

#[test]
fn doc_first_root_match_returns() {
    let doc = parse("* A\n* B\n* C\n").expect("parse");
    assert_eq!(
        doc.first_root_matching(|h| h.title() == "B")
            .expect("h")
            .title(),
        "B"
    );
}

#[test]
fn doc_count_roots_with_predicate() {
    let doc = parse("* TODO A\n* B\n* TODO C\n").expect("parse");
    assert_eq!(doc.count_roots_matching(|h| h.todo().is_some()), 2);
}

#[test]
fn doc_filter_roots_returns_matches() {
    let doc = parse("* TODO A\n* B\n* TODO C\n").expect("parse");
    let titles: Vec<&str> = doc
        .filter_roots(|h| h.todo().is_some())
        .iter()
        .map(|h| h.title())
        .collect();
    assert_eq!(titles, vec!["A", "C"]);
}

#[test]
fn doc_map_roots_returns_mapped() {
    let doc = parse("* A\n* B\n* C\n").expect("parse");
    let titles: Vec<String> = doc.map_roots(|h| h.title().to_owned());
    assert_eq!(titles, vec!["A", "B", "C"]);
}

#[test]
fn doc_fold_roots_aggregates() {
    let doc = parse("* abc\n* d\n").expect("parse");
    let total: usize = doc.fold_roots(0, |acc, h| acc + h.title().len());
    assert_eq!(total, 4);
}

#[test]
fn doc_any_root_predicate() {
    let doc = parse("* A\n* B :match:\n").expect("parse");
    assert!(doc.any_root(|h| h.has_tag("match")));
    assert!(!doc.any_root(|h| h.has_tag("none")));
}

#[test]
fn doc_all_roots_predicate() {
    let doc = parse("* A :work:\n* B :work:\n").expect("parse");
    assert!(doc.all_roots(|h| h.has_tag("work")));
    assert!(!doc.all_roots(|h| h.title() == "A"));
}

#[test]
fn doc_root_at_or_default_returns_first_or_target() {
    let doc = parse("* A\n* B\n").expect("parse");
    assert_eq!(doc.root_at_or_first(0).expect("h").title(), "A");
    assert_eq!(doc.root_at_or_first(99).expect("h").title(), "A");
}

#[test]
fn doc_position_predicate_returns_index() {
    let doc = parse("* A\n** B\n** C :match:\n").expect("parse");
    assert_eq!(
        doc.headline_index_of(|h| h.has_tag("match")),
        Some(2)
    );
    assert_eq!(
        doc.headline_index_of(|h| h.has_tag("none")),
        None
    );
}

#[test]
fn doc_max_priority_returns_highest_letter() {
    let doc = parse("* [#A] x\n* [#B] y\n* [#C] z\n").expect("parse");
    assert_eq!(doc.max_priority_letter(), Some('A'));
}

#[test]
fn doc_min_priority_returns_lowest_letter() {
    let doc = parse("* [#A] x\n* [#B] y\n* [#C] z\n").expect("parse");
    assert_eq!(doc.min_priority_letter(), Some('C'));
}

#[test]
fn doc_max_min_level_endpoints() {
    let doc = parse("* A\n** B\n*** C\n** D\n").expect("parse");
    assert_eq!(doc.min_level(), 1);
    assert_eq!(doc.max_depth(), 3);
}

#[test]
fn doc_max_priority_letter_returns_none_when_empty() {
    let doc = parse("* A\n* B\n").expect("parse");
    assert_eq!(doc.max_priority_letter(), None);
    assert_eq!(doc.min_priority_letter(), None);
}

#[test]
fn doc_first_root_with_priority_returns_match() {
    let doc = parse("* A\n* [#A] B\n* [#B] C\n").expect("parse");
    assert_eq!(
        doc.first_root_matching(|h| h.priority() == Some('B'))
            .expect("h")
            .title(),
        "C"
    );
}

#[test]
fn doc_root_index_of_returns_position() {
    let doc = parse("* A\n* B\n* C\n").expect("parse");
    assert_eq!(doc.root_index_of(|h| h.title() == "B"), Some(1));
    assert_eq!(doc.root_index_of(|h| h.title() == "Z"), None);
}

#[test]
fn doc_root_with_id_returns_match() {
    let src = "* A\n:PROPERTIES:\n:ID: x\n:END:\n* B\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.root_with_id("x").expect("h").title(), "A");
    assert!(doc.root_with_id("missing").is_none());
}

#[test]
fn doc_root_with_title_returns_match() {
    let doc = parse("* A\n* B\n").expect("parse");
    assert_eq!(doc.root_with_title("B").expect("h").title(), "B");
    assert!(doc.root_with_title("Z").is_none());
}

#[test]
fn doc_root_with_tag_returns_match() {
    let doc = parse("* A :work:\n* B :home:\n").expect("parse");
    assert_eq!(doc.root_with_tag("home").expect("h").title(), "B");
    assert!(doc.root_with_tag("none").is_none());
}

#[test]
fn descendant_count_counts_children_recursively() {
    let doc = parse("* Root\n** A\n*** A1\n** B\n").expect("parse");
    let root = &doc.roots()[0];
    assert_eq!(root.descendant_count(), 3);
}

#[test]
fn body_source_returns_contiguous_slice() {
    let doc = parse("* H\nfirst\nsecond\n").expect("parse");
    let body = doc.roots()[0].body_source();
    assert!(body.contains("first"));
    assert!(body.contains("second"));
}

#[test]
fn body_source_empty_for_no_body() {
    let doc = parse("* H\n").expect("parse");
    assert_eq!(doc.roots()[0].body_source(), "");
}

#[test]
fn body_lines_iterates_all_lines() {
    let doc = parse("* H\nfirst\nsecond\nthird\n").expect("parse");
    let lines: Vec<&str> = doc.roots()[0].body_lines().collect();
    assert_eq!(lines, vec!["first", "second", "third"]);
}

#[test]
fn body_byte_count_sums_body_spans() {
    let doc = parse("* H\nabc\ndef\n").expect("parse");
    assert!(doc.roots()[0].body_byte_count() >= 8);
}

#[test]
fn is_archived_method_recognises_archive_tag() {
    let doc = parse("* Old :ARCHIVE:\n* Live\n").expect("parse");
    assert!(doc.roots()[0].is_archived());
    assert!(!doc.roots()[1].is_archived());
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

#[test]
fn doc_root_with_todo_returns_match() {
    let doc = parse("* DONE A\n* TODO B\n").expect("parse");
    assert_eq!(doc.root_with_todo("TODO").expect("h").title(), "B");
    assert!(doc.root_with_todo("WAIT").is_none());
}

#[test]
fn doc_root_with_priority_returns_match() {
    let doc = parse("* [#A] First\n* [#B] Second\n").expect("parse");
    assert_eq!(doc.root_with_priority('B').expect("h").title(), "Second");
    assert!(doc.root_with_priority('C').is_none());
}

#[test]
fn doc_count_roots_with_todo_counts_match() {
    let doc = parse("* TODO A\n* DONE B\n* TODO C\n").expect("parse");
    assert_eq!(doc.count_roots_with_todo("TODO"), 2);
    assert_eq!(doc.count_roots_with_todo("WAIT"), 0);
}

#[test]
fn doc_count_roots_with_priority_counts_match() {
    let doc = parse("* [#A] One\n* [#B] Two\n* [#A] Three\n").expect("parse");
    assert_eq!(doc.count_roots_with_priority('A'), 2);
    assert_eq!(doc.count_roots_with_priority('C'), 0);
}

#[test]
fn doc_count_roots_with_tag_counts_match() {
    let doc = parse("* A :work:\n* B :home:\n* C :work:\n").expect("parse");
    assert_eq!(doc.count_roots_with_tag("work"), 2);
    assert_eq!(doc.count_roots_with_tag("none"), 0);
}

#[test]
fn doc_descendant_with_id_returns_match() {
    let src = "* Top\n** Inner\n:PROPERTIES:\n:ID: target\n:END:\n";
    let doc = parse(src).expect("parse");
    let h = doc.descendant_with_id("target").expect("hit");
    assert_eq!(h.title(), "Inner");
    assert!(doc.descendant_with_id("missing").is_none());
}

#[test]
fn doc_descendant_with_title_returns_match() {
    let src = "* Top\n** Inner\n*** Leaf\n";
    let doc = parse(src).expect("parse");
    assert_eq!(
        doc.descendant_with_title("Leaf").expect("hit").level(),
        3
    );
    assert!(doc.descendant_with_title("Missing").is_none());
}

#[test]
fn doc_descendant_with_tag_returns_match() {
    let src = "* Top\n** Inner :work:\n*** Leaf :home:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(
        doc.descendant_with_tag("home").expect("hit").title(),
        "Leaf"
    );
    assert!(doc.descendant_with_tag("none").is_none());
}

#[test]
fn doc_descendant_with_todo_returns_match() {
    let src = "* Top\n** TODO Inner\n*** DONE Leaf\n";
    let doc = parse(src).expect("parse");
    assert_eq!(
        doc.descendant_with_todo("DONE").expect("hit").title(),
        "Leaf"
    );
    assert!(doc.descendant_with_todo("WAIT").is_none());
}

#[test]
fn doc_descendant_with_priority_returns_match() {
    let src = "* Top\n** [#A] Inner\n*** [#B] Leaf\n";
    let doc = parse(src).expect("parse");
    assert_eq!(
        doc.descendant_with_priority('B').expect("hit").title(),
        "Leaf"
    );
    assert!(doc.descendant_with_priority('Z').is_none());
}

#[test]
fn doc_count_descendants_with_tag_counts_all() {
    let src = "* A :work:\n** B :work:\n* C :home:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.count_descendants_with_tag("work"), 2);
    assert_eq!(doc.count_descendants_with_tag("home"), 1);
    assert_eq!(doc.count_descendants_with_tag("none"), 0);
}

#[test]
fn doc_count_descendants_with_todo_counts_all() {
    let src = "* TODO A\n** DONE B\n* TODO C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.count_descendants_with_todo("TODO"), 2);
    assert_eq!(doc.count_descendants_with_todo("DONE"), 1);
    assert_eq!(doc.count_descendants_with_todo("WAIT"), 0);
}

#[test]
fn doc_count_descendants_with_priority_counts_all() {
    let src = "* [#A] One\n** [#B] Two\n* [#A] Three\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.count_descendants_with_priority('A'), 2);
    assert_eq!(doc.count_descendants_with_priority('B'), 1);
    assert_eq!(doc.count_descendants_with_priority('C'), 0);
}

#[test]
fn doc_count_descendants_at_level_counts_all() {
    let src = "* A\n** B\n** C\n*** D\n* E\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.count_descendants_at_level(1), 2);
    assert_eq!(doc.count_descendants_at_level(2), 2);
    assert_eq!(doc.count_descendants_at_level(3), 1);
    assert_eq!(doc.count_descendants_at_level(4), 0);
}

#[test]
fn doc_descendants_with_tag_returns_all() {
    let src = "* A :work:\n** B :work:\n* C :home:\n";
    let doc = parse(src).expect("parse");
    let v = doc.descendants_with_tag("work");
    assert_eq!(v.iter().map(|h| h.title()).collect::<Vec<_>>(), ["A", "B"]);
}

#[test]
fn doc_descendants_with_todo_returns_all() {
    let src = "* TODO A\n** DONE B\n* TODO C\n";
    let doc = parse(src).expect("parse");
    let v = doc.descendants_with_todo("TODO");
    assert_eq!(v.iter().map(|h| h.title()).collect::<Vec<_>>(), ["A", "C"]);
}

#[test]
fn doc_descendants_with_priority_returns_all() {
    let src = "* [#A] One\n** [#B] Two\n* [#A] Three\n";
    let doc = parse(src).expect("parse");
    let v = doc.descendants_with_priority('A');
    assert_eq!(
        v.iter().map(|h| h.title()).collect::<Vec<_>>(),
        ["One", "Three"]
    );
}

#[test]
fn doc_descendants_at_level_returns_all() {
    let src = "* A\n** B\n** C\n*** D\n* E\n";
    let doc = parse(src).expect("parse");
    let v = doc.descendants_at_level(2);
    assert_eq!(v.iter().map(|h| h.title()).collect::<Vec<_>>(), ["B", "C"]);
}

#[test]
fn doc_descendant_titles_at_level_returns_strs() {
    let src = "* A\n** B\n** C\n*** D\n* E\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.descendant_titles_at_level(2), vec!["B", "C"]);
    assert_eq!(doc.descendant_titles_at_level(3), vec!["D"]);
    assert!(doc.descendant_titles_at_level(4).is_empty());
}

#[test]
fn doc_descendant_titles_with_tag_returns_strs() {
    let src = "* A :work:\n** B :work:\n* C :home:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.descendant_titles_with_tag("work"), vec!["A", "B"]);
}

#[test]
fn doc_descendant_titles_with_todo_returns_strs() {
    let src = "* TODO A\n** DONE B\n* TODO C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.descendant_titles_with_todo("TODO"), vec!["A", "C"]);
}

#[test]
fn doc_descendants_archived_returns_all() {
    let src = "* A :ARCHIVE:\n** B\n* C :ARCHIVE:\n";
    let doc = parse(src).expect("parse");
    let v = doc.descendants_archived();
    assert_eq!(v.iter().map(|h| h.title()).collect::<Vec<_>>(), ["A", "C"]);
}

#[test]
fn doc_descendants_commented_returns_all() {
    let src = "* COMMENT A\n** B\n* COMMENT C\n";
    let doc = parse(src).expect("parse");
    let v = doc.descendants_commented();
    assert_eq!(
        v.iter().map(|h| h.title()).collect::<Vec<_>>(),
        ["COMMENT A", "COMMENT C"]
    );
}

#[test]
fn doc_descendants_with_property_returns_all() {
    let src = "* A\n:PROPERTIES:\n:CATEGORY: x\n:END:\n* B\n:PROPERTIES:\n:CATEGORY: y\n:END:\n* C\n";
    let doc = parse(src).expect("parse");
    let v = doc.descendants_with_property("CATEGORY");
    assert_eq!(v.iter().map(|h| h.title()).collect::<Vec<_>>(), ["A", "B"]);
}

#[test]
fn doc_count_descendants_with_property_counts_all() {
    let src = "* A\n:PROPERTIES:\n:K: 1\n:END:\n* B\n:PROPERTIES:\n:K: 2\n:END:\n* C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.count_descendants_with_property("K"), 2);
    assert_eq!(doc.count_descendants_with_property("MISSING"), 0);
}

#[test]
fn doc_descendants_with_id_returns_all() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n* B\n:PROPERTIES:\n:ID: b\n:END:\n* C\n";
    let doc = parse(src).expect("parse");
    let v = doc.descendants_with_id();
    assert_eq!(v.iter().map(|h| h.title()).collect::<Vec<_>>(), ["A", "B"]);
}

#[test]
fn doc_count_descendants_with_id_counts_all() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n* B\n* C\n:PROPERTIES:\n:ID: c\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.count_descendants_with_id(), 2);
}

#[test]
fn doc_descendant_titles_with_priority_returns_strs() {
    let src = "* [#A] One\n** [#B] Two\n* [#A] Three\n";
    let doc = parse(src).expect("parse");
    assert_eq!(
        doc.descendant_titles_with_priority('A'),
        vec!["One", "Three"]
    );
}

#[test]
fn doc_descendant_titles_archived_returns_strs() {
    let src = "* A :ARCHIVE:\n** B\n* C :ARCHIVE:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.descendant_titles_archived(), vec!["A", "C"]);
}

#[test]
fn doc_descendant_titles_commented_returns_strs() {
    let src = "* COMMENT A\n** B\n* COMMENT C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(
        doc.descendant_titles_commented(),
        vec!["COMMENT A", "COMMENT C"]
    );
}

#[test]
fn doc_descendant_titles_with_id_returns_strs() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n* B\n* C\n:PROPERTIES:\n:ID: c\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.descendant_titles_with_id(), vec!["A", "C"]);
}

#[test]
fn doc_descendants_with_property_value_returns_match() {
    let src = "* A\n:PROPERTIES:\n:K: x\n:END:\n* B\n:PROPERTIES:\n:K: y\n:END:\n* C\n:PROPERTIES:\n:K: x\n:END:\n";
    let doc = parse(src).expect("parse");
    let v = doc.descendants_with_property_value("K", "x");
    assert_eq!(v.iter().map(|h| h.title()).collect::<Vec<_>>(), ["A", "C"]);
    assert!(doc.descendants_with_property_value("K", "z").is_empty());
}

#[test]
fn doc_count_descendants_with_property_value_counts_match() {
    let src = "* A\n:PROPERTIES:\n:K: x\n:END:\n* B\n:PROPERTIES:\n:K: y\n:END:\n* C\n:PROPERTIES:\n:K: x\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.count_descendants_with_property_value("K", "x"), 2);
    assert_eq!(doc.count_descendants_with_property_value("K", "y"), 1);
    assert_eq!(doc.count_descendants_with_property_value("MISSING", "x"), 0);
}

#[test]
fn doc_roots_archived_returns_match() {
    let src = "* A :ARCHIVE:\n* B\n* C :ARCHIVE:\n";
    let doc = parse(src).expect("parse");
    let v = doc.roots_archived();
    assert_eq!(v.iter().map(|h| h.title()).collect::<Vec<_>>(), ["A", "C"]);
}

#[test]
fn doc_roots_commented_returns_match() {
    let src = "* COMMENT A\n* B\n* COMMENT C\n";
    let doc = parse(src).expect("parse");
    let v = doc.roots_commented();
    assert_eq!(
        v.iter().map(|h| h.title()).collect::<Vec<_>>(),
        ["COMMENT A", "COMMENT C"]
    );
}

#[test]
fn doc_count_roots_archived_counts_match() {
    let src = "* A :ARCHIVE:\n* B\n* C :ARCHIVE:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.count_roots_archived(), 2);
}

#[test]
fn doc_count_roots_commented_counts_match() {
    let src = "* COMMENT A\n* B\n* COMMENT C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.count_roots_commented(), 2);
}

#[test]
fn doc_roots_with_tag_returns_all() {
    let src = "* A :work:\n* B :home:\n* C :work:\n";
    let doc = parse(src).expect("parse");
    let v = doc.roots_with_tag("work");
    assert_eq!(v.iter().map(|h| h.title()).collect::<Vec<_>>(), ["A", "C"]);
}

#[test]
fn doc_roots_with_todo_returns_all() {
    let src = "* TODO A\n* DONE B\n* TODO C\n";
    let doc = parse(src).expect("parse");
    let v = doc.roots_with_todo("TODO");
    assert_eq!(v.iter().map(|h| h.title()).collect::<Vec<_>>(), ["A", "C"]);
}

#[test]
fn doc_roots_with_priority_returns_all() {
    let src = "* [#A] One\n* [#B] Two\n* [#A] Three\n";
    let doc = parse(src).expect("parse");
    let v = doc.roots_with_priority('A');
    assert_eq!(
        v.iter().map(|h| h.title()).collect::<Vec<_>>(),
        ["One", "Three"]
    );
}

#[test]
fn doc_root_titles_with_tag_returns_strs() {
    let src = "* A :work:\n* B :home:\n* C :work:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.root_titles_with_tag("work"), vec!["A", "C"]);
}

#[test]
fn doc_root_titles_with_todo_returns_strs() {
    let src = "* TODO A\n* DONE B\n* TODO C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.root_titles_with_todo("TODO"), vec!["A", "C"]);
}

#[test]
fn doc_root_titles_with_priority_returns_strs() {
    let src = "* [#A] One\n* [#B] Two\n* [#A] Three\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.root_titles_with_priority('A'), vec!["One", "Three"]);
}

#[test]
fn doc_root_titles_archived_returns_strs() {
    let src = "* A :ARCHIVE:\n* B\n* C :ARCHIVE:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.root_titles_archived(), vec!["A", "C"]);
}

#[test]
fn doc_root_titles_commented_returns_strs() {
    let src = "* COMMENT A\n* B\n* COMMENT C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(
        doc.root_titles_commented(),
        vec!["COMMENT A", "COMMENT C"]
    );
}

#[test]
fn doc_roots_with_id_returns_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n* B\n* C\n:PROPERTIES:\n:ID: c\n:END:\n";
    let doc = parse(src).expect("parse");
    let v = doc.roots_with_id();
    assert_eq!(v.iter().map(|h| h.title()).collect::<Vec<_>>(), ["A", "C"]);
}

#[test]
fn doc_root_titles_with_id_returns_strs() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n* B\n* C\n:PROPERTIES:\n:ID: c\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.root_titles_with_id(), vec!["A", "C"]);
}

#[test]
fn doc_count_roots_with_id_counts_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n* B\n* C\n:PROPERTIES:\n:ID: c\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.count_roots_with_id(), 2);
}

#[test]
fn doc_roots_with_property_returns_match() {
    let src = "* A\n:PROPERTIES:\n:K: 1\n:END:\n* B\n* C\n:PROPERTIES:\n:K: 2\n:END:\n";
    let doc = parse(src).expect("parse");
    let v = doc.roots_with_property("K");
    assert_eq!(v.iter().map(|h| h.title()).collect::<Vec<_>>(), ["A", "C"]);
}

#[test]
fn doc_count_roots_with_property_counts_match() {
    let src = "* A\n:PROPERTIES:\n:K: 1\n:END:\n* B\n* C\n:PROPERTIES:\n:K: 2\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.count_roots_with_property("K"), 2);
    assert_eq!(doc.count_roots_with_property("MISSING"), 0);
}

#[test]
fn doc_root_titles_with_property_returns_strs() {
    let src = "* A\n:PROPERTIES:\n:K: 1\n:END:\n* B\n* C\n:PROPERTIES:\n:K: 2\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.root_titles_with_property("K"), vec!["A", "C"]);
}

#[test]
fn doc_distinct_root_tags_sorted_unique() {
    let src = "* A :z:y:\n* B :a:y:\n* C :a:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(
        doc.distinct_root_tags(),
        vec!["a".to_owned(), "y".to_owned(), "z".to_owned()]
    );
}

#[test]
fn doc_distinct_root_todos_sorted_unique() {
    let src = "* TODO A\n* DONE B\n* TODO C\n* DONE D\n";
    let doc = parse(src).expect("parse");
    assert_eq!(
        doc.distinct_root_todos(),
        vec!["DONE".to_owned(), "TODO".to_owned()]
    );
}

#[test]
fn doc_distinct_root_priorities_sorted_unique() {
    let src = "* [#B] One\n* [#A] Two\n* [#A] Three\n* [#C] Four\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.distinct_root_priorities(), vec!['A', 'B', 'C']);
}

#[test]
fn doc_distinct_descendant_tags_sorted_unique() {
    let src = "* A :z:y:\n** B :a:y:\n* C :a:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(
        doc.distinct_descendant_tags(),
        vec!["a".to_owned(), "y".to_owned(), "z".to_owned()]
    );
}

#[test]
fn doc_distinct_descendant_todos_sorted_unique() {
    let src = "* TODO A\n** DONE B\n* TODO C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(
        doc.distinct_descendant_todos(),
        vec!["DONE".to_owned(), "TODO".to_owned()]
    );
}

#[test]
fn doc_distinct_descendant_priorities_sorted_unique() {
    let src = "* [#A] One\n** [#B] Two\n* [#A] Three\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.distinct_descendant_priorities(), vec!['A', 'B']);
}

#[test]
fn doc_distinct_descendant_levels_sorted_unique() {
    let src = "* A\n** B\n*** C\n** D\n* E\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.distinct_descendant_levels(), vec![1, 2, 3]);
}

#[test]
fn doc_distinct_descendant_tag_count_match() {
    let src = "* A :z:y:\n** B :a:y:\n* C :a:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.distinct_descendant_tag_count(), 3);
}

#[test]
fn doc_distinct_descendant_priority_count_match() {
    let src = "* [#A] One\n** [#B] Two\n* [#A] Three\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.distinct_descendant_priority_count(), 2);
}

#[test]
fn doc_distinct_descendant_todo_count_match() {
    let src = "* TODO A\n** DONE B\n* TODO C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.distinct_descendant_todo_count(), 2);
}

#[test]
fn doc_distinct_descendant_level_count_match() {
    let src = "* A\n** B\n*** C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.distinct_descendant_level_count(), 3);
}

#[test]
fn doc_distinct_root_tag_count_match() {
    let src = "* A :z:y:\n* B :a:y:\n* C :a:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.distinct_root_tag_count(), 3);
}

#[test]
fn doc_distinct_root_todo_count_match() {
    let src = "* TODO A\n* DONE B\n* TODO C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.distinct_root_todo_count(), 2);
}

#[test]
fn doc_distinct_root_priority_count_match() {
    let src = "* [#B] One\n* [#A] Two\n* [#A] Three\n* [#C] Four\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.distinct_root_priority_count(), 3);
}

#[test]
fn doc_root_tag_counts_returns_map() {
    let src = "* A :z:y:\n* B :y:\n* C :z:\n";
    let doc = parse(src).expect("parse");
    let m = doc.root_tag_counts();
    assert_eq!(m.get("y").copied(), Some(2));
    assert_eq!(m.get("z").copied(), Some(2));
    assert_eq!(m.get("missing").copied(), None);
}

#[test]
fn doc_root_todo_counts_returns_map() {
    let src = "* TODO A\n* DONE B\n* TODO C\n";
    let doc = parse(src).expect("parse");
    let m = doc.root_todo_counts();
    assert_eq!(m.get("TODO").copied(), Some(2));
    assert_eq!(m.get("DONE").copied(), Some(1));
}

#[test]
fn doc_root_priority_counts_returns_map() {
    let src = "* [#A] One\n* [#A] Two\n* [#B] Three\n";
    let doc = parse(src).expect("parse");
    let m = doc.root_priority_counts();
    assert_eq!(m.get(&'A').copied(), Some(2));
    assert_eq!(m.get(&'B').copied(), Some(1));
}

#[test]
fn doc_descendant_tag_counts_returns_map() {
    let src = "* A :z:y:\n** B :y:\n* C :z:\n";
    let doc = parse(src).expect("parse");
    let m = doc.descendant_tag_counts();
    assert_eq!(m.get("y").copied(), Some(2));
    assert_eq!(m.get("z").copied(), Some(2));
}

#[test]
fn doc_descendant_todo_counts_returns_map() {
    let src = "* TODO A\n** DONE B\n* TODO C\n";
    let doc = parse(src).expect("parse");
    let m = doc.descendant_todo_counts();
    assert_eq!(m.get("TODO").copied(), Some(2));
    assert_eq!(m.get("DONE").copied(), Some(1));
}

#[test]
fn doc_descendant_priority_counts_returns_map() {
    let src = "* [#A] One\n** [#A] Two\n* [#B] Three\n";
    let doc = parse(src).expect("parse");
    let m = doc.descendant_priority_counts();
    assert_eq!(m.get(&'A').copied(), Some(2));
    assert_eq!(m.get(&'B').copied(), Some(1));
}

#[test]
fn doc_descendant_level_counts_returns_map() {
    let src = "* A\n** B\n** C\n*** D\n* E\n";
    let doc = parse(src).expect("parse");
    let m = doc.descendant_level_counts();
    assert_eq!(m.get(&1).copied(), Some(2));
    assert_eq!(m.get(&2).copied(), Some(2));
    assert_eq!(m.get(&3).copied(), Some(1));
}

#[test]
fn doc_most_common_descendant_tag_returns_top() {
    let src = "* A :a:b:\n** B :a:\n* C :b:\n** D :a:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.most_common_descendant_tag(), Some("a".to_owned()));
}

#[test]
fn doc_most_common_descendant_todo_returns_top() {
    let src = "* TODO A\n** DONE B\n* TODO C\n* TODO D\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.most_common_descendant_todo(), Some("TODO".to_owned()));
}

#[test]
fn doc_most_common_descendant_priority_returns_top() {
    let src = "* [#A] One\n** [#A] Two\n* [#B] Three\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.most_common_descendant_priority(), Some('A'));
}

#[test]
fn doc_most_common_descendant_level_returns_top() {
    let src = "* A\n** B\n** C\n*** D\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.most_common_descendant_level(), Some(2));
}

#[test]
fn doc_most_common_root_tag_returns_top() {
    let src = "* A :a:b:\n* B :a:\n* C :b:\n";
    let doc = parse(src).expect("parse");
    let m = doc.most_common_root_tag();
    assert!(m == Some("a".to_owned()) || m == Some("b".to_owned()));
}

#[test]
fn doc_most_common_root_todo_returns_top() {
    let src = "* TODO A\n* DONE B\n* TODO C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.most_common_root_todo(), Some("TODO".to_owned()));
}

#[test]
fn doc_most_common_root_priority_returns_top() {
    let src = "* [#A] One\n* [#A] Two\n* [#B] Three\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.most_common_root_priority(), Some('A'));
}

#[test]
fn headline_subtree_tags_sorted_unique() {
    let src = "* Top :a:\n** Inner :b:a:\n*** Leaf :c:\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(
        top.subtree_tags(),
        vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]
    );
}

#[test]
fn headline_subtree_todos_sorted_unique() {
    let src = "* TODO Top\n** DONE Inner\n*** TODO Leaf\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(
        top.subtree_todos(),
        vec!["DONE".to_owned(), "TODO".to_owned()]
    );
}

#[test]
fn headline_subtree_priorities_sorted_unique() {
    let src = "* [#B] Top\n** [#A] Inner\n*** [#A] Leaf\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_priorities(), vec!['A', 'B']);
}

#[test]
fn headline_subtree_levels_sorted_unique() {
    let src = "* Top\n** Inner\n*** Leaf\n** Other\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_levels(), vec![1, 2, 3]);
}

#[test]
fn headline_subtree_tag_count_match() {
    let src = "* Top :a:\n** Inner :b:a:\n*** Leaf :c:\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_tag_count(), 3);
}

#[test]
fn headline_subtree_todo_count_match() {
    let src = "* TODO Top\n** DONE Inner\n*** TODO Leaf\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_todo_count(), 2);
}

#[test]
fn headline_subtree_priority_count_match() {
    let src = "* [#B] Top\n** [#A] Inner\n*** [#A] Leaf\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_priority_count(), 2);
}

#[test]
fn headline_subtree_level_count_match() {
    let src = "* Top\n** Inner\n*** Leaf\n** Other\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_level_count(), 3);
}

#[test]
fn headline_max_priority_letter_returns_highest() {
    let src = "* [#C] Top\n** [#A] Inner\n*** [#B] Leaf\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.max_priority_letter(), Some('A'));
}

#[test]
fn headline_min_priority_letter_returns_lowest() {
    let src = "* [#C] Top\n** [#A] Inner\n*** [#B] Leaf\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.min_priority_letter(), Some('C'));
}

#[test]
fn headline_first_todo_keyword_returns_first() {
    let src = "* TODO Top\n** DONE Inner\n*** TODO Leaf\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.first_todo_keyword(), Some("TODO"));
}

#[test]
fn headline_subtree_max_level_match() {
    let src = "* Top\n** Inner\n*** Leaf\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_max_level(), 3);
}

#[test]
fn headline_subtree_min_level_match() {
    let src = "* Top\n** Inner\n*** Leaf\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_min_level(), 1);
}

#[test]
fn headline_first_priority_letter_returns_first() {
    let src = "* Top\n** [#B] Inner\n*** [#A] Leaf\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.first_priority_letter(), Some('B'));
}

#[test]
fn headline_first_tag_returns_first() {
    let src = "* Top\n** Inner :work:\n*** Leaf :home:\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.first_tag(), Some("work"));
}

#[test]
fn headline_subtree_size_counts_self_and_descendants() {
    let src = "* Top\n** Inner\n*** Leaf\n** Other\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_size(), 4);
}

#[test]
fn headline_subtree_has_tag_returns_match() {
    let src = "* Top\n** Inner :work:\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert!(top.subtree_has_tag("work"));
    assert!(!top.subtree_has_tag("none"));
}

#[test]
fn headline_subtree_has_todo_returns_match() {
    let src = "* Top\n** TODO Inner\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert!(top.subtree_has_todo("TODO"));
    assert!(!top.subtree_has_todo("DONE"));
}

#[test]
fn headline_subtree_has_priority_returns_match() {
    let src = "* Top\n** [#A] Inner\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert!(top.subtree_has_priority('A'));
    assert!(!top.subtree_has_priority('B'));
}

#[test]
fn headline_subtree_contains_id_returns_match() {
    let src = "* Top\n** Inner\n:PROPERTIES:\n:ID: target\n:END:\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert!(top.subtree_contains_id("target"));
    assert!(!top.subtree_contains_id("missing"));
}

#[test]
fn headline_subtree_has_id_returns_match() {
    let src = "* Top\n** Inner\n:PROPERTIES:\n:ID: x\n:END:\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert!(top.subtree_has_id());
}

#[test]
fn headline_subtree_has_id_returns_false_when_absent() {
    let src = "* Top\n** Inner\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert!(!top.subtree_has_id());
}

#[test]
fn headline_subtree_count_with_tag_counts_self_plus_descendants() {
    let src = "* Top :work:\n** Inner :work:\n*** Leaf :home:\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_count_with_tag("work"), 2);
    assert_eq!(top.subtree_count_with_tag("home"), 1);
    assert_eq!(top.subtree_count_with_tag("none"), 0);
}

#[test]
fn headline_subtree_count_with_todo_counts_self_plus_descendants() {
    let src = "* TODO Top\n** DONE Inner\n*** TODO Leaf\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_count_with_todo("TODO"), 2);
    assert_eq!(top.subtree_count_with_todo("DONE"), 1);
}

#[test]
fn headline_subtree_count_with_priority_counts_self_plus_descendants() {
    let src = "* [#A] Top\n** [#A] Inner\n*** [#B] Leaf\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_count_with_priority('A'), 2);
    assert_eq!(top.subtree_count_with_priority('B'), 1);
}

#[test]
fn headline_subtree_count_at_level_counts_self_plus_descendants() {
    let src = "* Top\n** A\n** B\n*** C\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_count_at_level(1), 1);
    assert_eq!(top.subtree_count_at_level(2), 2);
    assert_eq!(top.subtree_count_at_level(3), 1);
}

#[test]
fn headline_subtree_count_with_property_counts_self_plus_descendants() {
    let src = "* Top\n:PROPERTIES:\n:K: 1\n:END:\n** Inner\n:PROPERTIES:\n:K: 2\n:END:\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_count_with_property("K"), 2);
    assert_eq!(top.subtree_count_with_property("MISSING"), 0);
}

#[test]
fn headline_subtree_count_with_id_counts_self_plus_descendants() {
    let src = "* Top\n:PROPERTIES:\n:ID: a\n:END:\n** Inner\n:PROPERTIES:\n:ID: b\n:END:\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_count_with_id(), 2);
}

#[test]
fn doc_max_level_match() {
    let doc = parse("* A\n** B\n*** C\n").expect("parse");
    assert_eq!(doc.max_level(), 3);
}

#[test]
fn doc_max_level_zero_on_empty() {
    let doc = parse("nothing\n").expect("parse");
    assert_eq!(doc.max_level(), 0);
}

#[test]
fn doc_level_range_match() {
    let doc = parse("* A\n** B\n*** C\n").expect("parse");
    assert_eq!(doc.level_range(), Some((1, 3)));
}

#[test]
fn doc_level_range_none_on_empty() {
    let doc = parse("nothing\n").expect("parse");
    assert_eq!(doc.level_range(), None);
}

#[test]
fn doc_total_headline_count_match() {
    let doc = parse("* A\n** B\n*** C\n* D\n").expect("parse");
    assert_eq!(doc.total_headline_count(), 4);
}

#[test]
fn doc_mean_level_match() {
    let doc = parse("* A\n** B\n** C\n").expect("parse");
    // levels: 1,2,2 -> (1+2+2)/3 = 1 (integer)
    assert_eq!(doc.mean_level(), 1);
}

#[test]
fn doc_mean_level_zero_when_empty() {
    let doc = parse("nothing\n").expect("parse");
    assert_eq!(doc.mean_level(), 0);
}

#[test]
fn doc_median_level_odd_match() {
    let doc = parse("* A\n** B\n*** C\n").expect("parse");
    // levels sorted: [1,2,3] -> 2
    assert_eq!(doc.median_level(), Some(2));
}

#[test]
fn doc_median_level_even_match() {
    let doc = parse("* A\n** B\n** C\n*** D\n").expect("parse");
    // levels sorted: [1,2,2,3] -> midpoint(2,2) = 2
    assert_eq!(doc.median_level(), Some(2));
}

#[test]
fn doc_mode_level_match() {
    let doc = parse("* A\n** B\n** C\n*** D\n").expect("parse");
    assert_eq!(doc.mode_level(), Some(2));
}

#[test]
fn doc_mode_level_none_on_empty() {
    let doc = parse("nothing\n").expect("parse");
    assert_eq!(doc.mode_level(), None);
}

#[test]
fn doc_mean_priority_match() {
    let doc = parse("* [#A] X\n** [#A] Y\n** [#C] Z\n").expect("parse");
    // letters as u32: A=65, A=65, C=67 -> mean = 65
    assert_eq!(doc.mean_priority_letter(), Some('A'));
}

#[test]
fn doc_mean_priority_none_on_empty() {
    let doc = parse("nothing\n").expect("parse");
    assert_eq!(doc.mean_priority_letter(), None);
}

#[test]
fn doc_mode_priority_match() {
    let doc = parse("* [#A] X\n** [#A] Y\n** [#B] Z\n").expect("parse");
    assert_eq!(doc.mode_priority(), Some('A'));
}

#[test]
fn doc_mode_priority_none_when_unset() {
    let doc = parse("* X\n* Y\n").expect("parse");
    assert_eq!(doc.mode_priority(), None);
}

#[test]
fn doc_mode_todo_match() {
    let doc = parse("* TODO X\n* DONE Y\n* TODO Z\n").expect("parse");
    assert_eq!(doc.mode_todo(), Some("TODO".to_owned()));
}

#[test]
fn doc_mode_tag_match() {
    let doc = parse("* X :work:\n* Y :work:home:\n* Z :home:\n").expect("parse");
    let m = doc.mode_tag();
    assert!(m == Some("work".to_owned()) || m == Some("home".to_owned()));
}

#[test]
fn doc_total_priority_count_match() {
    let doc = parse("* [#A] X\n* Y\n** [#B] Z\n").expect("parse");
    assert_eq!(doc.total_priority_count(), 2);
}

#[test]
fn doc_total_todo_count_match() {
    let doc = parse("* TODO X\n* Y\n** DONE Z\n").expect("parse");
    assert_eq!(doc.total_todo_count(), 2);
}

#[test]
fn doc_total_id_count_match() {
    let src = "* X\n:PROPERTIES:\n:ID: a\n:END:\n* Y\n** Z\n:PROPERTIES:\n:ID: z\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.total_id_count(), 2);
}

#[test]
fn doc_priority_pct_match() {
    let doc = parse("* [#A] X\n* Y\n* [#B] Z\n* W\n").expect("parse");
    // 2/4 = 50
    assert_eq!(doc.priority_pct(), 50);
}

#[test]
fn doc_priority_pct_zero_when_empty() {
    let doc = parse("nothing\n").expect("parse");
    assert_eq!(doc.priority_pct(), 0);
}

#[test]
fn doc_todo_pct_match() {
    let doc = parse("* TODO X\n* Y\n* DONE Z\n* W\n").expect("parse");
    // 2/4 = 50
    assert_eq!(doc.todo_pct(), 50);
}

#[test]
fn doc_id_pct_match() {
    let src = "* X\n:PROPERTIES:\n:ID: a\n:END:\n* Y\n";
    let doc = parse(src).expect("parse");
    // 1/2 = 50
    assert_eq!(doc.id_pct(), 50);
}

#[test]
fn doc_tagged_pct_match() {
    let doc = parse("* X :work:\n* Y\n* Z :home:\n* W\n").expect("parse");
    // 2/4 = 50
    assert_eq!(doc.tagged_pct(), 50);
}

#[test]
fn doc_tagged_pct_zero_when_empty() {
    let doc = parse("nothing\n").expect("parse");
    assert_eq!(doc.tagged_pct(), 0);
}

#[test]
fn doc_untagged_count_match() {
    let doc = parse("* X :work:\n* Y\n* Z :home:\n* W\n").expect("parse");
    assert_eq!(doc.untagged_count(), 2);
}

#[test]
fn doc_untagged_pct_match() {
    let doc = parse("* X :work:\n* Y\n* Z :home:\n* W\n").expect("parse");
    assert_eq!(doc.untagged_pct(), 50);
}

#[test]
fn doc_no_priority_count_match() {
    let doc = parse("* [#A] X\n* Y\n* [#B] Z\n* W\n").expect("parse");
    assert_eq!(doc.no_priority_count(), 2);
}

#[test]
fn doc_no_todo_count_match() {
    let doc = parse("* TODO X\n* Y\n* DONE Z\n* W\n").expect("parse");
    assert_eq!(doc.no_todo_count(), 2);
}

#[test]
fn doc_no_id_count_match() {
    let src = "* X\n:PROPERTIES:\n:ID: a\n:END:\n* Y\n* Z\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.no_id_count(), 2);
}

#[test]
fn doc_no_priority_pct_match() {
    let doc = parse("* [#A] X\n* Y\n* [#B] Z\n* W\n").expect("parse");
    assert_eq!(doc.no_priority_pct(), 50);
}

#[test]
fn doc_no_todo_pct_match() {
    let doc = parse("* TODO X\n* Y\n* DONE Z\n* W\n").expect("parse");
    assert_eq!(doc.no_todo_pct(), 50);
}

#[test]
fn doc_no_id_pct_match() {
    let src = "* X\n:PROPERTIES:\n:ID: a\n:END:\n* Y\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.no_id_pct(), 50);
}

#[test]
fn doc_has_any_priority_match() {
    let doc = parse("* [#A] X\n* Y\n").expect("parse");
    assert!(doc.has_any_priority());
    let none = parse("* X\n* Y\n").expect("parse");
    assert!(!none.has_any_priority());
}

#[test]
fn doc_has_any_todo_match() {
    let doc = parse("* TODO X\n* Y\n").expect("parse");
    assert!(doc.has_any_todo());
    let none = parse("* X\n* Y\n").expect("parse");
    assert!(!none.has_any_todo());
}

#[test]
fn doc_has_any_tag_match() {
    let doc = parse("* X :work:\n").expect("parse");
    assert!(doc.has_any_tag());
    let none = parse("* X\n").expect("parse");
    assert!(!none.has_any_tag());
}

#[test]
fn doc_has_any_id_match() {
    let src = "* X\n:PROPERTIES:\n:ID: a\n:END:\n";
    let doc = parse(src).expect("parse");
    assert!(doc.has_any_id());
    let none = parse("* X\n").expect("parse");
    assert!(!none.has_any_id());
}

#[test]
fn doc_contains_tag_returns_match() {
    let doc = parse("* X :work:\n* Y\n").expect("parse");
    assert!(doc.contains_tag("work"));
    assert!(!doc.contains_tag("home"));
}

#[test]
fn doc_contains_todo_returns_match() {
    let doc = parse("* TODO X\n* DONE Y\n").expect("parse");
    assert!(doc.contains_todo("TODO"));
    assert!(doc.contains_todo("DONE"));
    assert!(!doc.contains_todo("WAIT"));
}

#[test]
fn doc_contains_priority_returns_match() {
    let doc = parse("* [#A] X\n* Y\n").expect("parse");
    assert!(doc.contains_priority('A'));
    assert!(!doc.contains_priority('B'));
}

#[test]
fn doc_contains_id_returns_match() {
    let src = "* X\n:PROPERTIES:\n:ID: target\n:END:\n";
    let doc = parse(src).expect("parse");
    assert!(doc.contains_id("target"));
    assert!(!doc.contains_id("missing"));
}

#[test]
fn doc_headline_with_property_value_returns_first() {
    let src = "* A\n:PROPERTIES:\n:K: x\n:END:\n* B\n:PROPERTIES:\n:K: y\n:END:\n";
    let doc = parse(src).expect("parse");
    let h = doc.headline_with_property_value("K", "y").expect("hit");
    assert_eq!(h.title(), "B");
    assert!(doc.headline_with_property_value("K", "z").is_none());
}

#[test]
fn doc_first_with_priority_returns_first() {
    let doc = parse("* X\n* [#B] Y\n* [#A] Z\n").expect("parse");
    let h = doc.first_with_priority('A').expect("hit");
    assert_eq!(h.title(), "Z");
}

#[test]
fn doc_first_with_todo_returns_first() {
    let doc = parse("* X\n* TODO Y\n* DONE Z\n").expect("parse");
    let h = doc.first_with_todo("TODO").expect("hit");
    assert_eq!(h.title(), "Y");
}

#[test]
fn doc_first_with_tag_returns_first() {
    let doc = parse("* X\n* Y :work:\n* Z :work:home:\n").expect("parse");
    let h = doc.first_with_tag("work").expect("hit");
    assert_eq!(h.title(), "Y");
}

#[test]
fn doc_last_with_priority_returns_last() {
    let doc = parse("* [#A] X\n* Y\n* [#A] Z\n").expect("parse");
    let h = doc.last_with_priority('A').expect("hit");
    assert_eq!(h.title(), "Z");
}

#[test]
fn doc_last_with_todo_returns_last() {
    let doc = parse("* TODO X\n* Y\n* TODO Z\n").expect("parse");
    let h = doc.last_with_todo("TODO").expect("hit");
    assert_eq!(h.title(), "Z");
}

#[test]
fn doc_last_with_tag_returns_last() {
    let doc = parse("* X :work:\n* Y\n* Z :work:\n").expect("parse");
    let h = doc.last_with_tag("work").expect("hit");
    assert_eq!(h.title(), "Z");
}

#[test]
fn doc_last_with_property_returns_last() {
    let src = "* A\n:PROPERTIES:\n:K: 1\n:END:\n* B\n* C\n:PROPERTIES:\n:K: 2\n:END:\n";
    let doc = parse(src).expect("parse");
    let h = doc.last_with_property("K").expect("hit");
    assert_eq!(h.title(), "C");
}

#[test]
fn doc_last_with_id_returns_last() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n* B\n* C\n:PROPERTIES:\n:ID: c\n:END:\n";
    let doc = parse(src).expect("parse");
    let h = doc.last_with_id().expect("hit");
    assert_eq!(h.title(), "C");
}

#[test]
fn doc_first_with_id_returns_first() {
    let src = "* A\n* B\n:PROPERTIES:\n:ID: b\n:END:\n";
    let doc = parse(src).expect("parse");
    let h = doc.first_with_id().expect("hit");
    assert_eq!(h.title(), "B");
}

#[test]
fn doc_headline_at_dfs_index_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    assert_eq!(doc.headline_at_dfs_index(0).expect("0").title(), "A");
    assert_eq!(doc.headline_at_dfs_index(1).expect("1").title(), "B");
    assert_eq!(doc.headline_at_dfs_index(2).expect("2").title(), "C");
    assert_eq!(doc.headline_at_dfs_index(3).expect("3").title(), "D");
    assert!(doc.headline_at_dfs_index(4).is_none());
}

#[test]
fn doc_dfs_index_of_returns_index() {
    let src = "* A\n** B\n:PROPERTIES:\n:ID: b\n:END:\n** C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.dfs_index_of("b"), Some(1));
    assert_eq!(doc.dfs_index_of("missing"), None);
}

#[test]
fn doc_first_headline_returns_first() {
    let doc = parse("* X\n** Y\n").expect("parse");
    assert_eq!(doc.first_headline().expect("first").title(), "X");
}

#[test]
fn doc_first_headline_none_on_empty() {
    let doc = parse("nothing\n").expect("parse");
    assert!(doc.first_headline().is_none());
}

#[test]
fn doc_last_headline_returns_last() {
    let doc = parse("* X\n** Y\n* Z\n").expect("parse");
    assert_eq!(doc.last_headline().expect("last").title(), "Z");
}

#[test]
fn doc_all_titles_returns_collection() {
    let doc = parse("* A\n** B\n* C\n").expect("parse");
    assert_eq!(doc.all_titles(), vec!["A", "B", "C"]);
}

#[test]
fn doc_distinct_titles_sorted_unique() {
    let doc = parse("* A\n** A\n* B\n").expect("parse");
    assert_eq!(doc.distinct_titles(), vec!["A".to_owned(), "B".to_owned()]);
}

#[test]
fn doc_root_titles_returns_collection() {
    let doc = parse("* A\n** B\n* C\n").expect("parse");
    assert_eq!(doc.root_titles(), vec!["A", "C"]);
}

#[test]
fn doc_root_levels_returns_collection() {
    let doc = parse("* A\n* B\n").expect("parse");
    assert_eq!(doc.root_levels(), vec![1, 1]);
}

#[test]
fn doc_headline_levels_returns_collection() {
    let doc = parse("* A\n** B\n*** C\n* D\n").expect("parse");
    assert_eq!(doc.headline_levels(), vec![1, 2, 3, 1]);
}

#[test]
fn doc_headline_priorities_returns_collection() {
    let doc = parse("* [#A] X\n* Y\n** [#B] Z\n").expect("parse");
    assert_eq!(doc.headline_priorities(), vec!['A', 'B']);
}

#[test]
fn doc_headline_todos_returns_collection() {
    let doc = parse("* TODO X\n* Y\n** DONE Z\n").expect("parse");
    assert_eq!(doc.headline_todos(), vec!["TODO", "DONE"]);
}

#[test]
fn doc_headline_tags_returns_flat_collection() {
    let doc = parse("* X :a:b:\n* Y :c:\n").expect("parse");
    assert_eq!(doc.headline_tags(), vec!["a", "b", "c"]);
}

#[test]
fn doc_tag_set_returns_distinct_sorted() {
    let doc = parse("* X :b:a:\n** Y :c:a:\n").expect("parse");
    let s = doc.tag_set();
    let v: Vec<&&str> = s.iter().collect();
    assert_eq!(v, vec![&"a", &"b", &"c"]);
}

#[test]
fn doc_priority_set_returns_distinct_sorted() {
    let doc = parse("* [#B] X\n** [#A] Y\n* [#A] Z\n").expect("parse");
    let s = doc.priority_set();
    let v: Vec<char> = s.into_iter().collect();
    assert_eq!(v, vec!['A', 'B']);
}

#[test]
fn doc_todo_set_returns_distinct_sorted() {
    let doc = parse("* TODO X\n* DONE Y\n* TODO Z\n").expect("parse");
    let s = doc.todo_set();
    let v: Vec<String> = s.into_iter().collect();
    assert_eq!(v, vec!["DONE".to_owned(), "TODO".to_owned()]);
}

#[test]
fn doc_level_set_returns_distinct_sorted() {
    let doc = parse("* A\n** B\n*** C\n** D\n").expect("parse");
    let s = doc.level_set();
    let v: Vec<u8> = s.into_iter().collect();
    assert_eq!(v, vec![1, 2, 3]);
}

#[test]
fn doc_tag_set_count_match() {
    let doc = parse("* X :a:b:\n** Y :c:\n").expect("parse");
    assert_eq!(doc.tag_set_count(), 3);
}

#[test]
fn doc_priority_set_count_match() {
    let doc = parse("* [#A] X\n* [#A] Y\n* [#B] Z\n").expect("parse");
    assert_eq!(doc.priority_set_count(), 2);
}

#[test]
fn doc_todo_set_count_match() {
    let doc = parse("* TODO X\n* DONE Y\n").expect("parse");
    assert_eq!(doc.todo_set_count(), 2);
}

#[test]
fn doc_level_set_count_match() {
    let doc = parse("* A\n** B\n*** C\n").expect("parse");
    assert_eq!(doc.level_set_count(), 3);
}

#[test]
fn doc_source_byte_len_match() {
    let src = "* A\n** B\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.source_byte_len(), src.len());
}

#[test]
fn doc_source_line_count_match() {
    let doc = parse("* A\n** B\n* C\n").expect("parse");
    assert_eq!(doc.source_line_count(), 3);
}

#[test]
fn doc_source_line_count_empty_zero() {
    let doc = parse("").expect("parse");
    assert_eq!(doc.source_line_count(), 0);
}

#[test]
fn doc_source_word_count_match() {
    let doc = parse("* Hello world\n** Foo bar baz\n").expect("parse");
    // tokens: "*", "Hello", "world", "**", "Foo", "bar", "baz" => 7
    assert_eq!(doc.source_word_count(), 7);
}

#[test]
fn doc_source_char_count_match() {
    let src = "* A\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.source_char_count(), src.chars().count());
}

#[test]
fn doc_max_root_subtree_size_match() {
    let doc = parse("* A\n** B\n*** C\n* D\n").expect("parse");
    // subtree sizes: A=3, D=1
    assert_eq!(doc.max_root_subtree_size(), Some(3));
}

#[test]
fn doc_min_root_subtree_size_match() {
    let doc = parse("* A\n** B\n* C\n").expect("parse");
    // subtree sizes: A=2, C=1
    assert_eq!(doc.min_root_subtree_size(), Some(1));
}

#[test]
fn doc_max_root_subtree_size_none_empty() {
    let doc = parse("nothing\n").expect("parse");
    assert_eq!(doc.max_root_subtree_size(), None);
}

#[test]
fn doc_mean_root_subtree_size_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    // sizes: 3, 1 -> 2
    assert_eq!(doc.mean_root_subtree_size(), 2);
}

#[test]
fn doc_mean_root_subtree_size_zero_when_empty() {
    let doc = parse("nothing\n").expect("parse");
    assert_eq!(doc.mean_root_subtree_size(), 0);
}

#[test]
fn doc_median_root_subtree_size_match() {
    let doc = parse("* A\n** B\n** C\n* D\n* E\n").expect("parse");
    // sizes sorted: [1,1,3] -> 1
    assert_eq!(doc.median_root_subtree_size(), Some(1));
}

#[test]
fn doc_total_root_subtree_size_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    // sizes: 3 + 1 = 4
    assert_eq!(doc.total_root_subtree_size(), 4);
}

#[test]
fn doc_largest_root_returns_largest() {
    let doc = parse("* A\n** B\n*** C\n* D\n").expect("parse");
    assert_eq!(doc.largest_root().expect("hit").title(), "A");
}

#[test]
fn doc_smallest_root_returns_smallest() {
    let doc = parse("* A\n** B\n* C\n").expect("parse");
    assert_eq!(doc.smallest_root().expect("hit").title(), "C");
}

#[test]
fn doc_root_index_of_id_match() {
    let src = "* A\n* B\n:PROPERTIES:\n:ID: b\n:END:\n* C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.root_index_of_id("b"), Some(1));
    assert_eq!(doc.root_index_of_id("missing"), None);
}

#[test]
fn doc_root_position_of_title_match() {
    let doc = parse("* A\n* B\n* C\n").expect("parse");
    assert_eq!(doc.root_position_of_title("B"), Some(1));
    assert_eq!(doc.root_position_of_title("missing"), None);
}

#[test]
fn doc_largest_root_index_match() {
    let doc = parse("* A\n* B\n** C\n* D\n").expect("parse");
    assert_eq!(doc.largest_root_index(), Some(1));
}

#[test]
fn doc_smallest_root_index_match() {
    let doc = parse("* A\n** X\n* B\n").expect("parse");
    assert_eq!(doc.smallest_root_index(), Some(1));
}

#[test]
fn doc_subtree_size_of_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n** B\n** C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_size_of("a"), Some(3));
    assert_eq!(doc.subtree_size_of("missing"), None);
}

#[test]
fn doc_subtree_max_level_of_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n** B\n*** C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_max_level_of("a"), Some(3));
}

#[test]
fn doc_subtree_max_priority_of_match() {
    let src = "* [#B] A\n:PROPERTIES:\n:ID: a\n:END:\n** [#A] B\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_max_priority_of("a"), Some('A'));
}

#[test]
fn doc_subtree_min_level_of_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n** B\n*** C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_min_level_of("a"), Some(1));
}

#[test]
fn doc_subtree_min_priority_of_match() {
    let src = "* [#B] A\n:PROPERTIES:\n:ID: a\n:END:\n** [#A] B\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_min_priority_of("a"), Some('B'));
}

#[test]
fn doc_subtree_has_tag_of_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n** B :work:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_has_tag_of("a", "work"), Some(true));
    assert_eq!(doc.subtree_has_tag_of("a", "home"), Some(false));
    assert_eq!(doc.subtree_has_tag_of("missing", "work"), None);
}

#[test]
fn doc_subtree_has_todo_of_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n** TODO B\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_has_todo_of("a", "TODO"), Some(true));
    assert_eq!(doc.subtree_has_todo_of("a", "DONE"), Some(false));
}

#[test]
fn doc_subtree_has_priority_of_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n** [#A] B\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_has_priority_of("a", 'A'), Some(true));
    assert_eq!(doc.subtree_has_priority_of("a", 'B'), Some(false));
}

#[test]
fn doc_subtree_has_id_of_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n** B\n:PROPERTIES:\n:ID: b\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_has_id_of("a"), Some(true));
    assert_eq!(doc.subtree_has_id_of("missing"), None);
}

#[test]
fn doc_subtree_count_with_tag_of_match() {
    let src = "* A :work:\n:PROPERTIES:\n:ID: a\n:END:\n** B :work:\n** C :home:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_count_with_tag_of("a", "work"), Some(2));
    assert_eq!(doc.subtree_count_with_tag_of("missing", "work"), None);
}

#[test]
fn doc_subtree_count_with_todo_of_match() {
    let src = "* TODO A\n:PROPERTIES:\n:ID: a\n:END:\n** TODO B\n** DONE C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_count_with_todo_of("a", "TODO"), Some(2));
}

#[test]
fn doc_subtree_count_with_priority_of_match() {
    let src = "* [#A] A\n:PROPERTIES:\n:ID: a\n:END:\n** [#A] B\n** [#B] C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_count_with_priority_of("a", 'A'), Some(2));
}

#[test]
fn doc_subtree_count_at_level_of_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n** B\n** C\n*** D\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_count_at_level_of("a", 1), Some(1));
    assert_eq!(doc.subtree_count_at_level_of("a", 2), Some(2));
    assert_eq!(doc.subtree_count_at_level_of("a", 3), Some(1));
    assert_eq!(doc.subtree_count_at_level_of("missing", 1), None);
}

#[test]
fn doc_subtree_count_with_property_of_match() {
    let src = "* A\n:PROPERTIES:\n:K: 1\n:ID: a\n:END:\n** B\n:PROPERTIES:\n:K: 2\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_count_with_property_of("a", "K"), Some(2));
    assert_eq!(doc.subtree_count_with_property_of("a", "MISSING"), Some(0));
}

#[test]
fn doc_subtree_count_with_id_of_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n** B\n:PROPERTIES:\n:ID: b\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_count_with_id_of("a"), Some(2));
}

#[test]
fn doc_subtree_tags_of_match() {
    let src = "* A :a:\n:PROPERTIES:\n:ID: a\n:END:\n** B :b:a:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_tags_of("a").expect("hit"), vec!["a".to_owned(), "b".to_owned()]);
    assert!(doc.subtree_tags_of("missing").is_none());
}

#[test]
fn doc_subtree_todos_of_match() {
    let src = "* TODO A\n:PROPERTIES:\n:ID: a\n:END:\n** DONE B\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_todos_of("a").expect("hit"), vec!["DONE".to_owned(), "TODO".to_owned()]);
}

#[test]
fn doc_subtree_priorities_of_match() {
    let src = "* [#B] A\n:PROPERTIES:\n:ID: a\n:END:\n** [#A] B\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_priorities_of("a").expect("hit"), vec!['A', 'B']);
}

#[test]
fn doc_subtree_levels_of_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n** B\n*** C\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.subtree_levels_of("a").expect("hit"), vec![1, 2, 3]);
}

#[test]
fn doc_subtree_titles_of_match() {
    let src = "* Top\n:PROPERTIES:\n:ID: top\n:END:\n** A\n*** B\n";
    let doc = parse(src).expect("parse");
    let v = doc.subtree_titles_of("top").expect("hit");
    assert_eq!(v, vec!["Top", "A", "B"]);
}

#[test]
fn doc_subtree_ids_of_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n** B\n:PROPERTIES:\n:ID: b\n:END:\n";
    let doc = parse(src).expect("parse");
    let v = doc.subtree_ids_of("a").expect("hit");
    assert_eq!(v, vec!["a", "b"]);
}

#[test]
fn headline_subtree_distinct_titles_sorted() {
    let src = "* A\n** A\n** B\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(
        top.subtree_distinct_titles(),
        vec!["A".to_owned(), "B".to_owned()]
    );
}

#[test]
fn headline_subtree_distinct_title_count_match() {
    let src = "* A\n** A\n** B\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_distinct_title_count(), 2);
}

#[test]
fn headline_subtree_id_count_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n** B\n:PROPERTIES:\n:ID: b\n:END:\n** C\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_id_count(), 2);
}

#[test]
fn headline_subtree_property_keys_sorted_unique() {
    let src = "* A\n:PROPERTIES:\n:K1: 1\n:K2: 2\n:END:\n** B\n:PROPERTIES:\n:K2: 3\n:K3: 4\n:END:\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(
        top.subtree_property_keys(),
        vec!["K1".to_owned(), "K2".to_owned(), "K3".to_owned()]
    );
}

#[test]
fn headline_subtree_property_key_count_match() {
    let src = "* A\n:PROPERTIES:\n:K1: 1\n:END:\n** B\n:PROPERTIES:\n:K2: 2\n:END:\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_property_key_count(), 2);
}

#[test]
fn headline_subtree_total_property_count_match() {
    let src = "* A\n:PROPERTIES:\n:K1: 1\n:K2: 2\n:END:\n** B\n:PROPERTIES:\n:K3: 3\n:END:\n";
    let doc = parse(src).expect("parse");
    let top = &doc.roots()[0];
    assert_eq!(top.subtree_total_property_count(), 3);
}

#[test]
fn doc_subtree_property_keys_of_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:K1: 1\n:END:\n** B\n:PROPERTIES:\n:K2: 2\n:END:\n";
    let doc = parse(src).expect("parse");
    let keys = doc.subtree_property_keys_of("a").expect("hit");
    assert!(keys.contains(&"ID".to_owned()));
    assert!(keys.contains(&"K1".to_owned()));
    assert!(keys.contains(&"K2".to_owned()));
}

#[test]
fn doc_subtree_distinct_titles_of_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:END:\n** A\n** B\n";
    let doc = parse(src).expect("parse");
    assert_eq!(
        doc.subtree_distinct_titles_of("a").expect("hit"),
        vec!["A".to_owned(), "B".to_owned()]
    );
}

#[test]
fn doc_subtree_total_property_count_of_match() {
    let src = "* A\n:PROPERTIES:\n:ID: a\n:K1: 1\n:END:\n** B\n:PROPERTIES:\n:K2: 2\n:END:\n";
    let doc = parse(src).expect("parse");
    // 2 + 1 = 3
    assert_eq!(doc.subtree_total_property_count_of("a"), Some(3));
}

#[test]
fn doc_distinct_property_keys_sorted_unique() {
    let src = "* A\n:PROPERTIES:\n:K1: 1\n:K2: 2\n:END:\n* B\n:PROPERTIES:\n:K2: 3\n:K3: 4\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(
        doc.distinct_property_keys(),
        vec!["K1".to_owned(), "K2".to_owned(), "K3".to_owned()]
    );
}

#[test]
fn doc_distinct_property_key_count_match() {
    let src = "* A\n:PROPERTIES:\n:K1: 1\n:END:\n* B\n:PROPERTIES:\n:K1: 2\n:K2: 3\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.distinct_property_key_count(), 2);
}

#[test]
fn doc_distinct_property_values_for_key_match() {
    let src = "* A\n:PROPERTIES:\n:K: x\n:END:\n* B\n:PROPERTIES:\n:K: y\n:END:\n* C\n:PROPERTIES:\n:K: x\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(
        doc.distinct_property_values_for_key("K"),
        vec!["x".to_owned(), "y".to_owned()]
    );
    assert!(doc.distinct_property_values_for_key("MISSING").is_empty());
}

#[test]
fn doc_distinct_property_value_count_for_key_match() {
    let src = "* A\n:PROPERTIES:\n:K: x\n:END:\n* B\n:PROPERTIES:\n:K: y\n:END:\n* C\n:PROPERTIES:\n:K: x\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.distinct_property_value_count_for_key("K"), 2);
}

#[test]
fn doc_property_value_counts_for_key_returns_map() {
    let src = "* A\n:PROPERTIES:\n:K: x\n:END:\n* B\n:PROPERTIES:\n:K: y\n:END:\n* C\n:PROPERTIES:\n:K: x\n:END:\n";
    let doc = parse(src).expect("parse");
    let m = doc.property_value_counts_for_key("K");
    assert_eq!(m.get("x").copied(), Some(2));
    assert_eq!(m.get("y").copied(), Some(1));
}

#[test]
fn doc_property_key_counts_returns_map() {
    let src = "* A\n:PROPERTIES:\n:K1: 1\n:K2: 2\n:END:\n* B\n:PROPERTIES:\n:K2: 3\n:END:\n";
    let doc = parse(src).expect("parse");
    let m = doc.property_key_counts();
    assert_eq!(m.get("K1").copied(), Some(1));
    assert_eq!(m.get("K2").copied(), Some(2));
}

#[test]
fn doc_most_common_property_key_match() {
    let src = "* A\n:PROPERTIES:\n:K1: 1\n:K2: 2\n:END:\n* B\n:PROPERTIES:\n:K2: 3\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.most_common_property_key(), Some("K2".to_owned()));
}

#[test]
fn doc_distinct_property_key_count_zero_when_no_props() {
    let doc = parse("* A\n* B\n").expect("parse");
    assert_eq!(doc.distinct_property_key_count(), 0);
}

#[test]
fn doc_least_common_property_key_match() {
    let src = "* A\n:PROPERTIES:\n:K1: 1\n:K2: 2\n:END:\n* B\n:PROPERTIES:\n:K2: 3\n:END:\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.least_common_property_key(), Some("K1".to_owned()));
}

#[test]
fn doc_least_common_tag_match() {
    let doc = parse("* X :work:\n* Y :work:\n* Z :home:\n").expect("parse");
    assert_eq!(doc.least_common_tag(), Some("home".to_owned()));
}

#[test]
fn doc_least_common_todo_match() {
    let doc = parse("* TODO X\n* TODO Y\n* DONE Z\n").expect("parse");
    assert_eq!(doc.least_common_todo(), Some("DONE".to_owned()));
}

#[test]
fn doc_least_common_priority_match() {
    let doc = parse("* [#A] X\n* [#A] Y\n* [#B] Z\n").expect("parse");
    assert_eq!(doc.least_common_priority(), Some('B'));
}

#[test]
fn doc_least_common_level_match() {
    let doc = parse("* A\n** B\n** C\n").expect("parse");
    assert_eq!(doc.least_common_level(), Some(1));
}

#[test]
fn doc_min_body_word_count_match() {
    let doc = parse("* A\nhello\n* B\nfoo bar baz\n").expect("parse");
    assert_eq!(doc.min_body_word_count(), 1);
}

#[test]
fn doc_min_body_word_count_zero_when_no_headlines() {
    let doc = parse("nothing\n").expect("parse");
    assert_eq!(doc.min_body_word_count(), 0);
}

#[test]
fn doc_total_body_byte_count_match() {
    let doc = parse("* A\nhello\n* B\n").expect("parse");
    // "hello\n" = 6
    assert_eq!(doc.total_body_byte_count(), 6);
}

#[test]
fn doc_max_body_byte_count_match() {
    let doc = parse("* A\nfoo\n* B\nlonger body\n").expect("parse");
    assert_eq!(doc.max_body_byte_count(), "longer body\n".len());
}

#[test]
fn doc_min_body_byte_count_match() {
    let doc = parse("* A\nfoo\n* B\nlonger body\n").expect("parse");
    assert_eq!(doc.min_body_byte_count(), "foo\n".len());
}

#[test]
fn doc_median_body_word_count_match() {
    let doc = parse("* A\nfoo\n* B\nfoo bar\n* C\nfoo bar baz\n").expect("parse");
    // word counts: 1,2,3 -> median 2
    assert_eq!(doc.median_body_word_count(), Some(2));
}

#[test]
fn doc_median_body_byte_count_match() {
    let doc = parse("* A\nx\n* B\nxy\n* C\nxyz\n").expect("parse");
    // bytes: 2,3,4 -> median 3
    assert_eq!(doc.median_body_byte_count(), Some(3));
}

#[test]
fn doc_total_body_char_count_match() {
    let doc = parse("* A\nhello\n").expect("parse");
    // "hello\n" -> 6 chars
    assert_eq!(doc.total_body_char_count(), 6);
}

#[test]
fn doc_max_root_title_len_match() {
    let doc = parse("* Aa\n* Bbbb\n").expect("parse");
    assert_eq!(doc.max_root_title_len(), Some(4));
}

#[test]
fn doc_min_root_title_len_match() {
    let doc = parse("* Aa\n* Bbbb\n").expect("parse");
    assert_eq!(doc.min_root_title_len(), Some(2));
}

#[test]
fn doc_max_headline_title_len_match() {
    let doc = parse("* A\n** Long title\n").expect("parse");
    assert_eq!(doc.max_headline_title_len(), Some("Long title".len()));
}

#[test]
fn doc_min_headline_title_len_match() {
    let doc = parse("* A\n** Long title\n").expect("parse");
    assert_eq!(doc.min_headline_title_len(), Some(1));
}

#[test]
fn doc_mean_headline_title_len_match() {
    let doc = parse("* AA\n** BBBB\n").expect("parse");
    // chars: 2,4 -> mean 3
    assert_eq!(doc.mean_headline_title_len(), 3);
}

#[test]
fn doc_total_headline_title_len_match() {
    let doc = parse("* AA\n** BBBB\n").expect("parse");
    assert_eq!(doc.total_headline_title_len(), 6);
}

#[test]
fn doc_headline_title_len_counts_match() {
    let doc = parse("* AA\n** BB\n* CCC\n").expect("parse");
    let m = doc.headline_title_len_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn doc_mode_root_title_len_match() {
    let doc = parse("* AA\n* BB\n* CCC\n").expect("parse");
    // root title chars: 2,2,3 -> mode 2
    assert_eq!(doc.mode_root_title_len(), Some(2));
}

#[test]
fn doc_total_root_title_len_match() {
    let doc = parse("* AA\n* CCC\n").expect("parse");
    assert_eq!(doc.total_root_title_len(), 5);
}

#[test]
fn doc_mean_root_title_len_match() {
    let doc = parse("* AA\n* BBBB\n").expect("parse");
    // 2,4 -> 3
    assert_eq!(doc.mean_root_title_len(), 3);
}

#[test]
fn doc_median_root_title_len_match() {
    let doc = parse("* A\n* BB\n* CCCC\n").expect("parse");
    // 1,2,4 -> median 2
    assert_eq!(doc.median_root_title_len(), Some(2));
}

#[test]
fn doc_root_title_len_counts_match() {
    let doc = parse("* AA\n* BB\n* CCC\n").expect("parse");
    let m = doc.root_title_len_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn doc_median_headline_title_len_match() {
    let doc = parse("* A\n** BB\n** CCCC\n").expect("parse");
    // chars 1,2,4 -> median 2
    assert_eq!(doc.median_headline_title_len(), Some(2));
}

#[test]
fn doc_mode_headline_title_len_match() {
    let doc = parse("* AA\n** BB\n** CCC\n").expect("parse");
    // chars 2,2,3 -> mode 2
    assert_eq!(doc.mode_headline_title_len(), Some(2));
}

#[test]
fn headline_title_word_count_match() {
    let doc = parse("* one two three\n").expect("parse");
    assert_eq!(doc.roots()[0].title_word_count(), 3);
}

#[test]
fn doc_total_title_word_count_match() {
    let doc = parse("* a b\n** c d e\n").expect("parse");
    assert_eq!(doc.total_title_word_count(), 5);
}

#[test]
fn doc_mean_title_word_count_match() {
    let doc = parse("* a b\n** c d e f\n").expect("parse");
    // words 2,4 -> mean 3
    assert_eq!(doc.mean_title_word_count(), 3);
}

#[test]
fn doc_max_min_title_word_count_match() {
    let doc = parse("* a\n** b c d\n").expect("parse");
    assert_eq!(doc.max_title_word_count(), Some(3));
    assert_eq!(doc.min_title_word_count(), Some(1));
}

#[test]
fn doc_median_title_word_count_match() {
    let doc = parse("* a\n** b c\n** d e f g\n").expect("parse");
    // words 1,2,4 -> median 2
    assert_eq!(doc.median_title_word_count(), Some(2));
}

#[test]
fn doc_mode_title_word_count_match() {
    let doc = parse("* a b\n** c d\n** e f g\n").expect("parse");
    // words 2,2,3 -> mode 2
    assert_eq!(doc.mode_title_word_count(), Some(2));
}

#[test]
fn doc_title_word_count_counts_match() {
    let doc = parse("* a b\n** c d\n** e f g\n").expect("parse");
    let m = doc.title_word_count_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn doc_total_root_title_word_count_match() {
    let doc = parse("* a b\n** ignored child\n* c d e\n").expect("parse");
    // roots only: 2 + 3 = 5
    assert_eq!(doc.total_root_title_word_count(), 5);
}

#[test]
fn doc_mean_root_title_word_count_match() {
    let doc = parse("* a b\n* c d e f\n").expect("parse");
    // 2,4 -> 3
    assert_eq!(doc.mean_root_title_word_count(), 3);
}

#[test]
fn doc_max_min_root_title_word_count_match() {
    let doc = parse("* a\n* b c d\n").expect("parse");
    assert_eq!(doc.max_root_title_word_count(), Some(3));
    assert_eq!(doc.min_root_title_word_count(), Some(1));
}

#[test]
fn doc_median_root_title_word_count_match() {
    let doc = parse("* a\n* b c\n* d e f g\n").expect("parse");
    // 1,2,4 -> median 2
    assert_eq!(doc.median_root_title_word_count(), Some(2));
}

#[test]
fn doc_root_title_word_count_counts_match() {
    let doc = parse("* a b\n* c d\n* e f g\n").expect("parse");
    let m = doc.root_title_word_count_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn headline_body_line_count_match() {
    let doc = parse("* H\nline1\nline2\n").expect("parse");
    assert_eq!(doc.roots()[0].body_line_count(), 2);
}

#[test]
fn headline_body_line_count_zero_when_no_body() {
    let doc = parse("* H\n").expect("parse");
    assert_eq!(doc.roots()[0].body_line_count(), 0);
}

#[test]
fn doc_total_body_line_count_match() {
    let doc = parse("* A\nx\n* B\ny\nz\n").expect("parse");
    assert_eq!(doc.total_body_line_count(), 3);
}

#[test]
fn doc_mean_body_line_count_match() {
    let doc = parse("* A\nx\n* B\ny\nz\nw\n").expect("parse");
    // lines 1,3 -> mean 2
    assert_eq!(doc.mean_body_line_count(), 2);
}

#[test]
fn doc_max_min_body_line_count_match() {
    let doc = parse("* A\nx\n* B\ny\nz\n").expect("parse");
    assert_eq!(doc.max_body_line_count(), Some(2));
    assert_eq!(doc.min_body_line_count(), Some(1));
}

#[test]
fn doc_median_body_line_count_match() {
    let doc = parse("* A\n* B\nx\n* C\ny\nz\nw\n").expect("parse");
    // lines 0,1,3 -> median 1
    assert_eq!(doc.median_body_line_count(), Some(1));
}

#[test]
fn doc_body_line_count_counts_match() {
    let doc = parse("* A\nx\n* B\ny\n* C\nz\nw\n").expect("parse");
    // lines 1,1,2
    let m = doc.body_line_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn doc_mode_body_line_count_match() {
    let doc = parse("* A\nx\n* B\ny\n* C\nz\nw\n").expect("parse");
    assert_eq!(doc.mode_body_line_count(), Some(1));
}

#[test]
fn doc_total_root_body_line_count_match() {
    let doc = parse("* A\nx\n** child\nignored\n* B\ny\nz\n").expect("parse");
    // roots only: A body 1 line, B body 2 lines = 3
    assert_eq!(doc.total_root_body_line_count(), 3);
}

#[test]
fn doc_mean_root_body_line_count_match() {
    let doc = parse("* A\nx\n* B\ny\nz\nw\n").expect("parse");
    // 1,3 -> mean 2
    assert_eq!(doc.mean_root_body_line_count(), 2);
}

#[test]
fn doc_max_min_root_body_line_count_match() {
    let doc = parse("* A\nx\n* B\ny\nz\n").expect("parse");
    assert_eq!(doc.max_root_body_line_count(), Some(2));
    assert_eq!(doc.min_root_body_line_count(), Some(1));
}

#[test]
fn doc_median_root_body_line_count_match() {
    let doc = parse("* A\n* B\nx\n* C\ny\nz\nw\n").expect("parse");
    // 0,1,3 -> median 1
    assert_eq!(doc.median_root_body_line_count(), Some(1));
}

#[test]
fn doc_root_body_line_count_counts_match() {
    let doc = parse("* A\nx\n* B\ny\n* C\nz\nw\n").expect("parse");
    let m = doc.root_body_line_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn headline_body_char_count_match() {
    let doc = parse("* H\nabc\n").expect("parse");
    // body source "abc\n" -> 4 chars
    assert_eq!(doc.roots()[0].body_char_count(), 4);
}

#[test]
fn doc_mean_body_char_count_match() {
    let doc = parse("* A\nab\n* B\nabcdef\n").expect("parse");
    // body src "ab\n"=3, "abcdef\n"=7 -> mean 5
    assert_eq!(doc.mean_body_char_count(), 5);
}

#[test]
fn doc_max_min_body_char_count_match() {
    let doc = parse("* A\nx\n* B\nabcd\n").expect("parse");
    // 2, 5
    assert_eq!(doc.max_body_char_count(), Some(5));
    assert_eq!(doc.min_body_char_count(), Some(2));
}

#[test]
fn doc_median_body_char_count_match() {
    let doc = parse("* A\n* B\nx\n* C\nabc\n").expect("parse");
    // body src chars: 0, 2, 4 -> median 2
    assert_eq!(doc.median_body_char_count(), Some(2));
}

#[test]
fn doc_body_char_count_counts_match() {
    let doc = parse("* A\nx\n* B\ny\n* C\nabc\n").expect("parse");
    // chars 2,2,4
    let m = doc.body_char_count_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&4), Some(&1));
}

#[test]
fn doc_mode_body_char_count_match() {
    let doc = parse("* A\nx\n* B\ny\n* C\nabc\n").expect("parse");
    assert_eq!(doc.mode_body_char_count(), Some(2));
}

#[test]
fn doc_total_root_body_char_count_match() {
    let doc = parse("* A\nab\n** child\nignored body\n* B\nc\n").expect("parse");
    // roots only: "ab\n"=3 + "c\n"=2 = 5
    assert_eq!(doc.total_root_body_char_count(), 5);
}

#[test]
fn doc_mean_root_body_char_count_match() {
    let doc = parse("* A\nab\n* B\nabcdef\n").expect("parse");
    // 3,7 -> mean 5
    assert_eq!(doc.mean_root_body_char_count(), 5);
}

#[test]
fn doc_max_min_root_body_char_count_match() {
    let doc = parse("* A\nx\n* B\nabcd\n").expect("parse");
    assert_eq!(doc.max_root_body_char_count(), Some(5));
    assert_eq!(doc.min_root_body_char_count(), Some(2));
}

#[test]
fn doc_median_root_body_char_count_match() {
    let doc = parse("* A\n* B\nx\n* C\nabc\n").expect("parse");
    // 0,2,4 -> median 2
    assert_eq!(doc.median_root_body_char_count(), Some(2));
}

#[test]
fn doc_root_body_char_count_counts_match() {
    let doc = parse("* A\nx\n* B\ny\n* C\nabc\n").expect("parse");
    let m = doc.root_body_char_count_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&4), Some(&1));
}

#[test]
fn headline_header_byte_len_match() {
    let doc = parse("* Hi\n").expect("parse");
    // "* Hi\n" = 5 bytes
    assert_eq!(doc.roots()[0].header_byte_len(), 5);
}

#[test]
fn headline_header_char_count_match() {
    let doc = parse("* Hé\n").expect("parse");
    // chars: '*',' ','H','é','\n' = 5 chars (é is 2 bytes)
    assert_eq!(doc.roots()[0].header_char_count(), 5);
    assert_eq!(doc.roots()[0].header_byte_len(), 6);
}

#[test]
fn doc_total_header_byte_len_match() {
    let doc = parse("* A\n** BB\n").expect("parse");
    // "* A\n"=4 + "** BB\n"=6 = 10
    assert_eq!(doc.total_header_byte_len(), 10);
}

#[test]
fn doc_mean_header_byte_len_match() {
    let doc = parse("* A\n** BBB\n").expect("parse");
    // 4, 7 -> mean 5
    assert_eq!(doc.mean_header_byte_len(), 5);
}

#[test]
fn doc_max_min_header_byte_len_match() {
    let doc = parse("* A\n** BBB\n").expect("parse");
    assert_eq!(doc.max_header_byte_len(), Some(7));
    assert_eq!(doc.min_header_byte_len(), Some(4));
}

#[test]
fn doc_median_header_byte_len_match() {
    let doc = parse("* A\n** BB\n*** CCCCC\n").expect("parse");
    // "* A\n"=4, "** BB\n"=6, "*** CCCCC\n"=10 -> median 6
    assert_eq!(doc.median_header_byte_len(), Some(6));
}

#[test]
fn doc_header_byte_len_counts_match() {
    let doc = parse("* A\n* B\n** CCC\n").expect("parse");
    // "* A\n"=4, "* B\n"=4, "** CCC\n"=7
    let m = doc.header_byte_len_counts();
    assert_eq!(m.get(&4), Some(&2));
    assert_eq!(m.get(&7), Some(&1));
}

#[test]
fn doc_mode_header_byte_len_match() {
    let doc = parse("* A\n* B\n** CCC\n").expect("parse");
    assert_eq!(doc.mode_header_byte_len(), Some(4));
}

#[test]
fn doc_total_header_char_count_match() {
    let doc = parse("* A\n** BB\n").expect("parse");
    // "* A\n"=4 + "** BB\n"=6 = 10
    assert_eq!(doc.total_header_char_count(), 10);
}

#[test]
fn doc_total_header_char_count_unicode() {
    let doc = parse("* é\n").expect("parse");
    // chars '*',' ','é','\n' = 4 (byte len would be 5)
    assert_eq!(doc.total_header_char_count(), 4);
    assert_eq!(doc.total_header_byte_len(), 5);
}

#[test]
fn doc_mean_header_char_count_match() {
    let doc = parse("* A\n** BBB\n").expect("parse");
    // 4,7 -> mean 5
    assert_eq!(doc.mean_header_char_count(), 5);
}

#[test]
fn doc_max_min_header_char_count_match() {
    let doc = parse("* A\n** BBB\n").expect("parse");
    assert_eq!(doc.max_header_char_count(), Some(7));
    assert_eq!(doc.min_header_char_count(), Some(4));
}

#[test]
fn doc_median_header_char_count_match() {
    let doc = parse("* A\n** BB\n*** CCCCC\n").expect("parse");
    // 4,6,10 -> median 6
    assert_eq!(doc.median_header_char_count(), Some(6));
}

#[test]
fn doc_header_char_count_counts_match() {
    let doc = parse("* A\n* B\n** CCC\n").expect("parse");
    let m = doc.header_char_count_counts();
    assert_eq!(m.get(&4), Some(&2));
    assert_eq!(m.get(&7), Some(&1));
}

#[test]
fn doc_mode_header_char_count_match() {
    let doc = parse("* A\n* B\n** CCC\n").expect("parse");
    assert_eq!(doc.mode_header_char_count(), Some(4));
}

#[test]
fn doc_total_root_header_byte_len_match() {
    let doc = parse("* A\n** ignored\n* BBB\n").expect("parse");
    // roots only: "* A\n"=4 + "* BBB\n"=6 = 10
    assert_eq!(doc.total_root_header_byte_len(), 10);
}

#[test]
fn doc_max_min_root_header_byte_len_match() {
    let doc = parse("* A\n* BBB\n").expect("parse");
    assert_eq!(doc.max_root_header_byte_len(), Some(6));
    assert_eq!(doc.min_root_header_byte_len(), Some(4));
}

#[test]
fn doc_mean_root_header_byte_len_match() {
    let doc = parse("* A\n* BBB\n").expect("parse");
    // 4,6 -> 5
    assert_eq!(doc.mean_root_header_byte_len(), 5);
}

#[test]
fn doc_total_root_header_char_count_unicode() {
    let doc = parse("* é\n** ignored\n").expect("parse");
    // root only "* é\n" = 4 chars (5 bytes)
    assert_eq!(doc.total_root_header_char_count(), 4);
}

#[test]
fn doc_max_min_root_header_char_count_match() {
    let doc = parse("* A\n* BBB\n").expect("parse");
    assert_eq!(doc.max_root_header_char_count(), Some(6));
    assert_eq!(doc.min_root_header_char_count(), Some(4));
}

#[test]
fn headline_title_byte_len_match() {
    let doc = parse("* é\n").expect("parse");
    // "é" = 2 bytes, 1 char
    assert_eq!(doc.roots()[0].title_byte_len(), 2);
}

#[test]
fn doc_total_title_byte_len_match() {
    let doc = parse("* AA\n** BBBB\n").expect("parse");
    assert_eq!(doc.total_title_byte_len(), 6);
}

#[test]
fn doc_mean_title_byte_len_match() {
    let doc = parse("* AA\n** BBBB\n").expect("parse");
    // 2,4 -> 3
    assert_eq!(doc.mean_title_byte_len(), 3);
}

#[test]
fn doc_max_min_title_byte_len_match() {
    let doc = parse("* A\n** CCCC\n").expect("parse");
    assert_eq!(doc.max_title_byte_len(), Some(4));
    assert_eq!(doc.min_title_byte_len(), Some(1));
}

#[test]
fn doc_median_title_byte_len_match() {
    let doc = parse("* A\n** BB\n** CCCC\n").expect("parse");
    // 1,2,4 -> median 2
    assert_eq!(doc.median_title_byte_len(), Some(2));
}

#[test]
fn doc_title_byte_len_counts_match() {
    let doc = parse("* AA\n** BB\n** CCC\n").expect("parse");
    let m = doc.title_byte_len_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn doc_mode_title_byte_len_match() {
    let doc = parse("* AA\n** BB\n** CCC\n").expect("parse");
    assert_eq!(doc.mode_title_byte_len(), Some(2));
}

#[test]
fn doc_total_root_title_byte_len_match() {
    let doc = parse("* AA\n** ignored\n* CCC\n").expect("parse");
    // roots only: 2 + 3 = 5
    assert_eq!(doc.total_root_title_byte_len(), 5);
}

#[test]
fn doc_total_root_title_byte_len_unicode() {
    let doc = parse("* é\n").expect("parse");
    assert_eq!(doc.total_root_title_byte_len(), 2);
}

#[test]
fn doc_mean_root_title_byte_len_match() {
    let doc = parse("* AA\n* BBBB\n").expect("parse");
    // 2,4 -> 3
    assert_eq!(doc.mean_root_title_byte_len(), 3);
}

#[test]
fn doc_max_min_root_title_byte_len_match() {
    let doc = parse("* A\n* CCCC\n").expect("parse");
    assert_eq!(doc.max_root_title_byte_len(), Some(4));
    assert_eq!(doc.min_root_title_byte_len(), Some(1));
}

#[test]
fn doc_median_root_title_byte_len_match() {
    let doc = parse("* A\n* BB\n* CCCC\n").expect("parse");
    // 1,2,4 -> 2
    assert_eq!(doc.median_root_title_byte_len(), Some(2));
}

#[test]
fn doc_root_title_byte_len_counts_match() {
    let doc = parse("* AA\n* BB\n* CCC\n").expect("parse");
    let m = doc.root_title_byte_len_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn doc_min_child_count_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    // child counts: A=2, B=0, C=0, D=0 -> min 0
    assert_eq!(doc.min_child_count(), 0);
}

#[test]
fn doc_total_child_count_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    // 2+0+0+0 = 2
    assert_eq!(doc.total_child_count(), 2);
}

#[test]
fn doc_mean_child_count_match() {
    let doc = parse("* A\n** B\n** C\n** E\n* D\n").expect("parse");
    // A=3,B=0,C=0,E=0,D=0 total 3 / 5 = 0
    assert_eq!(doc.mean_child_count(), 0);
}

#[test]
fn doc_median_child_count_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    // sorted child counts [0,0,0,2] -> median midpoint(0,0)=0
    assert_eq!(doc.median_child_count(), Some(0));
}

#[test]
fn doc_child_count_counts_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    let m = doc.child_count_counts();
    assert_eq!(m.get(&0), Some(&3));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn doc_mode_child_count_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    assert_eq!(doc.mode_child_count(), Some(0));
}

#[test]
fn doc_total_root_child_count_match() {
    let doc = parse("* A\n** B\n** C\n* D\n** E\n").expect("parse");
    // roots A=2, D=1 -> total 3
    assert_eq!(doc.total_root_child_count(), 3);
}

#[test]
fn doc_max_min_root_child_count_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    assert_eq!(doc.max_root_child_count(), Some(2));
    assert_eq!(doc.min_root_child_count(), Some(0));
}

#[test]
fn doc_mean_root_child_count_match() {
    let doc = parse("* A\n** B\n** C\n** E\n* D\n").expect("parse");
    // roots A=3, D=0 -> mean 1
    assert_eq!(doc.mean_root_child_count(), 1);
}

#[test]
fn doc_median_root_child_count_match() {
    let doc = parse("* A\n** B\n* C\n** D\n** E\n* F\n").expect("parse");
    // roots A=1, C=2, F=0 -> sorted [0,1,2] median 1
    assert_eq!(doc.median_root_child_count(), Some(1));
}

#[test]
fn doc_root_child_count_counts_match() {
    let doc = parse("* A\n* B\n* C\n** D\n").expect("parse");
    // roots A=0, B=0, C=1
    let m = doc.root_child_count_counts();
    assert_eq!(m.get(&0), Some(&2));
    assert_eq!(m.get(&1), Some(&1));
}

#[test]
fn doc_max_subtree_size_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    // sizes A=3,B=1,C=1,D=1 -> max 3
    assert_eq!(doc.max_subtree_size(), Some(3));
}

#[test]
fn doc_min_subtree_size_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    assert_eq!(doc.min_subtree_size(), Some(1));
}

#[test]
fn doc_total_subtree_size_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    // 3+1+1+1 = 6
    assert_eq!(doc.total_subtree_size(), 6);
}

#[test]
fn doc_mean_subtree_size_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    // 6/4 = 1
    assert_eq!(doc.mean_subtree_size(), 1);
}

#[test]
fn doc_median_subtree_size_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    // sorted [1,1,1,3] -> midpoint(1,1) = 1
    assert_eq!(doc.median_subtree_size(), Some(1));
}

#[test]
fn doc_subtree_size_counts_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    let m = doc.subtree_size_counts();
    assert_eq!(m.get(&1), Some(&3));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn doc_mode_subtree_size_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    assert_eq!(doc.mode_subtree_size(), Some(1));
}

#[test]
fn doc_root_subtree_size_counts_match() {
    let doc = parse("* A\n** B\n* C\n* D\n").expect("parse");
    // roots A=2, C=1, D=1
    let m = doc.root_subtree_size_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn doc_min_descendant_count_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    // desc: A=2,B=0,C=0,D=0 -> min 0
    assert_eq!(doc.min_descendant_count(), 0);
}

#[test]
fn doc_total_descendant_count_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    // 2+0+0+0 = 2
    assert_eq!(doc.total_descendant_count(), 2);
}

#[test]
fn doc_mean_descendant_count_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    // 2/4 = 0
    assert_eq!(doc.mean_descendant_count(), 0);
}

#[test]
fn doc_median_descendant_count_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    // sorted [0,0,0,2] -> midpoint(0,0)=0
    assert_eq!(doc.median_descendant_count(), Some(0));
}

#[test]
fn doc_descendant_count_counts_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    let m = doc.descendant_count_counts();
    assert_eq!(m.get(&0), Some(&3));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn doc_mode_descendant_count_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    assert_eq!(doc.mode_descendant_count(), Some(0));
}

#[test]
fn doc_body_word_count_counts_match() {
    let doc = parse("* A\none\n* B\ntwo\n* C\nthree words here\n").expect("parse");
    // body words: 1,1,3
    let m = doc.body_word_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn doc_mode_body_word_count_match() {
    let doc = parse("* A\none\n* B\ntwo\n* C\nthree words here\n").expect("parse");
    assert_eq!(doc.mode_body_word_count(), Some(1));
}

#[test]
fn doc_total_root_body_word_count_match() {
    let doc = parse("* A\none two\n** child\nignored body words\n* B\nthree\n").expect("parse");
    // roots only: A body 2 words, B body 1 word = 3
    assert_eq!(doc.total_root_body_word_count(), 3);
}

#[test]
fn doc_max_min_root_body_word_count_match() {
    let doc = parse("* A\none\n* B\ntwo three four\n").expect("parse");
    assert_eq!(doc.max_root_body_word_count(), Some(3));
    assert_eq!(doc.min_root_body_word_count(), Some(1));
}

#[test]
fn doc_mean_root_body_word_count_match() {
    let doc = parse("* A\none\n* B\ntwo three four five\n").expect("parse");
    // 1,4 -> mean 2
    assert_eq!(doc.mean_root_body_word_count(), 2);
}

#[test]
fn doc_median_root_body_word_count_match() {
    let doc = parse("* A\n* B\none\n* C\ntwo three four\n").expect("parse");
    // 0,1,3 -> median 1
    assert_eq!(doc.median_root_body_word_count(), Some(1));
}

#[test]
fn doc_root_body_word_count_counts_match() {
    let doc = parse("* A\none\n* B\ntwo\n* C\nthree words here\n").expect("parse");
    let m = doc.root_body_word_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn doc_min_priority_match() {
    let doc = parse("* [#A] one\n* [#C] two\n").expect("parse");
    // chars 'A' < 'C' -> min 'A'
    assert_eq!(doc.min_priority(), Some('A'));
}

#[test]
fn doc_max_priority_match() {
    let doc = parse("* [#A] one\n* [#C] two\n").expect("parse");
    assert_eq!(doc.max_priority(), Some('C'));
}

#[test]
fn doc_priority_range_match() {
    let doc = parse("* [#A] one\n* [#B] two\n* [#C] three\n").expect("parse");
    assert_eq!(doc.priority_range(), Some(('A', 'C')));
}

#[test]
fn doc_priority_range_none_when_unprioritized() {
    let doc = parse("* one\n* two\n").expect("parse");
    assert_eq!(doc.priority_range(), None);
    assert_eq!(doc.min_priority(), None);
    assert_eq!(doc.max_priority(), None);
}

#[test]
fn doc_min_tag_count_match() {
    let doc = parse("* A :x:y:\n* B :x:\n* C\n").expect("parse");
    // tag counts A=2,B=1,C=0 -> min 0
    assert_eq!(doc.min_tag_count(), 0);
}

#[test]
fn doc_median_tag_count_match() {
    let doc = parse("* A :x:y:\n* B :x:\n* C\n").expect("parse");
    // sorted [0,1,2] -> median 1
    assert_eq!(doc.median_tag_count(), Some(1));
}

#[test]
fn doc_headline_tag_count_counts_match() {
    let doc = parse("* A :x:\n* B :y:\n* C :p:q:\n").expect("parse");
    // tag counts 1,1,2
    let m = doc.headline_tag_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn doc_mode_tag_count_match() {
    let doc = parse("* A :x:\n* B :y:\n* C :p:q:\n").expect("parse");
    assert_eq!(doc.mode_tag_count(), Some(1));
}

#[test]
fn doc_total_root_tag_count_match() {
    let doc = parse("* A :x:y:\n** child :ignored:\n* B :z:\n").expect("parse");
    // roots only: A=2, B=1 -> total 3
    assert_eq!(doc.total_root_tag_count(), 3);
}

#[test]
fn doc_max_min_root_tag_count_match() {
    let doc = parse("* A :x:y:\n* B\n").expect("parse");
    assert_eq!(doc.max_root_tag_count(), Some(2));
    assert_eq!(doc.min_root_tag_count(), Some(0));
}

#[test]
fn doc_mean_root_tag_count_match() {
    let doc = parse("* A :x:y:z:w:\n* B\n").expect("parse");
    // 4,0 -> mean 2
    assert_eq!(doc.mean_root_tag_count(), 2);
}

#[test]
fn doc_median_root_tag_count_match() {
    let doc = parse("* A\n* B :x:\n* C :p:q:\n").expect("parse");
    // 0,1,2 -> median 1
    assert_eq!(doc.median_root_tag_count(), Some(1));
}

#[test]
fn doc_root_tag_count_counts_match() {
    let doc = parse("* A :x:\n* B :y:\n* C :p:q:\n").expect("parse");
    let m = doc.root_tag_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn doc_min_link_count_match() {
    let doc = parse("* A\n[[l1]] [[l2]]\n* B\n[[l3]]\n* C\nno links\n").expect("parse");
    // link counts A=2,B=1,C=0 -> min 0
    assert_eq!(doc.min_link_count(), 0);
}

#[test]
fn doc_mean_link_count_match() {
    let doc = parse("* A\n[[l1]] [[l2]]\n* B\n[[l3]]\n* C\nno links\n").expect("parse");
    // 2,1,0 total 3 / 3 = 1
    assert_eq!(doc.mean_link_count(), 1);
}

#[test]
fn doc_median_link_count_match() {
    let doc = parse("* A\n[[l1]] [[l2]]\n* B\n[[l3]]\n* C\nno links\n").expect("parse");
    // sorted [0,1,2] -> median 1
    assert_eq!(doc.median_link_count(), Some(1));
}

#[test]
fn doc_link_count_counts_match() {
    let doc = parse("* A\n[[l1]]\n* B\n[[l2]]\n* C\n[[l3]] [[l4]]\n").expect("parse");
    // link counts 1,1,2
    let m = doc.link_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn doc_mode_link_count_match() {
    let doc = parse("* A\n[[l1]]\n* B\n[[l2]]\n* C\n[[l3]] [[l4]]\n").expect("parse");
    assert_eq!(doc.mode_link_count(), Some(1));
}

#[test]
fn doc_total_root_link_count_match() {
    let doc = parse("* A\n[[l1]] [[l2]]\n** child\n[[ignored]]\n* B\n[[l3]]\n").expect("parse");
    // roots only: A=2, B=1 -> total 3
    assert_eq!(doc.total_root_link_count(), 3);
}

#[test]
fn doc_max_min_root_link_count_match() {
    let doc = parse("* A\n[[l1]] [[l2]]\n* B\nno links\n").expect("parse");
    assert_eq!(doc.max_root_link_count(), Some(2));
    assert_eq!(doc.min_root_link_count(), Some(0));
}

#[test]
fn doc_mean_root_link_count_match() {
    let doc = parse("* A\n[[l1]] [[l2]] [[l3]] [[l4]]\n* B\nno\n").expect("parse");
    // 4,0 -> mean 2
    assert_eq!(doc.mean_root_link_count(), 2);
}

#[test]
fn doc_median_root_link_count_match() {
    let doc = parse("* A\nno\n* B\n[[l1]]\n* C\n[[l2]] [[l3]]\n").expect("parse");
    // 0,1,2 -> median 1
    assert_eq!(doc.median_root_link_count(), Some(1));
}

#[test]
fn doc_root_link_count_counts_match() {
    let doc = parse("* A\n[[l1]]\n* B\n[[l2]]\n* C\n[[l3]] [[l4]]\n").expect("parse");
    let m = doc.root_link_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn doc_min_property_count_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:K1: v\n:K2: w\n:END:\n* B\n:PROPERTIES:\n:K1: v\n:END:\n* C\n",
    )
    .expect("parse");
    // prop counts A=2,B=1,C=0 -> min 0
    assert_eq!(doc.min_property_count(), 0);
}

#[test]
fn doc_mean_property_count_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:K1: v\n:K2: w\n:END:\n* B\n:PROPERTIES:\n:K1: v\n:END:\n* C\n",
    )
    .expect("parse");
    // 2,1,0 total 3 / 3 = 1
    assert_eq!(doc.mean_property_count(), 1);
}

#[test]
fn doc_median_property_count_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:K1: v\n:K2: w\n:END:\n* B\n:PROPERTIES:\n:K1: v\n:END:\n* C\n",
    )
    .expect("parse");
    // sorted [0,1,2] -> median 1
    assert_eq!(doc.median_property_count(), Some(1));
}

#[test]
fn doc_property_count_counts_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:K1: v\n:END:\n* B\n:PROPERTIES:\n:K2: v\n:END:\n* C\n:PROPERTIES:\n:K3: v\n:K4: w\n:END:\n",
    )
    .expect("parse");
    // prop counts 1,1,2
    let m = doc.property_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn doc_mode_property_count_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:K1: v\n:END:\n* B\n:PROPERTIES:\n:K2: v\n:END:\n* C\n:PROPERTIES:\n:K3: v\n:K4: w\n:END:\n",
    )
    .expect("parse");
    assert_eq!(doc.mode_property_count(), Some(1));
}

#[test]
fn doc_total_root_property_count_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:K1: v\n:K2: w\n:END:\n** child\n:PROPERTIES:\n:IGN: x\n:END:\n* B\n:PROPERTIES:\n:K3: v\n:END:\n",
    )
    .expect("parse");
    // roots only: A=2, B=1 -> total 3
    assert_eq!(doc.total_root_property_count(), 3);
}

#[test]
fn doc_max_min_root_property_count_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:K1: v\n:K2: w\n:END:\n* B\n",
    )
    .expect("parse");
    assert_eq!(doc.max_root_property_count(), Some(2));
    assert_eq!(doc.min_root_property_count(), Some(0));
}

#[test]
fn doc_mean_root_property_count_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:K1: v\n:K2: w\n:K3: x\n:K4: y\n:END:\n* B\n",
    )
    .expect("parse");
    // 4,0 -> mean 2
    assert_eq!(doc.mean_root_property_count(), 2);
}

#[test]
fn doc_median_root_property_count_match() {
    let doc = parse(
        "* A\n* B\n:PROPERTIES:\n:K1: v\n:END:\n* C\n:PROPERTIES:\n:K2: v\n:K3: w\n:END:\n",
    )
    .expect("parse");
    // 0,1,2 -> median 1
    assert_eq!(doc.median_root_property_count(), Some(1));
}

#[test]
fn doc_root_property_count_counts_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:K1: v\n:END:\n* B\n:PROPERTIES:\n:K2: v\n:END:\n* C\n:PROPERTIES:\n:K3: v\n:K4: w\n:END:\n",
    )
    .expect("parse");
    let m = doc.root_property_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn doc_max_min_timestamp_count_match() {
    let doc = parse("* A\n<2026-01-01> <2026-01-02>\n* B\n<2026-01-03>\n* C\nno ts\n")
        .expect("parse");
    assert_eq!(doc.max_timestamp_count(), Some(2));
    assert_eq!(doc.min_timestamp_count(), Some(0));
}

#[test]
fn doc_median_timestamp_count_match() {
    let doc = parse("* A\nno\n* B\n<2026-01-01>\n* C\n<2026-01-02> <2026-01-03>\n")
        .expect("parse");
    // 0,1,2 -> median 1
    assert_eq!(doc.median_timestamp_count(), Some(1));
}

#[test]
fn doc_timestamp_count_counts_match() {
    let doc = parse("* A\n<2026-01-01>\n* B\n<2026-01-02>\n* C\n<2026-01-03> <2026-01-04>\n")
        .expect("parse");
    // counts 1,1,2
    let m = doc.timestamp_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn doc_mode_timestamp_count_match() {
    let doc = parse("* A\n<2026-01-01>\n* B\n<2026-01-02>\n* C\n<2026-01-03> <2026-01-04>\n")
        .expect("parse");
    assert_eq!(doc.mode_timestamp_count(), Some(1));
}

#[test]
fn doc_total_root_timestamp_count_match() {
    let doc = parse("* A\n<2026-01-01> <2026-01-02>\n** child\n<2026-09-09>\n* B\n<2026-01-03>\n")
        .expect("parse");
    // roots only: A=2, B=1 -> total 3
    assert_eq!(doc.total_root_timestamp_count(), 3);
}

#[test]
fn doc_max_min_root_timestamp_count_match() {
    let doc = parse("* A\n<2026-01-01> <2026-01-02>\n* B\nno\n").expect("parse");
    assert_eq!(doc.max_root_timestamp_count(), Some(2));
    assert_eq!(doc.min_root_timestamp_count(), Some(0));
}

#[test]
fn doc_mean_root_timestamp_count_match() {
    let doc = parse("* A\n<2026-01-01> <2026-01-02> <2026-01-03> <2026-01-04>\n* B\nno\n")
        .expect("parse");
    // 4,0 -> mean 2
    assert_eq!(doc.mean_root_timestamp_count(), 2);
}

#[test]
fn doc_median_root_timestamp_count_match() {
    let doc = parse("* A\nno\n* B\n<2026-01-01>\n* C\n<2026-01-02> <2026-01-03>\n")
        .expect("parse");
    // 0,1,2 -> median 1
    assert_eq!(doc.median_root_timestamp_count(), Some(1));
}

#[test]
fn doc_root_timestamp_count_counts_match() {
    let doc = parse("* A\n<2026-01-01>\n* B\n<2026-01-02>\n* C\n<2026-01-03> <2026-01-04>\n")
        .expect("parse");
    let m = doc.root_timestamp_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn doc_scheduled_pct_match() {
    let doc = parse("* A\nSCHEDULED: <2026-01-01>\n* B\n* C\n* D\n").expect("parse");
    // 1 of 4 scheduled -> 25
    assert_eq!(doc.scheduled_pct(), 25);
}

#[test]
fn doc_deadline_pct_match() {
    let doc = parse("* A\nDEADLINE: <2026-01-01>\n* B\n").expect("parse");
    // 1 of 2 -> 50
    assert_eq!(doc.deadline_pct(), 50);
}

#[test]
fn doc_count_timestamped_match() {
    let doc = parse("* A\n<2026-01-01>\n* B\nno ts\n* C\n<2026-02-02>\n").expect("parse");
    assert_eq!(doc.count_timestamped(), 2);
}

#[test]
fn doc_timestamped_pct_match() {
    let doc = parse("* A\n<2026-01-01>\n* B\nno\n* C\n<2026-02-02>\n* D\nno\n").expect("parse");
    // 2 of 4 -> 50
    assert_eq!(doc.timestamped_pct(), 50);
}

#[test]
fn doc_scheduled_pct_zero_when_empty() {
    let doc = parse("preamble only\n").expect("parse");
    assert_eq!(doc.scheduled_pct(), 0);
    assert_eq!(doc.timestamped_pct(), 0);
}

#[test]
fn doc_count_unscheduled_match() {
    let doc = parse("* A\nSCHEDULED: <2026-01-01>\n* B\n* C\n").expect("parse");
    // 2 of 3 not scheduled
    assert_eq!(doc.count_unscheduled(), 2);
}

#[test]
fn doc_unscheduled_pct_match() {
    let doc = parse("* A\nSCHEDULED: <2026-01-01>\n* B\n* C\n* D\n").expect("parse");
    // 3 of 4 -> 75
    assert_eq!(doc.unscheduled_pct(), 75);
}

#[test]
fn doc_count_no_deadline_match() {
    let doc = parse("* A\nDEADLINE: <2026-01-01>\n* B\n* C\n").expect("parse");
    assert_eq!(doc.count_no_deadline(), 2);
}

#[test]
fn doc_no_deadline_pct_match() {
    let doc = parse("* A\nDEADLINE: <2026-01-01>\n* B\n").expect("parse");
    // 1 of 2 -> 50
    assert_eq!(doc.no_deadline_pct(), 50);
}

#[test]
fn doc_count_untimestamped_match() {
    let doc = parse("* A\n<2026-01-01>\n* B\nno\n* C\nno\n").expect("parse");
    assert_eq!(doc.count_untimestamped(), 2);
}

#[test]
fn doc_untimestamped_pct_match() {
    let doc = parse("* A\n<2026-01-01>\n* B\nno\n* C\nno\n* D\nno\n").expect("parse");
    // 3 of 4 -> 75
    assert_eq!(doc.untimestamped_pct(), 75);
}

#[test]
fn doc_count_roots_scheduled_match() {
    let doc = parse("* A\nSCHEDULED: <2026-01-01>\n** child\nSCHEDULED: <2026-02-02>\n* B\n")
        .expect("parse");
    // roots only: A scheduled, B not -> 1
    assert_eq!(doc.count_roots_scheduled(), 1);
}

#[test]
fn doc_count_roots_with_deadline_match() {
    let doc = parse("* A\nDEADLINE: <2026-01-01>\n* B\n* C\nDEADLINE: <2026-03-03>\n")
        .expect("parse");
    assert_eq!(doc.count_roots_with_deadline(), 2);
}

#[test]
fn doc_root_scheduled_pct_match() {
    let doc = parse("* A\nSCHEDULED: <2026-01-01>\n* B\n* C\n* D\n").expect("parse");
    // 1 of 4 roots -> 25
    assert_eq!(doc.root_scheduled_pct(), 25);
}

#[test]
fn doc_root_deadline_pct_match() {
    let doc = parse("* A\nDEADLINE: <2026-01-01>\n* B\n").expect("parse");
    assert_eq!(doc.root_deadline_pct(), 50);
}

#[test]
fn doc_root_scheduled_pct_zero_when_no_roots() {
    let doc = parse("preamble only\n").expect("parse");
    assert_eq!(doc.root_scheduled_pct(), 0);
}

#[test]
fn doc_archived_pct_match() {
    let doc = parse("* A :ARCHIVE:\n* B\n* C\n* D\n").expect("parse");
    // 1 of 4 -> 25
    assert_eq!(doc.archived_pct(), 25);
}

#[test]
fn doc_comment_pct_match() {
    let doc = parse("* COMMENT A\n* B\n").expect("parse");
    // 1 of 2 -> 50
    assert_eq!(doc.comment_pct(), 50);
}

#[test]
fn doc_count_non_archived_match() {
    let doc = parse("* A :ARCHIVE:\n* B\n* C\n").expect("parse");
    assert_eq!(doc.count_non_archived(), 2);
}

#[test]
fn doc_non_archived_pct_match() {
    let doc = parse("* A :ARCHIVE:\n* B\n* C\n* D\n").expect("parse");
    assert_eq!(doc.non_archived_pct(), 75);
}

#[test]
fn doc_count_non_comment_match() {
    let doc = parse("* COMMENT A\n* B\n* C\n").expect("parse");
    assert_eq!(doc.count_non_comment(), 2);
}

#[test]
fn doc_non_comment_pct_match() {
    let doc = parse("* COMMENT A\n* B\n").expect("parse");
    assert_eq!(doc.non_comment_pct(), 50);
}

#[test]
fn doc_archived_pct_zero_when_empty() {
    let doc = parse("preamble only\n").expect("parse");
    assert_eq!(doc.archived_pct(), 0);
    assert_eq!(doc.comment_pct(), 0);
}

#[test]
fn doc_root_archived_pct_match() {
    let doc = parse("* A :ARCHIVE:\n* B\n* C\n* D\n").expect("parse");
    assert_eq!(doc.root_archived_pct(), 25);
}

#[test]
fn doc_root_comment_pct_match() {
    let doc = parse("* COMMENT A\n* B\n").expect("parse");
    assert_eq!(doc.root_comment_pct(), 50);
}

#[test]
fn doc_count_non_archived_roots_match() {
    let doc = parse("* A :ARCHIVE:\n* B\n* C\n").expect("parse");
    assert_eq!(doc.count_non_archived_roots(), 2);
}

#[test]
fn doc_count_non_comment_roots_match() {
    let doc = parse("* COMMENT A\n* B\n* C\n").expect("parse");
    assert_eq!(doc.count_non_comment_roots(), 2);
}

#[test]
fn doc_root_archived_pct_zero_when_no_roots() {
    let doc = parse("preamble only\n").expect("parse");
    assert_eq!(doc.root_archived_pct(), 0);
    assert_eq!(doc.root_comment_pct(), 0);
}

#[test]
fn doc_body_pct_match() {
    let doc = parse("* A\nbody\n* B\n* C\n* D\n").expect("parse");
    // 1 of 4 has body -> 25
    assert_eq!(doc.body_pct(), 25);
}

#[test]
fn doc_count_empty_body_match() {
    let doc = parse("* A\nbody\n* B\n* C\n").expect("parse");
    assert_eq!(doc.count_empty_body(), 2);
}

#[test]
fn doc_empty_body_pct_match() {
    let doc = parse("* A\nbody\n* B\n").expect("parse");
    assert_eq!(doc.empty_body_pct(), 50);
}

#[test]
fn doc_root_with_body_count_match() {
    let doc = parse("* A\nbody\n** child\nchild body\n* B\n").expect("parse");
    // roots only: A has body, B not -> 1
    assert_eq!(doc.root_with_body_count(), 1);
}

#[test]
fn doc_root_body_pct_match() {
    let doc = parse("* A\nbody\n* B\n* C\n* D\n").expect("parse");
    assert_eq!(doc.root_body_pct(), 25);
}

#[test]
fn doc_body_pct_zero_when_empty() {
    let doc = parse("preamble only\n").expect("parse");
    assert_eq!(doc.body_pct(), 0);
    assert_eq!(doc.root_body_pct(), 0);
}

#[test]
fn doc_count_no_id_match() {
    let doc = parse("* A\n:PROPERTIES:\n:ID: x1\n:END:\n* B\n* C\n").expect("parse");
    assert_eq!(doc.count_no_id(), 2);
}

#[test]
fn doc_count_roots_without_id_match() {
    let doc = parse("* A\n:PROPERTIES:\n:ID: x1\n:END:\n* B\n* C\n").expect("parse");
    assert_eq!(doc.count_roots_without_id(), 2);
}

#[test]
fn doc_root_id_pct_match() {
    let doc = parse("* A\n:PROPERTIES:\n:ID: x1\n:END:\n* B\n* C\n* D\n").expect("parse");
    // 1 of 4 roots -> 25
    assert_eq!(doc.root_id_pct(), 25);
}

#[test]
fn doc_root_id_pct_zero_when_no_roots() {
    let doc = parse("preamble only\n").expect("parse");
    assert_eq!(doc.root_id_pct(), 0);
}

#[test]
fn doc_count_roots_tagged_match() {
    let doc = parse("* A :x:\n* B\n* C :y:z:\n").expect("parse");
    assert_eq!(doc.count_roots_tagged(), 2);
}

#[test]
fn doc_count_roots_with_any_todo_match() {
    let doc = parse("* TODO A\n* B\n* DONE C\n").expect("parse");
    assert_eq!(doc.count_roots_with_any_todo(), 2);
}

#[test]
fn doc_count_roots_prioritized_match() {
    let doc = parse("* [#A] A\n* B\n* [#C] C\n").expect("parse");
    assert_eq!(doc.count_roots_prioritized(), 2);
}

#[test]
fn doc_root_tagged_pct_match() {
    let doc = parse("* A :x:\n* B\n* C\n* D\n").expect("parse");
    assert_eq!(doc.root_tagged_pct(), 25);
}

#[test]
fn doc_root_todo_pct_match() {
    let doc = parse("* TODO A\n* B\n").expect("parse");
    assert_eq!(doc.root_todo_pct(), 50);
}

#[test]
fn doc_root_priority_pct_match() {
    let doc = parse("* [#A] A\n* B\n* C\n* D\n").expect("parse");
    assert_eq!(doc.root_priority_pct(), 25);
}

#[test]
fn doc_root_tagged_pct_zero_when_no_roots() {
    let doc = parse("preamble only\n").expect("parse");
    assert_eq!(doc.root_tagged_pct(), 0);
    assert_eq!(doc.root_todo_pct(), 0);
    assert_eq!(doc.root_priority_pct(), 0);
}

#[test]
fn doc_count_roots_untagged_match() {
    let doc = parse("* A :x:\n* B\n* C\n").expect("parse");
    assert_eq!(doc.count_roots_untagged(), 2);
}

#[test]
fn doc_count_roots_without_todo_match() {
    let doc = parse("* TODO A\n* B\n* C\n").expect("parse");
    assert_eq!(doc.count_roots_without_todo(), 2);
}

#[test]
fn doc_count_roots_unprioritized_match() {
    let doc = parse("* [#A] A\n* B\n* C\n").expect("parse");
    assert_eq!(doc.count_roots_unprioritized(), 2);
}

#[test]
fn doc_root_untagged_pct_match() {
    let doc = parse("* A :x:\n* B\n* C\n* D\n").expect("parse");
    assert_eq!(doc.root_untagged_pct(), 75);
}

#[test]
fn doc_root_no_todo_pct_match() {
    let doc = parse("* TODO A\n* B\n").expect("parse");
    assert_eq!(doc.root_no_todo_pct(), 50);
}

#[test]
fn doc_root_unprioritized_pct_match() {
    let doc = parse("* [#A] A\n* B\n* C\n* D\n").expect("parse");
    assert_eq!(doc.root_unprioritized_pct(), 75);
}

#[test]
fn doc_count_branches_match() {
    let doc = parse("* A\n** B\n** C\n* D\n").expect("parse");
    // A has children (branch); B,C,D leaves -> 1 branch
    assert_eq!(doc.count_branches(), 1);
}

#[test]
fn doc_branch_pct_match() {
    let doc = parse("* A\n** B\n* C\n* D\n").expect("parse");
    // A branch; B,C,D leaves -> 1 of 4 = 25
    assert_eq!(doc.branch_pct(), 25);
}

#[test]
fn doc_count_root_leaves_match() {
    let doc = parse("* A\n** B\n* C\n* D\n").expect("parse");
    // roots: A branch, C leaf, D leaf -> 2 leaf roots
    assert_eq!(doc.count_root_leaves(), 2);
}

#[test]
fn doc_count_root_branches_match() {
    let doc = parse("* A\n** B\n* C\n** D\n* E\n").expect("parse");
    // roots: A branch, C branch, E leaf -> 2 branch roots
    assert_eq!(doc.count_root_branches(), 2);
}

#[test]
fn doc_root_leaf_pct_match() {
    let doc = parse("* A\n** B\n* C\n* D\n* E\n").expect("parse");
    // roots: A branch; C,D,E leaves -> 3 of 4 = 75
    assert_eq!(doc.root_leaf_pct(), 75);
}

#[test]
fn doc_root_branch_pct_match() {
    let doc = parse("* A\n** B\n* C\n* D\n* E\n").expect("parse");
    assert_eq!(doc.root_branch_pct(), 25);
}

#[test]
fn doc_branch_pct_zero_when_empty() {
    let doc = parse("preamble only\n").expect("parse");
    assert_eq!(doc.branch_pct(), 0);
    assert_eq!(doc.root_branch_pct(), 0);
}

#[test]
fn doc_most_common_tag_match() {
    let doc = parse("* A :x:\n* B :x:\n* C :y:\n").expect("parse");
    assert_eq!(doc.most_common_tag(), Some("x".to_owned()));
}

#[test]
fn doc_most_common_todo_match() {
    let doc = parse("* TODO A\n* TODO B\n* DONE C\n").expect("parse");
    assert_eq!(doc.most_common_todo(), Some("TODO".to_owned()));
}

#[test]
fn doc_most_common_level_match() {
    let doc = parse("* A\n* B\n** C\n").expect("parse");
    // levels 1,1,2 -> mode 1
    assert_eq!(doc.most_common_level(), Some(1));
}

#[test]
fn doc_most_common_priority_match() {
    let doc = parse("* [#A] X\n* [#A] Y\n* [#B] Z\n").expect("parse");
    assert_eq!(doc.most_common_priority(), Some('A'));
}

#[test]
fn doc_most_common_tag_none_when_none() {
    let doc = parse("* A\n").expect("parse");
    assert_eq!(doc.most_common_tag(), None);
    assert_eq!(doc.most_common_todo(), None);
    assert_eq!(doc.most_common_priority(), None);
}

#[test]
fn doc_property_values_for_key_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:Foo: 1\n:END:\n* B\n:PROPERTIES:\n:Foo: 2\n:END:\n",
    )
    .expect("parse");
    let mut vals = doc.property_values_for_key("Foo");
    vals.sort();
    assert_eq!(vals, vec!["1".to_owned(), "2".to_owned()]);
}

#[test]
fn doc_property_values_for_key_empty_for_unknown() {
    let doc = parse("* A\n:PROPERTIES:\n:Foo: 1\n:END:\n").expect("parse");
    assert!(doc.property_values_for_key("Nope").is_empty());
}

#[test]
fn doc_most_common_property_value_for_key_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:Foo: x\n:END:\n* B\n:PROPERTIES:\n:Foo: x\n:END:\n* C\n:PROPERTIES:\n:Foo: y\n:END:\n",
    )
    .expect("parse");
    assert_eq!(
        doc.most_common_property_value_for_key("Foo"),
        Some("x".to_owned())
    );
}

#[test]
fn doc_distinct_root_property_keys_sorted() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:Foo: 1\n:Bar: 2\n:END:\n** child\n:PROPERTIES:\n:Zed: 9\n:END:\n* B\n:PROPERTIES:\n:Foo: 3\n:END:\n",
    )
    .expect("parse");
    // roots only, child Zed excluded
    assert_eq!(
        doc.distinct_root_property_keys(),
        vec!["Bar".to_owned(), "Foo".to_owned()]
    );
}

#[test]
fn doc_distinct_root_property_key_count_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:Foo: 1\n:Bar: 2\n:END:\n",
    )
    .expect("parse");
    assert_eq!(doc.distinct_root_property_key_count(), 2);
}

#[test]
fn doc_root_property_key_counts_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:Foo: 1\n:Bar: 2\n:END:\n* B\n:PROPERTIES:\n:Foo: 3\n:END:\n",
    )
    .expect("parse");
    let m = doc.root_property_key_counts();
    assert_eq!(m.get("Foo"), Some(&2));
    assert_eq!(m.get("Bar"), Some(&1));
}

#[test]
fn doc_root_property_values_for_key_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:Foo: 1\n:END:\n** child\n:PROPERTIES:\n:Foo: 9\n:END:\n* B\n:PROPERTIES:\n:Foo: 2\n:END:\n",
    )
    .expect("parse");
    let mut vals = doc.root_property_values_for_key("Foo");
    vals.sort();
    // roots only; child Foo:9 excluded
    assert_eq!(vals, vec!["1".to_owned(), "2".to_owned()]);
}

#[test]
fn doc_distinct_root_property_values_for_key_sorted() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:Foo: x\n:END:\n* B\n:PROPERTIES:\n:Foo: x\n:END:\n* C\n:PROPERTIES:\n:Foo: y\n:END:\n",
    )
    .expect("parse");
    assert_eq!(
        doc.distinct_root_property_values_for_key("Foo"),
        vec!["x".to_owned(), "y".to_owned()]
    );
}

#[test]
fn doc_distinct_root_property_value_count_for_key_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:Foo: x\n:END:\n* B\n:PROPERTIES:\n:Foo: y\n:END:\n",
    )
    .expect("parse");
    assert_eq!(doc.distinct_root_property_value_count_for_key("Foo"), 2);
}

#[test]
fn doc_least_common_property_value_for_key_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:Foo: x\n:END:\n* B\n:PROPERTIES:\n:Foo: x\n:END:\n* C\n:PROPERTIES:\n:Foo: y\n:END:\n",
    )
    .expect("parse");
    // x=2, y=1 -> least y
    assert_eq!(
        doc.least_common_property_value_for_key("Foo"),
        Some("y".to_owned())
    );
}

#[test]
fn doc_least_common_property_value_for_key_none_for_unknown() {
    let doc = parse("* A\n:PROPERTIES:\n:Foo: 1\n:END:\n").expect("parse");
    assert_eq!(doc.least_common_property_value_for_key("Nope"), None);
}

#[test]
fn doc_property_pct_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:Foo: 1\n:END:\n* B\n* C\n* D\n",
    )
    .expect("parse");
    assert_eq!(doc.property_pct(), 25);
}

#[test]
fn doc_count_no_property_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:Foo: 1\n:END:\n* B\n* C\n",
    )
    .expect("parse");
    assert_eq!(doc.count_no_property(), 2);
}

#[test]
fn doc_no_property_pct_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:Foo: 1\n:END:\n* B\n",
    )
    .expect("parse");
    assert_eq!(doc.no_property_pct(), 50);
}

#[test]
fn doc_count_roots_with_any_property_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:Foo: 1\n:END:\n** child\n:PROPERTIES:\n:Bar: 9\n:END:\n* B\n",
    )
    .expect("parse");
    // roots only: A has props, B not -> 1
    assert_eq!(doc.count_roots_with_any_property(), 1);
}

#[test]
fn doc_root_property_pct_match() {
    let doc = parse(
        "* A\n:PROPERTIES:\n:Foo: 1\n:END:\n* B\n* C\n* D\n",
    )
    .expect("parse");
    assert_eq!(doc.root_property_pct(), 25);
}

#[test]
fn doc_property_pct_zero_when_empty() {
    let doc = parse("preamble only\n").expect("parse");
    assert_eq!(doc.property_pct(), 0);
    assert_eq!(doc.root_property_pct(), 0);
}
