//! A C ABI over [`closure_shell_core`], for shells that are not Rust.
//!
//! `docs/flutter-shell.md` chose this over a webview for a native UI,
//! and chose to keep the Dart side outside the hermetic gate because
//! the Dart toolchain is not reproducibly packaged (I10). This crate is
//! the half that stays inside: it is built, linted and covered like
//! every other crate, and the Flutter app is what consumes it.
//!
//! Three rules make the boundary safe, and none of them is enforced by
//! the type system once a caller is past it:
//!
//! 1. **No panic crosses it.** A panic through `extern "C"` is undefined
//!    behaviour, which is worse than a crash because it may not crash.
//!    Every entry point catches. This is I5 restated for a place where
//!    breaking it corrupts rather than aborts.
//! 2. **Closure frees what closure allocates.** Every pointer handed out
//!    comes back through a `closure_*_free`; Dart cannot call Rust's
//!    allocator, and a `free()` from the wrong one is a heap bug that
//!    surfaces somewhere else entirely.
//! 3. **A bad pointer in is an error out.** The caller is a hand-written
//!    FFI binding. Null is not an exceptional case there, it is Tuesday.
//!
//! The surface is deliberately small: enough for a shell to browse and
//! read, and no mutation yet. A mutating ABI has to answer where undo
//! lives and who owns the write, and that is worth its own item rather
//! than a guess made while wiring a list view.

#![forbid(unsafe_op_in_unsafe_fn)]

use std::ffi::{CStr, CString, c_char};

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

/// What a caller holds: the vault and the app looking at it.
///
/// Opaque on the C side — the layout is closure's business, and a
/// struct a binding can see is a struct a binding will depend on.
pub struct Session {
    shell: Shell,
    app: ModalApp,
}

/// Run `f`, returning `fallback` if it panics.
///
/// The catch is the whole point (rule 1). It is here rather than at
/// each call site so that adding an entry point without one is a
/// visible omission rather than an invisible hazard.
fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    // `AssertUnwindSafe` because `ModalApp` holds memos behind
    // `RefCell`/`Cell`, so it is not `UnwindSafe` by the compiler's
    // reckoning. The assertion is sound here for a specific reason
    // rather than because the error was in the way: every memo in this
    // codebase is guarded by the vault revision it was computed
    // against, so one left half-filled by a panic is *recomputed* on
    // the next read rather than trusted. There is no state behind this
    // boundary whose invariant a panic could break and a later call
    // could believe.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(fallback)
}

/// Borrow a session from a caller's pointer, or `None` if it is null.
///
/// # Safety
///
/// `handle` must be null or a pointer returned by [`closure_open`] and
/// not yet passed to [`closure_close`].
unsafe fn session<'a>(handle: *mut Session) -> Option<&'a mut Session> {
    if handle.is_null() {
        return None;
    }
    // SAFETY: non-null, and the caller's contract says it came from
    // `closure_open` and is still live.
    Some(unsafe { &mut *handle })
}

/// Hand a string to the caller. Freed with [`closure_string_free`].
fn out_string(s: &str) -> *mut c_char {
    CString::new(s).map_or(std::ptr::null_mut(), CString::into_raw)
}

/// The ABI version this library was built with.
///
/// A caller checks this against the `CLOSURE_ABI_VERSION` in its own
/// copy of `closure.h` before calling anything else. A `.so` and a set
/// of bindings that disagree is the one failure here that corrupts
/// silently rather than erroring — every other mistake produces a null
/// or a link error.
#[unsafe(no_mangle)]
pub extern "C" fn closure_ffi_abi_version() -> usize {
    1
}

/// Open the vault at `path`. Returns null if it cannot be opened.
///
/// # Safety
///
/// `path` must be null or a NUL-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn closure_open(path: *const c_char) -> *mut Session {
    guard(std::ptr::null_mut(), || {
        if path.is_null() {
            return std::ptr::null_mut();
        }
        // SAFETY: non-null, and the caller's contract says NUL-terminated.
        let bytes = unsafe { CStr::from_ptr(path) };
        // Not UTF-8 is refused rather than guessed at: a lossy path is a
        // different directory, and creating one would be worse than
        // failing.
        let Ok(text) = bytes.to_str() else {
            return std::ptr::null_mut();
        };
        let Ok(vault) = Vault::open(std::path::Path::new(text)) else {
            return std::ptr::null_mut();
        };
        Box::into_raw(Box::new(Session {
            shell: Shell::new(vault),
            app: ModalApp::new(InputMode::Notion),
        }))
    })
}

/// Close a session. Null is allowed.
///
/// # Safety
///
/// `handle` must be null or a live pointer from [`closure_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn closure_close(handle: *mut Session) {
    guard((), || {
        if handle.is_null() {
            return;
        }
        // SAFETY: non-null and from `Box::into_raw` in `closure_open`.
        drop(unsafe { Box::from_raw(handle) });
    });
}

/// How many outline rows the vault has. Zero for a null handle.
///
/// # Safety
///
/// `handle` must be null or a live pointer from [`closure_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn closure_row_count(handle: *mut Session) -> usize {
    guard(0, || {
        // SAFETY: the caller's contract, checked for null inside.
        let Some(s) = (unsafe { session(handle) }) else {
            return 0;
        };
        s.app.rows(&s.shell).len()
    })
}

/// The title of row `index`, or null if there is no such row.
///
/// Free the result with [`closure_string_free`].
///
/// # Safety
///
/// `handle` must be null or a live pointer from [`closure_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn closure_row_title(handle: *mut Session, index: usize) -> *mut c_char {
    guard(std::ptr::null_mut(), || {
        // SAFETY: the caller's contract, checked for null inside.
        let Some(s) = (unsafe { session(handle) }) else {
            return std::ptr::null_mut();
        };
        let rows = s.app.rows(&s.shell);
        rows.get(index)
            .map_or(std::ptr::null_mut(), |r| out_string(&r.title))
    })
}

/// Move the cursor to row `index`. Out of range and null are no-ops.
///
/// # Safety
///
/// `handle` must be null or a live pointer from [`closure_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn closure_select(handle: *mut Session, index: usize) {
    guard((), || {
        // SAFETY: the caller's contract, checked for null inside.
        let Some(s) = (unsafe { session(handle) }) else {
            return;
        };
        if index < s.app.rows(&s.shell).len() {
            s.app.select(index, &s.shell);
        }
    });
}

/// The selected headline's body as the reader should see it — the
/// preview, so entities, scripts, tables and compositions read the way
/// they do in every other shell (I7, I12).
///
/// Null when nothing is selected. Free with [`closure_string_free`].
///
/// # Safety
///
/// `handle` must be null or a live pointer from [`closure_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn closure_selected_body(handle: *mut Session) -> *mut c_char {
    guard(std::ptr::null_mut(), || {
        // SAFETY: the caller's contract, checked for null inside.
        let Some(s) = (unsafe { session(handle) }) else {
            return std::ptr::null_mut();
        };
        s.app
            .selected_detail(&s.shell)
            .map_or(std::ptr::null_mut(), |d| out_string(&d.body))
    })
}

/// Free a string this library handed out. Null is allowed.
///
/// # Safety
///
/// `s` must be null or a pointer returned by this library and not yet
/// freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn closure_string_free(s: *mut c_char) {
    guard((), || {
        if s.is_null() {
            return;
        }
        // SAFETY: non-null and from `CString::into_raw` in `out_string`.
        drop(unsafe { CString::from_raw(s) });
    });
}
