//! "sync with system clipboard (two way)" and "having something on the
//! system clipboard and being able to use p in Vim/helix/Doom mode to
//! paste would be nice … Using ctrl+v is quite inconsistent with Doom
//! Emacs keymap mode. Shall we build a clipboard watcher?"
//!
//! Inside a buffer both directions already worked: the window reads
//! the system clipboard into the register before every key, so `p`
//! pastes what another application copied, and a `yy` goes back out.
//! Verified on screen — `p` in the editor drops the X selection into
//! the line.
//!
//! The outline was left out of both. `take_clipboard` returned early
//! on anything that is not an editor, and a subtree cut with `d` went
//! onto the vault's own kill ring and nowhere else. So a headline you
//! cut could not be pasted into a browser, and org text copied from a
//! browser could not become a headline.
//!
//! No watcher, and this is the answer to that question: a watcher
//! means a timer, a thread and a wrong answer between ticks. The
//! clipboard is *read on the keypress that needs it*, which is exact
//! and costs nothing while you are not pressing anything.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Alpha
:PROPERTIES:
:ID: 01HQCLIP00000000000001
:END:
body a
* Beta
:PROPERTIES:
:ID: 01HQCLIP00000000000002
:END:
body b
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQCLIP00000000000001"));
    (dir, shell, app)
}

fn titles(shell: &Shell, app: &ModalApp) -> Vec<String> {
    app.rows(shell).into_iter().map(|r| r.title).collect()
}

#[test]
fn cutting_a_headline_offers_it_to_the_system_clipboard() {
    // The outward half. A window watches the register's generation and
    // writes whatever is new to the clipboard, so putting the cut
    // subtree there is all the outline has to do.
    let (_d, mut shell, mut app) = fixture();
    let before = app.register_generation();
    app.run(&mut shell, "delete");
    assert_ne!(
        app.register_generation(),
        before,
        "the cut never reached the register, so no window will offer it"
    );
    assert!(
        app.register_text().contains("Alpha"),
        "the register holds {:?}",
        app.register_text()
    );
}

#[test]
fn the_cut_text_is_the_org_of_the_subtree() {
    // Not the title alone: what goes on the clipboard has to be
    // something another org tool could paste back.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "delete");
    let text = app.register_text().to_owned();
    assert!(text.starts_with('*'), "not a headline: {text:?}");
    assert!(text.contains("body a"), "body missing: {text:?}");
}

#[test]
fn the_outline_reads_the_clipboard_too() {
    // The inward half. It used to refuse anything that was not an
    // editor, so the outline could never see what another application
    // had copied.
    let (_d, _sh, mut app) = fixture();
    app.set_register_from_clipboard("* From a browser\nsome body\n");
    assert!(app.register_text().contains("From a browser"));
}

#[test]
fn pasting_with_an_empty_ring_uses_the_clipboard() {
    // Nothing has been cut in closure, but there is org text on the
    // clipboard: `p` makes it a headline rather than reporting an
    // empty ring.
    let (_d, mut shell, mut app) = fixture();
    app.set_register_from_clipboard("* From a browser\nsome body\n");
    app.run(&mut shell, "paste-subtree");
    assert!(
        titles(&shell, &app).iter().any(|t| t == "From a browser"),
        "{:?} — {}",
        titles(&shell, &app),
        app.status()
    );
}

#[test]
fn a_real_cut_still_wins_over_the_clipboard() {
    // What was cut *here* is what `p` puts back. The clipboard is the
    // fallback, not a competitor — otherwise cut-and-paste inside the
    // outline would start pasting whatever a browser last held.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "delete");
    assert!(app.select_by_id(&shell, "01HQCLIP00000000000002"));
    app.run(&mut shell, "paste-subtree");
    let listed = titles(&shell, &app);
    assert!(listed.iter().any(|t| t == "Alpha"), "{listed:?}");
    assert_eq!(listed.iter().filter(|t| *t == "Alpha").count(), 1);
}

#[test]
fn plain_text_on_the_clipboard_becomes_a_headline() {
    // Text that is not org at all still has to land as something —
    // silently doing nothing is the outcome the report is about.
    let (_d, mut shell, mut app) = fixture();
    app.set_register_from_clipboard("just some prose");
    app.run(&mut shell, "paste-subtree");
    assert!(
        titles(&shell, &app)
            .iter()
            .any(|t| t.contains("just some prose")),
        "{:?} — {}",
        titles(&shell, &app),
        app.status()
    );
}

#[test]
fn pasting_between_two_headlines_does_not_swallow_the_next_one() {
    // Found on screen, not here: the first version trimmed the
    // clipboard text and spliced it without restoring the newline, so
    // the headline *after* the paste was glued onto the pasted body
    // line and stopped being a headline at all. The outline showed two
    // rows where there had been three, and the file on disk read
    // `with a body line* Beta`.
    let (_d, mut shell, mut app) = fixture();
    app.set_register_from_clipboard("* From a browser\nsome body\n");
    app.run(&mut shell, "paste-subtree");
    let listed = titles(&shell, &app);
    assert!(listed.iter().any(|t| t == "From a browser"), "{listed:?}");
    assert!(
        listed.iter().any(|t| t == "Beta"),
        "the headline after the paste was swallowed: {listed:?}"
    );
    assert!(listed.iter().any(|t| t == "Alpha"), "{listed:?}");
}

#[test]
fn a_clipboard_without_a_trailing_newline_is_still_safe() {
    // The exact shape that broke it: no newline at the end, which is
    // what a browser selection usually looks like.
    let (_d, mut shell, mut app) = fixture();
    app.set_register_from_clipboard("* No trailing newline");
    app.run(&mut shell, "paste-subtree");
    let listed = titles(&shell, &app);
    assert!(
        listed.iter().any(|t| t == "No trailing newline"),
        "{listed:?}"
    );
    assert!(listed.iter().any(|t| t == "Beta"), "{listed:?}");
}
