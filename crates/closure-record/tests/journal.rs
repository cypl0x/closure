//! Append-only command journal: each executed command becomes an org
//! headline in journal.org, with a deterministic timestamp.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_record::{Journal, format_entry};
use tempfile::TempDir;

fn vault() -> TempDir {
    tempfile::tempdir().expect("tempdir")
}

#[test]
fn format_entry_is_an_org_headline() {
    let line = format_entry(1_700_000_000, "capture", "Buy milk");
    assert!(line.starts_with("* "), "got {line}");
    assert!(line.contains("capture"));
    assert!(line.contains("Buy milk"));
    assert!(line.ends_with('\n'));
}

#[test]
fn format_entry_escapes_newlines_in_detail() {
    let line = format_entry(0, "eval", "line1\nline2");
    assert_eq!(line.matches('\n').count(), 1, "single trailing newline");
    assert!(line.contains("line1 line2"));
}

#[test]
fn record_appends_to_journal_when_enabled() {
    let td = vault();
    let j = Journal::new(td.path(), true);
    j.record(1_700_000_000, "capture", "First").expect("record");
    j.record(1_700_000_060, "rename", "Second").expect("record");
    let disk = fs::read_to_string(td.path().join("journal.org")).expect("read");
    assert!(
        disk.find("First").unwrap() < disk.find("Second").unwrap(),
        "append order"
    );
    assert_eq!(disk.matches("* ").count(), 2);
}

#[test]
fn record_is_noop_when_disabled() {
    let td = vault();
    let j = Journal::new(td.path(), false);
    j.record(0, "capture", "x").expect("record");
    assert!(
        !td.path().join("journal.org").exists(),
        "disabled writes nothing"
    );
}

#[test]
fn journal_roundtrips_as_org() {
    let td = vault();
    let j = Journal::new(td.path(), true);
    j.record(1_700_000_000, "capture", "Note").expect("record");
    let disk = fs::read_to_string(td.path().join("journal.org")).expect("read");
    // Every entry is a level-1 headline; the file is valid org (starts
    // with a star, no stray content).
    assert!(disk.lines().all(|l| l.is_empty() || l.starts_with('*')));
}

#[test]
fn enabled_reflects_flag() {
    let td = vault();
    assert!(Journal::new(td.path(), true).enabled());
    assert!(!Journal::new(td.path(), false).enabled());
}

#[test]
fn entries_reads_back_recorded_lines() {
    let td = vault();
    let j = Journal::new(td.path(), true);
    j.record(1, "capture", "Alpha").expect("r");
    j.record(2, "rename", "Beta").expect("r");
    let got = j.entries().expect("entries");
    assert_eq!(got.len(), 2);
    assert!(got[0].contains("Alpha"));
    assert!(got[1].contains("Beta"));
}

#[test]
fn entries_on_missing_journal_is_empty() {
    let td = vault();
    let j = Journal::new(td.path(), true);
    assert!(j.entries().expect("entries").is_empty());
}

#[test]
fn filtered_entries_match_needle_case_insensitive() {
    let td = vault();
    let j = Journal::new(td.path(), true);
    j.record(1, "capture", "Buy milk").expect("r");
    j.record(2, "capture", "Call bank").expect("r");
    let got = j.filtered("BANK").expect("filtered");
    assert_eq!(got.len(), 1);
    assert!(got[0].contains("Call bank"));
}
