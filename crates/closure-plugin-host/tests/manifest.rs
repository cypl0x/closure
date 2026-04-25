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
