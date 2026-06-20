//! V4c: load packages from a local registry directory (no network) and
//! lock a root manifest against it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_plugin_host::{extract_package_block, load_packages, render_lockfile, resolve};

fn registry() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("strings.org"),
        "* strings\n#+BEGIN_SRC closure-package\nname = strings\nversion = 1.0.7\n#+END_SRC\n",
    )
    .expect("w");
    fs::write(
        dir.path().join("greeter.org"),
        "#+BEGIN_SRC closure-package\nname = greeter\nversion = 2.0.0\ndep = strings >=1.0.0\ncommand = greet\n#+END_SRC\n",
    )
    .expect("w");
    fs::write(dir.path().join("notes.org"), "* just notes, no package\n").expect("w");
    dir
}

#[test]
fn extract_block_finds_package_content() {
    let src = "intro\n#+BEGIN_SRC closure-package\nname = x\nversion = 1.0.0\n#+END_SRC\nafter\n";
    let content = extract_package_block(src).expect("block");
    assert_eq!(content, "name = x\nversion = 1.0.0\n");
}

#[test]
fn extract_block_none_when_absent() {
    assert!(extract_package_block("* just a headline\n").is_none());
}

#[test]
fn load_packages_reads_every_package_file() {
    let dir = registry();
    let pkgs = load_packages(dir.path()).expect("load");
    assert_eq!(pkgs.len(), 2, "two package files, notes.org skipped");
    assert_eq!(pkgs["strings"].version, "1.0.7");
    assert_eq!(pkgs["greeter"].commands, vec!["greet".to_owned()]);
}

#[test]
fn resolve_over_loaded_registry() {
    let dir = registry();
    let pkgs = load_packages(dir.path()).expect("load");
    let root = pkgs["greeter"].clone();
    let lock = render_lockfile(&resolve(&root, &pkgs).expect("resolve"));
    assert!(
        lock.contains("strings 1.0.7"),
        "locked the transitive dep: {lock}"
    );
}
