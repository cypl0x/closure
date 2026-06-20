//! X1c hermetic core: vault -> rows -> QML document. The Qt window is
//! display-bound (exercised with `--features qt`); this runs in the
//! default build with no Qt.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_shell_qt::{qml_document, rows};
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
    let td = vault(&[("n.org", "* TODO Top\n** Sub\n")]);
    assert_eq!(rows(td.path()).expect("rows"), vec!["TODO Top", "  Sub"]);
}

#[test]
fn qml_document_embeds_rows_as_a_listview_model() {
    let doc = qml_document(&["Alpha".into(), "Beta".into()]);
    assert!(doc.contains("ListView"), "renders a list");
    assert!(doc.contains("model: [\"Alpha\",\"Beta\"]"), "embeds rows: {doc}");
    assert!(doc.contains("import QtQuick"), "is a QML document");
}

#[test]
fn qml_document_escapes_quotes_and_backslashes() {
    let doc = qml_document(&["say \"hi\"".into(), "a\\b".into()]);
    assert!(doc.contains("say \\\"hi\\\""), "escapes quotes: {doc}");
    assert!(doc.contains("a\\\\b"), "escapes backslash: {doc}");
}
