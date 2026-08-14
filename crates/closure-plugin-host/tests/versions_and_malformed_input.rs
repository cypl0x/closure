//! Version comparison, and every file this crate reads, malformed.
//!
//! Two clusters had never been exercised: the semver parsing behind
//! `api_compatible` and dependency resolution, and the error arms of
//! the four parsers (`parse_manifest`, `parse_package`,
//! `parse_lockfile`, `extract_package_block`).
//!
//! Both are places where being wrong is quiet. An `api_compatible` that
//! says yes too readily loads a plugin built against a different host
//! and finds out at the call. One that says no too readily refuses a
//! plugin that would have worked, and the user has no way to tell which
//! of the two versions is at fault. A parser that accepts a malformed
//! lockfile hands the resolver entries it invented.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::collections::BTreeMap;

use closure_plugin_host::{
    PluginError, SUPPORTED_API_VERSION, api_compatible, extract_package_block, parse_lockfile,
    parse_manifest, parse_package, render_lockfile, render_package, resolve,
};

// === api_compatible ===

#[test]
fn the_same_version_is_compatible_with_itself() {
    assert!(api_compatible(SUPPORTED_API_VERSION, SUPPORTED_API_VERSION));
}

#[test]
fn a_different_major_is_never_compatible() {
    assert!(!api_compatible("1.2.3", "2.0.0"));
    assert!(!api_compatible("2.0.0", "1.9.9"));
}

#[test]
fn a_plugin_may_be_older_in_minor_but_not_newer() {
    // The whole rule: the host promises what it had at its own minor,
    // so a plugin asking for less is fine and one asking for more is
    // asking for something that may not be there.
    assert!(api_compatible("1.5.0", "1.4.9"));
    assert!(api_compatible("1.5.0", "1.5.0"));
    assert!(!api_compatible("1.5.0", "1.6.0"));
}

#[test]
fn the_patch_number_is_ignored_on_both_sides() {
    assert!(api_compatible("1.2.9", "1.2.0"));
    assert!(api_compatible("1.2.0", "1.2.9"));
}

#[test]
fn a_version_that_is_not_three_numbers_is_never_compatible() {
    // Refusing is the only safe answer: a version this crate cannot
    // read is one it cannot reason about, and guessing would load a
    // plugin on the strength of a string it did not understand.
    for bad in ["1.2", "1.2.3.4", "", "one.two.three", "1.2.x", "v1.2.3"] {
        assert!(
            !api_compatible(SUPPORTED_API_VERSION, bad),
            "`{bad}` was accepted as a plugin version"
        );
        assert!(
            !api_compatible(bad, SUPPORTED_API_VERSION),
            "`{bad}` was accepted as a host version"
        );
    }
}

// === parse_manifest ===

#[test]
fn a_manifest_needs_an_id_and_a_name() {
    // Both are how a plugin is referred to afterwards. A manifest
    // missing either produces something that cannot be named in an
    // error message about itself.
    let missing_id = "name = Demo\napi_version = 0.1.0\n";
    let err = parse_manifest(missing_id).expect_err("accepted a manifest with no id");
    assert!(err.to_string().contains("id"), "{err}");

    let missing_name = "id = demo\napi_version = 0.1.0\n";
    let err = parse_manifest(missing_name).expect_err("accepted a manifest with no name");
    assert!(err.to_string().contains("name"), "{err}");
}

#[test]
fn a_manifest_line_without_an_equals_sign_is_refused_and_quoted() {
    let err = parse_manifest("id = demo\nthis line is not a setting\n")
        .expect_err("accepted a malformed line");
    let text = err.to_string();
    assert!(text.contains("key = value"), "{text}");
    assert!(
        text.contains("this line is not a setting"),
        "the error does not quote the line: {text}"
    );
}

#[test]
fn comments_and_blank_lines_in_a_manifest_are_skipped() {
    let m = parse_manifest(
        "# a comment\n\nid = demo\nname = Demo\napi_version = 0.1.0\n\n# trailing\n",
    )
    .expect("parse");
    assert_eq!(m.id, "demo");
    assert_eq!(m.name, "Demo");
}

// === packages ===

#[test]
fn a_package_block_is_found_and_its_absence_reported_as_none() {
    let src = "* Heading\n#+BEGIN_SRC closure-package\nname = p\nversion = 1.0.0\n#+END_SRC\n";
    assert!(extract_package_block(src).is_some());
    assert!(extract_package_block("* Just a heading\nand prose\n").is_none());
}

#[test]
fn a_package_key_nobody_knows_is_refused_rather_than_ignored() {
    // The opposite choice from a plugin manifest, and the right one
    // here: a package file is generated and consumed by this program,
    // so an unknown key means the two ends disagree about the format.
    let err = parse_package("name = p\nversion = 1.0.0\nwhat = 3\n")
        .expect_err("accepted an unknown package key");
    assert!(err.to_string().contains("what"), "{err}");
}

#[test]
fn a_dependency_without_a_version_is_refused() {
    let err =
        parse_package("name = p\nversion = 1.0.0\ndep = other\n").expect_err("accepted a bare dep");
    assert!(err.to_string().contains("version"), "{err}");
}

#[test]
fn a_package_line_without_an_equals_sign_is_refused() {
    assert!(parse_package("name = p\nnot a setting\n").is_err());
}

#[test]
fn a_package_survives_being_rendered_and_read_back() {
    // render/parse are two halves of one format; if they drift, a
    // registry written by this program stops being readable by it.
    let src = "name = greeter\nversion = 2.0.0\ndep = strings >=1.0.0\ncommand = greet\n";
    let pkg = parse_package(src).expect("parse");
    let rendered = render_package(&pkg);
    let back = parse_package(extract_package_block(&rendered).unwrap_or(&rendered))
        .expect("re-parse what it rendered");
    assert_eq!(back.name, pkg.name);
    assert_eq!(back.version, pkg.version);
    assert_eq!(back.commands, pkg.commands);
}

// === lockfiles ===

#[test]
fn a_lock_line_needs_three_fields() {
    let err = parse_lockfile("only-two fields\n").expect_err("accepted a short lock line");
    let text = err.to_string();
    assert!(text.contains("bad lock line"), "{text}");
    assert!(
        text.contains("only-two"),
        "the error does not quote it: {text}"
    );
}

#[test]
fn a_lockfile_skips_comments_and_blanks() {
    let entries =
        parse_lockfile("# generated\n\nalpha 1.0.0 abc123\n\nbeta 2.0.0 def456\n").expect("parse");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "alpha");
    assert_eq!(entries[1].hash, "def456");
}

#[test]
fn a_lockfile_survives_being_rendered_and_read_back() {
    let entries = parse_lockfile("alpha 1.0.0 abc123\nbeta 2.0.0 def456\n").expect("parse");
    let back = parse_lockfile(&render_lockfile(&entries)).expect("re-parse");
    assert_eq!(back.len(), entries.len());
    assert_eq!(back[0].name, entries[0].name);
    assert_eq!(back[1].version, entries[1].version);
}

#[test]
fn an_empty_lockfile_is_an_empty_list_not_an_error() {
    assert!(parse_lockfile("").expect("parse").is_empty());
    assert!(
        parse_lockfile("# nothing locked\n")
            .expect("parse")
            .is_empty()
    );
}

// === resolution ===

#[test]
fn a_dependency_requirement_that_nothing_satisfies_is_reported() {
    let root = parse_package("name = app\nversion = 0.1.0\ndep = lib >=2.0.0\n").expect("root");
    let lib = parse_package("name = lib\nversion = 1.0.0\n").expect("lib");
    let mut avail: BTreeMap<String, _> = BTreeMap::new();
    avail.insert("lib".to_owned(), lib);

    let err = resolve(&root, &avail).expect_err("resolved an impossible requirement");
    assert!(matches!(err, PluginError::Package(_)), "{err:?}");
}

#[test]
fn an_exact_requirement_matches_only_that_version() {
    let root = parse_package("name = app\nversion = 0.1.0\ndep = lib 1.0.0\n").expect("root");
    let right = parse_package("name = lib\nversion = 1.0.0\n").expect("lib");
    let wrong = parse_package("name = lib\nversion = 1.0.1\n").expect("lib");

    let mut ok: BTreeMap<String, _> = BTreeMap::new();
    ok.insert("lib".to_owned(), right);
    let mut bad: BTreeMap<String, _> = BTreeMap::new();
    bad.insert("lib".to_owned(), wrong);
    assert!(resolve(&root, &ok).is_ok());
    assert!(
        resolve(&root, &bad).is_err(),
        "an exact requirement accepted a different patch"
    );
}

#[test]
fn a_version_that_is_not_semver_satisfies_nothing() {
    let root = parse_package("name = app\nversion = 0.1.0\ndep = lib >=1.0.0\n").expect("root");
    let odd = parse_package("name = lib\nversion = latest\n").expect("lib");
    let mut avail: BTreeMap<String, _> = BTreeMap::new();
    avail.insert("lib".to_owned(), odd);
    assert!(
        resolve(&root, &avail).is_err(),
        "`latest` was treated as satisfying >=1.0.0"
    );
}
