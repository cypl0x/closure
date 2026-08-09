//! "command-palette (M-x) and all of the filtered popup views should
//! show the current selected item index next to the count of the
//! (filtered or unfiltered) item count."
//!
//! The screenshot is vertico in the user's Doom: `1/20` sitting beside
//! the `M-x` prompt, where 20 is what survived the filter. closure
//! showed the count alone — "60 headlines in this file" — so the list
//! told you how much there was and never where you were in it.
//!
//! On [`PickerView`] rather than in a shell, because every popup is
//! the same view and "which of them am I on" is the same question in
//! all of them.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

fn vault_of(n: usize) -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().unwrap();
    let mut org = String::new();
    for i in 0..n {
        use std::fmt::Write as _;
        let _ = write!(
            org,
            "* Headline number {i:02}\n:PROPERTIES:\n:ID: 01PICKPOS{i:015}\n:END:\n"
        );
    }
    std::fs::write(dir.path().join("notes.org"), org).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault))
}

#[test]
fn the_position_is_one_based_and_over_the_row_count() {
    let (_d, mut shell) = vault_of(60);
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut shell, "list-headlines");
    let view = app.picker_view(&shell).expect("a picker");
    assert_eq!(view.position(), "1/60", "the first row is 1, not 0");
}

#[test]
fn it_moves_with_the_cursor() {
    let (_d, mut shell) = vault_of(60);
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut shell, "list-headlines");
    for _ in 0..30 {
        app.on_key(&mut shell, "down", false, false, None);
    }
    let view = app.picker_view(&shell).expect("a picker");
    assert_eq!(view.position(), "31/60");
}

#[test]
fn the_count_is_what_survived_the_filter() {
    // vertico's `1/20` after typing `agent sh` is 20 matches, not the
    // whole command list — the number that is useful is the one you
    // are choosing from.
    let (_d, mut shell) = vault_of(60);
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut shell, "list-headlines");
    for c in "number 1".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    let view = app.picker_view(&shell).expect("a picker");
    let n = view.rows.len();
    assert!(n < 60, "the filter matched everything: {n}");
    assert_eq!(view.position(), format!("1/{n}"));
}

#[test]
fn nothing_matched_is_zero_of_zero() {
    let (_d, mut shell) = vault_of(10);
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut shell, "list-headlines");
    for c in "zzzznothing".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    let view = app.picker_view(&shell).expect("a picker");
    assert_eq!(view.position(), "0/0", "there is no first of nothing");
}
