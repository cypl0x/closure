//! Every surface renders into a headless buffer.
//!
//! `draw` was private and reachable only from a loop that needs a TTY,
//! which is most of why this crate carried 1,190 unexecuted lines. It
//! does not need a terminal: ratatui ships `TestBackend`, a buffer you
//! can draw into and read back.
//!
//! What this asserts is that every surface paints *something* at a
//! range of sizes without panicking. A render that panics takes the
//! whole shell with it, and it does so on somebody's machine at
//! whatever size their terminal happens to be — which is why the sizes
//! here include an absurd one.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_store::Vault;
use closure_tui::{App, draw};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const NOTE: &str = "\
* TODO Ship it :work:
SCHEDULED: <2026-06-20 Sat>
:PROPERTIES:
:ID: 01TUIRENDER000000001
:END:
a body line that is long enough to need wrapping in a narrow terminal
#+BEGIN_SRC sh
echo hi
#+END_SRC
** A child
:PROPERTIES:
:ID: 01TUIRENDER000000002
:END:
";

/// Terminal sizes worth surviving: ordinary, wide, and two that are
/// unreasonable — a render that divides by a width or subtracts a
/// border has its bug at the small end.
const SIZES: &[(u16, u16)] = &[(80, 24), (200, 60), (20, 5), (1, 1)];

fn vault() -> (tempfile::TempDir, Vault) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("notes.org"), NOTE).unwrap();
    let v = Vault::open(d.path()).unwrap();
    (d, v)
}

fn draw_at(app: &App, vault: &Vault, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).expect("backend");
    term.draw(|f| draw(f, app, vault)).expect("draw");
    let buf = term.backend().buffer().clone();
    buf.content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect()
}

#[test]
fn the_browse_surface_paints_at_every_size() {
    let (_d, v) = vault();
    for (w, h) in SIZES {
        let app = App::new(v.paths());
        let _ = draw_at(&app, &v, *w, *h);
    }
}

#[test]
fn every_surface_a_chord_opens_paints() {
    // The surfaces are reached the way a user reaches them, so a
    // surface nobody can open is not silently counted as covered.
    let (_d, v) = vault();
    for (chord, _cmd) in closure_tui::mode_bindings(closure_config::InputMode::Doom) {
        let mut app = App::new(v.paths());
        for stroke in chord.split_whitespace() {
            app.handle_stroke(stroke);
        }
        for (w, h) in SIZES {
            let _ = draw_at(&app, &v, *w, *h);
        }
    }
}

#[test]
fn the_browse_surface_shows_the_headline_it_has() {
    // Beyond "did not panic": the first surface a user sees has to
    // contain the note they opened.
    let (_d, v) = vault();
    let app = App::new(v.paths());
    let painted = draw_at(&app, &v, 120, 40);
    assert!(
        painted.contains("Ship it"),
        "the outline painted no headline"
    );
}

#[test]
fn an_empty_vault_still_paints() {
    let d = tempfile::tempdir().unwrap();
    let v = Vault::open(d.path()).unwrap();
    let app = App::new(Vec::new());
    for (w, h) in SIZES {
        let _ = draw_at(&app, &v, *w, *h);
    }
}
