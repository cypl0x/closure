//! Embedded wasm runtime tests (W1+). Feature-gated: the whole file is
//! empty without `--features wasmtime`, so the default hermetic build
//! never compiles wasmtime. WAT text fixtures run in-process — no
//! network, no external toolchain.
#![cfg(feature = "wasmtime")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_plugin_host::WasmRuntime;

#[test]
fn loads_and_instantiates_a_trivial_module() {
    let rt = WasmRuntime::new();
    // Empty module — loads + instantiates with no imports.
    rt.instantiate(b"(module)").expect("trivial module instantiates");
}

#[test]
fn malformed_wat_errors_without_panic() {
    let rt = WasmRuntime::new();
    let err = rt.instantiate(b"(this is not wasm");
    assert!(err.is_err(), "malformed input must error, not panic");
}

#[test]
fn accepts_wat_with_a_function() {
    let rt = WasmRuntime::new();
    rt.instantiate(b"(module (func (export \"noop\")))")
        .expect("module with a func instantiates");
}

#[test]
fn calls_an_exported_i32_function() {
    let rt = WasmRuntime::new();
    let wat = b"(module (func (export \"answer\") (result i32) i32.const 42))";
    let got = rt.call_i32(wat, "answer").expect("call answer");
    assert_eq!(got, 42);
}

#[test]
fn missing_export_errors_cleanly() {
    let rt = WasmRuntime::new();
    let wat = b"(module (func (export \"answer\") (result i32) i32.const 1))";
    let err = rt.call_i32(wat, "nope");
    assert!(err.is_err(), "missing export must error, not panic");
}

// === W3: the guest's ONLY way to act is the `closure.run_command` host
// import — it requests a command by name (read from its own memory);
// the host validates against the registry. No Document import exists,
// so the guest cannot mutate state any other way (I8 boundary). ===

#[test]
fn guest_requests_a_command_through_the_host_import() {
    let rt = WasmRuntime::new();
    let registry = closure_core::default_registry();
    // Guest writes "rename-headline" into its memory + calls the import.
    let wat = br#"(module
      (import "closure" "run_command" (func $run (param i32 i32)))
      (memory (export "memory") 1)
      (data (i32.const 0) "rename-headline")
      (func (export "main") (call $run (i32.const 0) (i32.const 15))))"#;
    let cmds = rt.run_with_commands(wat, "main", &registry).expect("run");
    assert_eq!(cmds, vec!["rename-headline".to_owned()]);
}

#[test]
fn unknown_command_is_rejected_by_the_registry_gate() {
    let rt = WasmRuntime::new();
    let registry = closure_core::default_registry();
    let wat = br#"(module
      (import "closure" "run_command" (func $run (param i32 i32)))
      (memory (export "memory") 1)
      (data (i32.const 0) "evil-mutate")
      (func (export "main") (call $run (i32.const 0) (i32.const 11))))"#;
    let err = rt.run_with_commands(wat, "main", &registry);
    assert!(err.is_err(), "unknown command must be rejected, not run");
}

#[test]
fn guest_without_the_import_cannot_emit_any_command() {
    let rt = WasmRuntime::new();
    let registry = closure_core::default_registry();
    // No import, no command channel — the only side-effect surface is
    // the host import, so the guest emits nothing.
    let wat = br#"(module (memory (export "memory") 1) (func (export "main")))"#;
    let cmds = rt.run_with_commands(wat, "main", &registry).expect("run");
    assert!(cmds.is_empty(), "no host import => no commands possible");
}
