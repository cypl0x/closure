//! Q3-V3 in the shell: clocking in, out, and finding the clock again.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const SRC: &str = "\
* TODO Write the parser
:PROPERTIES:
:ID: 01HQSCLOCK0000000000000001
:END:
notes
* TODO Something else
:PROPERTIES:
:ID: 01HQSCLOCK0000000000000002
:END:
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    app.set_now("2026-07-28 09:15");
    (dir, Shell::new(vault), app)
}

#[test]
fn v3_clock_in_and_out_write_the_logbook() {
    let (dir, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQSCLOCK0000000000000001"));

    app.run(&mut shell, "clock-in");
    assert!(
        app.running_clock(&shell)
            .is_some_and(|s| s.contains("Write the parser")),
        "the status line says what is running: {:?}",
        app.running_clock(&shell)
    );

    app.set_now("2026-07-28 10:45");
    app.run(&mut shell, "clock-out");
    assert_eq!(app.running_clock(&shell), None);

    let src = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(src.contains("=>  1:30"), "{src}");
}

#[test]
fn v3_clock_out_stops_the_clock_wherever_it_is_running() {
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQSCLOCK0000000000000001"));
    app.run(&mut shell, "clock-in");

    // Wander off to another note, then stop the clock.
    assert!(app.select_by_id(&shell, "01HQSCLOCK0000000000000002"));
    app.set_now("2026-07-28 10:00");
    app.run(&mut shell, "clock-out");

    assert_eq!(app.running_clock(&shell), None, "it stopped the right one");
}

#[test]
fn v3_clock_goto_puts_the_cursor_on_the_running_note() {
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQSCLOCK0000000000000001"));
    app.run(&mut shell, "clock-in");
    assert!(app.select_by_id(&shell, "01HQSCLOCK0000000000000002"));

    app.run(&mut shell, "clock-goto");
    assert_eq!(
        app.detail(&shell).map(|d| d.title),
        Some("Write the parser".to_owned())
    );
}

#[test]
fn v3_cancel_leaves_no_time_behind() {
    let (dir, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQSCLOCK0000000000000001"));
    app.run(&mut shell, "clock-in");
    app.run(&mut shell, "clock-cancel");

    assert_eq!(app.running_clock(&shell), None);
    let src = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(!src.contains("CLOCK:"), "{src}");
}

#[test]
fn v3_the_report_totals_what_was_clocked() {
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQSCLOCK0000000000000001"));
    app.run(&mut shell, "clock-in");
    app.set_now("2026-07-28 10:45");
    app.run(&mut shell, "clock-out");

    let report = ModalApp::clock_report(&shell);
    assert_eq!(report.first(), Some(&("Write the parser".to_owned(), 90)));
}

#[test]
fn v3_clocking_in_with_nothing_selected_says_so() {
    let (_d, mut shell, mut app) = fixture();
    app.clear_selection();
    app.run(&mut shell, "clock-in");
    assert_eq!(app.running_clock(&shell), None);
    assert!(!app.status().is_empty());
}
