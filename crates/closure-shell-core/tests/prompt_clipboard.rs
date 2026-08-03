//! "Input fields should work with the system clipboard just as the
//! editor does. C-k should add to the system clipboard."
//!
//! They had their own kill buffer. `line_kill` sat beside the editor's
//! register — two clipboards in one program, and only one of them was
//! mirrored to the system. So `C-k` in the capture prompt put the text
//! somewhere you could not paste from anywhere else, including from
//! the buffer two inches below.
//!
//! One register. The editor's, which is the one the system-clipboard
//! mirror already watches.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const ORG: &str = "* A\n:PROPERTIES:\n:ID: 01PROMPTCLIP00000000000A\n:END:\nbody\n";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell, ModalApp::new(closure_config::InputMode::Doom))
}

/// Type `text` into whatever prompt is open.
fn type_in(app: &mut ModalApp, shell: &mut Shell, text: &str) {
    for c in text.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
}

#[test]
fn ctrl_k_in_a_prompt_reaches_the_shared_register() {
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "capture");
    type_in(&mut app, &mut shell, "throw this away");
    // Back to the start, then kill to the end of the line.
    app.on_key(&mut shell, "a", true, false, None);
    app.on_key(&mut shell, "k", true, false, None);

    assert_eq!(
        app.register_text(),
        "throw this away",
        "the killed text went into a clipboard nothing else can see"
    );
}

#[test]
fn a_prompt_kill_tells_the_system_clipboard() {
    // The mirror only pushes when the generation moves; without the
    // bump this would copy internally and never leave the process,
    // which is the half of the request that is easy to miss.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "capture");
    type_in(&mut app, &mut shell, "hello");
    let before = app.register_generation();
    app.on_key(&mut shell, "a", true, false, None);
    app.on_key(&mut shell, "k", true, false, None);
    assert!(
        app.register_generation() > before,
        "the system clipboard was never told"
    );
}

#[test]
fn what_the_editor_yanked_can_be_pasted_into_a_prompt() {
    // The other direction, and the reason one register beats two: text
    // taken anywhere is text you can put anywhere.
    let (_d, mut shell, mut app) = app();
    app.set_register_from_clipboard("pasted from elsewhere");
    app.run(&mut shell, "capture");
    app.on_key(&mut shell, "y", true, false, None);
    assert_eq!(
        app.prompt_text().unwrap_or_default(),
        "pasted from elsewhere",
        "the prompt could not reach the shared register"
    );
}

#[test]
fn killing_nothing_does_not_clobber_what_is_held() {
    // `C-k` at the end of a line kills an empty string. Letting that
    // overwrite the register would make one stray keypress lose what
    // you had copied.
    let (_d, mut shell, mut app) = app();
    app.set_register_from_clipboard("keep me");
    app.run(&mut shell, "capture");
    app.on_key(&mut shell, "k", true, false, None);
    assert_eq!(app.register_text(), "keep me");
}
