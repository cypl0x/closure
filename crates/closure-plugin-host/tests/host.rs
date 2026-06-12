//! Plugin host: executable plugins (native or wasm-via-wasmtime)
//! register one command each; invocation captures stdout.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;

use closure_plugin_host::{Host, parse_manifest, runner_for};

fn script_plugin(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("greet");
    fs::write(&path, "#!/bin/sh\necho \"hello $1\"\n").expect("write");
    let mut perm = fs::metadata(&path).expect("meta").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&path, perm).expect("chmod");
    path
}

#[test]
fn runner_for_wasm_prepends_wasmtime() {
    let argv = runner_for(std::path::Path::new("plugin.wasm"));
    assert_eq!(argv[0], "wasmtime");
    assert!(argv[1].ends_with("plugin.wasm"));
}

#[test]
fn runner_for_native_is_direct_exec() {
    let argv = runner_for(std::path::Path::new("/bin/echo"));
    assert_eq!(argv, vec!["/bin/echo".to_owned()]);
}

#[test]
fn registered_command_invokes_plugin_and_captures_stdout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = script_plugin(dir.path());
    let manifest = parse_manifest(
        "id = greeter\nname = Greeter\napi_version = 1.0.0\ncommand = greet\n",
    )
    .expect("manifest");
    let mut host = Host::new();
    host.register_command(&manifest, &exe).expect("register");
    let out = host.invoke("greet", &["world"]).expect("invoke");
    assert_eq!(out.trim(), "hello world");
}

#[test]
fn manifest_without_command_key_cannot_register() {
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = script_plugin(dir.path());
    let manifest =
        parse_manifest("id = x\nname = X\napi_version = 1.0.0\n").expect("manifest");
    let mut host = Host::new();
    assert!(host.register_command(&manifest, &exe).is_err());
}

#[test]
fn invoking_unknown_command_errors() {
    let mut host = Host::new();
    assert!(host.invoke("nope", &[]).is_err());
}

#[test]
fn invoking_missing_executable_errors() {
    let manifest = parse_manifest(
        "id = gone\nname = Gone\napi_version = 1.0.0\ncommand = gone\n",
    )
    .expect("manifest");
    let mut host = Host::new();
    host.register_command(&manifest, std::path::Path::new("/no/such/exe"))
        .expect("register records the path");
    assert!(host.invoke("gone", &[]).is_err());
}

#[test]
fn commands_lists_registered_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = script_plugin(dir.path());
    let manifest = parse_manifest(
        "id = greeter\nname = Greeter\napi_version = 1.0.0\ncommand = greet\n",
    )
    .expect("manifest");
    let mut host = Host::new();
    host.register_command(&manifest, &exe).expect("register");
    assert_eq!(host.commands(), vec!["greet".to_owned()]);
}
