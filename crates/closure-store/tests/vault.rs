//! Vault loader tests.
//!
//! Confirms that closure-store discovers all `*.org` files in a
//! directory, parses each into a [`closure_core::Document`], and
//! exposes cross-file access by path or block id.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_store::Vault;
use tempfile::TempDir;

fn write_vault(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, body) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, body).expect("write");
    }
    dir
}

#[test]
fn vault_loads_all_org_files() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("notes/c.org", "* C\n"),
        ("readme.md", "# not an org file\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.len(), 3);
    let names: Vec<String> = v
        .paths()
        .iter()
        .map(|p| {
            p.strip_prefix(td.path())
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert!(names.iter().any(|s| s.ends_with("a.org")));
    assert!(names.iter().any(|s| s.ends_with("c.org")));
}

#[test]
fn vault_document_lookup_by_path() {
    let td = write_vault(&[("x.org", "* Target\n")]);
    let v = Vault::open(td.path()).expect("open");
    let doc = v.document(&td.path().join("x.org")).expect("lookup");
    assert_eq!(doc.roots()[0].title(), "Target");
}

#[test]
fn vault_block_id_index_across_files() {
    let td = write_vault(&[
        (
            "a.org",
            "* A\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n",
        ),
        (
            "b.org",
            "* B\n:PROPERTIES:\n:ID: 01HXBBBBBBBBBBBBBBBBBBBBBB\n:END:\n",
        ),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let id = closure_core::BlockId::from_existing("01HXBBBBBBBBBBBBBBBBBBBBBB");
    let found = v.find_by_id(&id).expect("find");
    assert!(found.1.to_string_lossy().ends_with("b.org"));
    assert_eq!(found.0.title(), "B");
}

#[test]
fn vault_save_writes_byte_exact_source() {
    let td = write_vault(&[("x.org", "* Original\n")]);
    let v = Vault::open(td.path()).expect("open");
    let doc = v.document(&td.path().join("x.org")).expect("lookup");
    v.save(&td.path().join("x.org"), &doc.source())
        .expect("save");
    let on_disk = fs::read_to_string(td.path().join("x.org")).expect("read");
    assert_eq!(on_disk, "* Original\n");
}
