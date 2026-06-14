//! Headless tests for the gpui shell's pure state core. No GPU /
//! window / gpui types — every keyboard transition is unit-testable,
//! the same pattern the TUI `App` uses. The gpui `Render` adapter
//! (behind the `gpui` feature) is a thin shell over this.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_shell_core::{App, Mode, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO Ship parser\n* Personal wiki\n** Subtopic\n* DONE Write spec\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

/// Type a string of plain characters into the app.
fn typ(app: &mut App, sh: &mut Shell, s: &str) {
    for c in s.chars() {
        app.on_key(sh, &c.to_string(), false, Some(c));
    }
}

#[test]
fn starts_in_browse_listing_all_headlines() {
    let (_d, sh) = shell();
    let app = App::new();
    assert_eq!(app.mode(), Mode::Browse);
    assert_eq!(app.query(), "");
    assert_eq!(app.rows(&sh).len(), 4, "all headlines listed");
    assert_eq!(app.selected(), 0);
}

#[test]
fn rows_carry_level_and_todo() {
    let (_d, sh) = shell();
    let app = App::new();
    let rows = app.rows(&sh);
    let ship = rows
        .iter()
        .find(|r| r.title == "Ship parser")
        .expect("ship");
    assert_eq!(ship.todo.as_deref(), Some("TODO"));
    let sub = rows.iter().find(|r| r.title == "Subtopic").expect("sub");
    assert_eq!(sub.level, 2);
}

#[test]
fn typing_filters_fuzzily_and_resets_cursor() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    app.on_key(&mut sh, "down", false, None);
    assert_eq!(app.selected(), 1);
    typ(&mut app, &mut sh, "wiki");
    assert_eq!(app.query(), "wiki");
    assert_eq!(app.selected(), 0, "filter resets the cursor");
    let rows = app.rows(&sh);
    let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
    assert!(titles.contains(&"Personal wiki"));
    assert!(!titles.contains(&"Ship parser"));
}

#[test]
fn backspace_edits_query() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    typ(&mut app, &mut sh, "spec");
    app.on_key(&mut sh, "backspace", false, None);
    assert_eq!(app.query(), "spe");
}

#[test]
fn down_up_move_and_clamp() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    for _ in 0..10 {
        app.on_key(&mut sh, "down", false, None);
    }
    assert_eq!(app.selected(), 3, "clamps at last of 4");
    for _ in 0..10 {
        app.on_key(&mut sh, "up", false, None);
    }
    assert_eq!(app.selected(), 0);
}

#[test]
fn ctrl_n_ctrl_p_also_navigate() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    app.on_key(&mut sh, "n", true, None);
    assert_eq!(app.selected(), 1);
    app.on_key(&mut sh, "p", true, None);
    assert_eq!(app.selected(), 0);
}

#[test]
fn enter_opens_selected_and_sets_status() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    app.on_key(&mut sh, "down", false, None); // Personal wiki
    app.on_key(&mut sh, "enter", false, None);
    assert!(
        app.status().contains("Personal wiki"),
        "status: {}",
        app.status()
    );
}

#[test]
fn escape_clears_query() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    typ(&mut app, &mut sh, "wiki");
    app.on_key(&mut sh, "escape", false, None);
    assert_eq!(app.query(), "");
    assert_eq!(app.rows(&sh).len(), 4);
}

#[test]
fn ctrl_c_capture_then_commit_writes_through_vault() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    app.on_key(&mut sh, "c", true, None);
    assert_eq!(app.mode(), Mode::Capture);
    typ(&mut app, &mut sh, "Buy milk"); // goes to capture buffer, not query
    assert_eq!(app.capture_buffer(), "Buy milk");
    assert_eq!(app.query(), "", "capture typing does not touch the filter");
    app.on_key(&mut sh, "enter", false, None);
    assert_eq!(app.mode(), Mode::Browse);
    assert!(
        sh.vault.find_by_title("Buy milk").is_some(),
        "captured via Vault (I8)"
    );
}

#[test]
fn capture_escape_cancels_without_writing() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    app.on_key(&mut sh, "c", true, None);
    typ(&mut app, &mut sh, "Nope");
    app.on_key(&mut sh, "escape", false, None);
    assert_eq!(app.mode(), Mode::Browse);
    assert!(sh.vault.find_by_title("Nope").is_none());
}

#[test]
fn ctrl_q_requests_quit() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    assert!(!app.should_quit());
    app.on_key(&mut sh, "q", true, None);
    assert!(app.should_quit());
}

#[test]
fn key_hints_shown_and_mode_specific() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    assert!(app.key_hints().contains("capture"));
    assert!(app.key_hints().contains("quit"));
    app.on_key(&mut sh, "c", true, None);
    assert!(app.key_hints().to_lowercase().contains("save") || app.key_hints().contains("Enter"));
}

#[test]
fn empty_vault_is_safe() {
    let dir = tempfile::tempdir().expect("tmp");
    let v = Vault::open(dir.path()).expect("open");
    let mut sh = Shell::new(v);
    let mut app = App::new();
    assert!(app.rows(&sh).is_empty());
    app.on_key(&mut sh, "down", false, None);
    app.on_key(&mut sh, "enter", false, None);
    assert_eq!(app.selected(), 0);
}

// --- editing parity: rename + delete (vision: editing in every UI) ---------

#[test]
fn rows_carry_block_id() {
    let (_d, sh) = shell();
    let app = App::new();
    let rows = app.rows(&sh);
    assert!(
        rows.iter().all(|r| !r.id.is_empty()),
        "every row has an :ID:"
    );
}

#[test]
fn fuzzy_rows_keep_level_todo_and_id() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    typ(&mut app, &mut sh, "ship");
    let rows = app.rows(&sh);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "Ship parser");
    assert_eq!(rows[0].todo.as_deref(), Some("TODO"));
    assert!(!rows[0].id.is_empty());
}

#[test]
fn ctrl_r_renames_selected_through_vault() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    app.on_key(&mut sh, "down", false, None); // Personal wiki
    app.on_key(&mut sh, "r", true, None);
    assert_eq!(app.mode(), Mode::Rename);
    assert_eq!(
        app.capture_buffer(),
        "Personal wiki",
        "prefilled with current title"
    );
    // clear + retype
    for _ in 0.."Personal wiki".len() {
        app.on_key(&mut sh, "backspace", false, None);
    }
    typ(&mut app, &mut sh, "Shared wiki");
    app.on_key(&mut sh, "enter", false, None);
    assert_eq!(app.mode(), Mode::Browse);
    assert!(sh.vault.find_by_title("Shared wiki").is_some());
    assert!(sh.vault.find_by_title("Personal wiki").is_none());
}

#[test]
fn rename_escape_cancels() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    app.on_key(&mut sh, "r", true, None);
    app.on_key(&mut sh, "escape", false, None);
    assert_eq!(app.mode(), Mode::Browse);
    assert!(sh.vault.find_by_title("Ship parser").is_some(), "unchanged");
}

#[test]
fn ctrl_d_deletes_selected_subtree() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    // select "Ship parser" (row 0)
    app.on_key(&mut sh, "d", true, None);
    assert!(
        sh.vault.find_by_title("Ship parser").is_none(),
        "deleted via Vault (I8)"
    );
    assert_eq!(app.mode(), Mode::Browse);
}

#[test]
fn ctrl_r_on_empty_list_is_noop() {
    let dir = tempfile::tempdir().expect("tmp");
    let v = Vault::open(dir.path()).expect("open");
    let mut sh = Shell::new(v);
    let mut app = App::new();
    app.on_key(&mut sh, "r", true, None);
    assert_eq!(app.mode(), Mode::Browse, "nothing to rename");
}

// --- detail / preview pane (vision: a real browser, not just a list) -------

fn rich_shell() -> (TempDir, Shell) {
    // closure-org attaches a :PROPERTIES: drawer or a planning line
    // when it immediately follows the headline (one or the other), so
    // the fixture keeps the property-bearing and planning-bearing
    // headlines separate.
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO [#A] Ship parser :work:urgent:\n\
         :PROPERTIES:\n:EFFORT: 3d\n:END:\n\
         body line one\nbody line two\n\
         * Plain headline\n\
         * DONE Review\nSCHEDULED: <2026-06-20>\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

#[test]
fn detail_of_selected_carries_full_headline() {
    let (_d, sh) = rich_shell();
    let app = App::new();
    let d = app.detail(&sh).expect("a row is selected");
    assert_eq!(d.title, "Ship parser");
    assert_eq!(d.todo.as_deref(), Some("TODO"));
    assert_eq!(d.priority, Some('A'));
    assert_eq!(d.tags, vec!["work".to_owned(), "urgent".to_owned()]);
    assert!(d.properties.iter().any(|(k, v)| k == "EFFORT" && v == "3d"));
    assert!(d.body.contains("body line one"));
    assert!(d.body.contains("body line two"));
}

#[test]
fn detail_surfaces_planning() {
    let (_d, mut sh) = rich_shell();
    let mut app = App::new();
    typ(&mut app, &mut sh, "Review");
    let d = app.detail(&sh).expect("detail");
    assert_eq!(d.todo.as_deref(), Some("DONE"));
    assert_eq!(d.scheduled.as_deref(), Some("<2026-06-20>"));
}

#[test]
fn detail_follows_selection() {
    let (_d, mut sh) = rich_shell();
    let mut app = App::new();
    app.on_key(&mut sh, "down", false, None);
    let d = app.detail(&sh).expect("detail");
    assert_eq!(d.title, "Plain headline");
    assert_eq!(d.todo, None);
    assert!(d.tags.is_empty());
}

#[test]
fn detail_none_on_empty_vault() {
    let dir = tempfile::tempdir().expect("tmp");
    let v = Vault::open(dir.path()).expect("open");
    let sh = Shell::new(v);
    let app = App::new();
    assert!(app.detail(&sh).is_none());
}

#[test]
fn detail_tracks_fuzzy_selection() {
    let (_d, mut sh) = rich_shell();
    let mut app = App::new();
    typ(&mut app, &mut sh, "plain");
    let d = app.detail(&sh).expect("detail");
    assert_eq!(d.title, "Plain headline");
}

// --- add-sibling (C-a) + mouse-style select(i) -----------------------------

#[test]
fn ctrl_a_adds_sibling_after_selected() {
    let (_d, mut sh) = shell();
    let mut app = App::new(); // selected = Ship parser
    app.on_key(&mut sh, "a", true, None);
    assert_eq!(app.mode(), Mode::AddSibling);
    typ(&mut app, &mut sh, "New task");
    app.on_key(&mut sh, "enter", false, None);
    assert_eq!(app.mode(), Mode::Browse);
    assert!(
        sh.vault.find_by_title("New task").is_some(),
        "added via Vault (I8)"
    );
}

#[test]
fn add_sibling_escape_cancels() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    app.on_key(&mut sh, "a", true, None);
    typ(&mut app, &mut sh, "Nope");
    app.on_key(&mut sh, "escape", false, None);
    assert_eq!(app.mode(), Mode::Browse);
    assert!(sh.vault.find_by_title("Nope").is_none());
}

#[test]
fn add_sibling_on_empty_list_is_noop() {
    let dir = tempfile::tempdir().expect("tmp");
    let v = Vault::open(dir.path()).expect("open");
    let mut sh = Shell::new(v);
    let mut app = App::new();
    app.on_key(&mut sh, "a", true, None);
    assert_eq!(app.mode(), Mode::Browse, "nothing to add after");
}

#[test]
fn select_sets_cursor_clamped() {
    let (_d, sh) = shell();
    let mut app = App::new();
    app.select(2, &sh);
    assert_eq!(app.selected(), 2);
    app.select(99, &sh); // out of range -> clamped to last
    assert_eq!(app.selected(), 3);
}

#[test]
fn add_sibling_key_hint_shown() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    assert!(app.key_hints().contains("add"));
    app.on_key(&mut sh, "a", true, None);
    assert!(app.key_hints().contains("Enter"));
}

// --- viewport windowing (keep selection visible, cap render nodes) ---------

fn big_shell(n: usize) -> (TempDir, Shell) {
    use std::fmt::Write as _;
    let dir = tempfile::tempdir().expect("tmp");
    let mut s = String::new();
    for i in 0..n {
        let _ = writeln!(s, "* Item {i:03}");
    }
    fs::write(dir.path().join("big.org"), s).expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

#[test]
fn view_window_returns_all_when_smaller_than_page() {
    let (_d, sh) = big_shell(5);
    let app = App::new();
    let (offset, rows) = app.view_window(&sh, 30);
    assert_eq!(offset, 0);
    assert_eq!(rows.len(), 5);
}

#[test]
fn view_window_caps_to_page_and_keeps_selection_visible() {
    let (_d, mut sh) = big_shell(200);
    let mut app = App::new();
    for _ in 0..120 {
        app.on_key(&mut sh, "down", false, None);
    }
    assert_eq!(app.selected(), 120);
    let page = 30;
    let (offset, rows) = app.view_window(&sh, page);
    assert_eq!(rows.len(), page, "never renders more than a page");
    assert!(
        (offset..offset + page).contains(&app.selected()),
        "selection {} must be within window [{}, {})",
        app.selected(),
        offset,
        offset + page
    );
}

#[test]
fn view_window_offset_zero_near_top() {
    let (_d, mut sh) = big_shell(200);
    let mut app = App::new();
    app.on_key(&mut sh, "down", false, None);
    let (offset, _) = app.view_window(&sh, 30);
    assert_eq!(offset, 0, "no scroll needed near the top");
}

#[test]
fn view_window_clamps_offset_at_end() {
    let (_d, mut sh) = big_shell(40);
    let mut app = App::new();
    for _ in 0..50 {
        app.on_key(&mut sh, "down", false, None);
    }
    let page = 30;
    let (offset, rows) = app.view_window(&sh, page);
    assert_eq!(offset, 40 - page, "last page fully filled, no overscroll");
    assert_eq!(rows.len(), page);
    assert_eq!(rows.last().unwrap().title, "Item 039");
}

// --- input mode display + cycle (launcher shell; see Decisions) ------------
// The GUI is a Notion/launcher-style shell (type-to-filter). The five
// editing modes are realized fully in the TUI; here the shared
// closure-input keymap drives the mode label, which-key hints, and a
// cycle affordance. Bare-letter modal command surfaces (vim j/k) stay
// the TUI's domain so they don't fight type-to-filter.

use closure_config::InputMode;

#[test]
fn default_mode_is_notion_and_types_to_filter() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    assert_eq!(app.input_mode(), InputMode::Notion);
    typ(&mut app, &mut sh, "wiki");
    assert_eq!(app.query(), "wiki");
}

#[test]
fn set_mode_changes_the_active_mode() {
    let mut app = App::new();
    app.set_mode(InputMode::Emacs);
    assert_eq!(app.input_mode(), InputMode::Emacs);
}

#[test]
fn ctrl_t_cycles_through_all_five_modes() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    let mut seen = std::collections::BTreeSet::new();
    seen.insert(format!("{:?}", app.input_mode()));
    for _ in 0..5 {
        app.on_key(&mut sh, "t", true, None); // C-t = cycle-mode (gpui affordance)
        seen.insert(format!("{:?}", app.input_mode()));
    }
    assert_eq!(seen.len(), 5, "cycle visits all five modes: {seen:?}");
}

#[test]
fn key_hints_name_the_active_mode_and_its_bindings() {
    let mut app = App::new();
    let notion = app.key_hints();
    assert!(
        notion.to_lowercase().contains("notion"),
        "hints name the mode: {notion}"
    );
    app.set_mode(InputMode::Emacs);
    let emacs = app.key_hints();
    assert!(emacs.to_lowercase().contains("emacs"));
    assert_ne!(notion, emacs, "hints differ per mode");
}

#[test]
fn type_to_filter_works_in_every_mode() {
    // Launcher behaviour is mode-independent: bare letters always filter.
    for mode in [
        InputMode::Notion,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Doom,
        InputMode::Helix,
    ] {
        let (_d, mut sh) = shell();
        let mut app = App::new();
        app.set_mode(mode);
        typ(&mut app, &mut sh, "ship");
        assert_eq!(app.query(), "ship", "{mode:?} still type-to-filter");
    }
}

// --- slash command palette (which-key-as-palette for the launcher) ---------

#[test]
fn slash_on_empty_query_opens_palette() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    app.on_key(&mut sh, "/", false, Some('/'));
    assert_eq!(app.mode(), Mode::Palette);
}

#[test]
fn slash_with_nonempty_query_still_filters() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    typ(&mut app, &mut sh, "wi");
    app.on_key(&mut sh, "/", false, Some('/'));
    assert_eq!(app.mode(), Mode::Browse, "slash mid-query is a filter char");
    assert_eq!(app.query(), "wi/");
}

#[test]
fn palette_lists_commands_with_key_hints() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    app.on_key(&mut sh, "/", false, Some('/'));
    let rows = app.palette_results();
    assert!(
        rows.iter()
            .any(|(name, key)| name == "capture" && !key.is_empty())
    );
    assert!(rows.iter().any(|(name, _)| name == "quit"));
    assert!(rows.iter().any(|(name, _)| name == "rename"));
}

#[test]
fn palette_filters_and_runs_selected_command() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    app.on_key(&mut sh, "/", false, Some('/'));
    for c in "capture".chars() {
        app.on_key(&mut sh, &c.to_string(), false, Some(c));
    }
    assert_eq!(
        app.palette_results().first().map(|(n, _)| n.as_str()),
        Some("capture")
    );
    app.on_key(&mut sh, "enter", false, None);
    assert_eq!(
        app.mode(),
        Mode::Capture,
        "selecting capture entered capture mode"
    );
}

#[test]
fn palette_escape_returns_to_browse() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    app.on_key(&mut sh, "/", false, Some('/'));
    app.on_key(&mut sh, "escape", false, None);
    assert_eq!(app.mode(), Mode::Browse);
}

#[test]
fn palette_runs_quit() {
    let (_d, mut sh) = shell();
    let mut app = App::new();
    app.on_key(&mut sh, "/", false, Some('/'));
    for c in "quit".chars() {
        app.on_key(&mut sh, &c.to_string(), false, Some(c));
    }
    app.on_key(&mut sh, "enter", false, None);
    assert!(app.should_quit());
}
