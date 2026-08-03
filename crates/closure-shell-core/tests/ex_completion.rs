//! "ex mode autocompletion"
//!
//! The `:` line took typing and Enter and nothing else: no TAB, no
//! candidates, no way to find out what it answers to short of reading
//! the source. It is a superset of the palette — every registry
//! command plus the vim set — and the palette has fuzzy matching and a
//! visible list, so the `:` line was the one surface where knowing the
//! name exactly was the price of entry.
//!
//! TAB completes, as it does in every shell and in vim's own cmdline.
//! The candidates come from the command registry rather than a second
//! list, so a command added anywhere is completable here the same day.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQEXCO00000000000001\n:END:\nbody\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQEXCO00000000000001"));
    (dir, shell, app)
}

/// Open `:` and type `text` into it.
fn ex(text: &str) -> (TempDir, Shell, ModalApp) {
    let (dir, mut shell, mut app) = fixture();
    app.run(&mut shell, "ex-command");
    assert_eq!(app.surface(), ModalSurface::Ex);
    for c in text.chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    (dir, shell, app)
}

fn tab(app: &mut ModalApp, shell: &mut Shell) {
    app.on_key(shell, "tab", false, false, None);
}

#[test]
fn tab_completes_a_unique_prefix() {
    let (_d, mut shell, mut app) = ex("open-con");
    tab(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("open-config"));
}

#[test]
fn the_candidates_come_from_the_registry() {
    // Not a second list. A command added anywhere is completable here
    // the same day, or the `:` line starts lying about what it knows.
    let (_d, _sh, app) = ex("tog");
    let names: Vec<String> = app.ex_completions();
    assert!(!names.is_empty(), "nothing offered for `tog`");
    for name in &names {
        assert!(
            closure_shell_core::palette_command_names().contains(&name.as_str()),
            "{name} is not a registry command"
        );
    }
}

#[test]
fn tab_on_an_ambiguous_prefix_fills_what_is_certain() {
    // What a shell does: complete to the common prefix rather than
    // guessing which of several was meant.
    let (_d, mut shell, mut app) = ex("toggle-i");
    tab(&mut app, &mut shell);
    let typed = app.prompt_text().unwrap_or_default().to_owned();
    assert!(
        typed.starts_with("toggle-i"),
        "it went backwards: {typed:?}"
    );
    assert!(
        closure_shell_core::palette_command_names()
            .iter()
            .any(|n| n.starts_with(&typed)),
        "completed to something no command starts with: {typed:?}"
    );
}

#[test]
fn tab_again_walks_the_candidates() {
    // Once the common prefix is exhausted, TAB cycles — the vim
    // cmdline behaviour, and the only thing left for it to do.
    //
    // A prefix with *several* completions, which the first version of
    // this used not to have: `g` matches one command, and asking a
    // single candidate to cycle is asking it to become a different
    // command.
    let (_d, mut shell, mut app) = ex("toggle-");
    assert!(app.ex_completions().len() > 1, "{:?}", app.ex_completions());
    tab(&mut app, &mut shell);
    let first = app.prompt_text().unwrap_or_default().to_owned();
    tab(&mut app, &mut shell);
    let second = app.prompt_text().unwrap_or_default().to_owned();
    assert_ne!(first, second, "TAB twice offered the same thing");
}

#[test]
fn a_lone_candidate_stays_put_however_often_you_press() {
    // The other half: there is nowhere to cycle to, and replacing it
    // with something else would be inventing a command.
    let (_d, mut shell, mut app) = ex("open-con");
    tab(&mut app, &mut shell);
    tab(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("open-config"));
}

#[test]
fn a_prefix_that_matches_nothing_leaves_the_line_alone() {
    // Silently clearing what somebody typed would be worse than doing
    // nothing.
    let (_d, mut shell, mut app) = ex("zzzznope");
    tab(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("zzzznope"));
}

#[test]
fn the_vim_lines_are_completable_too() {
    // `:w`, `:q`, `:wq` are the reason people open this line at all,
    // and they are not registry commands.
    let (_d, _sh, app) = ex("w");
    assert!(
        app.ex_completions().iter().any(|c| c == "wq"),
        "{:?}",
        app.ex_completions()
    );
}

#[test]
fn an_empty_line_offers_everything_rather_than_nothing() {
    // `:` then TAB is how you find out what there is.
    let (_d, _sh, app) = ex("");
    assert!(
        app.ex_completions().len() > 20,
        "{:?}",
        app.ex_completions()
    );
}

#[test]
fn completing_does_not_run_anything() {
    // TAB is not Enter. The one unforgivable outcome here is a command
    // executing because somebody was exploring.
    let (_d, mut shell, mut app) = ex("quit");
    tab(&mut app, &mut shell);
    assert_eq!(app.surface(), ModalSurface::Ex, "it ran the command");
}
