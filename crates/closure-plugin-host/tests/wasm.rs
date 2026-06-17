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
