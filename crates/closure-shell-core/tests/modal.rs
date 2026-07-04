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

// Contract revised 2026-07-04 (vim-modal editor): Esc in INSERT drops
// to NORMAL for navigation; Esc in NORMAL cancels the edit.
#[test]
fn body_editor_escape_goes_normal_then_cancels() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in "scratch".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody, "INSERT Esc -> NORMAL");
    assert_eq!(app.body_mode(), closure_shell_core::EditorMode::Normal);
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse, "NORMAL Esc cancels");
    assert_eq!(app.detail(&sh).expect("d").body.trim(), "");
}

#[test]
fn body_normal_mode_navigates_and_edits_vim_style() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in "ab\ncd".chars() {
        if c == '\n' {
            app.on_key(&mut sh, "enter", false, false, None);
        } else {
            app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
        }
    }
    app.on_key(&mut sh, "escape", false, false, None); // NORMAL, cursor at end
    assert_eq!(app.body_cursor(), (1, 2), "line 1 col 2 after 'cd'");
    app.on_key(&mut sh, "k", false, false, Some('k')); // up
    assert_eq!(app.body_cursor().0, 0);
    app.on_key(&mut sh, "h", false, false, Some('h')); // left
    app.on_key(&mut sh, "h", false, false, Some('h')); // clamp at col 0
    assert_eq!(app.body_cursor(), (0, 0));
    app.on_key(&mut sh, "x", false, false, Some('x')); // delete 'a'
    assert_eq!(app.body_buffer(), "b\ncd");
    app.on_key(&mut sh, "i", false, false, Some('i')); // back to INSERT
    app.on_key(&mut sh, "z", false, false, Some('z')); // inserts AT cursor
    assert_eq!(app.body_buffer(), "zb\ncd", "insert at cursor, not append");
    app.on_key(&mut sh, "enter", true, false, None); // C-Enter commits
    assert_eq!(app.detail(&sh).expect("d").body.trim_end(), "zb\ncd");
}

#[test]
fn body_normal_o_opens_line_below_in_insert() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in "top".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "o", false, false, Some('o'));
    assert_eq!(app.body_mode(), closure_shell_core::EditorMode::Insert);
    for c in "below".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    assert_eq!(app.body_buffer(), "top\nbelow");
}

#[test]
fn emacs_binds_edit_body_to_c_c_e() {
    use closure_input::chord_for_command;
    assert_eq!(
        chord_for_command(InputMode::Emacs, "edit-body"),
        Some("C-c e")
    );
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
    assert!(
        comp.iter()
            .any(|(rest, cmd)| rest == "g" && cmd == "first-file"),
        "{comp:?}"
    );
    assert!(
        comp.iter()
            .any(|(rest, cmd)| rest == "a" && cmd == "agenda"),
        "{comp:?}"
    );
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
    assert!(
        comp.iter()
            .any(|(rest, cmd)| rest == "C-c" && cmd == "quit"),
        "{comp:?}"
    );
    assert!(
        comp.iter().any(|(rest, cmd)| rest == "u" && cmd == "undo"),
        "{comp:?}"
    );
    assert!(
        comp.iter().any(|(rest, cmd)| rest == "r" && cmd == "redo"),
        "{comp:?}"
    );
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
    assert_eq!(
        titles,
        vec!["Ship it", "Pay rent"],
        "sorted by date: {rows:?}"
    );
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
    assert!(
        rows.iter()
            .any(|(_, lang, first)| lang == "rust" && first.contains("fn main"))
    );
    assert!(
        rows.iter()
            .any(|(_, lang, first)| lang == "python" && first.contains("print(1)"))
    );
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

// === R1 click-to-jump: activating an agenda/block row returns to
// Browse with the target headline/file selected. ===

#[test]
fn agenda_enter_jumps_to_the_headline() {
    let (_d, mut sh) = agenda_shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "g", false, false, Some('g'));
    app.on_key(&mut sh, "a", false, false, Some('a')); // agenda; row 0 = "Ship it"
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
    let rows = app.rows(&sh);
    assert_eq!(rows[app.selected()].title, "Ship it");
}

#[test]
fn blocks_enter_jumps_to_the_file() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("a.org"), "* Aye\nprose\n").expect("w");
    fs::write(
        dir.path().join("b.org"),
        "* Bee\n#+BEGIN_SRC rust\nfn x() {}\n#+END_SRC\n",
    )
    .expect("w");
    let v = Vault::open(dir.path()).expect("open");
    let mut sh = Shell::new(v);
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "e", false, false, Some('e')); // block-list (one block, in b.org)
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
    let rows = app.rows(&sh);
    assert!(
        rows[app.selected()].path.ends_with("b.org"),
        "jumped to block's file"
    );
}

#[test]
fn list_jump_out_of_range_is_safe() {
    let (_d, mut sh) = agenda_shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "g", false, false, Some('g'));
    app.on_key(&mut sh, "a", false, false, Some('a'));
    app.jump_list_row(&sh, 999); // no panic, returns to Browse
    assert_eq!(app.surface(), ModalSurface::Browse);
}

// === R2a modal-mode field editing: TODO-cycle + priority-cycle on the
// command surface, committed through the Vault (I8). ===

#[test]
fn vim_t_cycles_todo_on_selection() {
    let (_d, mut sh) = shell(); // row 0 = "* TODO Ship parser"
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "t", false, false, Some('t')); // toggle-todo
    assert_eq!(app.detail(&sh).unwrap().todo.as_deref(), Some("DONE"));
    app.on_key(&mut sh, "t", false, false, Some('t'));
    assert_eq!(app.detail(&sh).unwrap().todo, None);
}

#[test]
fn vim_p_cycles_priority_on_selection() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "p", false, false, Some('p')); // cycle-priority None->A
    assert_eq!(app.detail(&sh).unwrap().priority, Some('A'));
    app.on_key(&mut sh, "p", false, false, Some('p'));
    assert_eq!(app.detail(&sh).unwrap().priority, Some('B'));
}

#[test]
fn emacs_c_c_t_cycles_todo() {
    use closure_input::chord_for_command;
    assert_eq!(
        chord_for_command(InputMode::Emacs, "toggle-todo"),
        Some("C-c t")
    );
    assert_eq!(
        chord_for_command(InputMode::Vim, "cycle-priority"),
        Some("p")
    );
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Emacs);
    app.on_key(&mut sh, "c", true, false, None); // C-c
    app.on_key(&mut sh, "t", false, false, Some('t')); // C-c t -> toggle-todo
    assert_eq!(app.detail(&sh).unwrap().todo.as_deref(), Some("DONE"));
}

// === R2b modal-mode tag + property editing (single-line buffers fed
// via on_key, committed through the Vault, I8). ===

#[test]
fn modal_edit_tags_commits_split_on_whitespace() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "y", false, false, Some('y')); // edit-tags
    assert_eq!(app.surface(), ModalSurface::TagsEdit);
    for c in "work urgent".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert_eq!(
        app.detail(&sh).unwrap().tags,
        vec!["work".to_owned(), "urgent".to_owned()]
    );
}

#[test]
fn modal_edit_property_commits_key_value() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "o", false, false, Some('o')); // edit-property
    assert_eq!(app.surface(), ModalSurface::PropertyEdit);
    for c in "Status active".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "enter", false, false, None);
    let props = app.detail(&sh).unwrap().properties;
    assert!(
        props.iter().any(|(k, v)| k == "Status" && v == "active"),
        "{props:?}"
    );
}

#[test]
fn modal_edit_tags_escape_cancels() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "y", false, false, Some('y'));
    app.on_key(&mut sh, "x", false, false, Some('x'));
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert!(app.detail(&sh).unwrap().tags.is_empty());
}

#[test]
fn doom_z_toggles_the_fold_on_the_command_surface() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Top\n** Child\n*** Grandchild\n* Other\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    let mut sh = Shell::new(v);
    let mut app = ModalApp::new(InputMode::Doom);
    assert_eq!(app.rows(&sh).len(), 4);
    app.on_key(&mut sh, "z", false, false, Some('z'));
    let titles: Vec<String> = app.rows(&sh).iter().map(|r| r.title.clone()).collect();
    assert_eq!(titles, vec!["Top", "Other"], "z folds the selected subtree");
    app.on_key(&mut sh, "z", false, false, Some('z'));
    assert_eq!(app.rows(&sh).len(), 4, "z again unfolds");
}

// === GPUI-REF: the modal surface becomes the reference GUI's editor
// core. Mouse clicks need a public command entry; a top-notch editor
// needs undo/redo, rename, add-sibling, delete, and a palette on the
// command surface; which-key needs structured, clickable items. ===

fn nested() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO Ship parser\n** Subtask\n* Personal wiki\n* DONE Write spec\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

#[test]
fn public_run_drives_a_command_like_a_key() {
    // The mouse path: clicking a which-key item runs the same command
    // the chord would (I8 — one dispatch path, no shell-private verbs).
    let (_d, mut sh) = nested();
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut sh, "toggle-todo");
    let detail = app.detail(&sh).expect("detail");
    assert_eq!(detail.todo.as_deref(), Some("DONE"), "TODO flipped by run()");
}

#[test]
fn delete_removes_the_selected_subtree() {
    let (_d, mut sh) = nested();
    let mut app = ModalApp::new(InputMode::Doom);
    assert_eq!(app.rows(&sh).len(), 4);
    app.run(&mut sh, "delete");
    let titles: Vec<String> = app.rows(&sh).iter().map(|r| r.title.clone()).collect();
    assert!(!titles.contains(&"Ship parser".to_owned()));
    assert!(!titles.contains(&"Subtask".to_owned()), "subtree went with it");
}

#[test]
fn undo_and_redo_work_on_the_command_surface() {
    let (_d, mut sh) = nested();
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut sh, "delete");
    assert_eq!(app.rows(&sh).len(), 2);
    app.run(&mut sh, "undo");
    assert_eq!(app.rows(&sh).len(), 4, "undo restores the subtree");
    app.run(&mut sh, "redo");
    assert_eq!(app.rows(&sh).len(), 2, "redo re-applies the delete");
}

#[test]
fn rename_surface_prefills_and_commits() {
    let (_d, mut sh) = nested();
    let mut app = ModalApp::new(InputMode::Doom);
    app.on_key(&mut sh, "r", false, false, Some('r'));
    assert_eq!(app.surface(), ModalSurface::Rename);
    assert_eq!(app.field_buffer(), "Ship parser", "prefilled with the title");
    for _ in 0.."Ship parser".len() {
        app.on_key(&mut sh, "backspace", false, false, None);
    }
    for c in "Shipped".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert!(app.rows(&sh).iter().any(|r| r.title == "Shipped"));
}

#[test]
fn add_sibling_surface_inserts_after_selected() {
    let (_d, mut sh) = nested();
    let mut app = ModalApp::new(InputMode::Doom);
    app.on_key(&mut sh, "a", false, false, Some('a'));
    assert_eq!(app.surface(), ModalSurface::AddSibling);
    for c in "Brand new".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "enter", false, false, None);
    assert!(app.rows(&sh).iter().any(|r| r.title == "Brand new"));
}

#[test]
fn palette_on_the_command_surface_filters_and_runs() {
    let (_d, mut sh) = nested();
    let mut app = ModalApp::new(InputMode::Doom);
    app.on_key(&mut sh, ":", false, false, Some(':'));
    assert_eq!(app.surface(), ModalSurface::Palette);
    for c in "fold".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    let entries = app.palette_entries();
    assert!(
        entries.iter().any(|e| e.label == "fold"),
        "typed filter narrows the palette: {entries:?}"
    );
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert_eq!(app.rows(&sh).len(), 3, "running fold hid Subtask");
}

#[test]
fn palette_escape_cancels() {
    let (_d, mut sh) = nested();
    let mut app = ModalApp::new(InputMode::Doom);
    app.on_key(&mut sh, ":", false, false, Some(':'));
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}

#[test]
fn hint_items_mirror_the_mode_keymap() {
    // Structured which-key data a GUI renders as clickable chips: the
    // exact keymap pairs, never a hand-maintained list (I4).
    let app = ModalApp::new(InputMode::Doom);
    let items = app.hint_items();
    let km = closure_input::mode_keymap(InputMode::Doom);
    assert_eq!(items.len(), km.len());
    assert!(items.iter().any(|(c, cmd)| c == "z" && cmd == "toggle-fold"));
}

#[test]
fn row_fold_state_is_queryable_for_the_renderer() {
    let (_d, mut sh) = nested();
    let mut app = ModalApp::new(InputMode::Doom);
    let id = app.rows(&sh)[0].id.clone();
    assert!(!closure_shell_core::is_row_folded(&sh, &id));
    app.run(&mut sh, "toggle-fold");
    assert!(closure_shell_core::is_row_folded(&sh, &id), "▸ marker source");
}

#[test]
fn palette_click_runs_the_clicked_entry() {
    // Mouse path for the palette: clicking row i runs that entry —
    // same commit as Enter on a moved cursor.
    let (_d, mut sh) = nested();
    let mut app = ModalApp::new(InputMode::Doom);
    app.on_key(&mut sh, ":", false, false, Some(':'));
    let entries = app.palette_entries();
    let fold_at = entries
        .iter()
        .position(|e| e.label == "fold")
        .expect("fold entry");
    app.palette_click(&mut sh, fold_at);
    assert_eq!(app.surface(), ModalSurface::Browse, "palette closed");
    assert_eq!(app.rows(&sh).len(), 3, "clicked fold ran");
}

#[test]
fn org_tempo_s_tab_expands_to_src_block() {
    // Emacs org-tempo: `<s` + TAB becomes a source block, cursor after
    // `#+BEGIN_SRC ` so the language can be typed immediately.
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in "<s".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "tab", false, false, None);
    assert_eq!(app.body_buffer(), "#+BEGIN_SRC \n\n#+END_SRC");
    assert_eq!(app.body_cursor(), (0, 12), "cursor after BEGIN_SRC ");
    for c in "rust".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    assert_eq!(app.body_buffer(), "#+BEGIN_SRC rust\n\n#+END_SRC");
}

#[test]
fn org_tempo_expands_the_other_block_templates() {
    let cases = [
        ("<e", "#+BEGIN_EXAMPLE\n\n#+END_EXAMPLE"),
        ("<q", "#+BEGIN_QUOTE\n\n#+END_QUOTE"),
        ("<c", "#+BEGIN_CENTER\n\n#+END_CENTER"),
        ("<C", "#+BEGIN_COMMENT\n\n#+END_COMMENT"),
        ("<v", "#+BEGIN_VERSE\n\n#+END_VERSE"),
    ];
    for (trigger, expanded) in cases {
        let (_d, mut sh) = shell();
        let mut app = ModalApp::new(InputMode::Vim);
        app.on_key(&mut sh, "i", false, false, Some('i'));
        for c in trigger.chars() {
            app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
        }
        app.on_key(&mut sh, "tab", false, false, None);
        assert_eq!(app.body_buffer(), expanded, "{trigger}");
    }
}

#[test]
fn tab_without_template_indents() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in "x".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "tab", false, false, None);
    assert_eq!(app.body_buffer(), "x  ", "plain TAB = two-space indent");
}

// === Completion in the body editor: org keywords + dabbrev over the
// vault's words, cycled with C-n / C-p (Emacs dabbrev feel). ===

#[test]
fn c_n_completes_org_keywords() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in "TO".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "n", true, false, None); // C-n
    assert_eq!(app.body_buffer(), "TODO");
}

#[test]
fn c_n_completes_words_from_the_vault_dabbrev_style() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in "Pers".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "n", true, false, None);
    assert_eq!(app.body_buffer(), "Personal", "word mined from vault titles");
}

#[test]
fn c_n_cycles_and_c_p_cycles_back() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* alpha one\n* alphabet two\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    let mut sh = Shell::new(v);
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in "alph".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "n", true, false, None);
    assert_eq!(app.body_buffer(), "alpha");
    app.on_key(&mut sh, "n", true, false, None);
    assert_eq!(app.body_buffer(), "alphabet");
    app.on_key(&mut sh, "p", true, false, None); // C-p back
    assert_eq!(app.body_buffer(), "alpha");
    // Typing resumes normally and ends the completion session.
    app.on_key(&mut sh, "!", false, false, Some('!'));
    assert_eq!(app.body_buffer(), "alpha!");
}

// === Visual mode, registers, and readline chords in the body editor. ===

fn editor_with(sh: &mut Shell, text: &str) -> ModalApp {
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(sh, "i", false, false, Some('i'));
    for c in text.chars() {
        if c == '\n' {
            app.on_key(sh, "enter", false, false, None);
        } else {
            app.on_key(sh, &c.to_string(), false, false, Some(c));
        }
    }
    app
}

#[test]
fn visual_mode_selects_yanks_and_pastes() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "abc");
    app.on_key(&mut sh, "escape", false, false, None); // NORMAL, cursor col 3 -> clamp
    app.on_key(&mut sh, "0", false, false, Some('0'));
    app.on_key(&mut sh, "v", false, false, Some('v')); // VISUAL, anchor at 'a'
    assert_eq!(app.body_mode(), closure_shell_core::EditorMode::Visual);
    app.on_key(&mut sh, "l", false, false, Some('l'));
    app.on_key(&mut sh, "l", false, false, Some('l')); // cover "abc" (inclusive)
    app.on_key(&mut sh, "y", false, false, Some('y')); // yank -> NORMAL
    assert_eq!(app.body_mode(), closure_shell_core::EditorMode::Normal);
    assert_eq!(app.body_buffer(), "abc", "yank leaves text");
    app.on_key(&mut sh, "$", false, false, Some('$'));
    app.on_key(&mut sh, "p", false, false, Some('p')); // paste after cursor
    assert_eq!(app.body_buffer(), "abcabc");
}

#[test]
fn visual_d_deletes_the_selection_into_the_register() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "hello");
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "0", false, false, Some('0'));
    app.on_key(&mut sh, "v", false, false, Some('v'));
    app.on_key(&mut sh, "l", false, false, Some('l')); // "he"
    app.on_key(&mut sh, "d", false, false, Some('d'));
    assert_eq!(app.body_buffer(), "llo");
    app.on_key(&mut sh, "p", false, false, Some('p')); // paste "he" after 'l'
    assert_eq!(app.body_buffer(), "lhelo");
}

#[test]
fn dd_deletes_the_line_and_p_pastes_it_below() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "one\ntwo\nthree");
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "k", false, false, Some('k')); // line 1 ("two")
    app.on_key(&mut sh, "d", false, false, Some('d'));
    app.on_key(&mut sh, "d", false, false, Some('d')); // dd
    assert_eq!(app.body_buffer(), "one\nthree");
    app.on_key(&mut sh, "j", false, false, Some('j')); // line "three"
    app.on_key(&mut sh, "p", false, false, Some('p')); // linewise paste below
    assert_eq!(app.body_buffer(), "one\nthree\ntwo");
}

#[test]
fn yy_yanks_the_line_without_deleting() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "solo");
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "y", false, false, Some('y'));
    app.on_key(&mut sh, "y", false, false, Some('y')); // yy
    assert_eq!(app.body_buffer(), "solo");
    app.on_key(&mut sh, "p", false, false, Some('p'));
    assert_eq!(app.body_buffer(), "solo\nsolo");
}

#[test]
fn insert_readline_chords_move_and_kill() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "hello world");
    app.on_key(&mut sh, "a", true, false, None); // C-a -> line start
    assert_eq!(app.body_cursor(), (0, 0));
    app.on_key(&mut sh, "f", true, false, None); // C-f -> right
    assert_eq!(app.body_cursor(), (0, 1));
    app.on_key(&mut sh, "e", true, false, None); // C-e -> line end
    assert_eq!(app.body_cursor(), (0, 11));
    app.on_key(&mut sh, "w", true, false, None); // C-w -> delete word back
    assert_eq!(app.body_buffer(), "hello ");
    app.on_key(&mut sh, "a", true, false, None);
    app.on_key(&mut sh, "k", true, false, None); // C-k -> kill to line end
    assert_eq!(app.body_buffer(), "");
    app.on_key(&mut sh, "y", true, false, None); // C-y -> yank the kill back
    assert_eq!(app.body_buffer(), "hello ");
    app.on_key(&mut sh, "u", true, false, None); // C-u -> kill to line start
    assert_eq!(app.body_buffer(), "");
}

#[test]
fn completion_prefix_is_case_insensitive_dabbrev_style() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "pers");
    app.on_key(&mut sh, "n", true, false, None); // C-n
    assert_eq!(app.body_buffer(), "Personal", "case-insensitive match, candidate case wins");
}

// === L1: Visual selection accessor for the renderer. ===

#[test]
fn body_selection_is_none_outside_visual() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "hello");
    assert_eq!(app.body_selection(), None, "INSERT");
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.body_selection(), None, "NORMAL");
}

#[test]
fn body_selection_reports_the_inclusive_byte_range() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "hello");
    app.on_key(&mut sh, "escape", false, false, None); // Normal
    app.on_key(&mut sh, "0", false, false, Some('0'));
    app.on_key(&mut sh, "v", false, false, Some('v')); // Visual
    app.on_key(&mut sh, "l", false, false, Some('l')); // cursor on 'e'
    assert_eq!(app.body_selection(), Some((0, 2)));
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.body_selection(), None);
}
