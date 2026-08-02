//! "polish keybindings for org table (not so good) … Navigating in an
//! existing table that I almost have to write by hand, because there
//! are all the creation/deletion/moving row/column shortcuts missing.
//! Please research these shortcuts in doom emacs and implement and
//! polish them."
//!
//! The chords were all there and all reaching the editor — org's own
//! set, `M-<arrow>` to move and `M-S-<arrow>` to add and remove. What
//! was missing is that they *worked*: on screen, `M-S-<down>` on a data
//! row inserted a second horizontal rule instead of an empty row, and
//! the realign that follows every table edit dropped the padding that
//! makes a column a column.
//!
//! So this is the table verbs held to what org does with them, on a
//! table with a rule in it — which is every table anybody writes.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{
    table_delete_column, table_insert_column, table_insert_hline, table_insert_row, table_kill_row,
    table_move_column, table_move_row,
};

/// The shape of every org table: a header, a rule, then rows.
const TABLE: &str = "\
| name | qty | note |
|------+-----+------|
| aaa  |   1 | one  |
| bbb  |   2 | two  |
";

/// The table's lines, for readable assertions.
fn lines(text: &str) -> Vec<String> {
    text.lines().map(ToOwned::to_owned).collect()
}

/// Is this an org horizontal rule?
fn is_rule(line: &str) -> bool {
    line.trim_start().starts_with("|-")
}

#[test]
fn inserting_a_row_gives_you_an_empty_row() {
    // Not another rule. `M-S-<down>` on `| aaa |` is how you add the
    // line you are about to type into.
    let out = table_insert_row(TABLE, 2).expect("inserted");
    let rows = lines(&out);
    assert!(
        !is_rule(&rows[2]),
        "a rule was inserted where a row belongs:\n{out}"
    );
    assert!(rows[2].starts_with('|'), "{out}");
    assert_eq!(rows[2].matches('|').count(), 4, "three empty cells: {out}");
    assert!(rows[3].contains("aaa"), "the row it was added above: {out}");
}

#[test]
fn inserting_a_row_keeps_the_rule_where_it_was() {
    let out = table_insert_row(TABLE, 3).expect("inserted");
    let rows = lines(&out);
    assert!(is_rule(&rows[1]), "the header rule moved:\n{out}");
    assert_eq!(
        rows.iter().filter(|l| is_rule(l)).count(),
        1,
        "one rule, not two:\n{out}"
    );
}

#[test]
fn killing_a_row_takes_the_row_and_not_the_rule() {
    let out = table_kill_row(TABLE, 2).expect("killed");
    assert!(!out.contains("aaa"), "{out}");
    assert!(out.contains("bbb"), "{out}");
    assert_eq!(
        lines(&out).iter().filter(|l| is_rule(l)).count(),
        1,
        "{out}"
    );
}

#[test]
fn a_rule_is_not_shifted_by_moving_a_row() {
    // `M-<down>` on the last data row has nowhere to go; it must not
    // swap the row with the rule above the header.
    let out = table_move_row(TABLE, 2, true).expect("moved");
    let rows = lines(&out);
    assert!(is_rule(&rows[1]), "the rule stayed put:\n{out}");
    assert!(rows[2].contains("bbb") && rows[3].contains("aaa"), "{out}");
}

#[test]
fn inserting_a_column_widens_every_row_including_the_rule() {
    let out = table_insert_column(TABLE, 0, 1).expect("inserted");
    for (i, row) in lines(&out).iter().enumerate() {
        // A rule separates its columns with `+`, a row with `|`.
        let sep = if is_rule(row) { '+' } else { '|' };
        let cells = row.trim().trim_matches('|').split(sep).count();
        assert_eq!(cells, 4, "row {i} has {cells} columns:\n{out}");
    }
}

#[test]
fn deleting_a_column_narrows_every_row_including_the_rule() {
    let out = table_delete_column(TABLE, 0, 1).expect("deleted");
    for (i, row) in lines(&out).iter().enumerate() {
        let sep = if is_rule(row) { '+' } else { '|' };
        let cells = row.trim().trim_matches('|').split(sep).count();
        assert_eq!(cells, 2, "row {i} has {cells} columns:\n{out}");
    }
    assert!(!out.contains("qty"), "{out}");
}

#[test]
fn moving_a_column_moves_the_header_with_it() {
    let out = table_move_column(TABLE, 2, 0, true).expect("moved");
    let rows = lines(&out);
    let header: Vec<&str> = rows[0].split('|').map(str::trim).collect();
    assert_eq!(header[1], "qty", "{out}");
    assert_eq!(header[2], "name", "{out}");
}

#[test]
fn a_rule_can_be_added_under_a_row() {
    let out = table_insert_hline(TABLE, 2).expect("ruled");
    let rows = lines(&out);
    assert_eq!(
        rows.iter().filter(|l| is_rule(l)).count(),
        2,
        "the one it had plus the new one:\n{out}"
    );
}

#[test]
fn every_edit_leaves_the_columns_lined_up() {
    // The realign is what makes a table readable, and it ran on every
    // edit already — badly. A column is as wide as its widest cell, and
    // every row agrees about it.
    for out in [
        table_insert_row(TABLE, 2).expect("row"),
        table_kill_row(TABLE, 2).expect("kill"),
        table_insert_column(TABLE, 0, 1).expect("column"),
        table_move_column(TABLE, 2, 0, true).expect("move column"),
        table_move_row(TABLE, 2, true).expect("move row"),
    ] {
        let widths: Vec<Vec<usize>> = out
            .lines()
            .filter(|l| !is_rule(l))
            .map(|l| l.split('|').map(str::len).collect())
            .collect();
        let first = &widths[0];
        for (i, row) in widths.iter().enumerate() {
            assert_eq!(row, first, "row {i} is a different shape:\n{out}");
        }
    }
}

#[test]
fn a_rule_is_as_wide_as_the_columns_it_rules() {
    let out = table_insert_column(TABLE, 0, 1).expect("inserted");
    let rows = lines(&out);
    let rule = rows.iter().find(|l| is_rule(l)).expect("a rule");
    let data = rows.iter().find(|l| l.contains("aaa")).expect("a row");
    assert_eq!(rule.len(), data.len(), "\n{rule}\n{data}\n");
}

#[test]
fn nothing_happens_outside_a_table() {
    let prose = "just a line\nand another\n";
    assert!(table_insert_row(prose, 0).is_none());
    assert!(table_kill_row(prose, 1).is_none());
    assert!(table_move_column(prose, 0, 0, true).is_none());
}

#[test]
fn a_column_of_numbers_is_right_aligned() {
    // org's own rule, and the reason a table of figures is readable at
    // all: a column whose cells are numbers lines up on its last digit,
    // everything else on its first letter. The realign was padding
    // every cell on the right, so `|   1 |` came back as `| 1   |` the
    // first time you touched the table.
    let out = table_insert_row(TABLE, 2).expect("inserted");
    let row = lines(&out)
        .into_iter()
        .find(|l| l.contains("aaa"))
        .expect("a data row");
    assert!(row.contains("|   1 |"), "qty is a number column:\n{out}");
    assert!(row.contains("| aaa "), "and name is not:\n{out}");
}

#[test]
fn a_column_of_mixed_content_stays_left_aligned() {
    // A header is not a number, so "mostly numbers" cannot mean "every
    // cell": org ignores the header and asks about the body.
    let mixed = "\
| name | note |
|------+------|
| aaa  | 1    |
| bbb  | two  |
";
    let out = table_insert_row(mixed, 2).expect("inserted");
    assert!(out.contains("| 1    |"), "left aligned:\n{out}");
}
