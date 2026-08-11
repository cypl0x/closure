//! `#+TBLFM:` recomputed into the file — org's `C-c C-c` on a formula
//! line.
//!
//! `eval_table_formulas` says what each cell should be, and nothing
//! put it back, so a table with a total column still had a total
//! somebody had typed. Putting it back is a mutation, which makes it a
//! command: through the registry (I8), producing an `Edit` the undo
//! tree can reverse (I3). That is the same split composition landed on
//! — deriving is a view, writing is a command — and this is the write.
//!
//! One edit, not one per cell. A half-recomputed table is a table that
//! never existed, and `undo` has to be able to say so.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::{BlockId, Command as _, Document, RecomputeTable};

const DOC: &str = "\
* Costs
:PROPERTIES:
:ID: 01RECOMP0000000000001A
:END:
| item | qty | each | total |
|------+-----+------+-------|
| nail | 10  | 2    |       |
| bolt | 3   | 5    | wrong |
#+TBLFM: $4=$2*$3
";

fn recompute(src: &str, id: &str) -> Document {
    let mut doc = Document::load_str(src).expect("parse");
    let cmd = RecomputeTable::new(BlockId::from_existing(id));
    cmd.apply(&mut doc).expect("apply");
    doc
}

#[test]
fn every_row_gets_the_value_its_formula_says() {
    let doc = recompute(DOC, "01RECOMP0000000000001A");
    let src = doc.source();
    assert!(src.contains("| nail | 10  | 2    | 20"), "{src}");
    assert!(src.contains("| bolt | 3   | 5    | 15"), "{src}");
}

#[test]
fn a_stale_value_is_replaced_rather_than_kept() {
    let doc = recompute(DOC, "01RECOMP0000000000001A");
    assert!(!doc.source().contains("wrong"), "{}", doc.source());
}

#[test]
fn the_header_and_the_separator_are_left_alone() {
    // A formula is about data rows. Computing over the header would
    // put a number where the column's name goes.
    let doc = recompute(DOC, "01RECOMP0000000000001A");
    let src = doc.source();
    assert!(src.contains("| item | qty | each | total |"), "{src}");
    assert!(src.contains("|------+"), "the separator went: {src}");
}

#[test]
fn the_formula_line_survives() {
    // Recomputing is not consuming: the table has to be recomputable
    // again tomorrow.
    let doc = recompute(DOC, "01RECOMP0000000000001A");
    assert!(doc.source().contains("#+TBLFM: $4=$2*$3"));
}

#[test]
fn one_undo_puts_the_whole_table_back() {
    // I3. A half-recomputed table is one that never existed.
    let mut doc = Document::load_str(DOC).expect("parse");
    let before = doc.source();
    let cmd = RecomputeTable::new(BlockId::from_existing("01RECOMP0000000000001A"));
    cmd.apply(&mut doc).expect("apply");
    assert_ne!(doc.source(), before, "nothing happened");
    doc.undo().expect("undo");
    assert_eq!(doc.source(), before);
}

#[test]
fn a_headline_with_no_formula_is_left_exactly_as_it_was() {
    let src = "* Plain\n:PROPERTIES:\n:ID: 01RECOMP0000000000002A\n:END:\n| a | b |\n";
    let mut doc = Document::load_str(src).expect("parse");
    let cmd = RecomputeTable::new(BlockId::from_existing("01RECOMP0000000000002A"));
    let _ = cmd.apply(&mut doc);
    assert_eq!(doc.source(), src);
}

#[test]
fn it_is_a_registered_command_with_a_key() {
    // I4: every command carries its keybinding.
    let cmd = RecomputeTable::new_placeholder();
    assert_eq!(closure_core::Command::name(&cmd), "recompute-table");
    assert!(!closure_core::Command::keys(&cmd).is_empty());
}
