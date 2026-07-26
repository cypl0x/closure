//! The headline-editing chords the keymap advertises, in the terminal.
//!
//! `closure-input` binds `t`, `p`, `y`, `o`, `a`, `r`, `d`, `z`,
//! `M-h`/`M-l`, `M-k`/`M-j`, `M-RET`, `g u`, `g x`, `i` — the GUI
//! served all of them and the TUI served none. These drive the pure
//! `App`; the terminal driver drains the requests into the vault.

use std::path::PathBuf;

use closure_tui::{App, AppMode, HeadlineRecord};

fn paths() -> Vec<PathBuf> {
    vec![PathBuf::from("a.org"), PathBuf::from("b.org")]
}

fn rec(id: &str, title: &str) -> HeadlineRecord {
    HeadlineRecord {
        path: PathBuf::from("a.org"),
        id: id.to_owned(),
        title: title.to_owned(),
        body: String::new(),
        todo: None,
        priority: None,
        tags: Vec::new(),
        folded: false,
    }
}

/// An app over a.org with two headlines, cursor on the first.
fn app() -> App {
    let mut app = App::new(paths());
    app.set_headlines(vec![rec("id-1", "Alpha"), rec("id-2", "Beta")]);
    app
}

// === TODO / priority / tags cycling. ===

#[test]
fn t_cycles_the_todo_keyword_through_todo_done_none() {
    let mut app = app();
    app.handle_stroke("t");
    assert_eq!(
        app.take_todo_request(),
        Some(("id-1".to_owned(), Some("TODO".to_owned())))
    );

    let mut with_todo = App::new(paths());
    let mut r = rec("id-1", "Alpha");
    r.todo = Some("TODO".to_owned());
    with_todo.set_headlines(vec![r]);
    with_todo.handle_stroke("t");
    assert_eq!(
        with_todo.take_todo_request(),
        Some(("id-1".to_owned(), Some("DONE".to_owned())))
    );

    let mut done = App::new(paths());
    let mut r = rec("id-1", "Alpha");
    r.todo = Some("DONE".to_owned());
    done.set_headlines(vec![r]);
    done.handle_stroke("t");
    assert_eq!(done.take_todo_request(), Some(("id-1".to_owned(), None)));
}

#[test]
fn p_cycles_the_priority_through_a_b_c_none() {
    let mut app = app();
    app.handle_stroke("p");
    assert_eq!(
        app.take_priority_request(),
        Some(("id-1".to_owned(), Some('A')))
    );

    let mut b = App::new(paths());
    let mut r = rec("id-1", "Alpha");
    r.priority = Some('B');
    b.set_headlines(vec![r]);
    b.handle_stroke("p");
    assert_eq!(
        b.take_priority_request(),
        Some(("id-1".to_owned(), Some('C')))
    );

    let mut c = App::new(paths());
    let mut r = rec("id-1", "Alpha");
    r.priority = Some('C');
    c.set_headlines(vec![r]);
    c.handle_stroke("p");
    assert_eq!(c.take_priority_request(), Some(("id-1".to_owned(), None)));
}

#[test]
fn y_opens_a_tag_minibuffer_prefilled_and_commits_a_list() {
    let mut app = App::new(paths());
    let mut r = rec("id-1", "Alpha");
    r.tags = vec!["work".to_owned(), "urgent".to_owned()];
    app.set_headlines(vec![r]);
    app.handle_stroke("y");
    assert_eq!(app.mode(), AppMode::EditTags);
    assert_eq!(app.query(), "work urgent", "prefilled with the live tags");
    app.handle_stroke("DEL");
    app.handle_stroke("RET");
    assert_eq!(
        app.take_tags_request(),
        Some((
            "id-1".to_owned(),
            vec!["work".to_owned(), "urgen".to_owned()]
        ))
    );
    assert_eq!(app.mode(), AppMode::Browse);
}

#[test]
fn tag_editing_can_be_cancelled() {
    let mut app = app();
    app.handle_stroke("y");
    app.handle_stroke("ESC");
    assert_eq!(app.mode(), AppMode::Browse);
    assert_eq!(app.take_tags_request(), None);
}

#[test]
fn clearing_the_tag_buffer_commits_an_empty_list() {
    let mut app = App::new(paths());
    let mut r = rec("id-1", "Alpha");
    r.tags = vec!["ab".to_owned()];
    app.set_headlines(vec![r]);
    app.handle_stroke("y");
    app.handle_stroke("DEL");
    app.handle_stroke("DEL");
    app.handle_stroke("RET");
    assert_eq!(
        app.take_tags_request(),
        Some(("id-1".to_owned(), Vec::new()))
    );
}

// === Structure. ===

#[test]
fn meta_h_and_meta_l_promote_and_demote() {
    let mut app = app();
    app.handle_stroke("M-h");
    assert_eq!(
        app.take_struct_request(),
        Some(("promote".to_owned(), "id-1".to_owned()))
    );
    app.handle_stroke("M-l");
    assert_eq!(
        app.take_struct_request(),
        Some(("demote".to_owned(), "id-1".to_owned()))
    );
}

#[test]
fn meta_j_moves_the_subtree_below_its_next_sibling() {
    let mut app = app();
    app.handle_stroke("M-j");
    assert_eq!(
        app.take_move_request(),
        Some(("id-1".to_owned(), "id-2".to_owned()))
    );
}

#[test]
fn meta_k_moves_the_subtree_above_its_previous_sibling() {
    let mut app = app();
    app.handle_stroke("l"); // headline list
    app.handle_stroke("j"); // cursor on "Beta"
    app.handle_stroke("ESC");
    app.handle_stroke("M-k");
    assert_eq!(
        app.take_move_request(),
        Some(("id-1".to_owned(), "id-2".to_owned())),
        "the previous sibling moves below us"
    );
}

#[test]
fn moving_past_the_ends_is_a_no_op() {
    let mut app = app();
    app.handle_stroke("M-k");
    assert_eq!(app.take_move_request(), None, "already first");
}

#[test]
fn meta_ret_adds_a_heading_after_the_cursor() {
    let mut app = app();
    app.handle_stroke("M-RET");
    assert_eq!(
        app.take_add_request(),
        Some(("id-1".to_owned(), "untitled".to_owned()))
    );
}

#[test]
fn z_folds_and_unfolds_through_the_visibility_property() {
    let mut app = app();
    app.handle_stroke("z");
    assert_eq!(
        app.take_property_request(),
        Some((
            "id-1".to_owned(),
            "VISIBILITY".to_owned(),
            "folded".to_owned()
        ))
    );

    let mut folded = App::new(paths());
    let mut r = rec("id-1", "Alpha");
    r.folded = true;
    folded.set_headlines(vec![r]);
    folded.handle_stroke("z");
    assert_eq!(
        folded.take_property_request(),
        Some(("id-1".to_owned(), "VISIBILITY".to_owned(), "all".to_owned()))
    );
}

// === Minibuffer commands reachable from Browse. ===

#[test]
fn a_r_and_d_reach_the_headline_from_browse() {
    let mut app = app();
    app.handle_stroke("a");
    assert_eq!(app.mode(), AppMode::AddHeadline);
    app.handle_stroke("ESC");

    app.handle_stroke("r");
    assert_eq!(app.mode(), AppMode::Rename);
    assert_eq!(app.query(), "Alpha", "rename starts from the live title");
    app.handle_stroke("ESC");

    app.handle_stroke("d");
    assert_eq!(app.mode(), AppMode::ConfirmDelete);
}

#[test]
fn i_opens_the_body_editor_from_browse() {
    let mut app = App::new(paths());
    let mut r = rec("id-1", "Alpha");
    r.body = "body text\n".to_owned();
    app.set_headlines(vec![r]);
    app.handle_stroke("i");
    assert_eq!(app.mode(), AppMode::EditBody);
    assert_eq!(app.buffer(), "body text\n");
}

#[test]
fn o_opens_the_property_editor_from_browse() {
    let mut app = app();
    app.handle_stroke("o");
    assert_eq!(app.mode(), AppMode::EditCell);
}

// === Panes. ===

#[test]
fn g_u_opens_the_undo_history_pane() {
    let mut app = app();
    app.set_history(vec![
        ("root".to_owned(), false),
        ("set body".to_owned(), true),
    ]);
    app.handle_stroke("g");
    app.handle_stroke("u");
    assert_eq!(app.mode(), AppMode::UndoHistory);
    assert_eq!(
        app.result_cursor(),
        1,
        "the cursor opens on the active node"
    );
    assert_eq!(app.history_rows().len(), 2);
}

#[test]
fn undo_history_ret_asks_the_driver_to_jump() {
    let mut app = app();
    app.set_history(vec![
        ("root".to_owned(), false),
        ("set body".to_owned(), true),
    ]);
    app.handle_stroke("g");
    app.handle_stroke("u");
    app.handle_stroke("k");
    app.handle_stroke("RET");
    assert_eq!(app.take_history_request(), Some(0));
    assert_eq!(app.mode(), AppMode::Browse);
}

#[test]
fn g_x_evaluates_the_block_under_the_blocks_cursor() {
    let mut app = App::new(paths());
    app.set_blocks(vec![(PathBuf::from("a.org"), "sh: echo hi".to_owned())]);
    app.handle_stroke("g");
    app.handle_stroke("x");
    assert_eq!(
        app.take_eval_request(),
        Some((PathBuf::from("a.org"), 0)),
        "the selected file's first block runs"
    );
}

// === Every advertised chord answers. ===

#[test]
fn commands_that_need_a_headline_say_so_when_there_is_none() {
    let mut app = App::new(paths());
    app.handle_stroke("t");
    assert!(
        app.status().contains("no headline"),
        "status was {:?}",
        app.status()
    );
    assert_eq!(app.take_todo_request(), None);
}
