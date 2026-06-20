//! X1b hermetic core: the GTK list content reflects the vault. The
//! window itself is display-bound (exercised under `.#webview`); this
//! runs in the default build with no GTK.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use tempfile::TempDir;

fn vault(files: &[(&str, &str)]) -> TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    for (name, body) in files {
        fs::write(d.path().join(name), body).expect("write");
    }
    d
}

#[test]
fn rows_list_headlines_with_indent_and_todo() {
    let td = vault(&[("n.org", "* TODO Top task\n** Subtask\n* Plain\n")]);
    let rows = closure_shell_gtk::rows(td.path()).expect("rows");
    assert_eq!(rows, vec!["TODO Top task", "  Subtask", "Plain"]);
}

#[test]
fn rows_empty_vault_is_empty() {
    let td = vault(&[]);
    assert!(closure_shell_gtk::rows(td.path()).expect("rows").is_empty());
}
