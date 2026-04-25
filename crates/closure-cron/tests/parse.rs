#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_cron::{Field, parse};

#[test]
fn star_fields_become_any() {
    let s = parse("* * * * * mycmd").unwrap();
    assert_eq!(s.minute, Field::Any);
    assert_eq!(s.hour, Field::Any);
    assert_eq!(s.dom, Field::Any);
    assert_eq!(s.month, Field::Any);
    assert_eq!(s.dow, Field::Any);
    assert_eq!(s.command, "mycmd");
}

#[test]
fn numeric_fields_parsed() {
    let s = parse("0 9 * * * morning-cmd").unwrap();
    assert_eq!(s.minute, Field::Exact(0));
    assert_eq!(s.hour, Field::Exact(9));
    assert_eq!(s.command, "morning-cmd");
}

#[test]
fn missing_command_is_error() {
    let err = parse("0 9 * * *").unwrap_err();
    let _ = err; // shape error
}

#[test]
fn bad_number_is_error() {
    let err = parse("xyz * * * * cmd").unwrap_err();
    let _ = err;
}

#[test]
fn command_with_spaces_kept_intact() {
    let s = parse("* * * * * echo hello world").unwrap();
    assert_eq!(s.command, "echo hello world");
}
