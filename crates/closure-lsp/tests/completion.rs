//! L2: completion. Context-sensitive over the source line up to the
//! cursor: an unterminated `[[id:` completes to vault ids (title as
//! detail), the TODO-keyword slot of a headline completes the configured
//! keywords, and a trailing `:tag:` region completes known vault tags.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_lsp::{CompletionKind, completion, handle_message};
use closure_store::Vault;

const ID: &str = "01HXAAAAAAAAAAAAAAAAAAAAAA";

fn vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("target.org"),
        format!("* Project\n** TODO The target :work:\n:PROPERTIES:\n:ID: {ID}\n:END:\nbody\n"),
    )
    .expect("write target");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn completes_id_link_with_title_detail() {
    let (_d, v) = vault();
    let src = "see [[id:01\n"; // cursor right after "01"
    let items = completion(src, &v, 0, 11);
    let hit = items.iter().find(|i| i.label == ID).expect("id is offered");
    assert_eq!(hit.kind, CompletionKind::Reference);
    assert_eq!(hit.detail, "The target", "title shown as detail");
}

#[test]
fn completes_todo_keyword_in_headline_slot() {
    let (_d, v) = vault();
    let items = completion("* TO\n", &v, 0, 4);
    assert!(
        items
            .iter()
            .any(|i| i.label == "TODO" && i.kind == CompletionKind::Keyword),
        "TODO offered: {items:?}"
    );
    assert!(
        !items.iter().any(|i| i.label == "DONE"),
        "DONE does not start with TO: {items:?}"
    );
}

#[test]
fn completes_tag_from_vault_tags() {
    let (_d, v) = vault();
    let items = completion("* Task :wo\n", &v, 0, 10);
    assert!(
        items
            .iter()
            .any(|i| i.label == "work" && i.kind == CompletionKind::Tag),
        "work tag offered: {items:?}"
    );
}

#[test]
fn no_context_yields_nothing() {
    let (_d, v) = vault();
    assert!(completion("just prose\n", &v, 0, 5).is_empty());
}

#[test]
fn id_completion_filters_by_partial() {
    let (_d, v) = vault();
    // A partial that no id starts with → no id items.
    let items = completion("see [[id:ZZ\n", &v, 0, 11);
    assert!(!items.iter().any(|i| i.label == ID));
}

#[test]
fn protocol_textdocument_completion_returns_items() {
    let (_d, v) = vault();
    let req = "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"textDocument/completion\",\
         \"params\":{\"textDocument\":{\"uri\":\"file://target.org\"},\
         \"position\":{\"line\":1,\"character\":4}}}";
    let resp = handle_message(&v, req).expect("response");
    assert!(resp.contains("\"id\":3"), "echoes id: {resp}");
    // line 1 is `** TODO The target` — keyword slot partial "TODO".
    assert!(resp.contains("\"label\":\"TODO\""), "offers TODO: {resp}");
    assert!(resp.contains("\"kind\":14"), "keyword kind: {resp}");
}

#[test]
fn protocol_initialize_advertises_completion() {
    let (_d, v) = vault();
    let req = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}";
    let resp = handle_message(&v, req).expect("response");
    assert!(
        resp.contains("completionProvider"),
        "advertises completion: {resp}"
    );
}
