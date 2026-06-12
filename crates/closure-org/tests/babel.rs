//! Doc-wide code-block enumeration and `#+RESULTS:` attachment —
//! babel blocks live under headlines, not just in the preamble.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{NodeKind, parse, print, rewrite_attach_results_to_code_block};

const MIXED: &str = "\
#+BEGIN_SRC shell
echo pre
#+END_SRC
* Heading
body
#+BEGIN_SRC shell
echo under-headline
#+END_SRC
** Child
#+BEGIN_SRC python
print('child')
#+END_SRC
";

#[test]
fn code_blocks_enumerates_doc_wide_in_source_order() {
    let doc = parse(MIXED).expect("parse");
    let blocks = doc.code_blocks();
    assert_eq!(blocks.len(), 3);
    let contents: Vec<&str> = blocks
        .iter()
        .filter_map(|n| n.as_code_block())
        .map(|cb| cb.content)
        .collect();
    assert_eq!(
        contents,
        vec!["echo pre\n", "echo under-headline\n", "print('child')\n"]
    );
    assert!(blocks.iter().all(|n| n.kind() == NodeKind::CodeBlock));
}

#[test]
fn attach_results_to_block_under_headline() {
    let doc = parse(MIXED).expect("parse");
    let new = rewrite_attach_results_to_code_block(&doc, 1, "ran\n").expect("attach");
    let out = print(&new);
    let block_at = out.find("echo under-headline").expect("block present");
    let results_at = out.find("#+RESULTS:\n: ran\n").expect("results attached");
    assert!(results_at > block_at, "results follow their block");
    assert!(
        results_at < out.find("** Child").expect("child"),
        "results land before the next headline"
    );
}

#[test]
fn attach_results_preserves_all_other_bytes() {
    let doc = parse(MIXED).expect("parse");
    let new = rewrite_attach_results_to_code_block(&doc, 2, "out\n").expect("attach");
    let out = print(&new);
    let without: String = out.replace("#+RESULTS:\n: out\n", "");
    assert_eq!(without, MIXED, "I1: only the results insertion differs");
}

#[test]
fn attach_results_replaces_existing_results_under_headline() {
    let src = "* H\n#+BEGIN_SRC shell\necho x\n#+END_SRC\n#+RESULTS:\n: old\ntail\n";
    let doc = parse(src).expect("parse");
    let new = rewrite_attach_results_to_code_block(&doc, 0, "fresh\n").expect("attach");
    let out = print(&new);
    assert!(out.contains(": fresh\n"));
    assert!(!out.contains(": old\n"));
    assert!(out.contains("tail\n"), "trailing content survives");
}

#[test]
fn attach_results_out_of_range_errors() {
    let doc = parse(MIXED).expect("parse");
    assert!(rewrite_attach_results_to_code_block(&doc, 9, "x\n").is_err());
}

const NAMED: &str = "\
#+NAME: first
#+BEGIN_SRC shell
echo one
#+END_SRC
* H
#+NAME: deep
#+BEGIN_SRC shell
echo two
#+END_SRC
#+BEGIN_SRC shell
echo anon
#+END_SRC
";

#[test]
fn code_block_name_reads_name_keyword_above() {
    let doc = parse(NAMED).expect("parse");
    assert_eq!(doc.code_block_name(0).as_deref(), Some("first"));
    assert_eq!(doc.code_block_name(1).as_deref(), Some("deep"));
    assert_eq!(doc.code_block_name(2), None, "anonymous block");
    assert_eq!(doc.code_block_name(9), None, "out of range");
}

#[test]
fn code_block_name_is_case_insensitive() {
    let doc = parse("#+name: lower\n#+BEGIN_SRC shell\ntrue\n#+END_SRC\n").expect("parse");
    assert_eq!(doc.code_block_name(0).as_deref(), Some("lower"));
}

#[test]
fn code_block_index_by_name_resolves() {
    let doc = parse(NAMED).expect("parse");
    assert_eq!(doc.code_block_index_by_name("deep"), Some(1));
    assert_eq!(doc.code_block_index_by_name("first"), Some(0));
    assert_eq!(doc.code_block_index_by_name("nope"), None);
}
