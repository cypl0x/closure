//! "Since we expose the commands to the user and via MCP server …
//! Even if we skip variables, we have to pass through arguments to a
//! function/command."
//!
//! Commands were names and nothing else. `capture` opened a bar to
//! type a title into; there was no way to *say* the title. That is
//! fine for a chord — a chord is a name — and it is not fine for
//! everything else that runs a command: a cron line, a `:` line, a
//! key bound in config.org, an agent on the other end of the bridge.
//! Each of those knows what it wants and had nowhere to put it.
//!
//! So one argument mechanism at the one door every command comes
//! through: `<name> <rest of the line>`. A name with no space in it
//! behaves exactly as it did.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;

use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

const ORG: &str = "\
* Alpha
:PROPERTIES:
:ID: 01ARGS0000000000000000001
:END:
body of alpha
* Beta
:PROPERTIES:
:ID: 01ARGS0000000000000000002
:END:
";

/// Everything written under the vault, in one string — `capture` files
/// where the crumbs point, which is the file being looked at rather
/// than always the inbox, and that is the same target the capture bar
/// uses. What matters here is that the title reached the vault.
fn vault_text(dir: &std::path::Path) -> String {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| std::fs::read_to_string(e.ok()?.path()).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

fn app() -> (tempfile::TempDir, ModalApp, Shell) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, ModalApp::new(InputMode::Doom), shell)
}

#[test]
fn capture_can_be_told_the_title() {
    let (dir, mut app, mut shell) = app();
    app.run(&mut shell, "capture Weekly review");
    let disk = vault_text(dir.path());
    assert!(
        disk.contains("Weekly review"),
        "the title went nowhere:\n{disk}\nstatus: {}",
        app.status()
    );
}

#[test]
fn capture_with_no_argument_still_opens_the_bar() {
    // The chord is a name, and a name has to keep meaning what it did.
    let (_dir, mut app, mut shell) = app();
    app.run(&mut shell, "capture");
    assert!(
        app.surface() == ModalSurface::Capture,
        "the capture bar stopped opening for the chord that opens it"
    );
}

#[test]
fn search_can_be_told_what_to_search_for() {
    let (_dir, mut app, mut shell) = app();
    app.run(&mut shell, "search Beta");
    let rows = app.rows(&shell);
    assert_eq!(
        rows.len(),
        1,
        "{:?}",
        rows.iter().map(|r| &r.title).collect::<Vec<_>>()
    );
    assert_eq!(rows[0].title, "Beta");
}

#[test]
fn goto_takes_the_id_of_the_row_to_select() {
    let (_dir, mut app, mut shell) = app();
    app.run(&mut shell, "goto 01ARGS0000000000000000002");
    assert_eq!(app.rows(&shell)[app.selected()].title, "Beta");
}

#[test]
fn rename_can_be_told_the_new_title() {
    let (dir, mut app, mut shell) = app();
    app.run(&mut shell, "goto 01ARGS0000000000000000001");
    app.run(&mut shell, "rename Alpha, renamed");
    let disk = std::fs::read_to_string(dir.path().join("notes.org")).unwrap();
    assert!(disk.contains("* Alpha, renamed"), "{disk}");
}

#[test]
fn an_argument_to_a_command_that_takes_none_says_so() {
    // Silently ignoring it is how a cron line runs for a week doing
    // something other than what it says.
    let (_dir, mut app, mut shell) = app();
    app.run(&mut shell, "next-file please");
    assert!(
        app.status().contains("next-file") && app.status().contains("argument"),
        "status was {:?}",
        app.status()
    );
}

#[test]
fn a_cron_line_can_carry_one() {
    // The case that made this worth doing: a job is a command line,
    // and until now it could only be a command *name*.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("jobs.org"),
        "* Jobs\n#+BEGIN_SRC cron\n0 9 * * * capture Daily standup\n#+END_SRC\n",
    )
    .unwrap();
    let mut shell = Shell::new(Vault::open(dir.path()).unwrap());
    let mut app = ModalApp::new(InputMode::Doom);
    let ran = app.cron_tick_at(&mut shell, 0, 9, 1, 1, 1);
    assert_eq!(ran, vec!["capture Daily standup".to_owned()]);
    let disk = vault_text(dir.path());
    assert!(disk.contains("Daily standup"), "{disk}");
}

#[test]
fn the_palette_runs_what_was_typed_when_it_carries_an_argument() {
    // "Since we expose the commands to the user": M-x is how a person
    // reaches a command by name, and it filtered a list of names. Typed
    // with an argument it fuzzy-matched `capture` and ran the bare
    // form — the argument disappeared between the typing and the
    // running, which is worse than refusing it.
    let (dir, mut app, mut shell) = app();
    app.on_key(&mut shell, "x", false, true, None);
    assert_eq!(app.surface(), ModalSurface::Palette);
    for c in "capture Weekly review".chars() {
        app.on_key(&mut shell, "char", false, false, Some(c));
    }
    app.on_key(&mut shell, "enter", false, false, None);
    let disk = vault_text(dir.path());
    assert!(
        disk.contains("Weekly review"),
        "typed with an argument, ran without it. status: {}\n{disk}",
        app.status()
    );
}

#[test]
fn the_palette_still_picks_from_the_list_when_nothing_was_typed_after_the_name() {
    let (_dir, mut app, mut shell) = app();
    app.on_key(&mut shell, "x", false, true, None);
    for c in "capture".chars() {
        app.on_key(&mut shell, "char", false, false, Some(c));
    }
    app.on_key(&mut shell, "enter", false, false, None);
    assert_eq!(
        app.surface(),
        ModalSurface::Capture,
        "the bare name stopped opening the bar"
    );
}
