//! A window over gpui's stub platform, for tests.
//!
//! Compiled only under the `gpui-test` feature: `gpui/test-support`
//! brings a `TestAppContext` with no GPU behind it, which is what makes
//! the window itself — its render, its key path, its mouse handlers —
//! reachable at all. Everything in this crate under `feature = "gpui"`
//! was compile-checked by CI and never run before this existed.

// A test harness has nowhere to return an error *to*: a vault that
// cannot be created is a broken test machine, not a case to handle.
#![allow(clippy::expect_used)]

use closure_shell_core::{Shell, Theme};
use closure_store::Vault;

use crate::GpuiView;

/// A window over a one-file vault containing `org`.
///
/// The `TempDir` comes back with it: dropping it removes the vault, so
/// the caller has to hold it for as long as the window is used.
///
/// # Panics
///
/// On a vault that cannot be created or opened — a test with no vault
/// has nothing to assert about.
#[must_use]
pub fn test_window(
    cx: &mut gpui::TestAppContext,
    org: &str,
) -> (tempfile::TempDir, gpui::WindowHandle<GpuiView>) {
    let dir = tempfile::tempdir().expect("a temp vault");
    std::fs::write(dir.path().join("notes.org"), org).expect("write the vault");
    let vault = Vault::open(dir.path()).expect("open the vault");
    let window = cx.add_window(|_w, cx| {
        GpuiView::new(
            Shell::new(vault),
            closure_config::InputMode::Vim,
            Theme::doom_vibrant(),
            cx,
        )
    });
    (dir, window)
}
