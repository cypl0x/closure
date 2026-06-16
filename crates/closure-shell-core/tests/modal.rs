//! Modal command-surface state machine (the "modal GUI" experiment):
//! keys resolve against closure-input::mode_keymap, so the five editing
//! modes drive a GUI command surface (vim j/k, g g, emacs C-x C-c),
//! with a search overlay for typing. Proves the modes work outside the
//! TUI; fully headless. Browse never types-to-filter (unlike the
//! launcher App) — that's the whole point.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO Ship parser\n* Personal wiki\n* DONE Write spec\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

#[test]
fn vim_jk_navigate_on_the_command_surface() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "j", false, false, Some('j'));
    assert_eq!(app.selected(), 1);
    assert_eq!(app.query(), "", "browse is a command surface, not a filter");
    app.on_key(&mut sh, "k", false, false, Some('k'));
    assert_eq!(app.selected(), 0);
}

#[test]
fn vim_gg_and_capital_g_jump() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "G", false, false, Some('G'));
    assert_eq!(app.selected(), 2, "G -> last");
    app.on_key(&mut sh, "g", false, false, Some('g')); // pending
    app.on_key(&mut sh, "g", false, false, Some('g')); // g g -> first
    assert_eq!(app.selected(), 0);
}

#[test]
fn emacs_ctrl_nav_and_ctrl_x_ctrl_c_quit() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Emacs);
    app.on_key(&mut sh, "n", true, false, None); // C-n
    assert_eq!(app.selected(), 1);
    app.on_key(&mut sh, "p", true, false, None); // C-p
    assert_eq!(app.selected(), 0);
    app.on_key(&mut sh, "x", true, false, None); // C-x (pending)
    assert!(!app.should_quit());
    app.on_key(&mut sh, "c", true, false, None); // C-x C-c -> quit
    assert!(app.should_quit());
}

#[test]
fn emacs_meta_less_jumps_first() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Emacs);
    app.on_key(&mut sh, "n", true, false, None);
    app.on_key(&mut sh, "n", true, false, None);
    assert_eq!(app.selected(), 2);
    app.on_key(&mut sh, "<", false, true, Some('<')); // M-< -> first
    assert_eq!(app.selected(), 0);
}

#[test]
fn browse_ignores_unbound_printable_no_filter() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "z", false, false, Some('z')); // unbound in vim
    assert_eq!(app.query(), "");
    assert_eq!(app.selected(), 0);
    assert_eq!(app.surface(), ModalSurface::Browse);
}

#[test]
fn search_overlay_types_filters_and_picks() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "/", false, false, Some('/')); // search-start
    assert_eq!(app.surface(), ModalSurface::Search);
    for c in "wiki".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    assert_eq!(app.query(), "wiki");
    let rows = app.rows(&sh);
    assert!(rows.iter().any(|r| r.title == "Personal wiki"));
    assert!(!rows.iter().any(|r| r.title == "Ship parser"));
    app.on_key(&mut sh, "enter", false, false, None); // pick -> back to Browse
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert_eq!(app.query(), "", "query cleared after picking");
}

#[test]
fn search_escape_cancels_back_to_browse() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "/", false, false, Some('/'));
    app.on_key(&mut sh, "x", false, false, Some('x'));
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert_eq!(app.query(), "");
}

#[test]
fn capture_via_command_then_commit() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "c", false, false, Some('c')); // capture-start
    assert_eq!(app.surface(), ModalSurface::Capture);
    for c in "Modal task".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert!(sh.vault.find_by_title("Modal task").is_some());
}

#[test]
fn cycle_mode_rotates() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    // Doom binds "M" -> cycle-mode (bare, fine on a command surface).
    let start = format!("{:?}", app.input_mode());
    app.on_key(&mut sh, "M", false, false, Some('M'));
    assert_ne!(format!("{:?}", app.input_mode()), start);
}

#[test]
fn key_hints_list_real_bindings_for_the_mode() {
    let app = ModalApp::new(InputMode::Emacs);
    let hints = app.key_hints();
    assert!(hints.contains("C-n"), "emacs hints show C-n: {hints}");
    assert!(hints.contains("quit"));
}

// === draw-parity accessors (G2): the egui/gpui window draws ModalApp
// the same way it draws App — detail pane, mouse-click select, and a
// paged viewport. Pure + headless, so they are tested here, not in the
// window. ===

#[test]
fn detail_resolves_selected_headline() {
    let (_d, sh) = shell();
    let app = ModalApp::new(InputMode::Vim);
    let d = app.detail(&sh).expect("first row has a detail");
    assert_eq!(d.title, "Ship parser");
    assert_eq!(d.todo.as_deref(), Some("TODO"));
}

#[test]
fn detail_follows_selection() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "j", false, false, Some('j')); // -> row 1
    let d = app.detail(&sh).expect("detail");
    assert_eq!(d.title, "Personal wiki");
}

#[test]
fn select_clamps_to_last_row() {
    let (_d, sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.select(99, &sh); // 3 rows -> clamp to index 2
    assert_eq!(app.selected(), 2);
    let d = app.detail(&sh).expect("detail");
    assert_eq!(d.title, "Write spec");
}

#[test]
fn view_window_pages_and_keeps_selection_visible() {
    let (_d, sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    // page larger than row count -> offset 0, all rows.
    let (off, rows) = app.view_window(&sh, 10);
    assert_eq!(off, 0);
    assert_eq!(rows.len(), 3);
    // page of 1, selection on last row -> offset tracks selection.
    app.select(2, &sh);
    let (off, rows) = app.view_window(&sh, 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(off, 2);
    assert_eq!(rows[0].title, "Write spec");
}

// === E1 body editor on the modal surface (org-edit-special). ===

#[test]
fn vim_i_enters_body_editor_and_commits() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i')); // edit-body
    assert_eq!(app.surface(), ModalSurface::EditBody);
    for c in "note".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "enter", true, false, None); // C-Enter commit
    assert_eq!(app.surface(), ModalSurface::Browse);
    let d = app.detail(&sh).expect("detail");
    assert_eq!(d.body.trim_end(), "note");
}

#[test]
fn body_editor_escape_cancels() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    app.body_buffer_mut().push_str("scratch");
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert_eq!(app.detail(&sh).expect("d").body.trim(), "");
}

#[test]
fn emacs_binds_edit_body_to_c_c_e() {
    use closure_input::chord_for_command;
    assert_eq!(chord_for_command(InputMode::Emacs, "edit-body"), Some("C-c e"));
    assert_eq!(chord_for_command(InputMode::Vim, "edit-body"), Some("i"));
}

// === P1 which-key popup: while a multi-stroke chord is pending, expose
// the prefix + completions (remaining chord -> command) from the keymap.

#[test]
fn no_pending_chord_means_no_completions() {
    let app = ModalApp::new(InputMode::Vim);
    assert_eq!(app.pending_chord(), "");
    assert!(app.completions().is_empty());
}

#[test]
fn vim_g_prefix_lists_completions() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "g", false, false, Some('g')); // pending "g"
    assert_eq!(app.pending_chord(), "g");
    let comp = app.completions();
    // vim binds "g g" -> first-file and "g a" -> agenda.
    assert!(comp.iter().any(|(rest, cmd)| rest == "g" && cmd == "first-file"), "{comp:?}");
    assert!(comp.iter().any(|(rest, cmd)| rest == "a" && cmd == "agenda"), "{comp:?}");
    // single-stroke bindings like "j" are NOT completions of "g".
    assert!(!comp.iter().any(|(_, cmd)| cmd == "next-file"));
}

#[test]
fn emacs_ctrl_x_prefix_lists_completions() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Emacs);
    app.on_key(&mut sh, "x", true, false, None); // C-x pending
    assert_eq!(app.pending_chord(), "C-x");
    let comp = app.completions();
    assert!(comp.iter().any(|(rest, cmd)| rest == "C-c" && cmd == "quit"), "{comp:?}");
    assert!(comp.iter().any(|(rest, cmd)| rest == "u" && cmd == "undo"), "{comp:?}");
    assert!(comp.iter().any(|(rest, cmd)| rest == "r" && cmd == "redo"), "{comp:?}");
}

#[test]
fn completions_clear_after_chord_resolves() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "g", false, false, Some('g'));
    app.on_key(&mut sh, "g", false, false, Some('g')); // "g g" resolves -> first-file
    assert_eq!(app.pending_chord(), "");
    assert!(app.completions().is_empty());
}

// === Q1 search-headlines: the `s` binding (search-headline-start)
// opens the same headline-filter Search surface as `/`, not a no-op. ===

#[test]
fn s_opens_headline_search_and_filters() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim); // vim binds "s" -> search-headline-start
    app.on_key(&mut sh, "s", false, false, Some('s'));
    assert_eq!(app.surface(), ModalSurface::Search);
    for c in "spec".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    let rows = app.rows(&sh);
    assert!(rows.iter().any(|r| r.title == "Write spec"), "{rows:?}");
    assert!(!rows.iter().any(|r| r.title == "Ship parser"));
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert_eq!(app.query(), "");
}

#[test]
fn s_search_with_no_hits_lists_nothing() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "s", false, false, Some('s'));
    for c in "zzzzz".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    assert!(app.rows(&sh).is_empty(), "no headline matches zzzzz");
}

// === Q2 backlinks: a read-only pane listing headlines that link to the
// selected one (Vault backlink index). ===

fn linked_shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Target\n:PROPERTIES:\n:ID: 01TARGET0000000000000000\n:END:\n\
         * Source\n:PROPERTIES:\n:ID: 01SOURCE0000000000000000\n:END:\n\
         See [[id:01TARGET0000000000000000]].\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

#[test]
fn backlinks_lists_linking_headline() {
    let (_d, mut sh) = linked_shell();
    let mut app = ModalApp::new(InputMode::Vim); // selection 0 = Target
    app.on_key(&mut sh, "b", false, false, Some('b')); // backlinks
    assert_eq!(app.surface(), ModalSurface::Backlinks);
    let rows = app.backlink_rows(&sh);
    assert!(rows.iter().any(|(_, title)| title == "Source"), "{rows:?}");
}

#[test]
fn backlinks_of_unlinked_headline_is_empty() {
    let (_d, mut sh) = linked_shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "j", false, false, Some('j')); // select Source (no incoming links)
    app.on_key(&mut sh, "b", false, false, Some('b'));
    assert!(app.backlink_rows(&sh).is_empty());
}

#[test]
fn backlinks_escape_returns_to_browse() {
    let (_d, mut sh) = linked_shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "b", false, false, Some('b'));
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}

// === Q3 agenda: a read-only pane of scheduled/deadline entries. ===

fn agenda_shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO Ship it\nSCHEDULED: <2026-06-20 Sat>\n* Plain\n* Pay rent\nDEADLINE: <2026-06-25 Thu>\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

#[test]
fn agenda_lists_scheduled_and_deadline_sorted_by_date() {
    let (_d, mut sh) = agenda_shell();
    let mut app = ModalApp::new(InputMode::Vim); // "g a" -> agenda
    app.on_key(&mut sh, "g", false, false, Some('g'));
    app.on_key(&mut sh, "a", false, false, Some('a'));
    assert_eq!(app.surface(), ModalSurface::Agenda);
    let rows = app.agenda_rows(&sh);
    let titles: Vec<&str> = rows.iter().map(|(_, t, _)| t.as_str()).collect();
    assert_eq!(titles, vec!["Ship it", "Pay rent"], "sorted by date: {rows:?}");
    assert!(rows[0].0.contains("2026-06-20"), "date carried: {rows:?}");
}

#[test]
fn agenda_on_empty_vault_is_empty() {
    let dir = tempfile::tempdir().expect("tmp");
    let v = Vault::open(dir.path()).expect("open");
    let mut sh = Shell::new(v);
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "g", false, false, Some('g'));
    app.on_key(&mut sh, "a", false, false, Some('a'));
    assert!(app.agenda_rows(&sh).is_empty());
}

#[test]
fn agenda_escape_returns_to_browse() {
    let (_d, mut sh) = agenda_shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "g", false, false, Some('g'));
    app.on_key(&mut sh, "a", false, false, Some('a'));
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}

// === Q4 block-list: a read-only pane of every #+BEGIN_SRC block. ===

fn blocks_shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* A\n#+BEGIN_SRC rust\nfn main() {}\n#+END_SRC\n\
         * B\n#+BEGIN_SRC python\nprint(1)\n#+END_SRC\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

#[test]
fn block_list_enumerates_code_blocks_with_language() {
    let (_d, mut sh) = blocks_shell();
    let mut app = ModalApp::new(InputMode::Vim); // "e" -> block-list
    app.on_key(&mut sh, "e", false, false, Some('e'));
    assert_eq!(app.surface(), ModalSurface::Blocks);
    let rows = app.block_rows(&sh);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows.iter().any(|(_, lang, first)| lang == "rust" && first.contains("fn main")));
    assert!(rows.iter().any(|(_, lang, first)| lang == "python" && first.contains("print(1)")));
}

#[test]
fn block_list_empty_when_no_blocks() {
    let (_d, mut sh) = shell(); // no src blocks
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "e", false, false, Some('e'));
    assert!(app.block_rows(&sh).is_empty());
}

#[test]
fn block_list_escape_returns_to_browse() {
    let (_d, mut sh) = blocks_shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "e", false, false, Some('e'));
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}
