//! D7: OS-clipboard bridge. The kill ring stays the hub; a `Clipboard`
//! adapter mirrors its top *out* to an external clipboard and pulls
//! external text *in* — but cut/paste keep working with no clipboard at
//! all (the bridge is additive, never the only path). The hermetic
//! default is an in-memory clipboard; a real system clipboard lives behind
//! the `clipboard` feature.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_core::BlockId;
use closure_store::{Clipboard, MemoryClipboard, Vault};
use tempfile::TempDir;

fn write_vault(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, body) in files {
        fs::write(dir.path().join(name), body).expect("write");
    }
    dir
}

fn three() -> TempDir {
    write_vault(&[(
        "a.org",
        "* One\n:PROPERTIES:\n:ID: 01HX0000000000000000000001\n:END:\nbody one\n\
         * Two\n:PROPERTIES:\n:ID: 01HX0000000000000000000002\n:END:\n\
         * Three\n:PROPERTIES:\n:ID: 01HX0000000000000000000003\n:END:\n",
    )])
}

fn id(v: &Vault, title: &str) -> BlockId {
    v.find_by_title(title).expect("title").0.id().clone()
}

#[test]
fn memory_clipboard_round_trips_text() {
    let mut clip = MemoryClipboard::default();
    assert_eq!(clip.get(), None, "empty by default");
    clip.set("hello");
    assert_eq!(clip.get().as_deref(), Some("hello"));
}

#[test]
fn ring_top_mirrors_out_to_the_clipboard() {
    let td = three();
    let mut v = Vault::open(td.path()).expect("open");
    let path = td.path().join("a.org");
    let one = id(&v, "One");

    v.cut(&path, &one).expect("cut");
    let top = v.ring_top().expect("ring filled").to_owned();

    let mut clip = MemoryClipboard::default();
    v.mirror_ring_top_to_clipboard(&mut clip);
    assert_eq!(
        clip.get().as_deref(),
        Some(top.as_str()),
        "cut -> kill ring -> clipboard"
    );
}

#[test]
fn external_clipboard_pulls_into_the_ring_then_pastes() {
    let td = three();
    let mut v = Vault::open(td.path()).expect("open");
    let path = td.path().join("a.org");
    let three_id = id(&v, "Three");

    // Text that arrived from *outside* (another app) via the clipboard.
    let mut clip = MemoryClipboard::default();
    clip.set("* Pasted\n:PROPERTIES:\n:ID: 01HX0000000000000000000099\n:END:\n");

    v.pull_clipboard_to_ring(&clip);
    assert_eq!(
        v.ring_top(),
        Some("* Pasted\n:PROPERTIES:\n:ID: 01HX0000000000000000000099\n:END:\n"),
        "clipboard -> kill ring"
    );

    v.paste(&path, &three_id).expect("paste from ring");
    assert!(
        v.find_by_title("Pasted").is_some(),
        "external clipboard content landed in the vault via the ring"
    );
}

#[test]
fn kill_ring_works_without_any_clipboard() {
    // The bridge is additive: cut+paste never require a clipboard.
    let td = three();
    let mut v = Vault::open(td.path()).expect("open");
    let path = td.path().join("a.org");
    let one = id(&v, "One");
    let three_id = id(&v, "Three");

    v.cut(&path, &one).expect("cut");
    v.paste(&path, &three_id).expect("paste");
    assert!(v.find_by_title("One").is_some(), "moved, still present");
}
