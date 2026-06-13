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

// --- header args -----------------------------------------------------------

use closure_eval::{HeaderArgs, var_prelude};

#[test]
fn header_args_parse_results_and_vars() {
    let h = HeaderArgs::parse(":results output :var x=5 :var name=world");
    assert_eq!(h.results.as_deref(), Some("output"));
    assert_eq!(
        h.vars,
        vec![
            ("x".to_owned(), "5".to_owned()),
            ("name".to_owned(), "world".to_owned())
        ]
    );
}

#[test]
fn header_args_empty_and_none_are_default() {
    let h = HeaderArgs::parse("");
    assert_eq!(h.results, None);
    assert!(h.vars.is_empty());
}

#[test]
fn header_args_silent_results() {
    let h = HeaderArgs::parse(":results silent");
    assert!(h.is_silent());
    assert!(!HeaderArgs::parse(":results output").is_silent());
}

#[test]
fn var_prelude_for_shell_and_python() {
    let vars = vec![("x".to_owned(), "5".to_owned())];
    assert_eq!(var_prelude("shell", &vars), "x='5'\n");
    assert_eq!(var_prelude("python", &vars), "x = \"5\"\n");
    assert_eq!(var_prelude("unknown-lang", &vars), "");
}

#[test]
fn shell_eval_with_var_prelude_sees_the_variable() {
    use closure_eval::{Backend, ShellBackend};
    let vars = vec![("name".to_owned(), "world".to_owned())];
    let src = format!("{}echo \"hi $name\"", var_prelude("shell", &vars));
    let out = ShellBackend.eval(&src).expect("eval");
    assert_eq!(out.stdout.trim(), "hi world");
}

#[test]
fn header_args_tangle_target() {
    let h = HeaderArgs::parse(":tangle out.sh :results silent");
    assert_eq!(h.tangle.as_deref(), Some("out.sh"));
    assert!(HeaderArgs::parse(":results output").tangle.is_none());
    assert!(HeaderArgs::parse(":tangle no").tangle.is_none(), "':tangle no' disables");
}
