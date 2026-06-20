//! L1: hover. Over an `id:` link, preview the linked headline (title +
//! file/ancestor breadcrumb); over a headline, describe it (level, id,
//! todo, tags). `None` when nothing resolvable sits under the cursor.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_lsp::{handle_message, hover};
use closure_store::Vault;

const ID: &str = "01HXAAAAAAAAAAAAAAAAAAAAAA";

fn vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("target.org"),
        format!("* Project\n** TODO The target\n:PROPERTIES:\n:ID: {ID}\n:END:\nbody\n"),
    )
    .expect("write target");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn hover_over_id_link_previews_title_file_and_ancestor() {
    let (_d, v) = vault();
    let src = format!("see [[id:{ID}]] for details\n");
    // character 12 sits inside the id value.
    let out = hover(&src, &v, 0, 12).expect("resolves a hover");
    assert!(out.contains("The target"), "shows target title: {out}");
    assert!(out.contains("Project"), "shows ancestor headline: {out}");
    assert!(out.contains("target.org"), "shows owning file: {out}");
}

#[test]
fn hover_over_headline_describes_level_and_id() {
    let (_d, v) = vault();
    let src = format!("* Project\n** TODO The target\n:PROPERTIES:\n:ID: {ID}\n:END:\nbody\n");
    let out = hover(&src, &v, 1, 4).expect("headline hover");
    assert!(out.contains("level 2"), "shows outline level: {out}");
    assert!(out.contains(ID), "shows the headline id: {out}");
}

#[test]
fn hover_over_plain_text_is_none() {
    let (_d, v) = vault();
    assert!(hover("just prose here\n", &v, 0, 3).is_none());
}

#[test]
fn hover_over_unknown_id_link_is_none() {
    let (_d, v) = vault();
    let src = "see [[id:01HXZZZZZZZZZZZZZZZZZZZZZZ]] now\n";
    assert!(hover(src, &v, 0, 12).is_none());
}

#[test]
fn hover_out_of_range_position_is_none() {
    let (_d, v) = vault();
    assert!(hover("* H\n", &v, 99, 0).is_none());
}

#[test]
fn protocol_textdocument_hover_returns_contents() {
    let (_d, v) = vault();
    let req = "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"textDocument/hover\",\
         \"params\":{\"textDocument\":{\"uri\":\"file://target.org\"},\
         \"position\":{\"line\":1,\"character\":4}}}";
    let resp = handle_message(&v, req).expect("response");
    assert!(resp.contains("\"id\":7"), "echoes id: {resp}");
    assert!(resp.contains("contents"), "wraps hover contents: {resp}");
    assert!(resp.contains("level 2"), "carries the hover text: {resp}");
}

#[test]
fn protocol_initialize_advertises_hover() {
    let (_d, v) = vault();
    let req = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}";
    let resp = handle_message(&v, req).expect("response");
    assert!(resp.contains("hoverProvider"), "advertises hover: {resp}");
}
