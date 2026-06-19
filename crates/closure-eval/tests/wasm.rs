//! C1c: the wasm sandbox backend. A `wasm` code block (WAT text or
//! binary) runs under wasmtime with NO host imports — a genuinely
//! sandboxed exec tier. The block's result is the `i32` returned by its
//! exported `run` function. Feature-gated (`--features wasmtime`); the
//! default build never pulls wasmtime.
#![cfg(feature = "wasmtime")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_eval::{Backend, WasmBackend};

#[test]
fn wat_run_export_returns_its_i32() {
    let wat = "(module (func (export \"run\") (result i32) i32.const 42))";
    let out = WasmBackend.eval(wat).expect("sandboxed eval");
    assert_eq!(out.stdout.trim(), "42");
    assert_eq!(out.exit, 0);
}

#[test]
fn module_requiring_a_host_import_is_rejected() {
    // The sandbox grants zero imports: a module that needs one cannot
    // instantiate, so it cannot escape to the host.
    let wat = "(module (import \"env\" \"evil\" (func)) \
               (func (export \"run\") (result i32) i32.const 0))";
    assert!(
        WasmBackend.eval(wat).is_err(),
        "imports must be denied (no host surface)"
    );
}

#[test]
fn missing_run_export_errors() {
    let wat = "(module (func (export \"other\") (result i32) i32.const 1))";
    assert!(WasmBackend.eval(wat).is_err(), "no `run` export");
}

#[test]
fn malformed_wat_errors_without_panic() {
    assert!(WasmBackend.eval("(module (this is not wat").is_err());
}

#[test]
fn backend_for_wasm_resolves_under_feature() {
    let b = closure_eval::backend_for("wasm").expect("wasm backend wired");
    assert_eq!(b.language(), "wasm");
}
