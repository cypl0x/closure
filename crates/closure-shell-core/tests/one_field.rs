//! One input field, everywhere.
//!
//! "I think we have to streamline or create a custom view component
//! which shares the same behavior. It should be consistent. And not for
//! every new input field the same keybinding problems."
//!
//! Two of the fields were [`LineInput`] — the type that knows readline —
//! and the rest were bare `String`s with a hand-rolled `push`/`pop` per
//! surface. Several of those handlers were not even *given* the ctrl and
//! alt flags, so a chord could not have reached them if they had wanted
//! it. That is why `C-a` worked in the capture prompt and did nothing in
//! the assistant's question box.
//!
//! Every field is now the same field. Two shapes of it:
//!
//! * a field on its own (capture, rename, the new-headline prompts,
//!   tags, property, `:`, the ticket box, the assistant) gets the whole
//!   readline set, `C-k`/`C-y` included;
//! * a field attached to a list (search, body search, the pickers) lends
//!   `C-n`/`C-p`/`C-j`/`C-k` to the list, because in a filter those mean
//!   "next result" and always have, and keeps everything else.
//!
//! Nothing else differs, and that is the whole point of the item.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Alpha
:PROPERTIES:
:ID: 01HQ1FLD00000000000000001
:END:
alpha body
* Beta
:PROPERTIES:
:ID: 01HQ1FLD00000000000000002
:END:
beta body
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

/// Open `command`'s surface with a row selected, the way a user does,
/// on an empty field — rename and the tag/property prompts open on what
/// is already there, and `C-u` is how a user clears one.
fn open(command: &str) -> (TempDir, Shell, ModalApp) {
    let (dir, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQ1FLD00000000000000001"));
    app.run(&mut shell, command);
    app.on_key(&mut shell, "u", true, false, None);
    (dir, shell, app)
}

fn type_in(app: &mut ModalApp, shell: &mut Shell, s: &str) {
    for c in s.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
}

fn press(app: &mut ModalApp, shell: &mut Shell, key: &str, ctrl: bool, alt: bool) {
    app.on_key(shell, key, ctrl, alt, None);
}

/// Every surface that has a one-line field, and the command that opens
/// it. Kept in one place so a new field cannot quietly skip the rules.
const FIELDS: &[&str] = &[
    "capture-start",
    "rename",
    "add-sibling",
    "edit-tags",
    "edit-property",
    "ex-command",
    "sync",
    "llm",
    "palette",
    "search-start",
    "body-search",
    "buffer-list",
    "recent-files",
    "tag-picker",
    "refile",
];

/// The fields a space can be typed into. The tag picker's space ticks
/// the tag under the cursor — tags are one word, so the key is free
/// there and it is how a new tag gets into a vault at all.
const WORD_FIELDS: &[&str] = &[
    "capture-start",
    "rename",
    "add-sibling",
    "edit-tags",
    "edit-property",
    "ex-command",
    "sync",
    "llm",
    "palette",
    "search-start",
    "body-search",
    "buffer-list",
    "recent-files",
    "refile",
];

/// The ones whose field is not sharing chords with a result list.
///
/// Capture is missing on purpose: `C-j`/`C-k` walk its history there,
/// asked for earlier and covered by its own test. It is the one field
/// with a history, so it is the one field that spends two chords on
/// one — and it now also answers `M-p`/`M-n`, which is what a
/// minibuffer with a history has always used.
const LONE_FIELDS: &[&str] = &[
    "rename",
    "add-sibling",
    "edit-tags",
    "edit-property",
    "ex-command",
    "sync",
    "llm",
];

#[test]
fn every_surface_with_a_field_reports_one() {
    for cmd in FIELDS {
        let (_d, mut shell, mut app) = open(cmd);
        type_in(&mut app, &mut shell, "x");
        assert_eq!(
            app.prompt_text(),
            Some("x"),
            "{cmd} has no field the shells can read"
        );
    }
}

#[test]
fn typing_lands_in_every_field() {
    for cmd in FIELDS {
        let (_d, mut shell, mut app) = open(cmd);
        type_in(&mut app, &mut shell, "alpha");
        assert_eq!(app.prompt_text(), Some("alpha"), "{cmd}");
        assert_eq!(app.prompt_cursor(), 5, "{cmd} cursor follows the typing");
    }
}

#[test]
fn c_a_and_c_e_reach_both_ends_everywhere() {
    for cmd in FIELDS {
        let (_d, mut shell, mut app) = open(cmd);
        type_in(&mut app, &mut shell, "alphabeta");
        press(&mut app, &mut shell, "a", true, false);
        assert_eq!(app.prompt_cursor(), 0, "{cmd}: C-a to the start");
        press(&mut app, &mut shell, "e", true, false);
        assert_eq!(app.prompt_cursor(), 9, "{cmd}: C-e to the end");
    }
}

#[test]
fn the_word_motions_reach_every_field() {
    for cmd in WORD_FIELDS {
        let (_d, mut shell, mut app) = open(cmd);
        type_in(&mut app, &mut shell, "alpha beta");
        press(&mut app, &mut shell, "b", false, true);
        assert_eq!(app.prompt_cursor(), 6, "{cmd}: M-b to the start of `beta`");
        press(&mut app, &mut shell, "f", false, true);
        assert_eq!(app.prompt_cursor(), 10, "{cmd}: M-f back over it");
    }
}

#[test]
fn the_arrows_move_the_caret_in_every_field() {
    for cmd in FIELDS {
        let (_d, mut shell, mut app) = open(cmd);
        type_in(&mut app, &mut shell, "abc");
        press(&mut app, &mut shell, "left", false, false);
        assert_eq!(app.prompt_cursor(), 2, "{cmd}");
        press(&mut app, &mut shell, "right", false, false);
        assert_eq!(app.prompt_cursor(), 3, "{cmd}");
    }
}

#[test]
fn alt_backspace_kills_a_word_back_in_every_field() {
    // "ctlr+backpsace when trying to delete a word backwards" — the
    // same complaint, and it has to hold in all of them.
    for cmd in WORD_FIELDS {
        let (_d, mut shell, mut app) = open(cmd);
        type_in(&mut app, &mut shell, "alpha beta");
        press(&mut app, &mut shell, "backspace", false, true);
        assert_eq!(app.prompt_text(), Some("alpha "), "{cmd}");
    }
}

#[test]
fn a_lone_field_kills_and_yanks() {
    for cmd in LONE_FIELDS {
        let (_d, mut shell, mut app) = open(cmd);
        type_in(&mut app, &mut shell, "alpha beta");
        press(&mut app, &mut shell, "a", true, false);
        press(&mut app, &mut shell, "k", true, false);
        assert_eq!(app.prompt_text(), Some(""), "{cmd}: C-k killed the line");
        press(&mut app, &mut shell, "y", true, false);
        assert_eq!(
            app.prompt_text(),
            Some("alpha beta"),
            "{cmd}: C-y put it back"
        );
    }
}

#[test]
fn a_field_beside_a_list_leaves_the_list_its_chords() {
    // `C-n` in a filter means "next result" and always has; the field
    // takes everything the list does not.
    for cmd in ["search-start", "buffer-list", "recent-files"] {
        let (_d, mut shell, mut app) = open(cmd);
        type_in(&mut app, &mut shell, "alpha");
        press(&mut app, &mut shell, "n", true, false);
        assert_eq!(
            app.prompt_text(),
            Some("alpha"),
            "{cmd}: C-n moved the list, not the text"
        );
    }
}

#[test]
fn the_kill_ring_is_shared_between_fields() {
    // Kill in one prompt, yank in the next: one field means one ring.
    let (_d, mut shell, mut app) = open("rename");
    type_in(&mut app, &mut shell, "borrowed");
    press(&mut app, &mut shell, "a", true, false);
    press(&mut app, &mut shell, "k", true, false);
    press(&mut app, &mut shell, "escape", false, false);

    app.run(&mut shell, "ex-command");
    press(&mut app, &mut shell, "y", true, false);
    assert_eq!(app.prompt_text(), Some("borrowed"));
}
