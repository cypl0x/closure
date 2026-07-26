//! Org table editing.
//!
//! In org, a table is text you type between pipes and TAB moves to the
//! next cell, realigning the whole thing as it goes. That realignment
//! is the feature: it is what makes a plain-text table readable, and
//! doing it by hand is what makes plain-text tables miserable without
//! it.
//!
//! All of this is pure text-to-text, so it belongs in the core where
//! every shell gets it, and it is testable without a cursor.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{align_table, next_table_cell, table_bounds};

// === finding the table around the cursor ===

const BODY: &str = "intro\n\
                    | a | b |\n\
                    |---+---|\n\
                    | c | d |\n\
                    tail\n";

#[test]
fn the_table_around_a_row_is_its_contiguous_run() {
    // Lines 1..=3 are the table; the prose either side is not.
    assert_eq!(table_bounds(BODY, 1), Some(1..4));
    assert_eq!(table_bounds(BODY, 2), Some(1..4), "from the rule");
    assert_eq!(table_bounds(BODY, 3), Some(1..4), "from the last row");
}

#[test]
fn prose_is_not_in_a_table() {
    assert_eq!(table_bounds(BODY, 0), None);
    assert_eq!(table_bounds(BODY, 4), None);
}

#[test]
fn a_blank_line_ends_a_table() {
    let body = "| a |\n\n| b |\n";
    assert_eq!(table_bounds(body, 0), Some(0..1));
    assert_eq!(
        table_bounds(body, 2),
        Some(2..3),
        "a second, separate table"
    );
}

#[test]
fn a_line_index_past_the_end_is_not_a_table() {
    assert_eq!(table_bounds(BODY, 99), None);
}

// === alignment ===

#[test]
fn columns_are_padded_to_their_widest_cell() {
    let table = "| name | qty |\n| a | 100 |\n";
    assert_eq!(
        align_table(table),
        "| name | qty |\n| a    | 100 |\n",
        "every column as wide as its widest cell"
    );
}

#[test]
fn a_rule_row_is_redrawn_to_match() {
    let table = "| name | qty |\n|-+-|\n| a | 100 |\n";
    assert_eq!(
        align_table(table),
        "| name | qty |\n|------+-----|\n| a    | 100 |\n",
        "the rule spans the aligned columns"
    );
}

#[test]
fn ragged_cell_counts_do_not_lose_content() {
    // A half-typed row has fewer cells than its neighbours; padding it
    // out is fine, dropping the ones it does have is not.
    let aligned = align_table("| a | b | c |\n| x |\n");
    assert!(aligned.contains('x'), "{aligned}");
    assert_eq!(aligned.lines().count(), 2);
}

#[test]
fn alignment_is_idempotent() {
    // Pressing TAB twice must not keep growing the table.
    let once = align_table("| name | qty |\n|-+-|\n| a | 100 |\n");
    assert_eq!(align_table(&once), once);
}

#[test]
fn surrounding_whitespace_in_cells_is_normalised() {
    assert_eq!(align_table("|a|b|\n"), "| a | b |\n");
    assert_eq!(align_table("|   a   |  b |\n"), "| a | b |\n");
}

#[test]
fn a_unicode_cell_is_measured_in_characters() {
    // Padding by bytes would misalign anything non-ASCII.
    let aligned = align_table("| \u{e4}\u{f6}\u{fc} | x |\n| a | y |\n");
    let widths: Vec<usize> = aligned
        .lines()
        .map(|l| l.split('|').nth(1).unwrap_or("").chars().count())
        .collect();
    assert_eq!(widths[0], widths[1], "columns line up: {aligned:?}");
}

#[test]
fn an_empty_table_is_left_alone() {
    assert_eq!(align_table(""), "");
}

// === TAB between cells ===

#[test]
fn tab_moves_to_the_next_cell_on_the_same_row() {
    // Cursor in the first cell of `| a | b |` -> start of the second.
    let row = "| a | b |";
    let at = row.find('a').expect("a");
    let next = next_table_cell(row, at).expect("a next cell");
    assert_eq!(&row[next..=next], "b");
}

#[test]
fn tab_in_the_last_cell_has_nowhere_to_go_on_this_row() {
    let row = "| a | b |";
    let at = row.find('b').expect("b");
    assert_eq!(
        next_table_cell(row, at),
        None,
        "the caller wraps to the next row"
    );
}

#[test]
fn tab_lands_on_the_content_not_the_padding() {
    let row = "| a    | bcd |";
    let at = row.find('a').expect("a");
    let next = next_table_cell(row, at).expect("next");
    assert_eq!(&row[next..next + 3], "bcd", "skips the leading spaces");
}

#[test]
fn tab_on_a_non_table_line_does_nothing() {
    assert_eq!(next_table_cell("just prose", 2), None);
}

#[test]
fn tab_into_an_empty_cell_still_moves() {
    let row = "| a |  | c |";
    let at = row.find('a').expect("a");
    let next = next_table_cell(row, at).expect("next");
    assert!(next < row.len(), "somewhere in the empty cell: {next}");
    assert!(
        row[next..].starts_with(' ') || row[next..].starts_with('|'),
        "the empty cell, not past it: {:?}",
        &row[next..]
    );
}

// === TAB in the editor ===
//
// TAB already expanded tempo snippets and indented. Inside a table it
// has to do the org thing instead: realign, then move to the next
// cell — wrapping to the row below when there is no next cell.

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

fn editing(body: &str) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        format!("* Note\n:PROPERTIES:\n:ID: 01HQTABLE0000000000001\n:END:\n{body}"),
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let (mut shell, mut app) = (Shell::new(vault), ModalApp::new(InputMode::Doom));
    app.select(0, &shell);
    app.run(&mut shell, "edit-body");
    (dir, shell, app)
}

#[test]
fn tab_in_a_table_realigns_it() {
    let (_d, mut shell, mut app) = editing("| name | qty |\n| a | 100 |\n");
    app.body_click(0, 3); // inside "name"
    app.on_key(&mut shell, "tab", false, false, None);
    assert!(
        app.body_buffer().contains("| a    | 100 |"),
        "realigned: {:?}",
        app.body_buffer()
    );
}

#[test]
fn tab_in_a_table_moves_to_the_next_cell() {
    let (_d, mut shell, mut app) = editing("| a | b |\n");
    app.body_click(0, 2); // on "a"
    app.on_key(&mut shell, "tab", false, false, None);
    let (line, col) = app.body_cursor();
    assert_eq!(line, 0, "same row");
    let text: String = app.body_buffer().lines().next().unwrap_or("").to_owned();
    let at: usize = text.chars().take(col).map(char::len_utf8).sum();
    assert_eq!(&text[at..=at], "b", "landed on the next cell: {text:?}");
}

#[test]
fn tab_in_the_last_cell_wraps_to_the_row_below() {
    let (_d, mut shell, mut app) = editing("| a | b |\n| c | d |\n");
    app.body_click(0, 6); // on "b", the last cell of row 0
    app.on_key(&mut shell, "tab", false, false, None);
    assert_eq!(app.body_cursor().0, 1, "wrapped to the next row");
}

#[test]
fn tab_outside_a_table_still_indents_or_expands() {
    // The existing behaviour must survive: TAB is only special inside
    // a table.
    let (_d, mut shell, mut app) = editing("plain prose\n");
    app.body_click(0, 0);
    let before = app.body_buffer().to_owned();
    app.on_key(&mut shell, "tab", false, false, None);
    assert_ne!(app.body_buffer(), before, "TAB still did its old job");
}
