//! "The bottom left corner shows body saved, when I save a body. Does
//! the whole inbox.org file gets rewritten if I save a body? If that's
//! the case please include the filename as well. Because I don't know
//! where and what body gets written. Make it more intuitive. Can you
//! make it human readable? Like bytes becoming kb, becoming mb etc.?"
//!
//! The answer to the question is yes: every mutation writes the whole
//! file from the in-memory document. So "body saved" was hiding the
//! part worth knowing — a note lives in a file with other notes in it,
//! and the message never said which file it had just rewritten.
//!
//! Saying the size is the honest completion of that: it is the size of
//! the *file*, because that is what was written, not the size of the
//! body the user edited.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::human_bytes;

#[test]
fn bytes_stay_bytes_while_they_are_readable() {
    assert_eq!(human_bytes(0), "0 B");
    assert_eq!(human_bytes(1), "1 B");
    assert_eq!(human_bytes(999), "999 B");
}

#[test]
fn a_kilobyte_reads_as_one_kb() {
    // 1024, not 1000: this is a file on disk, and every other tool the
    // user has (ls -h, du -h) says KB for 1024.
    assert_eq!(human_bytes(1024), "1.0 KB");
    assert_eq!(human_bytes(1536), "1.5 KB");
}

#[test]
fn a_megabyte_reads_as_mb() {
    assert_eq!(human_bytes(1024 * 1024), "1.0 MB");
    assert_eq!(human_bytes(5 * 1024 * 1024 + 512 * 1024), "5.5 MB");
}

#[test]
fn a_gigabyte_does_not_overflow_into_nonsense() {
    // A vault is plain text, so this is unlikely — but a unit table
    // that runs out silently prints a number with no unit at all.
    assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
}

#[test]
fn the_step_between_units_does_not_read_as_a_thousand_and_something() {
    // 1023 B is not "1.0 KB" and 1024 is not "1024 B": the boundary is
    // where a rounding bug shows up as a size that looks wrong.
    assert_eq!(human_bytes(1023), "1023 B");
    assert_eq!(human_bytes(1024 * 1024 - 1), "1024.0 KB");
}

#[test]
fn a_save_message_names_the_file_and_its_size() {
    // The report itself: "I don't know where and what body gets
    // written."
    let msg = closure_shell_core::save_report("inbox.org", 2048);
    assert!(msg.contains("inbox.org"), "no file named: {msg}");
    assert!(msg.contains("2.0 KB"), "no readable size: {msg}");
}

#[test]
fn the_save_message_says_the_whole_file_was_written() {
    // Because it was. Every mutation rewrites the file from the
    // in-memory document, and a message that said "body saved" let the
    // user believe otherwise — which is exactly the thing they asked
    // about.
    let msg = closure_shell_core::save_report("notes.org", 10);
    assert!(
        msg.contains("wrote"),
        "the message does not say what it did: {msg}"
    );
}
