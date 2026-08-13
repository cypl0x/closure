//! The rules a C ABI has to keep, tested from Rust.
//!
//! Three things make this boundary safe, and all three are invisible to
//! the type system once you are past it:
//!
//! 1. No panic crosses it. A panic through `extern "C"` is undefined
//!    behaviour — worse than a crash, because it may not crash. This is
//!    I5 restated for a place where violating it corrupts rather than
//!    aborts.
//! 2. Every pointer closure hands out is freed by a `closure_*_free` of
//!    closure's own. Dart cannot call Rust's allocator.
//! 3. A null or garbage pointer in is an error out, not a dereference.
//!    The caller is a Dart FFI binding, and those get written by hand.
//!
//! Tested through the real `extern "C"` functions rather than an inner
//! Rust API, because the wrapper is the part that can be wrong.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::ffi::{CStr, CString};

const VAULT: &str = "\
* TODO Ship it :work:
:PROPERTIES:
:ID: 01FFIABI000000000001
:END:
a body
* Second
:PROPERTIES:
:ID: 01FFIABI000000000002
:END:
";

fn vault_dir() -> tempfile::TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    std::fs::write(d.path().join("notes.org"), VAULT).expect("write");
    d
}

fn c(s: &str) -> CString {
    CString::new(s).expect("no interior nul")
}

#[test]
fn opening_a_vault_gives_a_handle_and_closing_it_takes_it_back() {
    let d = vault_dir();
    let path = c(&d.path().to_string_lossy());
    let h = unsafe { closure_ffi::closure_open(path.as_ptr()) };
    assert!(!h.is_null(), "a real vault did not open");
    unsafe { closure_ffi::closure_close(h) };
}

#[test]
fn opening_a_vault_that_is_not_there_returns_null_rather_than_crashing() {
    let path = c("/nonexistent/vault/for/a/test");
    let h = unsafe { closure_ffi::closure_open(path.as_ptr()) };
    assert!(h.is_null(), "a missing vault reported success");
}

#[test]
fn a_null_path_is_an_error_not_a_dereference() {
    let h = unsafe { closure_ffi::closure_open(std::ptr::null()) };
    assert!(h.is_null());
}

#[test]
fn every_entry_point_survives_a_null_handle() {
    // Hand-written bindings pass null. Each of these must return a
    // defined "nothing" rather than reading through it.
    unsafe {
        assert_eq!(closure_ffi::closure_row_count(std::ptr::null_mut()), 0);
        assert!(closure_ffi::closure_row_title(std::ptr::null_mut(), 0).is_null());
        assert!(closure_ffi::closure_selected_body(std::ptr::null_mut()).is_null());
        closure_ffi::closure_select(std::ptr::null_mut(), 0);
        closure_ffi::closure_close(std::ptr::null_mut());
    }
}

#[test]
fn the_rows_are_the_vault_s_headlines() {
    let d = vault_dir();
    let path = c(&d.path().to_string_lossy());
    let h = unsafe { closure_ffi::closure_open(path.as_ptr()) };
    assert_eq!(unsafe { closure_ffi::closure_row_count(h) }, 2);
    let title = unsafe { closure_ffi::closure_row_title(h, 0) };
    assert!(!title.is_null());
    let text = unsafe { CStr::from_ptr(title) }
        .to_string_lossy()
        .into_owned();
    assert!(text.contains("Ship it"), "{text}");
    unsafe { closure_ffi::closure_string_free(title) };
    unsafe { closure_ffi::closure_close(h) };
}

#[test]
fn an_index_past_the_end_is_null_rather_than_a_read() {
    let d = vault_dir();
    let path = c(&d.path().to_string_lossy());
    let h = unsafe { closure_ffi::closure_open(path.as_ptr()) };
    assert!(unsafe { closure_ffi::closure_row_title(h, 9999) }.is_null());
    unsafe { closure_ffi::closure_close(h) };
}

#[test]
fn selecting_changes_what_the_body_says() {
    let d = vault_dir();
    let path = c(&d.path().to_string_lossy());
    let h = unsafe { closure_ffi::closure_open(path.as_ptr()) };
    unsafe { closure_ffi::closure_select(h, 0) };
    let first = unsafe { closure_ffi::closure_selected_body(h) };
    assert!(!first.is_null());
    let first_text = unsafe { CStr::from_ptr(first) }
        .to_string_lossy()
        .into_owned();
    unsafe { closure_ffi::closure_string_free(first) };
    assert!(first_text.contains("a body"), "{first_text}");
    unsafe { closure_ffi::closure_close(h) };
}

#[test]
fn freeing_null_is_allowed() {
    // C callers free unconditionally. A free that faults on null makes
    // every error path in the binding a crash.
    unsafe { closure_ffi::closure_string_free(std::ptr::null_mut()) };
}

#[test]
fn a_vault_path_that_is_not_utf8_is_refused_rather_than_guessed() {
    // A `char*` is bytes, and a path from Dart may not be UTF-8.
    let bad = [0xff_u8, 0xfe, 0];
    let h = unsafe { closure_ffi::closure_open(bad.as_ptr().cast()) };
    assert!(h.is_null());
}
