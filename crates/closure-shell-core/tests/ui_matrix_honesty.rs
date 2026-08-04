//! "5 of the 8 shells aren't shells. … The doc comments are honest
//! about this ('display-bound, feature-gated follow-up'), but
//! `ui-matrix` and the crate descriptions present them as peers of
//! gpui (10,215 lines). One real GUI, one real TUI, six mappings."
//!
//! Reported 2026-08-04 in the Opus 5 review. The matrix is not wrong —
//! `gtk_view` and `qml_view` really do map every node kind — but a
//! table of ticks with nothing to distinguish "you can run this" from
//! "this is a function that returns a widget tree" reads as the first.
//!
//! So the table says which is which. The claim it makes is small and
//! checkable: a column marked as runnable is one you can start.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

#[test]
fn the_matrix_says_which_of_them_you_can_actually_run() {
    let table = closure_shell_core::ui_matrix_table();
    assert!(
        table.contains("mapping"),
        "the table does not distinguish a shell from a widget-tree mapping:\n{table}"
    );
}

#[test]
fn the_two_that_ship_are_named_as_the_two_that_ship() {
    // gpui is the reference shell and the TUI is the other one that
    // runs. Everything else is a `ViewTree` mapping behind a feature
    // gate that CI never executes.
    let table = closure_shell_core::ui_matrix_table();
    let legend = table
        .split("Legend:")
        .nth(1)
        .expect("the table has a legend");
    assert!(legend.contains("gpui"), "{legend}");
    assert!(
        legend.contains("TUI") || legend.contains("terminal"),
        "{legend}"
    );
}

#[test]
fn a_mapping_is_still_listed_rather_than_hidden() {
    // Honesty is not deletion: the mappings are real work and the
    // matrix is how you see a node kind nobody renders yet.
    let table = closure_shell_core::ui_matrix_table();
    for column in ["GTK", "QT", "WEB"] {
        assert!(table.contains(column), "{column} vanished from the table");
    }
}
