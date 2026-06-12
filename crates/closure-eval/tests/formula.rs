//! Coda-style formulas: a user-language program receives database
//! rows as tab-separated stdin and its stdout becomes cell values.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_eval::{eval_with_input, formula_column};

#[test]
fn eval_with_input_pipes_data_on_stdin() {
    let out = eval_with_input("wc -l", "a\nb\n").expect("eval");
    assert_eq!(out.exit, 0);
    assert_eq!(out.stdout.trim(), "2");
}

#[test]
fn eval_with_input_reports_exit_code() {
    let out = eval_with_input("exit 3", "").expect("eval");
    assert_eq!(out.exit, 3);
}

#[test]
fn eval_with_input_captures_stderr() {
    let out = eval_with_input("echo oops >&2", "").expect("eval");
    assert_eq!(out.stderr.trim(), "oops");
}

#[test]
fn formula_column_computes_one_value_per_row() {
    let rows = vec![
        vec!["1".to_owned(), "2".to_owned()],
        vec!["3".to_owned(), "4".to_owned()],
    ];
    let col = formula_column("awk -F'\t' '{print $1 + $2}'", &rows).expect("formula");
    assert_eq!(col, vec!["3".to_owned(), "7".to_owned()]);
}

#[test]
fn formula_column_passes_cells_tab_separated() {
    let rows = vec![vec!["Ship".to_owned(), "TODO".to_owned()]];
    let col = formula_column("cut -f2", &rows).expect("formula");
    assert_eq!(col, vec!["TODO".to_owned()]);
}

#[test]
fn formula_column_empty_rows_is_empty() {
    let col = formula_column("cat", &[]).expect("formula");
    assert!(col.is_empty());
}
