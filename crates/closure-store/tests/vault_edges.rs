//! What a vault does with a directory that is not tidy.
//!
//! The store is where a mistake costs data rather than a redraw, and
//! 425 of its lines were unexecuted. What is missing is not the happy
//! path — that is thoroughly covered — but the edges: a file that is
//! not org, a name that is taken, a path outside the vault, a save that
//! has to be atomic.
//!
//! These are the operations a shell calls on whatever the user does
//! next, and every one of them can be handed something unreasonable.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_store::Vault;

fn vault_with(files: &[(&str, &str)]) -> (tempfile::TempDir, Vault) {
    let d = tempfile::tempdir().expect("tempdir");
    for (name, body) in files {
        let path = d.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(path, body).expect("write");
    }
    let v = Vault::open(d.path()).expect("open");
    (d, v)
}

const NOTE: &str = "* A note\n:PROPERTIES:\n:ID: 01VAULTEDGE000000001\n:END:\nbody\n";

#[test]
fn a_directory_with_no_org_files_opens_empty() {
    // A vault someone just made, or pointed at the wrong folder. Both
    // have to open rather than error, because the shell's next move is
    // to show it.
    let (_d, v) = vault_with(&[("readme.md", "# not org\n"), ("notes.txt", "plain")]);
    assert_eq!(v.len(), 0);
    assert_eq!(v.headline_count(), 0);
}

#[test]
fn a_nested_directory_is_walked() {
    let (_d, v) = vault_with(&[("top.org", NOTE), ("sub/deep.org", NOTE)]);
    assert_eq!(v.len(), 2, "a file in a subdirectory was not found");
}

#[test]
fn a_file_that_is_not_utf8_does_not_stop_the_vault() {
    // One unreadable file must not make the other twenty invisible.
    let d = tempfile::tempdir().expect("tempdir");
    std::fs::write(d.path().join("good.org"), NOTE).expect("write");
    std::fs::write(d.path().join("bad.org"), [0xff, 0xfe, 0x00, 0x80]).expect("write");
    // Refusing the whole vault is a defensible answer too, as long as
    // it is an error rather than a panic.
    if let Ok(v) = Vault::open(d.path()) {
        assert!(!v.is_empty(), "the readable file went missing");
    }
}

#[test]
fn creating_a_file_that_exists_does_not_overwrite_it() {
    // The gesture is "new note", and a new note that silently replaces
    // an old one is the worst possible reading of it.
    let (d, mut v) = vault_with(&[("notes.org", NOTE)]);
    let before = std::fs::read_to_string(d.path().join("notes.org")).expect("read");
    let _ = v.create_file(std::path::Path::new("notes.org"), "* Different\n");
    let after = std::fs::read_to_string(d.path().join("notes.org")).expect("read");
    assert_eq!(before, after, "`create_file` overwrote an existing note");
}

#[test]
fn renaming_onto_an_existing_name_does_not_destroy_it() {
    let (d, mut v) = vault_with(&[("a.org", NOTE), ("b.org", "* B\n")]);
    let b_before = std::fs::read_to_string(d.path().join("b.org")).expect("read");
    let _ = v.rename_file(std::path::Path::new("a.org"), std::path::Path::new("b.org"));
    if let Ok(b_after) = std::fs::read_to_string(d.path().join("b.org")) {
        assert_eq!(b_before, b_after, "`rename_file` clobbered the target");
    }
}

#[test]
fn deleting_a_file_that_is_not_there_is_an_error_not_a_panic() {
    let (_d, mut v) = vault_with(&[("notes.org", NOTE)]);
    let out = v.delete_file(std::path::Path::new("nosuch.org"));
    assert!(out.is_err(), "deleting a missing file reported success");
}

#[test]
fn a_save_leaves_a_file_that_parses() {
    // The claim `save` is making. A half-written file is worse than an
    // unsaved one, because the unsaved one is still in the editor.
    let (d, mut v) = vault_with(&[("notes.org", NOTE)]);
    let path = d.path().join("notes.org");
    let new = "* Rewritten\n:PROPERTIES:\n:ID: 01VAULTEDGE000000002\n:END:\n";
    v.set_source(&path, new).expect("set");
    v.save(&path, new).expect("save");
    let back = std::fs::read_to_string(&path).expect("read");
    assert!(
        closure_org::parse(&back).is_ok(),
        "saved something unparseable"
    );
    assert!(back.contains("Rewritten"), "{back}");
}

#[test]
fn save_with_backup_keeps_the_previous_contents_somewhere() {
    let (d, mut v) = vault_with(&[("notes.org", NOTE)]);
    let path = d.path().join("notes.org");
    v.set_source(&path, "* Replaced\n").expect("set");
    if v.save_with_backup(&path, "* Replaced\n").is_ok() {
        let names: Vec<String> = std::fs::read_dir(d.path())
            .expect("dir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().any(|n| n != "notes.org"),
            "a backup save left nothing but the new file: {names:?}"
        );
    }
}

#[test]
fn looking_up_an_id_that_is_not_there_finds_nothing() {
    let (_d, v) = vault_with(&[("notes.org", NOTE)]);
    let missing = closure_core::BlockId::from_existing("01NOSUCH000000000000");
    assert!(v.find_by_id(&missing).is_none());
    assert!(!v.has_id(&missing));
}

#[test]
fn the_statistics_hold_on_an_empty_vault() {
    // Every one of these divides by something. An empty vault is the
    // input that makes the something zero.
    let (_d, v) = vault_with(&[]);
    let _ = v.property_pct();
    let _ = v.no_property_pct();
    let _ = v.empty_title_pct();
    let _ = v.nonempty_title_pct();
    let _ = v.unique_title_pct();
    let _ = v.median_headlines_per_path();
    let _ = v.mode_headlines_per_path();
    let _ = v.mean_file_with_property_count();
    let _ = v.median_file_with_property_count();
    let _ = v.min_file_with_property_count();
    let _ = v.path_with_max_headlines();
    let _ = v.path_with_min_headlines();
    let _ = v.duplicate_title_count();
    let _ = v.has_duplicate_titles();
}

#[test]
fn duplicate_titles_are_noticed() {
    let (_d, v) = vault_with(&[
        (
            "a.org",
            "* Same\n:PROPERTIES:\n:ID: 01VAULTEDGE000000003\n:END:\n",
        ),
        (
            "b.org",
            "* Same\n:PROPERTIES:\n:ID: 01VAULTEDGE000000004\n:END:\n",
        ),
    ]);
    assert!(v.has_duplicate_titles());
    assert_eq!(v.distinct_title_count(), 1);
}

#[test]
fn searching_is_case_sensitive_unless_it_says_otherwise() {
    let (_d, v) = vault_with(&[("notes.org", "* Ship It\n")]);
    assert_eq!(v.paths_containing("Ship It").len(), 1);
    assert!(v.paths_containing("ship it").is_empty());
    assert_eq!(v.paths_containing_ignore_case("ship it").len(), 1);
}
