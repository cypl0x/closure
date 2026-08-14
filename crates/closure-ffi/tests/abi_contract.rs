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

#[test]
fn the_header_declares_every_exported_function() {
    // The header is hand-written, so it can drift from the library —
    // and a binding author reads the header, not the Rust. A function
    // exported and undeclared is unreachable; one declared and absent
    // is a link error at somebody else's build, which is worse because
    // it happens on their machine.
    let src = include_str!("../src/lib.rs");
    let header = include_str!("../include/closure.h");
    let exported: Vec<&str> = src
        .lines()
        .filter_map(|l| l.trim().strip_prefix("pub unsafe extern \"C\" fn "))
        .filter_map(|l| l.split('(').next())
        .collect();
    let safe: Vec<&str> = src
        .lines()
        .filter_map(|l| l.trim().strip_prefix("pub extern \"C\" fn "))
        .filter_map(|l| l.split('(').next())
        .collect();
    let exported: Vec<&str> = exported.into_iter().chain(safe).collect();
    assert!(exported.len() >= 7, "the scan is wrong: {exported:?}");
    for name in &exported {
        assert!(
            header.contains(name),
            "`{name}` is exported and the header does not declare it"
        );
    }

    // And the other direction, which is the one that bit. A header may
    // declare a function the library does not export; nothing in Rust
    // notices, because Rust never reads the header. It fails at the
    // link or the dlopen on somebody else's machine — here, it took a
    // `const fn` that was never `extern "C"` all the way to a Dart
    // symbol lookup before anything complained.
    for line in header.lines() {
        let Some(rest) = line.split_once("closure_").map(|(_, r)| r) else {
            continue;
        };
        if !line.contains('(') || line.trim_start().starts_with('*') {
            continue;
        }
        let name = format!(
            "closure_{}",
            rest.split('(').next().unwrap_or_default().trim()
        );
        assert!(
            exported.contains(&name.as_str()),
            "`{name}` is declared in closure.h and the library does not export it"
        );
    }
}

#[test]
fn the_abi_version_is_reported_and_matches_the_header() {
    // The check a caller makes before anything else. A .so and a set of
    // bindings that disagree is the failure that corrupts silently
    // instead of erroring.
    let header = include_str!("../include/closure.h");
    let declared = header
        .lines()
        .find_map(|l| l.trim().strip_prefix("#define CLOSURE_ABI_VERSION "))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .expect("the header declares a version");
    assert_eq!(closure_ffi::closure_ffi_abi_version(), declared);
}

#[test]
fn selecting_past_the_end_leaves_the_cursor_where_it_was() {
    // The doc comment promises out-of-range is a no-op, and until
    // coverage pointed at the skipped branch nothing on a live session
    // proved it. A binding that computes an index badly must get the
    // old body back, not a panic and not somebody else's row.
    let d = vault_dir();
    let path = c(&d.path().to_string_lossy());
    let h = unsafe { closure_ffi::closure_open(path.as_ptr()) };
    unsafe { closure_ffi::closure_select(h, 1) };
    let before = unsafe { closure_ffi::closure_selected_body(h) };
    let before_s = unsafe { std::ffi::CStr::from_ptr(before) }
        .to_string_lossy()
        .into_owned();

    for absurd in [2usize, 99, usize::MAX] {
        unsafe { closure_ffi::closure_select(h, absurd) };
        let after = unsafe { closure_ffi::closure_selected_body(h) };
        assert!(!after.is_null(), "index {absurd} unselected everything");
        let after_s = unsafe { std::ffi::CStr::from_ptr(after) }
            .to_string_lossy()
            .into_owned();
        assert_eq!(after_s, before_s, "index {absurd} moved the cursor");
        unsafe { closure_ffi::closure_string_free(after) };
    }

    unsafe { closure_ffi::closure_string_free(before) };
    unsafe { closure_ffi::closure_close(h) };
}
