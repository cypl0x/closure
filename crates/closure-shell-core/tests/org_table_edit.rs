//! Editing an org table the way org edits one.
//!
//! TAB realigning and stepping was the whole of it. Org's table
//! vocabulary is the arrows: `M-<left>`/`M-<right>` move the column you
//! are in, `M-<up>`/`M-<down>` move the row; add shift and they delete
//! and insert instead. `C-c -` rules a line under the header. All of it
//! is the *same key* as the outline command it shadows — `M-<left>` on
//! a headline promotes, on a table row it moves the column — which is
//! org's own dispatch, read out of `org-metaleft` and its siblings.
//!
//! These are the pure transforms; `tests/org_table_keys.rs` drives them
//! through the editor's keys.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{
    table_column_at, table_delete_column, table_insert_column, table_insert_hline,
    table_insert_row, table_kill_row, table_move_column, table_move_row, table_previous_cell,
};

const TABLE: &str = "\
| Deutsch | English | Indonesian |
|---------+---------+------------|
| Lauch   | Leek    | daun bawang |
| Zwiebel | Onion   | bawang      |
";

/// The table with the trailing prose a real buffer has around it.
fn buffer() -> String {
    format!("intro line\n{TABLE}after the table\n")
}

// === which column the cursor is in ===

#[test]
fn the_column_is_counted_by_pipes_before_the_cursor() {
    let row = "| a | b | c |";
    assert_eq!(table_column_at(row, 2), Some(0));
    assert_eq!(table_column_at(row, 6), Some(1));
    assert_eq!(table_column_at(row, 10), Some(2));
}

#[test]
fn a_line_that_is_not_a_row_has_no_column() {
    assert_eq!(table_column_at("prose", 2), None);
}

// === moving ===

#[test]
fn a_column_moves_right_and_takes_the_rule_with_it() {
    let out = table_move_column(&buffer(), 1, 0, true).expect("moved");
    let cells = |line: &str| -> Vec<String> {
        line.trim_matches('|')
            .split('|')
            .map(|c| c.trim().to_owned())
            .collect()
    };
    let first = out.lines().nth(1).expect("header");
    assert_eq!(
        cells(first),
        vec!["English", "Deutsch", "Indonesian"],
        "the header swapped: {first}"
    );
    let body = out.lines().nth(3).expect("row");
    assert_eq!(
        cells(body),
        vec!["Leek", "Lauch", "daun bawang"],
        "and so did the row: {body}"
    );
    assert!(
        out.lines().nth(2).is_some_and(|l| l.contains("---")),
        "the rule is still a rule"
    );
    assert!(out.starts_with("intro line\n"), "and the prose is untouched");
    assert!(out.ends_with("after the table\n"));
}

#[test]
fn the_leftmost_column_does_not_move_left() {
    assert_eq!(table_move_column(&buffer(), 1, 0, false), None);
}

#[test]
fn the_rightmost_column_does_not_move_right() {
    assert_eq!(table_move_column(&buffer(), 1, 2, true), None);
}

#[test]
fn a_row_moves_down_past_its_neighbour() {
    let out = table_move_row(&buffer(), 3, true).expect("moved");
    let rows: Vec<&str> = out.lines().collect();
    assert!(rows[3].contains("Zwiebel"), "they swapped: {:?}", rows[3]);
    assert!(rows[4].contains("Lauch"));
}

#[test]
fn a_row_will_not_move_out_of_its_table() {
    assert_eq!(table_move_row(&buffer(), 1, false), None, "off the top");
    assert_eq!(table_move_row(&buffer(), 4, true), None, "off the bottom");
}

// === inserting and deleting ===

#[test]
fn a_column_is_inserted_before_the_one_you_are_in() {
    let out = table_insert_column(&buffer(), 1, 1).expect("inserted");
    let header = out.lines().nth(1).expect("header");
    assert!(
        header.starts_with("| Deutsch |  | English |"),
        "an empty column: {header}"
    );
    assert_eq!(
        out.lines().nth(3).map(|l| l.matches('|').count()),
        Some(5),
        "every row grew, rules included"
    );
}

#[test]
fn a_column_is_deleted_from_every_row() {
    let out = table_delete_column(&buffer(), 1, 1).expect("deleted");
    assert!(!out.contains("English"), "{out}");
    assert!(!out.contains("Leek"), "{out}");
    assert!(out.contains("Deutsch"), "and the others stayed: {out}");
}

#[test]
fn deleting_the_last_column_leaves_the_table_alone() {
    let one = "| only |\n";
    assert_eq!(table_delete_column(one, 0, 0), None);
}

#[test]
fn a_row_is_inserted_empty_above_the_one_you_are_in() {
    let out = table_insert_row(&buffer(), 3).expect("inserted");
    let rows: Vec<&str> = out.lines().collect();
    assert!(
        rows[3].trim().starts_with('|') && !rows[3].contains("Lauch"),
        "a blank row went in above: {:?}",
        rows[3]
    );
    assert!(rows[4].contains("Lauch"), "and pushed it down");
}

#[test]
fn a_row_is_killed_whole() {
    let out = table_kill_row(&buffer(), 3).expect("killed");
    assert!(!out.contains("Lauch"), "{out}");
    assert!(out.contains("Zwiebel"), "{out}");
    assert!(out.contains("after the table"), "{out}");
}

#[test]
fn the_last_row_of_a_table_can_still_be_killed() {
    let one = "| only |\n";
    let out = table_kill_row(one, 0).expect("killed");
    assert_eq!(out, "", "the table is gone rather than half there");
}

#[test]
fn a_rule_is_inserted_under_the_row_you_are_in() {
    let out = table_insert_hline(&buffer(), 3).expect("ruled");
    let rows: Vec<&str> = out.lines().collect();
    assert!(rows[3].contains("Lauch"));
    assert!(
        rows[4].starts_with('|') && rows[4].contains("---"),
        "a rule below it: {:?}",
        rows[4]
    );
}

#[test]
fn nothing_happens_off_a_table() {
    let text = "just prose\n";
    assert_eq!(table_move_column(text, 0, 0, true), None);
    assert_eq!(table_move_row(text, 0, true), None);
    assert_eq!(table_insert_column(text, 0, 0), None);
    assert_eq!(table_delete_column(text, 0, 0), None);
    assert_eq!(table_insert_row(text, 0), None);
    assert_eq!(table_kill_row(text, 0), None);
    assert_eq!(table_insert_hline(text, 0), None);
}

// === S-TAB, the other direction ===

#[test]
fn the_previous_cell_is_the_one_before_the_cursor() {
    let row = "| a | b | c |";
    assert_eq!(table_previous_cell(row, 10), Some(6), "from c back to b");
    assert_eq!(table_previous_cell(row, 6), Some(2), "from b back to a");
    assert_eq!(table_previous_cell(row, 2), None, "and no further");
}

// === every transform leaves a table a table ===

#[test]
fn every_edit_leaves_an_aligned_table() {
    let edits: Vec<Option<String>> = vec![
        table_move_column(&buffer(), 1, 0, true),
        table_move_row(&buffer(), 3, true),
        table_insert_column(&buffer(), 1, 1),
        table_delete_column(&buffer(), 1, 1),
        table_insert_row(&buffer(), 3),
        table_insert_hline(&buffer(), 3),
    ];
    for out in edits.into_iter().flatten() {
        for line in out.lines().filter(|l| l.starts_with('|')) {
            assert!(line.ends_with('|'), "row is closed: {line:?}");
        }
        let widths: Vec<usize> = out
            .lines()
            .filter(|l| l.starts_with('|'))
            .map(str::chars)
            .map(Iterator::count)
            .collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "every row is the same width: {widths:?}\n{out}"
        );
    }
}
