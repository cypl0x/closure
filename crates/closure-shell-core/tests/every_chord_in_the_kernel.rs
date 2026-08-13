//! Every chord of every mode, through the kernel, on four vaults.
//!
//! `closure-shell-core` is the largest remaining coverage hole — 1,441
//! unexecuted lines of 15,155 — and most of it is `run_command`'s flat
//! dispatch, roughly two hundred arms reached through `on_key`. The
//! same shape that took the CLI from 37% to 68% and the TUI's dispatch
//! with it.
//!
//! The claim is deliberately weak and broad: pressing a key does not
//! panic and does not leave the app unable to answer for itself. What a
//! command *did* belongs in a test per command — there are two hundred
//! of those already. That every arm can be reached at all is what
//! nothing was asserting.
//!
//! Four vaults, and the last three are the ones that find things: a
//! full one, an empty one, one with no files, and one whose single file
//! is a headline with nothing in it. An arm that indexes the selected
//! headline is fine until there is not one.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const MODES: &[InputMode] = &[
    InputMode::Doom,
    InputMode::Emacs,
    InputMode::Vim,
    InputMode::Helix,
    InputMode::Notion,
];

const FULL: &str = "\
* TODO Ship it :work:
SCHEDULED: <2026-06-20 Sat>
:PROPERTIES:
:ID: 01KERNCHORD000000001
:END:
a body line with a [[id:01KERNCHORD000000002]] link
** A child
:PROPERTIES:
:ID: 01KERNCHORD000000002
:END:
#+BEGIN_SRC sh
echo hi
#+END_SRC
* DONE Done thing
:PROPERTIES:
:ID: 01KERNCHORD000000003
:END:
";

fn shell_for(src: Option<&str>) -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().unwrap();
    if let Some(text) = src {
        std::fs::write(dir.path().join("notes.org"), text).unwrap();
    }
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault))
}

/// One press of every chord the mode defines, each on a fresh app so a
/// chord cannot be judged by what the previous one left behind.
fn press_every_chord(mode: InputMode, src: Option<&str>) {
    for (chord, _cmd) in closure_input::mode_keymap(mode) {
        let (_dir, mut shell) = shell_for(src);
        let mut app = ModalApp::new(mode);
        for stroke in chord.split_whitespace() {
            let (key, ctrl, alt) = split_stroke(stroke);
            let text = (key.chars().count() == 1 && !ctrl && !alt)
                .then(|| key.chars().next().unwrap_or(' '));
            app.on_key(&mut shell, &key, ctrl, alt, text);
        }
        // The app has to still be able to answer for itself. A command
        // that leaves the cursor past the end of its own row list is a
        // panic on the next frame rather than on this one.
        let rows = app.rows(&shell);
        assert!(
            app.selected() <= rows.len(),
            "`{chord}` in {mode:?} left the cursor at {} of {} rows",
            app.selected(),
            rows.len()
        );
    }
}

/// `C-c`, `M-x`, `g`, `SPC` — into the parts `on_key` takes.
fn split_stroke(stroke: &str) -> (String, bool, bool) {
    let mut ctrl = false;
    let mut alt = false;
    let mut rest = stroke;
    loop {
        if let Some(r) = rest.strip_prefix("C-") {
            ctrl = true;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("M-") {
            alt = true;
            rest = r;
        } else {
            break;
        }
    }
    let key = match rest {
        // The *name*, not the character. `modal_stroke` maps "space"
        // to `SPC`; a literal " " falls through to the text branch and
        // becomes a stroke called " ", so `SPC q r` never matched and
        // the `q` was taken as the standalone quit binding. That was my
        // harness, not the chord trie — which resolves exact match
        // first, then prefix, and is correct.
        "SPC" => "space",
        "RET" => "enter",
        "ESC" => "escape",
        "TAB" => "tab",
        other => other,
    };
    (key.to_owned(), ctrl, alt)
}

#[test]
fn every_chord_on_a_full_vault() {
    for mode in MODES {
        press_every_chord(*mode, Some(FULL));
    }
}

#[test]
fn every_chord_on_an_empty_file() {
    // Where "the selected headline" is not one.
    for mode in MODES {
        press_every_chord(*mode, Some(""));
    }
}

#[test]
fn every_chord_on_a_vault_with_no_files() {
    for mode in MODES {
        press_every_chord(*mode, None);
    }
}

#[test]
fn every_chord_on_a_headline_with_nothing_in_it() {
    // A headline with no body, no properties, no id — every accessor
    // that assumes one of those has an arm here.
    for mode in MODES {
        press_every_chord(*mode, Some("* Bare\n"));
    }
}

#[test]
fn every_chord_twice_lands_in_whatever_it_opened() {
    // The second press goes to the surface the first one opened, which
    // for most of these is the only visitor that surface gets.
    for (chord, _cmd) in closure_input::mode_keymap(InputMode::Doom) {
        let (_dir, mut shell) = shell_for(Some(FULL));
        let mut app = ModalApp::new(InputMode::Doom);
        for _ in 0..2 {
            for stroke in chord.split_whitespace() {
                let (key, ctrl, alt) = split_stroke(stroke);
                let text = (key.chars().count() == 1 && !ctrl && !alt)
                    .then(|| key.chars().next().unwrap_or(' '));
                app.on_key(&mut shell, &key, ctrl, alt, text);
            }
        }
    }
}

#[test]
fn escape_gets_back_out_of_whatever_a_chord_opened() {
    // The property that makes a modal kernel usable. Two escapes,
    // because a prompt inside a pane is two deep.
    for (chord, cmd) in closure_input::mode_keymap(InputMode::Doom) {
        // `q` is the one chord whose job is to leave.
        if *cmd == "quit" {
            continue;
        }
        // Multi-stroke chords included. They were skipped for a while
        // because `SPC q r` appeared to quit; that was this harness
        // sending a literal space rather than the key named "space".
        let (_dir, mut shell) = shell_for(Some(FULL));
        let mut app = ModalApp::new(InputMode::Doom);
        for stroke in chord.split_whitespace() {
            let (key, ctrl, alt) = split_stroke(stroke);
            let text = (key.chars().count() == 1 && !ctrl && !alt)
                .then(|| key.chars().next().unwrap_or(' '));
            app.on_key(&mut shell, &key, ctrl, alt, text);
        }
        app.on_key(&mut shell, "escape", false, false, None);
        app.on_key(&mut shell, "escape", false, false, None);
        assert!(
            !app.should_quit(),
            "`{chord}` ({cmd}) then escape asked to quit"
        );
    }
}

#[test]
fn the_leader_disambiguates_a_chord_from_a_standalone_binding() {
    // Doom binds both `q` -> quit and `SPC q r` -> reload-shell, so the
    // leader state is the only thing telling them apart. Worth its own
    // test because getting it wrong quits instead of reloading, which
    // loses whatever was unsaved.
    //
    // It works. What did not was this file's own stroke encoding, which
    // sent a literal space where `modal_stroke` wants the key named
    // "space" — so `SPC q r` never matched and the `q` fell through to
    // quit. Recorded here rather than quietly fixed, because for a
    // while I believed the trie was broken and it was not.
    let (_dir, mut shell) = shell_for(Some(FULL));
    let mut app = ModalApp::new(InputMode::Doom);
    for stroke in ["SPC", "q", "r"] {
        let (key, ctrl, alt) = split_stroke(stroke);
        let text =
            (key.chars().count() == 1 && !ctrl && !alt).then(|| key.chars().next().unwrap_or(' '));
        app.on_key(&mut shell, &key, ctrl, alt, text);
    }
    assert!(
        !app.should_quit(),
        "`SPC q r` quit instead of reloading — the leader did not engage"
    );
}

#[test]
fn the_standalone_binding_still_works_on_its_own() {
    // The other half. A leader that swallowed `q` everywhere would make
    // the chord work and the plain key dead.
    let (_dir, mut shell) = shell_for(Some(FULL));
    let mut app = ModalApp::new(InputMode::Doom);
    app.on_key(&mut shell, "q", false, false, Some('q'));
    assert!(app.should_quit(), "`q` on its own no longer quits");
}
