//! V4a: package manifest + lockfile. A package = name + version + deps +
//! provided commands, declared in a `closure-package` block (plain text,
//! no JSON/YAML). A lockfile pins resolved versions + content hashes.
//! Both round-trip byte-exact.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_plugin_host::{
    Package, parse_lockfile, parse_package, render_lockfile, render_package,
};

const PKG: &str = "name = greeter\n\
                   version = 1.2.0\n\
                   dep = strings >=1.0.0\n\
                   dep = colors >=2.1.0\n\
                   command = greet\n\
                   command = farewell\n";

#[test]
fn parse_package_extracts_fields() {
    let p = parse_package(PKG).expect("parses");
    assert_eq!(p.name, "greeter");
    assert_eq!(p.version, "1.2.0");
    assert_eq!(
        p.deps,
        vec![
            ("strings".to_owned(), ">=1.0.0".to_owned()),
            ("colors".to_owned(), ">=2.1.0".to_owned()),
        ]
    );
    assert_eq!(p.commands, vec!["greet".to_owned(), "farewell".to_owned()]);
}

#[test]
fn package_round_trips_byte_exact() {
    let p = parse_package(PKG).expect("parses");
    assert_eq!(render_package(&p), PKG, "canonical render is byte-exact");
}

#[test]
fn package_requires_name_and_version() {
    assert!(parse_package("version = 1.0.0\n").is_err());
    assert!(parse_package("name = x\n").is_err());
}

const LOCK: &str = "colors 2.1.4 sha256:deadbeef\n\
                    strings 1.0.7 sha256:cafef00d\n";

#[test]
fn lockfile_round_trips_byte_exact_and_sorted() {
    let lock = parse_lockfile(LOCK).expect("parses");
    assert_eq!(lock.len(), 2);
    // Rendered sorted by name → byte-exact for this already-sorted input.
    assert_eq!(render_lockfile(&lock), LOCK);
}

#[test]
fn lockfile_render_sorts_unsorted_input() {
    let unsorted = "strings 1.0.7 sha256:cafef00d\ncolors 2.1.4 sha256:deadbeef\n";
    let lock = parse_lockfile(unsorted).expect("parses");
    assert_eq!(
        render_lockfile(&lock),
        LOCK,
        "deterministic sorted output (I6)"
    );
}

#[test]
fn empty_package_commands_and_deps_ok() {
    let p = Package {
        name: "bare".to_owned(),
        version: "0.1.0".to_owned(),
        deps: vec![],
        commands: vec![],
    };
    assert_eq!(render_package(&p), "name = bare\nversion = 0.1.0\n");
}
