//! `#+TBLFM: $3=$1*$2` — table formulas.
//!
//! The line was preserved and never read, so a table with a total
//! column had a total column somebody had typed by hand.
//!
//! Two halves, and only the first is here. Reading a formula and
//! working out what a cell *should* say is arithmetic over a table.
//! Putting the answer back into the file is a mutation, so it is a
//! command, undoable, and its own item — the same split the widget
//! work landed on, and for the same reason: I12 keeps derivation and
//! writing apart on purpose.
//!
//! The subset is column assignments over the four operators, numeric
//! literals and `$N` references. Org's own language has ranges,
//! `vsum`, `@row` addressing and lisp; what is here computes the
//! formula people actually write and says nothing about the rest.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{Formula, eval_table_formulas, table_formulas};

#[test]
fn a_formula_line_is_read() {
    let f = table_formulas("#+TBLFM: $3=$1*$2").expect("a formula line");
    assert_eq!(
        f,
        vec![Formula {
            target: 3,
            expression: "$1*$2".to_owned()
        }]
    );
}

#[test]
fn several_formulas_on_one_line() {
    let f = table_formulas("#+TBLFM: $3=$1*$2::$4=$3+1").expect("formulas");
    assert_eq!(f.len(), 2);
    assert_eq!(f[1].target, 4);
}

#[test]
fn a_line_that_is_not_a_formula_line_is_none() {
    assert_eq!(table_formulas("| a | b |"), None);
    assert_eq!(table_formulas("#+TBLFM:"), None);
}

#[test]
fn a_product_is_computed_for_every_row() {
    let rows = vec![
        vec!["2".to_owned(), "3".to_owned(), String::new()],
        vec!["4".to_owned(), "5".to_owned(), String::new()],
    ];
    let f = table_formulas("#+TBLFM: $3=$1*$2").expect("formula");
    let out = eval_table_formulas(&rows, &f);
    assert_eq!(out[0][2], "6");
    assert_eq!(out[1][2], "20");
}

#[test]
fn the_four_operators_work() {
    let rows = vec![vec!["10".to_owned(), "4".to_owned(), String::new()]];
    let at = |e: &str| {
        let f = table_formulas(&format!("#+TBLFM: $3={e}")).expect("formula");
        eval_table_formulas(&rows, &f)[0][2].clone()
    };
    assert_eq!(at("$1+$2"), "14");
    assert_eq!(at("$1-$2"), "6");
    assert_eq!(at("$1*$2"), "40");
    assert_eq!(at("$1/$2"), "2.5");
}

#[test]
fn a_literal_can_appear_in_the_expression() {
    let rows = vec![vec!["7".to_owned(), String::new()]];
    let f = table_formulas("#+TBLFM: $2=$1*2").expect("formula");
    assert_eq!(eval_table_formulas(&rows, &f)[0][1], "14");
}

#[test]
fn a_cell_that_is_not_a_number_leaves_the_target_alone() {
    // The row is still a row. Blanking it or writing NaN would lose
    // what the author typed, and a formula is not a licence to do that.
    let rows = vec![vec![
        "soon".to_owned(),
        "3".to_owned(),
        "keep me".to_owned(),
    ]];
    let f = table_formulas("#+TBLFM: $3=$1*$2").expect("formula");
    assert_eq!(eval_table_formulas(&rows, &f)[0][2], "keep me");
}

#[test]
fn dividing_by_zero_leaves_the_target_alone_too() {
    let rows = vec![vec!["1".to_owned(), "0".to_owned(), "untouched".to_owned()]];
    let f = table_formulas("#+TBLFM: $3=$1/$2").expect("formula");
    assert_eq!(eval_table_formulas(&rows, &f)[0][2], "untouched");
}

#[test]
fn a_whole_number_has_no_decimal_point() {
    let rows = vec![vec!["2".to_owned(), "2".to_owned(), String::new()]];
    let f = table_formulas("#+TBLFM: $3=$1*$2").expect("formula");
    assert_eq!(eval_table_formulas(&rows, &f)[0][2], "4", "not 4.0");
}

#[test]
fn a_target_column_that_is_not_there_is_ignored() {
    // A formula about a column the table does not have is a mistake in
    // the file, and widening every row to fit it would be a worse one.
    let rows = vec![vec!["1".to_owned(), "2".to_owned()]];
    let f = table_formulas("#+TBLFM: $9=$1+$2").expect("formula");
    assert_eq!(eval_table_formulas(&rows, &f), rows);
}

#[test]
fn evaluating_does_not_touch_the_input() {
    let rows = vec![vec!["1".to_owned(), "2".to_owned(), String::new()]];
    let before = rows.clone();
    let f = table_formulas("#+TBLFM: $3=$1+$2").expect("formula");
    let _ = eval_table_formulas(&rows, &f);
    assert_eq!(rows, before, "the caller's table was mutated");
}
