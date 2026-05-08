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
fn vault_headline_and_word_counts_aggregate() {
    let td = write_vault(&[
        ("a.org", "* One\n* Two\nbody words here\n"),
        ("b.org", "* Three\nmore words\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.headline_count(), 3);
    assert!(v.word_count() >= 7);
}

#[test]
fn vault_all_tags_and_all_todos_aggregate() {
    let td = write_vault(&[
        ("a.org", "* TODO Fix :work:\n"),
        ("b.org", "* DONE Ship :work:urgent:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.all_tags(), vec!["urgent".to_owned(), "work".to_owned()]);
    assert_eq!(v.all_todos(), vec!["DONE".to_owned(), "TODO".to_owned()]);
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

#[test]
fn vault_create_file_adds_new_document() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    assert_eq!(v.len(), 1);
    let p = v
        .create_file(std::path::Path::new("b.org"), "* B\n")
        .expect("create");
    assert!(p.ends_with("b.org"));
    assert_eq!(v.len(), 2);
    assert!(v.document(&p).is_some());
}

#[test]
fn vault_delete_file_drops_entry() {
    let td = write_vault(&[("a.org", "* A\n"), ("b.org", "* B\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    assert_eq!(v.len(), 2);
    let a = td.path().join("a.org");
    v.delete_file(&a).expect("delete");
    assert_eq!(v.len(), 1);
    assert!(!a.exists());
}

#[test]
fn vault_rename_file_updates_indexes() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let from = td.path().join("a.org");
    let to_rel = std::path::Path::new("renamed.org");
    let new_path = v.rename_file(&from, to_rel).expect("rename");
    assert!(!from.exists());
    assert!(new_path.exists());
    assert!(v.document(&new_path).is_some());
    assert!(v.document(&from).is_none());
}

#[test]
fn vault_create_file_refuses_to_overwrite() {
    let td = write_vault(&[("x.org", "* X\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let err = v
        .create_file(std::path::Path::new("x.org"), "* New\n")
        .unwrap_err();
    let _ = err;
}

#[test]
fn vault_save_with_backup_writes_bak() {
    let td = write_vault(&[("x.org", "* Original\n")]);
    let v = Vault::open(td.path()).expect("open");
    let path = td.path().join("x.org");
    v.save_with_backup(&path, "* Updated\n").expect("save");
    let now = fs::read_to_string(&path).expect("read");
    let bak = fs::read_to_string(td.path().join("x.org.bak")).expect("read bak");
    assert_eq!(now, "* Updated\n");
    assert_eq!(bak, "* Original\n");
}

#[test]
fn vault_reload_picks_up_new_file() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    assert_eq!(v.len(), 1);
    fs::write(td.path().join("b.org"), "* B\n").expect("write");
    v.reload().expect("reload");
    assert_eq!(v.len(), 2);
}

#[test]
fn vault_backlinks_index_finds_linkers() {
    let td = write_vault(&[
        (
            "target.org",
            "* T\n:PROPERTIES:\n:ID: 01HXTGTTARGET0000000000AA\n:END:\n",
        ),
        (
            "src.org",
            "* Source\nSee [[id:01HXTGTTARGET0000000000AA][T]].\n",
        ),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // Either form (with or without `id:` prefix) should work.
    let bare = v.backlinks_of("01HXTGTTARGET0000000000AA");
    assert_eq!(bare.len(), 1);
    let prefixed = v.backlinks_of("id:01HXTGTTARGET0000000000AA");
    assert_eq!(prefixed.len(), 1);
}

#[test]
fn vault_watcher_observes_modify() {
    let td = write_vault(&[("x.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    let watcher = v.watch().expect("watch");
    // Give inotify a moment to register.
    std::thread::sleep(std::time::Duration::from_millis(50));
    fs::write(td.path().join("x.org"), "* B\n").expect("rewrite");
    // Wait up to a few seconds for the event.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut saw = false;
    while std::time::Instant::now() < deadline {
        if watcher.try_recv().is_some() {
            saw = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(saw, "expected at least one watcher event");
}
