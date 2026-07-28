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

/// A *painted* window over a one-file vault containing `org`.
///
/// [`test_window`] reaches the view's methods; this reaches the window
/// the user actually gets. `add_window_view` opens a maximized window
/// over gpui's stub display and lays it out for real, so
/// `VisualTestContext` can click at painted coordinates
/// (`debug_bounds`), press keys through gpui's own dispatch tree, and
/// resize the window — none of which the entity-level harness can do.
///
/// The view is focused here because [`crate::run`] focuses it: a
/// harness that skips it would keep passing while the shipped window
/// took no keys at all.
///
/// # Panics
///
/// On a vault that cannot be created or opened.
#[must_use]
pub fn visual_window<'a>(
    cx: &'a mut gpui::TestAppContext,
    org: &str,
) -> (
    tempfile::TempDir,
    gpui::Entity<GpuiView>,
    &'a mut gpui::VisualTestContext,
) {
    let dir = tempfile::tempdir().expect("a temp vault");
    std::fs::write(dir.path().join("notes.org"), org).expect("write the vault");
    let vault = Vault::open(dir.path()).expect("open the vault");
    let (view, vcx) = cx.add_window_view(|_w, cx| {
        GpuiView::new(
            Shell::new(vault),
            closure_config::InputMode::Vim,
            Theme::doom_vibrant(),
            cx,
        )
    });
    vcx.update(|window, cx| {
        let focus = gpui::Focusable::focus_handle(view.read(cx), cx);
        window.focus(&focus);
    });
    vcx.run_until_parked();
    (dir, view, vcx)
}

/// The commands that walk the window from Browse to `surface` —
/// empty for the surface it already starts on.
///
/// The match is exhaustive on purpose: a surface added to
/// [`crate::ModalSurface`] and not given a route here stops the build,
/// rather than quietly becoming a pane no test ever paints. That is
/// the whole failure mode this table exists to close — a panic in a
/// rarely-opened pane is one the user finds.
///
/// Most surfaces are one command from Browse. `edit-special` is not:
/// it lifts the source block *under the cursor* out of an open body
/// (or off the block list), so reaching it takes the door as well as
/// the room.
#[must_use]
pub const fn opening_route(surface: crate::ModalSurface) -> &'static [&'static str] {
    use crate::ModalSurface as S;
    match surface {
        S::Browse => &[],
        S::Search => &["search-start"],
        S::Capture => &["capture-start"],
        S::EditBody => &["edit-body"],
        S::Backlinks => &["backlinks"],
        S::Agenda => &["agenda"],
        S::Blocks => &["block-list"],
        S::TagsEdit => &["edit-tags"],
        S::PropertyEdit => &["edit-property"],
        S::Rename => &["rename"],
        S::AddSibling => &["add-sibling"],
        S::Palette => &["palette"],
        S::UndoHistory => &["undo-history"],
        S::Headlines => &["headline-list"],
        S::DbView => &["db-view"],
        S::BodySearch => &["body-search"],
        S::Sniffer => &["sniffer"],
        S::Conflicts => &["conflicts"],
        S::Ex => &["ex-command"],
        S::EditBlock => &["block-list", "edit-special"],
        S::Sync => &["sync"],
        S::Graph => &["graph"],
        S::Journal => &["journal"],
        S::Cron => &["cron"],
        S::Llm => &["llm"],
        S::EditFile => &["toggle-view"],
        S::Buffers => &["buffer-list"],
        S::Files => &["recent-files"],
        S::DatePick => &["schedule"],
    }
}

/// Every surface, so a sweep can paint all of them.
///
/// [`opening_command`] keeps the *mapping* honest at compile time;
/// this keeps the *iteration* honest, and `every_surface_is_swept`
/// pins the two together.
pub const ALL_SURFACES: &[crate::ModalSurface] = {
    use crate::ModalSurface as S;
    &[
        S::Browse,
        S::Search,
        S::Capture,
        S::EditBody,
        S::Backlinks,
        S::Agenda,
        S::Blocks,
        S::TagsEdit,
        S::PropertyEdit,
        S::Rename,
        S::AddSibling,
        S::Palette,
        S::UndoHistory,
        S::Headlines,
        S::DbView,
        S::BodySearch,
        S::Sniffer,
        S::Conflicts,
        S::Ex,
        S::EditBlock,
        S::Sync,
        S::Graph,
        S::Journal,
        S::Cron,
        S::Llm,
        S::EditFile,
        S::Buffers,
        S::Files,
        S::DatePick,
    ]
};
