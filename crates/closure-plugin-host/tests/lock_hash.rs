//! Q8-P1: lockfile content hashes are cryptographic (BLAKE3).
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::collections::BTreeMap;

use closure_plugin_host::{Package, resolve};

fn pkg(name: &str, version: &str, deps: &[(&str, &str)]) -> Package {
    Package {
        name: name.into(),
        version: version.into(),
        deps: deps
            .iter()
            .map(|(n, r)| ((*n).to_owned(), (*r).to_owned()))
            .collect(),
        commands: vec![],
    }
}

fn avail(pkgs: &[Package]) -> BTreeMap<String, Package> {
    pkgs.iter().map(|p| (p.name.clone(), p.clone())).collect()
}

#[test]
fn lockfile_hashes_are_blake3() {
    let root = pkg("app", "1.0.0", &[("lib", ">=1.0.0")]);
    let lock = resolve(&root, &avail(&[pkg("lib", "1.2.3", &[])])).expect("resolve");
    let entry = lock.iter().find(|e| e.name == "lib").expect("lib entry");
    assert!(
        entry.hash.starts_with("b3:"),
        "BLAKE3-prefixed: {}",
        entry.hash
    );
    assert_eq!(
        entry.hash.len(),
        3 + 64,
        "256-bit hex digest: {}",
        entry.hash
    );
}

#[test]
fn hash_is_deterministic_and_content_sensitive() {
    let root = pkg("app", "1.0.0", &[("x", ">=0.0.1")]);
    let l1 = resolve(&root, &avail(&[pkg("x", "1.0.0", &[])])).expect("r1");
    let l2 = resolve(&root, &avail(&[pkg("x", "1.0.0", &[])])).expect("r2");
    let l3 = resolve(&root, &avail(&[pkg("x", "2.0.0", &[])])).expect("r3");
    let h = |l: &[closure_plugin_host::LockEntry]| {
        l.iter().find(|e| e.name == "x").unwrap().hash.clone()
    };
    assert_eq!(h(&l1), h(&l2), "deterministic (I6)");
    assert_ne!(h(&l1), h(&l3), "content change changes the hash");
}

// === Q8-P3: the in-repo example registry is a real, resolvable corpus. ===

#[test]
fn example_registry_resolves_end_to_end() {
    let reg = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap()
        .join("fixtures")
        .join("registry");
    let packages = closure_plugin_host::load_packages(&reg).expect("load");
    assert_eq!(packages.len(), 3, "three example packages: {packages:?}");
    let root = packages.get("capture-pack").expect("root").clone();
    let lock = resolve(&root, &packages).expect("resolve transitive deps");
    let names: Vec<&str> = lock.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"widget-pack") && names.contains(&"formula-pack"));
    assert!(lock.iter().all(|e| e.hash.starts_with("b3:")));
}
