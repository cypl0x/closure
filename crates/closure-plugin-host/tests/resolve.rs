//! V4b: deterministic dependency resolver. Resolve a package's
//! transitive deps over a local package set (no network), checking
//! version requirements, detecting cycles, and writing a reproducible,
//! order-independent lockfile (I6).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use closure_plugin_host::{Package, PluginError, render_lockfile, resolve};

fn pkg(name: &str, version: &str, deps: &[(&str, &str)]) -> Package {
    Package {
        name: name.to_owned(),
        version: version.to_owned(),
        deps: deps
            .iter()
            .map(|(n, r)| ((*n).to_owned(), (*r).to_owned()))
            .collect(),
        commands: vec![],
    }
}

fn set(pkgs: &[Package]) -> BTreeMap<String, Package> {
    pkgs.iter().map(|p| (p.name.clone(), p.clone())).collect()
}

#[test]
fn resolves_transitive_dependencies() {
    let root = pkg("root", "1.0.0", &[("a", ">=1.0.0"), ("b", ">=1.0.0")]);
    let avail = set(&[
        pkg("a", "1.2.0", &[("c", ">=1.0.0")]),
        pkg("b", "1.0.0", &[]),
        pkg("c", "1.5.0", &[]),
    ]);
    let lock = resolve(&root, &avail).expect("resolves");
    let names: Vec<&str> = lock.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"a") && names.contains(&"b") && names.contains(&"c"));
    assert_eq!(lock.len(), 3, "no duplicates");
}

#[test]
fn unsatisfiable_version_errors() {
    let root = pkg("root", "1.0.0", &[("a", ">=2.0.0")]);
    let avail = set(&[pkg("a", "1.0.0", &[])]);
    assert!(matches!(
        resolve(&root, &avail),
        Err(PluginError::Package(_))
    ));
}

#[test]
fn missing_dependency_errors() {
    let root = pkg("root", "1.0.0", &[("ghost", ">=1.0.0")]);
    assert!(resolve(&root, &BTreeMap::new()).is_err());
}

#[test]
fn cycle_is_detected() {
    let root = pkg("root", "1.0.0", &[("a", ">=1.0.0")]);
    let avail = set(&[
        pkg("a", "1.0.0", &[("b", ">=1.0.0")]),
        pkg("b", "1.0.0", &[("a", ">=1.0.0")]),
    ]);
    assert!(matches!(
        resolve(&root, &avail),
        Err(PluginError::Package(_))
    ));
}

#[test]
fn resolution_is_order_independent_and_reproducible() {
    let avail = set(&[
        pkg("a", "1.2.0", &[("c", ">=1.0.0")]),
        pkg("b", "1.0.0", &[]),
        pkg("c", "1.5.0", &[]),
    ]);
    let root1 = pkg("root", "1.0.0", &[("a", ">=1.0.0"), ("b", ">=1.0.0")]);
    let root2 = pkg("root", "1.0.0", &[("b", ">=1.0.0"), ("a", ">=1.0.0")]);
    let l1 = render_lockfile(&resolve(&root1, &avail).unwrap());
    let l2 = render_lockfile(&resolve(&root2, &avail).unwrap());
    assert_eq!(l1, l2, "dep order does not change the lockfile (I6)");
    assert_eq!(
        l1,
        render_lockfile(&resolve(&root1, &avail).unwrap()),
        "reproducible"
    );
}

#[test]
fn exact_version_requirement_matches() {
    let root = pkg("root", "1.0.0", &[("a", "1.0.0")]);
    assert!(resolve(&root, &set(&[pkg("a", "1.0.0", &[])])).is_ok());
    assert!(resolve(&root, &set(&[pkg("a", "1.0.1", &[])])).is_err());
}
