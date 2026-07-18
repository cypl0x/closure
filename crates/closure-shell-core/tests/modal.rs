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
    assert_eq!(
        app.surface(),
        ModalSurface::EditBody,
        "INSERT Esc -> NORMAL"
    );
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
    // Contract revised 2026-07-04 (Q1-E3 repair): Esc applies the vim
    // rule and steps back onto the last typed char.
    app.on_key(&mut sh, "escape", false, false, None); // NORMAL, on 'd'
    assert_eq!(app.body_cursor(), (1, 1), "line 1, on the 'd'");
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
    assert_eq!(
        detail.todo.as_deref(),
        Some("DONE"),
        "TODO flipped by run()"
    );
}

#[test]
fn delete_removes_the_selected_subtree() {
    let (_d, mut sh) = nested();
    let mut app = ModalApp::new(InputMode::Doom);
    assert_eq!(app.rows(&sh).len(), 4);
    app.run(&mut sh, "delete");
    let titles: Vec<String> = app.rows(&sh).iter().map(|r| r.title.clone()).collect();
    assert!(!titles.contains(&"Ship parser".to_owned()));
    assert!(
        !titles.contains(&"Subtask".to_owned()),
        "subtree went with it"
    );
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
    assert_eq!(
        app.field_buffer(),
        "Ship parser",
        "prefilled with the title"
    );
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
    assert!(
        items
            .iter()
            .any(|(c, cmd)| c == "z" && cmd == "toggle-fold")
    );
}

#[test]
fn row_fold_state_is_queryable_for_the_renderer() {
    let (_d, mut sh) = nested();
    let mut app = ModalApp::new(InputMode::Doom);
    let id = app.rows(&sh)[0].id.clone();
    assert!(!closure_shell_core::is_row_folded(&sh, &id));
    app.run(&mut sh, "toggle-fold");
    assert!(
        closure_shell_core::is_row_folded(&sh, &id),
        "▸ marker source"
    );
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
    assert_eq!(
        app.body_buffer(),
        "Personal",
        "word mined from vault titles"
    );
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
    assert_eq!(
        app.body_buffer(),
        "Personal",
        "case-insensitive match, candidate case wins"
    );
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

// === L2: backlink rows are click targets. ===

#[test]
fn backlink_click_jumps_like_enter() {
    let (_d, mut sh) = linked_shell();
    let mut app = ModalApp::new(InputMode::Vim); // selection 0 = Target
    app.on_key(&mut sh, "b", false, false, Some('b')); // open backlinks
    assert_eq!(app.surface(), ModalSurface::Backlinks);
    app.backlink_click(&sh, 0);
    assert_eq!(app.surface(), ModalSurface::Browse);
    assert_eq!(
        app.detail(&sh).expect("detail").title,
        "Source",
        "the jump landed"
    );
    // out-of-range safety: press "b" again (now on Source, empty backlinks),
    // click 99 must not panic (clamps or no-ops).
    app.on_key(&mut sh, "b", false, false, Some('b'));
    app.backlink_click(&sh, 99);
    assert!(
        matches!(
            app.surface(),
            ModalSurface::Browse | ModalSurface::Backlinks
        ),
        "surface after out-of-range backlink click"
    );
}

// === L4: wheel scrolls the viewport, not the cursor. ===

#[test]
fn scroll_by_moves_the_viewport_not_the_selection() {
    let (_d, sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    assert_eq!(app.selected(), 0);
    app.scroll_by(2, &sh, 1); // page of 1, scroll down 2
    let (off, rows) = app.view_window(&sh, 1);
    assert_eq!(off, 2);
    assert_eq!(rows[0].title, "Write spec");
    assert_eq!(app.selected(), 0, "selection unchanged");
}

#[test]
fn scroll_clamps_to_the_row_range() {
    let (_d, sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.scroll_by(99, &sh, 1);
    let (off, _rows) = app.view_window(&sh, 1);
    assert_eq!(off, 2, "clamped to last");
    app.scroll_by(-99, &sh, 1);
    let (off, _rows) = app.view_window(&sh, 1);
    assert_eq!(off, 0);
}

#[test]
fn selection_movement_reclaims_the_viewport() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.scroll_by(2, &sh, 1);
    app.on_key(&mut sh, "j", false, false, Some('j')); // selection -> 1
    let (off, rows) = app.view_window(&sh, 1);
    assert_eq!(off, 1, "keep-selection-visible rule back in charge");
    assert_eq!(rows[0].title, "Personal wiki");
}

// === L6: live match count in the search overlay. ===

#[test]
fn search_context_counts_matches() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "/", false, false, Some('/'));
    for c in "wiki".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    assert_eq!(
        app.search_context(&sh),
        "\u{2315} wiki\u{258f} \u{b7} 1 match"
    );
    app.on_key(&mut sh, "escape", false, false, None);
}

#[test]
fn search_context_pluralizes() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "/", false, false, Some('/'));
    assert_eq!(
        app.search_context(&sh),
        "\u{2315} \u{258f} \u{b7} 3 matches"
    );
    for c in "zzz-no-match".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
        assert!(app.search_context(&sh).ends_with("0 matches"));
    }
}

// === Q1-E1: linewise VISUAL. ===

#[test]
fn visual_line_selects_whole_lines_and_yanks() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "one\ntwo\nthree");
    app.on_key(&mut sh, "escape", false, false, None); // NORMAL, cursor on line 2
    app.on_key(&mut sh, "k", false, false, Some('k')); // line 1, "two"
    app.on_key(&mut sh, "V", false, false, Some('V'));
    assert_eq!(app.body_mode(), closure_shell_core::EditorMode::VisualLine);
    app.on_key(&mut sh, "y", false, false, Some('y'));
    assert_eq!(app.body_mode(), closure_shell_core::EditorMode::Normal);
    assert_eq!(app.body_buffer(), "one\ntwo\nthree");
    app.on_key(&mut sh, "p", false, false, Some('p'));
    assert_eq!(app.body_buffer(), "one\ntwo\ntwo\nthree");
}

#[test]
fn visual_line_extends_and_deletes() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "a\nb\nc\nd");
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "k", false, false, Some('k'));
    app.on_key(&mut sh, "k", false, false, Some('k')); // cursor line 1, "b"
    app.on_key(&mut sh, "V", false, false, Some('V'));
    app.on_key(&mut sh, "j", false, false, Some('j')); // extend to line 2, covers "b" and "c"
    app.on_key(&mut sh, "d", false, false, Some('d'));
    assert_eq!(app.body_buffer(), "a\nd");
    assert_eq!(app.body_mode(), closure_shell_core::EditorMode::Normal);
    app.on_key(&mut sh, "p", false, false, Some('p'));
    assert_eq!(app.body_buffer(), "a\nb\nc\nd");
}

#[test]
fn visual_line_escape_leaves_buffer_alone() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "x\ny");
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "V", false, false, Some('V'));
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.body_mode(), closure_shell_core::EditorMode::Normal);
    assert_eq!(app.body_buffer(), "x\ny");
}

// === Q1-E2: counts in Normal mode. ===

#[test]
fn count_repeats_motions() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "abcdef");
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "0", false, false, Some('0'));
    assert_eq!(app.body_cursor(), (0, 0));
    app.on_key(&mut sh, "3", false, false, Some('3'));
    app.on_key(&mut sh, "l", false, false, Some('l'));
    assert_eq!(app.body_cursor(), (0, 3));
}

#[test]
fn count_repeats_x_and_dd() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "abcdef");
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "0", false, false, Some('0'));
    app.on_key(&mut sh, "3", false, false, Some('3'));
    app.on_key(&mut sh, "x", false, false, Some('x'));
    assert_eq!(app.body_buffer(), "def");

    // fresh editor (new app + shell) for the 2dd case, as other tests do
    let (_d2, mut sh2) = shell();
    let mut app = editor_with(&mut sh2, "one\ntwo\nthree");
    app.on_key(&mut sh2, "escape", false, false, None);
    app.on_key(&mut sh2, "k", false, false, Some('k'));
    app.on_key(&mut sh2, "k", false, false, Some('k'));
    app.on_key(&mut sh2, "2", false, false, Some('2'));
    app.on_key(&mut sh2, "d", false, false, Some('d'));
    app.on_key(&mut sh2, "d", false, false, Some('d'));
    assert_eq!(app.body_buffer(), "three");
}

#[test]
fn zero_without_count_is_still_home() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "hello");
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "$", false, false, Some('$'));
    app.on_key(&mut sh, "0", false, false, Some('0'));
    assert_eq!(app.body_cursor(), (0, 0));
}

#[test]
fn escape_clears_a_pending_count() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "abc");
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "0", false, false, Some('0'));
    app.on_key(&mut sh, "9", false, false, Some('9'));
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody);
    assert_eq!(app.body_mode(), closure_shell_core::EditorMode::Normal);
    app.on_key(&mut sh, "l", false, false, Some('l'));
    assert_eq!(app.body_cursor(), (0, 1));
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}

// === Q1-E3: editor-local undo/redo. ===

#[test]
fn u_undoes_x_and_ctrl_r_redoes() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "abc");
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "0", false, false, Some('0'));
    app.on_key(&mut sh, "x", false, false, Some('x'));
    assert_eq!(app.body_buffer(), "bc");
    app.on_key(&mut sh, "u", false, false, Some('u'));
    assert_eq!(app.body_buffer(), "abc");
    assert_eq!(app.body_cursor(), (0, 0));
    app.on_key(&mut sh, "r", true, false, None); // ctrl+r redo
    assert_eq!(app.body_buffer(), "bc");
}

#[test]
fn u_undoes_dd() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "one\ntwo");
    app.on_key(&mut sh, "escape", false, false, None);
    // cursor after editor_with+escape sits on LAST line, dd deletes it
    app.on_key(&mut sh, "d", false, false, Some('d'));
    app.on_key(&mut sh, "d", false, false, Some('d'));
    assert_eq!(app.body_buffer(), "one");
    app.on_key(&mut sh, "u", false, false, Some('u'));
    assert_eq!(app.body_buffer(), "one\ntwo");
}

#[test]
fn undo_stack_is_bounded_and_safe() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "a");
    app.on_key(&mut sh, "escape", false, false, None);
    // Contract revised by G4: the seed typing is itself an INSERT
    // burst, so the first u undoes it; the rest are safe no-ops.
    for _ in 0..5 {
        app.on_key(&mut sh, "u", false, false, Some('u'));
    }
    assert_eq!(app.body_buffer(), "");
    app.on_key(&mut sh, "r", true, false, None); // redo the burst
    assert_eq!(app.body_buffer(), "a");
    app.on_key(&mut sh, "x", false, false, Some('x'));
    assert_eq!(app.body_buffer(), "");
    app.on_key(&mut sh, "u", false, false, Some('u'));
    assert_eq!(app.body_buffer(), "a");
    app.on_key(&mut sh, "u", false, false, Some('u'));
    assert_eq!(app.body_buffer(), "", "second undo pops the seed burst");
    app.on_key(&mut sh, "u", false, false, Some('u'));
    assert_eq!(app.body_buffer(), "", "empty stack stays a safe no-op");
}

// === Q1-E4: word motions. ===

#[test]
fn w_jumps_to_next_word_start() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "one two three");
    app.on_key(&mut sh, "escape", false, false, None); // cursor on the final 'e'
    app.on_key(&mut sh, "0", false, false, Some('0')); // col 0
    app.on_key(&mut sh, "w", false, false, Some('w'));
    assert_eq!(app.body_cursor(), (0, 4), "start of \"two\"");
    app.on_key(&mut sh, "w", false, false, Some('w'));
    assert_eq!(app.body_cursor(), (0, 8), "start of \"three\"");
}

#[test]
fn b_jumps_to_previous_word_start() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "one two three");
    app.on_key(&mut sh, "escape", false, false, None); // cursor on last 'e' col 12
    app.on_key(&mut sh, "b", false, false, Some('b'));
    assert_eq!(app.body_cursor(), (0, 8));
    app.on_key(&mut sh, "b", false, false, Some('b'));
    assert_eq!(app.body_cursor(), (0, 4));
    app.on_key(&mut sh, "b", false, false, Some('b'));
    assert_eq!(app.body_cursor(), (0, 0));
    app.on_key(&mut sh, "b", false, false, Some('b'));
    assert_eq!(app.body_cursor(), (0, 0), "stays at start, no panic");
}

#[test]
fn count_applies_to_word_motions() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "a bb ccc dddd");
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "0", false, false, Some('0'));
    app.on_key(&mut sh, "3", false, false, Some('3'));
    app.on_key(&mut sh, "w", false, false, Some('w'));
    assert_eq!(app.body_cursor(), (0, 9), "start of \"dddd\"");
}

#[test]
fn w_crosses_lines() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "end\nnew");
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "k", false, false, Some('k'));
    app.on_key(&mut sh, "0", false, false, Some('0'));
    app.on_key(&mut sh, "w", false, false, Some('w'));
    assert_eq!(
        app.body_cursor(),
        (1, 0),
        "start of \"new\" on the next line"
    );
}

// === Q2-F1: fuzzy-ranked completions. ===

#[test]
fn fuzzy_subsequence_matches_are_included() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* weathervane spins\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    let got = closure_shell_core::body_completions("wvn", &v);
    assert!(
        got.iter().any(|s| s == "weathervane"),
        "wvn should fuzzy-match weathervane: {got:?}"
    );
}

#[test]
fn tighter_fuzzy_match_ranks_first() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* tempest test\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    let got = closure_shell_core::body_completions("tst", &v);
    assert_eq!(got[0], "test", "contiguous ranks higher: {got:?}");
    assert!(
        got.iter().any(|s| s == "tempest"),
        "tempest also matches: {got:?}"
    );
}

#[test]
fn keywords_tie_break_before_words() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* TODdler note\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    let got = closure_shell_core::body_completions("TOD", &v);
    assert_eq!(got[0], "TODO", "keyword wins tie: {got:?}");
    assert!(
        got.iter().any(|s| s == "TODdler"),
        "TODdler also present: {got:?}"
    );
}

#[test]
fn equal_scores_stay_alphabetical() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* alpha alphabet\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    let got = closure_shell_core::body_completions("alph", &v);
    let pos_alpha = got
        .iter()
        .position(|s| s == "alpha")
        .expect("alpha present");
    let pos_alphabet = got
        .iter()
        .position(|s| s == "alphabet")
        .expect("alphabet present");
    assert!(
        pos_alpha < pos_alphabet,
        "alpha before alphabet on equal score: {got:?}"
    );
}

#[test]
fn empty_prefix_still_yields_nothing() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* foo\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    assert!(
        closure_shell_core::body_completions("", &v).is_empty(),
        "empty prefix must yield empty"
    );
}

// === Q2-F2: TAB accepts the completion session; popup cap. ===

#[test]
fn tab_accepts_the_active_completion_session() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* alpha one\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    let mut sh = Shell::new(v);
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in "alp".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "n", true, false, None);
    assert_eq!(app.body_buffer(), "alpha");
    app.on_key(&mut sh, "tab", false, false, None);
    assert_eq!(
        app.body_buffer(),
        "alpha",
        "TAB accepts, no trailing indent"
    );
    assert!(app.body_completion_items().is_empty(), "session over");
}

#[test]
fn tab_without_a_session_still_indents() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in "hi".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "tab", false, false, None);
    assert_eq!(app.body_buffer(), "hi  ", "soft indent unchanged");
}

#[test]
fn completion_session_caps_at_eight_candidates() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* zebra01 zebra02 zebra03 zebra04 zebra05 zebra06 zebra07 zebra08 zebra09 zebra10\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    let mut sh = Shell::new(v);
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in "zebra".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "n", true, false, None);
    assert_eq!(app.body_completion_items().len(), 8);
}

// === Q3-A1: agenda context rows. ===

#[test]
fn agenda_context_rows_are_date_sorted_with_flags() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO past\nDEADLINE: <2026-01-02 Fri>\n* TODO now\nSCHEDULED: <2026-07-05 Sun>\n* TODO later\nSCHEDULED: <2026-12-24 Thu>\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    let sh = Shell::new(v);
    let app = ModalApp::new(InputMode::Vim);
    let rows = app.agenda_context(&sh, "2026-07-05");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].date, "2026-01-02");
    assert_eq!(rows[0].kind, "DEADLINE");
    assert_eq!(rows[0].title, "past");
    assert!(rows[0].is_overdue);
    assert!(!rows[0].is_today);
    assert_eq!(rows[1].date, "2026-07-05");
    assert_eq!(rows[1].kind, "SCHEDULED");
    assert_eq!(rows[1].title, "now");
    assert!(rows[1].is_today);
    assert!(!rows[1].is_overdue);
    assert_eq!(rows[2].date, "2026-12-24");
    assert_eq!(rows[2].title, "later");
    assert!(!rows[2].is_today);
    assert!(!rows[2].is_overdue);
}

#[test]
fn agenda_context_is_empty_without_planning() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* plain\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    let sh = Shell::new(v);
    let app = ModalApp::new(InputMode::Vim);
    assert!(app.agenda_context(&sh, "2026-07-05").is_empty());
}

// === Q5-N1/N2: desktop-standard INSERT editing. ===

#[test]
fn ctrl_backspace_deletes_word_back() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "one two");
    app.on_key(&mut sh, "backspace", true, false, None);
    assert_eq!(app.body_buffer(), "one ");
}

#[test]
fn plain_backspace_still_deletes_one_char() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "ab");
    app.on_key(&mut sh, "backspace", false, false, None);
    assert_eq!(app.body_buffer(), "a");
}

#[test]
fn ctrl_arrows_jump_words_in_insert() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "one two three");
    app.on_key(&mut sh, "left", true, false, None);
    assert_eq!(app.body_cursor(), (0, 8));
    app.on_key(&mut sh, "left", true, false, None);
    assert_eq!(app.body_cursor(), (0, 4));
    app.on_key(&mut sh, "right", true, false, None);
    assert_eq!(app.body_cursor(), (0, 8));
    app.on_key(&mut sh, "X", false, false, Some('X'));
    assert_eq!(app.body_buffer(), "one two Xthree");
}

#[test]
fn alt_arrows_jump_words_in_insert() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "aa bb cc");
    app.on_key(&mut sh, "left", false, true, None);
    assert_eq!(app.body_cursor(), (0, 6));
    app.on_key(&mut sh, "left", false, true, None);
    assert_eq!(app.body_cursor(), (0, 3));
    app.on_key(&mut sh, "right", false, true, None);
    assert_eq!(app.body_cursor(), (0, 6));
}

// === Q6-S1/S2/S3: structural editing from the outline. ===

#[test]
fn demote_and_promote_change_the_selected_level() {
    let (_d, mut sh) = nested();
    let mut app = ModalApp::new(InputMode::Doom);
    let rows = app.rows(&sh);
    let old = rows[0].level;
    app.run(&mut sh, "demote");
    assert_eq!(app.rows(&sh)[0].level, old + 1);
    app.run(&mut sh, "promote");
    assert_eq!(app.rows(&sh)[0].level, old);
    // Level 1 promote clamps
    app.run(&mut sh, "promote");
    assert!(app.rows(&sh)[0].level >= 1, "level stays >= 1, no panic");
}

#[test]
fn move_subtree_down_swaps_siblings() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* first\n* second\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    let mut sh = Shell::new(v);
    let mut app = ModalApp::new(InputMode::Doom);
    // select row 0 (default)
    app.run(&mut sh, "move-subtree-down");
    let rows = app.rows(&sh);
    assert_eq!(rows[0].title, "second");
    assert_eq!(rows[1].title, "first");
    app.run(&mut sh, "move-subtree-up");
    let rows = app.rows(&sh);
    assert_eq!(rows[0].title, "first");
    assert_eq!(rows[1].title, "second", "original order restored");
    // last-sibling clamp: safe no-op
    app.select(1, &sh);
    app.run(&mut sh, "move-subtree-down");
    let rows = app.rows(&sh);
    assert_eq!(rows[0].title, "first");
    assert_eq!(rows[1].title, "second", "order unchanged, no panic");
}

#[test]
fn add_heading_inserts_a_sibling_below() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* first\n* second\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    let mut sh = Shell::new(v);
    let mut app = ModalApp::new(InputMode::Doom);
    // select row 0
    app.run(&mut sh, "add-heading");
    let rows = app.rows(&sh);
    assert_eq!(rows.len(), 3, "grew by 1");
    assert_eq!(rows[1].level, rows[0].level, "same level");
    assert!(
        rows[1].title != "second",
        "inserted sibling, not the old second"
    );
}

#[test]
fn doom_keymap_binds_structural_editing() {
    let km = closure_input::mode_keymap(InputMode::Doom);
    assert!(
        km.iter().any(|(c, cmd)| *c == "M-h" && *cmd == "promote"),
        "M-h -> promote"
    );
    assert!(
        km.iter().any(|(c, cmd)| *c == "M-l" && *cmd == "demote"),
        "M-l -> demote"
    );
    assert!(
        km.iter()
            .any(|(c, cmd)| *c == "M-k" && *cmd == "move-subtree-up"),
        "M-k -> move-subtree-up"
    );
    assert!(
        km.iter()
            .any(|(c, cmd)| *c == "M-j" && *cmd == "move-subtree-down"),
        "M-j -> move-subtree-down"
    );
    assert!(
        km.iter()
            .any(|(c, cmd)| *c == "M-RET" && *cmd == "add-heading"),
        "M-RET -> add-heading"
    );
}

// === Q8-M1/M2: the palette covers the keymap; M-x opens it. ===

#[test]
fn palette_covers_every_keymap_command() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.on_key(&mut sh, ":", false, false, Some(':'));
    let entries = app.palette_entries();
    let cmds: std::collections::BTreeSet<&str> = closure_input::mode_keymap(InputMode::Doom)
        .iter()
        .map(|(_, cmd)| *cmd)
        .collect();
    for cmd in &cmds {
        assert!(
            entries.iter().any(|e| e.action.command() == *cmd),
            "palette misses {cmd}"
        );
    }
}

#[test]
fn palette_lists_structural_commands_with_chords() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.on_key(&mut sh, ":", false, false, Some(':'));
    let entries = app.palette_entries();
    assert!(
        entries
            .iter()
            .any(|e| e.action.command() == "promote" && e.action.chord() == "M-h"),
        "promote should be listed with its M-h chord"
    );
}

#[test]
fn alt_x_opens_the_palette() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.on_key(&mut sh, "x", false, true, Some('x'));
    assert_eq!(app.surface(), ModalSurface::Palette);
}

#[test]
fn palette_query_still_fuzzy_filters() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.on_key(&mut sh, ":", false, false, Some(':'));
    for c in "fold".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    let entries = app.palette_entries();
    assert!(!entries.is_empty());
    assert!(
        entries.iter().any(|e| e.action.command() == "toggle-fold"),
        "fold query still surfaces toggle-fold"
    );
}

// === Q9-W1: grouped which-key data for the popup. ===

#[test]
fn which_key_groups_cover_the_whole_keymap() {
    let app = ModalApp::new(InputMode::Doom);
    let total: usize = app.which_key_groups().iter().map(|(_, v)| v.len()).sum();
    assert_eq!(total, closure_input::mode_keymap(InputMode::Doom).len());
}

#[test]
fn which_key_groups_place_known_commands() {
    let app = ModalApp::new(InputMode::Doom);
    let groups = app.which_key_groups();
    let find = |cmd: &str| -> Option<String> {
        groups
            .iter()
            .find_map(|(t, v)| v.iter().any(|(_, c)| c == cmd).then(|| t.clone()))
    };
    assert_eq!(find("toggle-fold"), Some("Navigate".to_owned()));
    assert_eq!(find("promote"), Some("Command".to_owned()));
    assert_eq!(find("rename"), Some("Edit".to_owned()));
}

#[test]
fn which_key_groups_are_ordered_and_nonempty() {
    let app = ModalApp::new(InputMode::Doom);
    let groups = app.which_key_groups();
    for (_, v) in &groups {
        assert!(!v.is_empty(), "no empty groups: {groups:?}");
    }
    let titles: Vec<&str> = groups.iter().map(|(t, _)| t.as_str()).collect();
    let expected = ["Navigate", "Edit", "Mode", "App", "Command"];
    let mut last_pos: Option<usize> = None;
    for &want in &expected {
        if let Some(pos) = titles.iter().position(|&t| t == want) {
            if let Some(lp) = last_pos {
                assert!(pos > lp, "titles in order, got {titles:?}");
            }
            last_pos = Some(pos);
        }
    }
}

#[test]
fn which_key_group_entries_sort_by_chord() {
    let app = ModalApp::new(InputMode::Doom);
    let groups = app.which_key_groups();
    for (_, v) in &groups {
        assert!(
            v.windows(2).all(|w| w[0].0 <= w[1].0),
            "entries sorted by chord in {v:?}"
        );
    }
}

// === Q7-C2: completion auto-popup contract. ===

#[test]
fn popup_predicate_needs_three_word_chars_and_insert() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "Pe");
    assert!(!app.completion_should_popup(&sh));
    app.on_key(&mut sh, "r", false, false, Some('r'));
    app.on_key(&mut sh, "s", false, false, Some('s'));
    assert_eq!(app.body_buffer(), "Pers");
    assert!(app.completion_should_popup(&sh));
    app.on_key(&mut sh, "escape", false, false, None);
    assert!(!app.completion_should_popup(&sh));
}

#[test]
fn open_popup_shows_without_applying() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "Pers");
    app.open_completion_popup(&sh);
    assert_eq!(app.body_buffer(), "Pers", "unchanged");
    assert!(!app.body_completion_items().is_empty());
    assert!(app.body_completion_ix().is_none());
    assert!(
        !app.completion_should_popup(&sh),
        "session active -> no re-popup"
    );
}

#[test]
fn c_n_after_popup_applies_the_first_candidate() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "Pers");
    app.open_completion_popup(&sh);
    let first = app.body_completion_items()[0].clone();
    app.on_key(&mut sh, "n", true, false, None);
    assert_eq!(app.body_buffer(), first);
    assert_eq!(app.body_completion_ix(), Some(0));
}

#[test]
fn tab_after_popup_accepts_the_first_candidate() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "Pers");
    app.open_completion_popup(&sh);
    let first = app.body_completion_items()[0].clone();
    app.on_key(&mut sh, "tab", false, false, None);
    assert_eq!(app.body_buffer(), first);
    assert!(app.body_completion_items().is_empty());
}

// === Q11-U1: undo-history surface. ===

#[test]
fn undo_history_opens_and_closes() {
    let (_d, mut sh) = nested();
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut sh, "undo-history");
    assert_eq!(app.surface(), ModalSurface::UndoHistory);
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}

#[test]
fn undo_history_rows_show_edits() {
    let (_d, mut sh) = nested();
    let mut app = ModalApp::new(InputMode::Doom);
    assert!(app.undo_history_rows(&sh).is_empty());
    app.run(&mut sh, "toggle-todo");
    let rows = app.undo_history_rows(&sh);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].is_current);
    assert!(
        rows[0].label.to_lowercase().contains("todo"),
        "label describes the edit: {rows:?}"
    );
}

#[test]
fn undo_history_marks_current_after_undo() {
    let (_d, mut sh) = nested();
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut sh, "toggle-todo");
    app.run(&mut sh, "toggle-todo");
    app.run(&mut sh, "undo");
    let rows = app.undo_history_rows(&sh);
    assert_eq!(rows.len(), 2);
    assert!(
        rows[0].is_current && !rows[1].is_current,
        "current steps back: {rows:?}"
    );
}

#[test]
fn doom_binds_undo_history() {
    let km = closure_input::mode_keymap(InputMode::Doom);
    assert!(
        km.iter()
            .any(|(c, cmd)| *c == "g u" && *cmd == "undo-history"),
        "g u -> undo-history"
    );
}

// === Q5-N4: TAB cycles the fold on the selected heading (org TAB). ===

#[test]
fn tab_toggles_the_selected_fold_in_browse() {
    let (_d, mut sh) = nested();
    let mut app = ModalApp::new(InputMode::Doom);
    let id = app.rows(&sh)[0].id.clone();
    assert!(!closure_shell_core::is_row_folded(&sh, &id));
    app.on_key(&mut sh, "tab", false, false, None);
    assert!(closure_shell_core::is_row_folded(&sh, &id), "TAB folds");
    app.on_key(&mut sh, "tab", false, false, None);
    assert!(!closure_shell_core::is_row_folded(&sh, &id), "TAB unfolds");
}

#[test]
fn every_mode_binds_tab_to_toggle_fold() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        let km = closure_input::mode_keymap(mode);
        assert!(
            km.iter()
                .any(|(c, cmd)| *c == "TAB" && *cmd == "toggle-fold"),
            "{mode:?} binds TAB"
        );
    }
}

// === Q10-D2: mouse into the body editor. ===

#[test]
fn click_places_the_cursor() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "one two\nthree");
    app.body_click(1, 2);
    assert_eq!(app.body_cursor(), (1, 2));
    app.body_click(0, 4);
    assert_eq!(app.body_cursor(), (0, 4));
    // still INSERT: type "X" inserts at the new cursor position
    app.on_key(&mut sh, "X", false, false, Some('X'));
    assert_eq!(app.body_buffer(), "one Xtwo\nthree");
}

#[test]
fn click_clamps_to_line_and_buffer() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "ab");
    // out of range must not panic; both line and col clamped
    app.body_click(5, 9);
    assert_eq!(app.body_cursor().0, 0);
    assert_eq!(app.body_cursor().1, 2, "col clamped to line end");
}

#[test]
fn double_click_selects_the_word() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "one two three");
    app.on_key(&mut sh, "escape", false, false, None);
    app.body_double_click(0, 5); // inside "two"
    assert_eq!(app.body_mode(), closure_shell_core::EditorMode::Visual);
    assert_eq!(
        app.body_selection(),
        Some((4, 7)),
        "exactly the bytes of two: {:?}",
        app.body_selection()
    );
}

#[test]
fn click_ends_a_completion_session() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* alpha one\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    let mut sh = Shell::new(v);
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in "alp".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "n", true, false, None);
    assert_eq!(app.body_buffer(), "alpha");
    app.body_click(0, 0);
    assert!(app.body_completion_items().is_empty());
    assert_eq!(app.body_cursor(), (0, 0));
}

#[test]
fn drag_extends_charwise_selection_from_click() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "one two three");
    app.on_key(&mut sh, "escape", false, false, None);
    app.body_click(0, 0);
    app.body_drag(0, 6);
    assert_eq!(app.body_mode(), closure_shell_core::EditorMode::Visual);
    assert_eq!(app.body_selection(), Some((0, 7)));
}

#[test]
fn drag_backwards_selects_to_the_anchor() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "one two three");
    app.on_key(&mut sh, "escape", false, false, None);
    app.body_click(0, 6);
    app.body_drag(0, 2);
    assert_eq!(app.body_selection(), Some((2, 7)));
}

#[test]
fn drag_crosses_lines() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "one\ntwo");
    app.on_key(&mut sh, "escape", false, false, None);
    app.body_click(0, 1);
    app.body_drag(1, 1);
    assert_eq!(app.body_selection(), Some((1, 6)));
}

#[test]
fn continued_drag_reextends_from_the_same_anchor() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "one two three");
    app.on_key(&mut sh, "escape", false, false, None);
    app.body_click(0, 0);
    app.body_drag(0, 2);
    app.body_drag(0, 4);
    assert_eq!(app.body_mode(), closure_shell_core::EditorMode::Visual);
    assert_eq!(app.body_selection(), Some((0, 5)));
}

#[test]
fn drag_from_insert_enters_visual() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "one two three");
    app.body_click(0, 0);
    app.body_drag(0, 3);
    assert_eq!(app.body_mode(), closure_shell_core::EditorMode::Visual);
    assert_eq!(app.body_selection(), Some((0, 4)));
}

#[test]
fn drag_to_the_click_position_keeps_the_mode() {
    // A click's micro-movement fires a drag event on the same cell; it
    // must not flip the editor into a one-char Visual selection.
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "one two three");
    app.body_click(0, 1);
    app.body_drag(0, 1);
    assert_eq!(
        app.body_mode(),
        closure_shell_core::EditorMode::Insert,
        "same-cell drag stays in the click's mode"
    );
    assert_eq!(app.body_selection(), None);
}

// === INSERT-burst undo: one checkpoint per INSERT entry; first buffer-
// changing edit records pre-edit state so Esc+u undoes the whole burst. ===

#[test]
fn insert_burst_undoes_as_one_unit() {
    let (_d, mut sh) = shell();
    // editor_with TYPES the seed, so "one" is the first burst; leave
    // INSERT and re-enter so XYZ forms its own burst.
    let mut app = editor_with(&mut sh, "one");
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "a", false, false, Some('a'));
    app.on_key(&mut sh, "X", false, false, Some('X'));
    app.on_key(&mut sh, "Y", false, false, Some('Y'));
    app.on_key(&mut sh, "Z", false, false, Some('Z'));
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "u", false, false, Some('u'));
    assert_eq!(
        app.body_buffer(),
        "one",
        "one undo removes the whole burst"
    );
    app.on_key(&mut sh, "r", true, false, None); // ctrl+r redo
    assert_eq!(app.body_buffer(), "oneXYZ", "redo restores the burst");
}

#[test]
fn two_insert_bursts_undo_separately() {
    let (_d, mut sh) = shell();
    // Seed typing is burst #1 (one checkpoint per INSERT entry); XYZ
    // typed in the same session belongs to it. Re-entering INSERT via
    // a starts burst #2.
    let mut app = editor_with(&mut sh, "one");
    app.on_key(&mut sh, "X", false, false, Some('X'));
    app.on_key(&mut sh, "Y", false, false, Some('Y'));
    app.on_key(&mut sh, "Z", false, false, Some('Z'));
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "a", false, false, Some('a')); // burst #2
    app.on_key(&mut sh, "A", false, false, Some('A'));
    app.on_key(&mut sh, "B", false, false, Some('B'));
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "u", false, false, Some('u'));
    assert_eq!(app.body_buffer(), "oneXYZ", "second burst undone first");
    app.on_key(&mut sh, "u", false, false, Some('u'));
    assert_eq!(app.body_buffer(), "", "first burst (the whole session) next");
}

#[test]
fn undo_with_no_burst_is_a_safe_noop() {
    let (_d, mut sh) = shell();
    let mut app = editor_with(&mut sh, "one");
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "u", false, false, Some('u')); // pops the seed burst
    assert_eq!(app.body_buffer(), "");
    // an empty undo stack is a safe no-op, mode untouched
    app.on_key(&mut sh, "u", false, false, Some('u'));
    assert_eq!(app.body_buffer(), "");
    assert_eq!(app.body_mode(), closure_shell_core::EditorMode::Normal);
}

// === Drag-drop outline row reorder (mouse DnD on the command surface). ===

#[test]
fn drag_drop_moves_a_row_down() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.drag_drop_rows(&mut sh, 0, 2);
    let rows = app.rows(&sh);
    let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(titles, vec!["Personal wiki", "Write spec", "Ship parser"]);
    assert_eq!(app.selected(), 2, "selection follows the dragged row");
}

#[test]
fn drag_drop_moves_a_row_up() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.drag_drop_rows(&mut sh, 2, 0);
    let rows = app.rows(&sh);
    let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(titles, vec!["Write spec", "Ship parser", "Personal wiki"]);
    assert_eq!(app.selected(), 0);
}

#[test]
fn drag_drop_to_the_same_index_is_a_noop() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    let before: Vec<String> = app.rows(&sh).iter().map(|r| r.title.clone()).collect();
    app.drag_drop_rows(&mut sh, 1, 1);
    let after: Vec<String> = app.rows(&sh).iter().map(|r| r.title.clone()).collect();
    assert_eq!(after, before);
}

#[test]
fn drag_drop_out_of_range_is_safe() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.drag_drop_rows(&mut sh, 9, 0);
    let rows = app.rows(&sh);
    let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(titles, vec!["Ship parser", "Personal wiki", "Write spec"]);
    app.drag_drop_rows(&mut sh, 0, 9);
    let rows = app.rows(&sh);
    let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(titles, vec!["Personal wiki", "Write spec", "Ship parser"]);
}

#[test]
fn drag_drop_preserves_block_ids() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    let mut before: Vec<_> = app.rows(&sh).iter().map(|r| r.id.clone()).collect();
    before.sort();
    app.drag_drop_rows(&mut sh, 0, 2);
    let mut after: Vec<_> = app.rows(&sh).iter().map(|r| r.id.clone()).collect();
    after.sort();
    assert_eq!(after, before);
}
