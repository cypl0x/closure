//! "Please research the Doom Emacs keybindings for quick header
//! creation in the editor (for example ctrl+enter -> new sibling
//! headline below, ctrl+shift+enter -> new sibling headline above, …)"
//!
//! The reference is the user's own Doom, `modules/lang/org/config.el`:
//!
//! ```elisp
//! (setq org-M-RET-may-split-line nil
//!       org-insert-heading-respect-content t)
//! "C-RET"   #'+org/insert-item-below
//! "C-S-RET" #'+org/insert-item-above
//! "C-M-RET" #'org-insert-subheading
//! ;; and in evil-org-mode-map, :ni — normal *and* insert state
//! ```
//!
//! plus stock org for the meta layer: `M-RET` is `org-meta-return` (a
//! heading at the same level) and `M-S-RET` is `org-insert-todo-heading`.
//!
//! So: shift is the *above/below* axis on the control layer and the
//! *TODO* axis on the meta layer, and the child lives on `C-M-RET`.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

const ORG: &str = "\
* Alpha
:PROPERTIES:
:ID: 01QUICKAAAAAAAAAAAAAAAAAAA
:END:
alpha body
** Alpha child
:PROPERTIES:
:ID: 01QUICKCCCCCCCCCCCCCCCCCCC
:END:
* Beta
:PROPERTIES:
:ID: 01QUICKBBBBBBBBBBBBBBBBBBB
:END:
beta body
";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell, ModalApp::new(closure_config::InputMode::Doom))
}

fn type_in(app: &mut ModalApp, shell: &mut Shell, text: &str) {
    for c in text.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
}

/// Run the chord's command, answer the title prompt, and give back the
/// file as it now is on disk.
fn make(app: &mut ModalApp, shell: &mut Shell, command: &str, title: &str) -> String {
    app.run(shell, command);
    assert_eq!(
        app.surface(),
        ModalSurface::AddSibling,
        "{command} did not ask for a title (status: {})",
        app.status()
    );
    type_in(app, shell, title);
    app.on_key(shell, "enter", false, false, None);
    shell.vault.iter().map(|(_, doc)| doc.source()).collect()
}

#[test]
fn the_five_chords_are_bound_the_way_doom_binds_them() {
    use closure_config::InputMode;
    for mode in [InputMode::Doom, InputMode::Emacs] {
        let keys = closure_input::mode_keymap(mode);
        let of = |chord: &str| {
            keys.iter()
                .find(|(c, _)| *c == chord)
                .map_or("(unbound)", |(_, cmd)| *cmd)
        };
        assert_eq!(of("M-RET"), "add-heading", "{mode:?}");
        assert_eq!(of("M-S-RET"), "add-todo-heading", "{mode:?}");
        // Doom's `+org/insert-item-below` / `-above`: shift is the
        // direction here, not the keyword.
        assert_eq!(of("C-RET"), "add-heading", "{mode:?}");
        assert_eq!(of("C-S-RET"), "add-heading-above", "{mode:?}");
        // `org-insert-subheading`.
        assert_eq!(of("C-M-RET"), "add-child-heading", "{mode:?}");
    }
}

#[test]
fn a_sibling_lands_above_the_one_you_are_on() {
    let (_d, mut shell, mut app) = app();
    app.select(1, &shell); // Alpha child
    let out = make(&mut app, &mut shell, "add-heading-above", "Before");
    let before = out.find("** Before").expect("no heading made");
    let child = out
        .find("** Alpha child")
        .expect("Alpha child went missing");
    assert!(before < child, "it landed below, not above:\n{out}");
}

#[test]
fn a_new_heading_lands_after_the_whole_subtree() {
    // `org-insert-heading-respect-content t` is what the config sets,
    // and it applies to `M-RET` as much as to `C-RET` — which is why
    // both chords are one command here. A heading that landed inside
    // the subtree would have adopted it.
    let (_d, mut shell, mut app) = app();
    app.select(0, &shell); // Alpha, which has a child
    let out = make(&mut app, &mut shell, "add-heading", "After all");
    let child = out.find("** Alpha child").expect("child went missing");
    let made = out.find("* After all").expect("no heading made");
    assert!(
        child < made,
        "it landed inside the subtree instead of after it:\n{out}"
    );
}

#[test]
fn the_new_headline_is_the_one_you_are_on_afterwards() {
    // "Jumps to newly created headline" — otherwise every chord is
    // followed by finding the thing you just made.
    let (_d, mut shell, mut app) = app();
    app.select(0, &shell);
    make(&mut app, &mut shell, "add-heading", "Fresh");
    let title = app
        .rows(&shell)
        .get(app.selected())
        .map(|r| r.title.clone())
        .unwrap_or_default();
    assert_eq!(title, "Fresh", "the selection stayed where it was");
}

#[test]
fn the_chords_work_in_the_file_buffer() {
    // "quick header creation *in the editor*": Doom binds these `:ni`
    // in `evil-org-mode-map`, so they are buffer chords first. The
    // outline is where they were wired.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "toggle-view");
    assert_eq!(app.surface(), ModalSurface::EditFile, "no file buffer");
    app.on_key(&mut shell, "enter", false, true, None); // M-RET
    assert_eq!(
        app.surface(),
        ModalSurface::AddSibling,
        "M-RET did nothing in the buffer (status: {})",
        app.status()
    );
}

#[test]
fn the_chip_says_which_direction_you_asked_for() {
    // The prompt's chip is the only thing on screen that says which of
    // the six chords you pressed. It leaves the keyword out — the
    // prompt paints that in its own field, in the keyword's colour —
    // but above and below are not the keyword, and a chip that says
    // "sibling" for both is a chip you cannot check yourself against.
    let (_d, mut shell, mut app) = app();
    app.select(1, &shell);
    app.run(&mut shell, "add-heading-above");
    assert_eq!(app.new_heading_kind(), "sibling above");
    app.on_key(&mut shell, "escape", false, false, None);
    app.run(&mut shell, "add-heading");
    assert_eq!(app.new_heading_kind(), "sibling");
}

#[test]
fn the_title_prompt_keeps_the_buffer_behind_it() {
    // "everything is shifting and I always get confused": pressing a
    // heading chord in the editor swapped the whole window back to the
    // outline while you typed the title, and swapped it again when you
    // accepted. The `:` line already learned this — a prompt opened
    // over a buffer is a bar on that buffer, not a different screen.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "toggle-view");
    assert_eq!(app.surface(), ModalSurface::EditFile);
    app.run(&mut shell, "add-heading");
    assert_eq!(app.surface(), ModalSurface::AddSibling);
    assert_eq!(
        app.surface_beneath(),
        ModalSurface::EditFile,
        "the buffer vanished while the title was being typed"
    );
}

#[test]
fn accepting_the_title_gives_the_buffer_back() {
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "toggle-view");
    app.run(&mut shell, "add-heading");
    type_in(&mut app, &mut shell, "Made here");
    app.on_key(&mut shell, "enter", false, false, None);
    assert_eq!(
        app.surface(),
        ModalSurface::EditFile,
        "it dropped me back on the outline"
    );
}

#[test]
fn from_the_outline_the_prompt_still_sits_on_the_outline() {
    let (_d, mut shell, mut app) = app();
    app.select(0, &shell);
    app.run(&mut shell, "add-heading");
    assert!(
        !app.surface_beneath().is_editor(),
        "a prompt opened from the outline claimed a buffer"
    );
}
