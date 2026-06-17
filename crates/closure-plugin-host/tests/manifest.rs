#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::collections::HashMap;

use closure_plugin_host::{Host, Manifest, parse_manifest};

#[test]
fn parses_required_keys_and_meta() {
    let m =
        parse_manifest("id = my-plugin\nname = \"My Plugin\"\napi_version = 0.1.0\nauthor = me\n")
            .unwrap();
    assert_eq!(m.id, "my-plugin");
    assert_eq!(m.name, "My Plugin");
    assert_eq!(m.api_version, "0.1.0");
    assert_eq!(m.meta.get("author").map(String::as_str), Some("me"));
}

#[test]
fn missing_required_key_is_error() {
    let err = parse_manifest("id = x\nname = y\n").unwrap_err();
    let _ = err;
}

#[test]
fn host_collects_manifests() {
    let mut h = Host::new();
    h.register(Manifest {
        id: "a".into(),
        name: "A".into(),
        api_version: "0.1.0".into(),
        meta: HashMap::new(),
    });
    assert_eq!(h.manifests().len(), 1);
}

// === W4 API version gate: a plugin's manifest api_version must be
// compatible with the host's supported API (same major; plugin minor
// not newer than the host). Pure + hermetic (no wasmtime feature). ===

#[test]
fn supported_api_version_is_exposed() {
    // The host advertises a concrete semver it accepts plugins against.
    let v = closure_plugin_host::SUPPORTED_API_VERSION;
    assert!(v.split('.').count() == 3, "semver MAJOR.MINOR.PATCH: {v}");
}

#[test]
fn api_compatible_same_major_and_not_newer_minor() {
    use closure_plugin_host::api_compatible;
    assert!(api_compatible("0.3.0", "0.1.0"), "older plugin minor ok");
    assert!(api_compatible("0.3.0", "0.3.2"), "same minor, any patch ok");
    assert!(
        !api_compatible("0.3.0", "0.4.0"),
        "newer minor than host: no"
    );
    assert!(!api_compatible("1.0.0", "0.9.0"), "different major: no");
    assert!(!api_compatible("0.3.0", "not-semver"), "garbage: no");
}
