//! "Please research about the hlissner's Doom Emacs keybindings.
//! Especially he has something like g c c (e.g.
//! evil-toggle-line-comment)"
//!
//! Doom binds `gc` to `evilnc-comment-operator` — evil-nerd-commenter —
//! so `gcc` toggles the current line and `gc` over a visual selection
//! toggles all of it. What makes it worth having rather than typing
//! `#` yourself is that it knows *which* comment: the item spells the
//! cases out, with a commented and an uncommented copy of each line.
//!
//!     #+begin_src nix          →  # programs.emacs.enabled = true;
//!     #+begin_src javascript   →  // console.log("JS Comment")
//!     #+begin_src org          →  # Org comment inside a src block
//!     (org text, no block)     →  # Org comment outside of src block
//!
//! So the comment token comes from the enclosing source block's
//! language, and from org itself when there is no block.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

const ORG: &str = "\
* Notes
:PROPERTIES:
:ID: 01COMMENTAAAAAAAAAAAAAAAAA
:END:
Org comment outside of src block
#+begin_src nix
programs.emacs.enabled = true;
#+end_src
#+begin_src javascript
console.log(\"JS Comment\")
function foo() {
    console.log(\"foo\")
}
#+end_src
";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell, ModalApp::new(closure_config::InputMode::Doom))
}

/// Open the whole file and put the caret on the first line containing
/// `needle`.
fn caret_on(app: &mut ModalApp, shell: &mut Shell, needle: &str) {
    app.run(shell, "toggle-view");
    assert_eq!(app.surface(), ModalSurface::EditFile);
    let line = app
        .body_buffer()
        .lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no line with {needle}"));
    app.body_click(line, 0);
}

/// The line the caret is on, after running `toggle-line-comment`.
fn toggled(app: &mut ModalApp, shell: &mut Shell, needle: &str) -> String {
    caret_on(app, shell, needle);
    app.run(shell, "toggle-line-comment");
    app.body_buffer()
        .lines()
        .find(|l| l.contains(needle))
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn nix_gets_a_hash() {
    let (_d, mut shell, mut app) = app();
    let line = toggled(&mut app, &mut shell, "programs.emacs.enabled");
    assert_eq!(line, "# programs.emacs.enabled = true;");
}

#[test]
fn javascript_gets_two_slashes() {
    let (_d, mut shell, mut app) = app();
    let line = toggled(&mut app, &mut shell, "JS Comment");
    assert_eq!(line, "// console.log(\"JS Comment\")");
}

#[test]
fn org_text_outside_a_block_gets_orgs_own_hash() {
    let (_d, mut shell, mut app) = app();
    let line = toggled(&mut app, &mut shell, "outside of src block");
    assert_eq!(line, "# Org comment outside of src block");
}

#[test]
fn it_toggles_rather_than_only_commenting() {
    // `evilnc-comment-or-uncomment-lines`: the same chord takes it off.
    let (_d, mut shell, mut app) = app();
    caret_on(&mut app, &mut shell, "JS Comment");
    app.run(&mut shell, "toggle-line-comment");
    app.run(&mut shell, "toggle-line-comment");
    let line = app
        .body_buffer()
        .lines()
        .find(|l| l.contains("JS Comment"))
        .unwrap_or_default();
    assert_eq!(
        line, "console.log(\"JS Comment\")",
        "it did not come back off"
    );
}

#[test]
fn the_indent_survives() {
    // The item's own multi-line example keeps the body indented under
    // the comment marker; a toggle that flattened it would reformat
    // the code it was asked to comment.
    let (_d, mut shell, mut app) = app();
    let line = toggled(&mut app, &mut shell, "console.log(\"foo\")");
    assert_eq!(line, "    // console.log(\"foo\")");
}

#[test]
fn a_visual_selection_takes_all_of_it() {
    // The item shows a whole function commented out at once, which is
    // `gc` over a motion or a selection rather than `gcc` three times.
    let (_d, mut shell, mut app) = app();
    caret_on(&mut app, &mut shell, "function foo()");
    app.on_key(&mut shell, "v", false, false, Some('v'));
    app.on_key(&mut shell, "j", false, false, Some('j'));
    app.on_key(&mut shell, "j", false, false, Some('j'));
    app.run(&mut shell, "toggle-line-comment");
    let body = app.body_buffer();
    for want in ["// function foo() {", "//     console.log(\"foo\")", "// }"] {
        assert!(body.contains(want), "missing {want}:\n{body}");
    }
}

#[test]
fn the_chord_is_g_c_c_in_the_modal_modes() {
    use closure_config::InputMode;
    for mode in [InputMode::Doom, InputMode::Vim, InputMode::Helix] {
        assert_eq!(
            closure_input::command_for(mode, "g c c"),
            Some("toggle-line-comment"),
            "{mode:?}"
        );
    }
    // Emacs has no `g` prefix; `M-;` is its own comment chord.
    assert_eq!(
        closure_input::command_for(InputMode::Emacs, "M-;"),
        Some("toggle-line-comment")
    );
}

#[test]
fn g_c_c_works_in_the_buffer_where_g_lives() {
    // The chord is what was asked for, and inside a buffer `g` belongs
    // to the vim engine rather than to the keymap dispatcher — so
    // binding it in the keymap alone would advertise a chord that did
    // nothing where you would press it.
    let (_d, mut shell, mut app) = app();
    caret_on(&mut app, &mut shell, "JS Comment");
    for k in ["g", "c", "c"] {
        app.on_key(&mut shell, k, false, false, k.chars().next());
    }
    let line = app
        .body_buffer()
        .lines()
        .find(|l| l.contains("JS Comment"))
        .unwrap_or_default();
    assert_eq!(line, "// console.log(\"JS Comment\")");
}

#[test]
fn g_c_over_a_visual_selection_fires_at_once() {
    let (_d, mut shell, mut app) = app();
    caret_on(&mut app, &mut shell, "function foo()");
    app.on_key(&mut shell, "v", false, false, Some('v'));
    app.on_key(&mut shell, "j", false, false, Some('j'));
    app.on_key(&mut shell, "j", false, false, Some('j'));
    app.on_key(&mut shell, "g", false, false, Some('g'));
    app.on_key(&mut shell, "c", false, false, Some('c'));
    let body = app.body_buffer();
    for want in ["// function foo() {", "//     console.log(\"foo\")", "// }"] {
        assert!(body.contains(want), "missing {want}:\n{body}");
    }
}

#[test]
fn a_half_typed_g_c_says_so() {
    // which-key reads this: a chord waiting on its second `c` has to
    // look like one, or `gc` is indistinguishable from a key that did
    // nothing.
    let (_d, mut shell, mut app) = app();
    caret_on(&mut app, &mut shell, "JS Comment");
    app.on_key(&mut shell, "g", false, false, Some('g'));
    app.on_key(&mut shell, "c", false, false, Some('c'));
    assert_eq!(app.body_pending_chord(), "gc");
}

#[test]
fn the_fence_lines_belong_to_org_not_to_the_block() {
    // `#+begin_src javascript` is org's own syntax, not JavaScript, so
    // commenting the fence with `//` produces a line that is neither.
    // `enclosing_src_block` counts the fences as part of the block on
    // purpose — that is how `edit-special` is reached with the point on
    // one — so the comment has to ask the narrower question.
    let (_d, mut shell, mut app) = app();
    let line = toggled(&mut app, &mut shell, "#+begin_src javascript");
    assert_eq!(line, "# #+begin_src javascript");
}

#[test]
fn the_line_just_inside_the_fence_still_gets_the_block() {
    let (_d, mut shell, mut app) = app();
    let line = toggled(&mut app, &mut shell, "console.log(\"JS Comment\")");
    assert_eq!(line, "// console.log(\"JS Comment\")");
}

#[test]
fn an_org_keyword_line_is_not_already_a_comment() {
    // `#+begin_src` starts with `#` and is not a comment — it is org
    // syntax. Reading it as one made the first `gcc` on a fence line
    // *uncomment* it, leaving `+begin_src javascript`: not a fence, not
    // a comment, and not something you would notice until the block
    // stopped being a block.
    let (_d, mut shell, mut app) = app();
    caret_on(&mut app, &mut shell, "#+begin_src javascript");
    app.run(&mut shell, "toggle-line-comment");
    assert!(
        app.body_buffer().contains("# #+begin_src javascript"),
        "the fence was not commented:\n{}",
        app.body_buffer()
    );
    app.run(&mut shell, "toggle-line-comment");
    assert!(
        app.body_buffer().contains("\n#+begin_src javascript"),
        "the fence did not come back intact:\n{}",
        app.body_buffer()
    );
}
