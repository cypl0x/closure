//! Every way of making a new headline makes the *same* kind of
//! headline.
//!
//! "How do I create a new headline/sibling/subtree that is still
//! compatible with the P2P sync and is treated as a normal captured
//! item?" — the answer has to be "any of them", and for that they all
//! have to leave the same thing on disk: an `:ID:`.
//!
//! An id is what a block *is* to everything above the parser. Sync
//! addresses blocks by id; so do the undo tree, the links, the cursor
//! memory and the row cache. A headline parsed without one still gets
//! an id — a fresh ULID, in memory, for that run only — so it works
//! perfectly until the file is read a second time, and then it is a
//! different block. That is exactly the failure that cannot be allowed
//! to reach a peer.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_core::BlockId;
use closure_store::Vault;
use tempfile::TempDir;

fn vault(src: &str) -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("a.org"), src).expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

fn id_of(v: &Vault, title: &str) -> BlockId {
    let (h, _) = v.find_by_title(title).expect("headline exists");
    h.id().clone()
}

/// Every `:ID:` value in the file, in order.
fn ids(dir: &TempDir) -> Vec<String> {
    fs::read_to_string(dir.path().join("a.org"))
        .expect("read")
        .lines()
        .filter_map(|l| l.trim().strip_prefix(":ID:").map(|v| v.trim().to_owned()))
        .collect()
}

#[test]
fn a_captured_child_is_written_with_an_id() {
    let (dir, mut v) = vault("* Parent\n:PROPERTIES:\n:ID: 01HQNEW00000000000000001A\n:END:\n");
    let parent = id_of(&v, "Parent");
    v.capture_under(&parent, "TODO ", "Child").expect("capture");
    assert_eq!(ids(&dir).len(), 2, "parent and child");
}

#[test]
fn an_added_sibling_is_written_with_an_id() {
    let (dir, mut v) = vault("* First\n:PROPERTIES:\n:ID: 01HQNEW00000000000000001A\n:END:\n");
    let first = id_of(&v, "First");
    v.add_sibling(&first, "Second").expect("add");
    assert_eq!(ids(&dir).len(), 2);
}

#[test]
fn a_headline_typed_into_a_body_is_written_with_an_id() {
    // The third way, and the one that had no id: a `* Foo` line typed
    // into the body editor is filed as a child (that is the point of
    // it), and it arrived on disk as bare stars — so every reload gave
    // it a new identity and no peer could ever agree with us about it.
    let (dir, mut v) =
        vault("* Parent\n:PROPERTIES:\n:ID: 01HQNEW00000000000000001A\n:END:\nprose\n");
    let parent = id_of(&v, "Parent");
    v.set_body_with_children(&parent, "prose\n", "* Typed\n** Deeper\n")
        .expect("write");
    let on_disk = fs::read_to_string(dir.path().join("a.org")).expect("read");
    assert_eq!(
        ids(&dir).len(),
        3,
        "parent, the typed child and its own child: {on_disk}"
    );
}

#[test]
fn a_typed_headline_keeps_its_id_across_a_reload() {
    // The property that actually matters. An id that changes when the
    // file is read again is not an id.
    let (dir, mut v) = vault("* Parent\n:PROPERTIES:\n:ID: 01HQNEW00000000000000001A\n:END:\n");
    let parent = id_of(&v, "Parent");
    v.set_body_with_children(&parent, "", "* Typed\n")
        .expect("write");
    let before = id_of(&v, "Typed");
    let reopened = Vault::open(dir.path()).expect("reopen");
    assert_eq!(
        id_of(&reopened, "Typed"),
        before,
        "same block after a second read"
    );
}

#[test]
fn a_typed_headline_that_brought_its_own_id_keeps_it() {
    // Pasting a subtree from somewhere else is the case: its ids are
    // already the right ids, and inventing new ones would fork it from
    // the block every peer and every link already knows.
    let (dir, mut v) = vault("* Parent\n:PROPERTIES:\n:ID: 01HQNEW00000000000000001A\n:END:\n");
    let parent = id_of(&v, "Parent");
    v.set_body_with_children(
        &parent,
        "",
        "* Pasted\n:PROPERTIES:\n:ID: 01HQNEW00000000000000009Z\n:END:\n",
    )
    .expect("write");
    assert!(
        ids(&dir).contains(&"01HQNEW00000000000000009Z".to_owned()),
        "the id it came with: {:?}",
        ids(&dir)
    );
    assert_eq!(ids(&dir).len(), 2, "and no second one invented");
}

#[test]
fn a_vault_is_every_org_file_under_it_including_subdirectories() {
    // "How do I handle multiple .org files?" — you do not: the vault is
    // the directory, every `*.org` in it is loaded, and the outline is
    // all of them in one list with the file as a column. Only where a
    // capture *lands* is per-file.
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("a.org"), "* From A\n").expect("write");
    fs::write(dir.path().join("b.org"), "* From B\n").expect("write");
    fs::create_dir(dir.path().join("projects")).expect("mkdir");
    fs::write(dir.path().join("projects/c.org"), "* From C\n").expect("write");
    fs::write(dir.path().join("notes.md"), "# not org\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    for title in ["From A", "From B", "From C"] {
        assert!(v.find_by_title(title).is_some(), "{title} is in the vault");
    }
    assert_eq!(v.iter().count(), 3, "and the markdown file is not");
}

#[test]
fn every_new_headline_is_reachable_by_id_in_the_index() {
    // "Treated as a normal captured item" means the rest of the app
    // can find it: the row list, links, the undo tree and sync all go
    // through the id index.
    let (_d, mut v) = vault("* Parent\n:PROPERTIES:\n:ID: 01HQNEW00000000000000001A\n:END:\n");
    let parent = id_of(&v, "Parent");
    v.capture_under(&parent, "", "Captured").expect("capture");
    v.add_sibling(&parent, "Sibling").expect("sibling");
    v.set_body_with_children(&parent, "", "* Typed\n")
        .expect("typed");
    for title in ["Captured", "Sibling", "Typed"] {
        let id = id_of(&v, title);
        assert!(
            v.find_by_id(&id).is_some(),
            "{title} resolves through the index"
        );
    }
}
