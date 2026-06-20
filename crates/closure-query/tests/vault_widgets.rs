//! V2b: vault-wide widget registry. Widget definitions live in any file
//! (e.g. `widgets.org`) and are referenced from another; `closure-query`
//! resolves references across the whole vault.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_query::{WidgetError, expand_doc_widgets, vault_widget_names};
use closure_store::Vault;

fn vault(files: &[(&str, &str)]) -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tmp");
    for (name, body) in files {
        fs::write(dir.path().join(name), body).expect("write");
    }
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn reference_resolves_across_files() {
    let (_d, v) = vault(&[
        (
            "widgets.org",
            "#+BEGIN: closure-widget :name banner\n== closure ==\n#+END:\n",
        ),
        (
            "notes.org",
            "* Home\n#+BEGIN: closure-widget :name page\ntop {{banner}} bottom\n#+END:\n",
        ),
    ]);
    let out = expand_doc_widgets(&v, std::path::Path::new("notes.org")).unwrap();
    assert!(
        out.contains("top == closure == bottom"),
        "cross-file ref expanded: {out}"
    );
}

#[test]
fn widget_names_lists_every_definition_with_its_file() {
    let (_d, v) = vault(&[
        (
            "widgets.org",
            "#+BEGIN: closure-widget :name a\nA\n#+END:\n",
        ),
        ("more.org", "#+BEGIN: closure-widget :name b\nB\n#+END:\n"),
    ]);
    let names = vault_widget_names(&v);
    assert!(
        names
            .iter()
            .any(|(n, p)| n == "a" && p.ends_with("widgets.org"))
    );
    assert!(
        names
            .iter()
            .any(|(n, p)| n == "b" && p.ends_with("more.org"))
    );
    // Deterministic order (I6).
    assert_eq!(vault_widget_names(&v), vault_widget_names(&v));
}

#[test]
fn unknown_cross_file_reference_errors() {
    let (_d, v) = vault(&[(
        "notes.org",
        "#+BEGIN: closure-widget :name p\n{{nope}}\n#+END:\n",
    )]);
    assert_eq!(
        expand_doc_widgets(&v, std::path::Path::new("notes.org")),
        Err(WidgetError::Unknown("nope".to_owned()))
    );
}
