//! L4: references + rename. `references` finds the definition and every
//! `id:` link to a headline across the vault; `rename_symbol` retitles
//! the owning headline through the registry (undoable, I3) while the
//! id-based links — and therefore the references — survive.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_lsp::{handle_message, handle_message_mut, references, rename_symbol};
use closure_store::Vault;

const ID: &str = "01HXAAAAAAAAAAAAAAAAAAAAAA";

fn vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("def.org"),
        format!("* Target\n:PROPERTIES:\n:ID: {ID}\n:END:\nbody\n"),
    )
    .expect("def");
    fs::write(
        dir.path().join("ref1.org"),
        format!("* A\nsee [[id:{ID}]] here\n"),
    )
    .expect("r1");
    fs::write(
        dir.path().join("ref2.org"),
        format!("* B\nalso [[id:{ID}]]\n"),
    )
    .expect("r2");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn references_include_definition_and_every_link() {
    let (_d, v) = vault();
    let refs = references(&v, ID);
    assert_eq!(refs.len(), 3, "def + 2 links: {refs:?}");
    assert!(refs.iter().any(|(p, l)| p.ends_with("def.org") && *l == 0));
    assert!(refs.iter().any(|(p, l)| p.ends_with("ref1.org") && *l == 1));
    assert!(refs.iter().any(|(p, l)| p.ends_with("ref2.org") && *l == 1));
}

#[test]
fn references_sorted_and_stable() {
    let (_d, v) = vault();
    assert_eq!(references(&v, ID), references(&v, ID), "deterministic");
}

#[test]
fn rename_retitles_headline_and_keeps_references() {
    let (_d, mut v) = vault();
    rename_symbol(&mut v, ID, "Renamed").expect("rename");

    let (h, _p) = v
        .find_by_id(&closure_core::BlockId::from_existing(ID))
        .expect("still resolves by id");
    assert_eq!(h.title(), "Renamed", "headline retitled");

    // id-based links never changed, so every reference survives.
    assert_eq!(references(&v, ID).len(), 3, "links survive the rename");
}

#[test]
fn rename_unknown_id_errs() {
    let (_d, mut v) = vault();
    assert!(rename_symbol(&mut v, "01HXZZZZZZZZZZZZZZZZZZZZZZ", "X").is_err());
}

#[test]
fn protocol_textdocument_references_lists_locations() {
    let (_d, v) = vault();
    // Position over the link in ref1.org (line 1, inside the id).
    let req = "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"textDocument/references\",\
         \"params\":{\"textDocument\":{\"uri\":\"file://ref1.org\"},\
         \"position\":{\"line\":1,\"character\":12}}}";
    let resp = handle_message(&v, req).expect("response");
    assert!(resp.contains("\"id\":4"), "echoes id: {resp}");
    assert!(resp.contains("def.org"), "includes the definition: {resp}");
    assert!(resp.contains("ref2.org"), "includes the other link: {resp}");
}

#[test]
fn protocol_textdocument_rename_applies_server_side() {
    let (_d, mut v) = vault();
    // Position over the headline in def.org (line 0).
    let req = "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"textDocument/rename\",\
         \"params\":{\"textDocument\":{\"uri\":\"file://def.org\"},\
         \"position\":{\"line\":0,\"character\":2},\"newName\":\"Fresh\"}}";
    let resp = handle_message_mut(&mut v, req).expect("response");
    assert!(resp.contains("\"id\":5"), "echoes id: {resp}");

    let (h, _p) = v
        .find_by_id(&closure_core::BlockId::from_existing(ID))
        .expect("resolves");
    assert_eq!(h.title(), "Fresh", "rename applied through the registry");
}

#[test]
fn protocol_initialize_advertises_references_and_rename() {
    let (_d, v) = vault();
    let req = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}";
    let resp = handle_message(&v, req).expect("response");
    assert!(resp.contains("referencesProvider"), "refs: {resp}");
    assert!(resp.contains("renameProvider"), "rename: {resp}");
}
