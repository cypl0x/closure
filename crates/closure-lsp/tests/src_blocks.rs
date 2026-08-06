//! "even better org-edit-special on src blocks and then fiddle with the
//! source code."
//!
//! An org file is mostly prose with islands of another language in it.
//! Inside `#+BEGIN_SRC rust` the thing that knows what the cursor is
//! on is rust-analyzer, not closure — and the thing that knows where
//! line 12 of the block is in the file is closure, not
//! rust-analyzer. So the block is handed over as a document of its
//! own, and every line number that comes back is shifted home again.
//!
//! The geometry is the part that can be quietly wrong, so it is
//! measured on its own before any process is involved.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_lsp::SrcBlock;

const ORG: &str = "\
* A note
:PROPERTIES:
:ID: 01SRCBLOCK00000000000001
:END:
Some prose about the code below.

#+BEGIN_SRC rust
fn main() {
    println!(\"hi\");
}
#+END_SRC

More prose.

#+BEGIN_SRC python
print(\"hi\")
#+END_SRC
";

#[test]
fn a_position_in_prose_is_in_no_block() {
    assert!(SrcBlock::at(ORG, 4).is_none(), "prose was read as code");
    assert!(
        SrcBlock::at(ORG, 6).is_none(),
        "the #+BEGIN_SRC line itself is not inside the block"
    );
    assert!(
        SrcBlock::at(ORG, 10).is_none(),
        "the #+END_SRC line is not inside the block"
    );
}

#[test]
fn a_position_in_a_block_knows_its_language_and_where_it_starts() {
    let b = SrcBlock::at(ORG, 7).expect("line 7 is the first line of the rust block");
    assert_eq!(b.language, "rust");
    assert_eq!(b.first_line, 7);
    assert_eq!(b.text, "fn main() {\n    println!(\"hi\");\n}\n");
}

#[test]
fn the_second_block_is_its_own_document() {
    let b = SrcBlock::at(ORG, 15).expect("the python block");
    assert_eq!(b.language, "python");
    assert_eq!(b.text, "print(\"hi\")\n");
}

#[test]
fn a_line_goes_in_and_comes_back_out_where_it_started() {
    let b = SrcBlock::at(ORG, 8).expect("the rust block");
    // Into the block's own coordinates and back again.
    assert_eq!(b.to_inner(8), 1);
    assert_eq!(b.to_outer(1), 8);
    assert_eq!(b.to_outer(b.to_inner(9)), 9);
    // The block's first line is line 0 of the document it becomes.
    assert_eq!(b.to_inner(7), 0);
}

#[test]
fn an_answer_about_the_block_is_shifted_home() {
    // The shape a language server's reply has: ranges in the virtual
    // document, which mean nothing to an editor showing the org file.
    let b = SrcBlock::at(ORG, 8).expect("the rust block");
    let reply = r#"{"range":{"start":{"line":1,"character":4},"end":{"line":1,"character":12}}}"#;
    let home = b.shift_home(reply);
    assert!(
        home.contains(r#""line":8"#),
        "line 1 of the block is line 8 of the file, got {home}"
    );
    assert!(
        home.contains(r#""character":4"#),
        "columns are not shifted — a block is not indented into the file: {home}"
    );
}

#[test]
fn shifting_home_leaves_everything_that_is_not_a_line_alone() {
    let b = SrcBlock::at(ORG, 8).expect("the rust block");
    let reply = r#"{"contents":"line 1 of the docs","range":{"start":{"line":0,"character":0}}}"#;
    let home = b.shift_home(reply);
    assert!(
        home.contains("line 1 of the docs"),
        "prose that happens to say `line` was rewritten: {home}"
    );
    assert!(home.contains(r#""line":7"#), "{home}");
}
