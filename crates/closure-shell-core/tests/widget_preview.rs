//! "The window shows the template verbatim and never the expansion."
//!
//! Composition composes, takes arguments and is typed, and none of it
//! had ever been on screen. A vault with a `card` widget and two call
//! sites showed `{{card title=Today count=3}}` in the detail pane,
//! which is the source, not the page.
//!
//! Which view gets which is not a detail — it is I12. The file holds
//! the template and the *editor* must show exactly that, because the
//! editor writes back what it shows and what it cannot show it must not
//! be able to destroy. The detail pane reads and never writes, so it
//! shows what the template means. One invariant, opposite answers, for
//! the same reason.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const VAULT: &str = "\
* Widgets
:PROPERTIES:
:ID: 01HQWPREV00000000000001
:END:
#+BEGIN: closure-widget :name card :inputs title,count:number
| {{title}} | {{count}} |
#+END:

* Home
:PROPERTIES:
:ID: 01HQWPREV00000000000002
:END:
#+BEGIN: closure-widget :name board
{{card title=Today count=3}}
{{card title=Later count=12}}
#+END:
";

fn app(src: &str) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), src).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

/// The detail for the row titled `title`.
fn detail_of(
    app: &mut ModalApp,
    shell: &Shell,
    title: &str,
) -> std::sync::Arc<closure_shell_core::Detail> {
    let rows = app.rows(shell);
    let i = rows
        .iter()
        .position(|r| r.title == title)
        .expect("a row with that title");
    app.select(i, shell);
    app.selected_detail(shell).expect("a detail")
}

#[test]
fn the_preview_shows_what_the_composition_means() {
    let (_d, shell, mut app) = app(VAULT);
    let d = detail_of(&mut app, &shell, "Home");
    let shown = format!("{}\n{}", d.body, d.children);
    assert!(
        shown.contains("| Today | 3 |"),
        "the preview is still showing the template: {shown}"
    );
    assert!(
        shown.contains("| Later | 12 |"),
        "only the first call site expanded: {shown}"
    );
}

#[test]
fn a_composition_that_fails_says_so_where_it_is() {
    // A composition that cannot expand must not read as an empty page.
    let src = "* Home\n:PROPERTIES:\n:ID: 01HQWPREV00000000000003\n:END:\n\
               #+BEGIN: closure-widget :name board\n{{nosuchwidget}}\n#+END:\n";
    let (_d, shell, mut app) = app(src);
    let d = detail_of(&mut app, &shell, "Home");
    let shown = format!("{}\n{}", d.body, d.children);
    assert!(
        shown.contains("unknown widget"),
        "the preview said nothing about the failure: {shown}"
    );
}

#[test]
fn the_editor_still_holds_the_template() {
    // I12's other half, and the reason the preview may differ at all.
    let (_d, mut shell, mut app) = app(VAULT);
    let _ = detail_of(&mut app, &shell, "Home");
    // `i` opens the body editor on the selected headline.
    app.on_key(&mut shell, "i", false, false, Some('i'));
    let buffer = app.body_buffer().to_owned();
    assert!(
        buffer.contains("{{card title=Today count=3}}"),
        "the editor is showing an expansion it would then write back: {buffer}"
    );
}

#[test]
fn a_headline_without_a_widget_is_untouched() {
    let src = "* Plain\n:PROPERTIES:\n:ID: 01HQWPREV00000000000004\n:END:\n\
               just some prose with {{braces}} that are not a widget\n";
    let (_d, shell, mut app) = app(src);
    let d = detail_of(&mut app, &shell, "Plain");
    assert!(
        d.body.contains("{{braces}}"),
        "text outside a widget block was expanded: {}",
        d.body
    );
}
