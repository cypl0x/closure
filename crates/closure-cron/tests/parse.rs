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

#[test]
fn scheduler_fires_matching_job() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let mut s = closure_cron::Scheduler::new();
    let count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&count);
    let spec = parse("0 9 * * * morning").unwrap();
    s.add(spec, move || {
        c.fetch_add(1, Ordering::SeqCst);
    });

    s.tick(0, 9, 1, 1, 1);
    s.tick(1, 9, 1, 1, 1);
    s.tick(0, 9, 5, 6, 3);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn job_filters_by_time() {
    let spec_a = parse("0 9 * * * morning").unwrap();
    let spec_b = parse("30 17 * * * evening").unwrap();
    let jobs = vec![
        closure_cron::Job::new(spec_a, "morning"),
        closure_cron::Job::new(spec_b, "evening"),
    ];
    let m = closure_cron::jobs_matching(&jobs, 0, 9, 1, 1, 1);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].command, "morning");
}

#[test]
fn step_field_matches_multiples() {
    let s = parse("*/15 * * * * step").unwrap();
    assert_eq!(s.minute, Field::Step(15));
    assert!(closure_cron::matches_time(&s, 0, 0, 1, 1, 1));
    assert!(closure_cron::matches_time(&s, 15, 0, 1, 1, 1));
    assert!(closure_cron::matches_time(&s, 30, 0, 1, 1, 1));
    assert!(!closure_cron::matches_time(&s, 7, 0, 1, 1, 1));
}

#[test]
fn list_field_matches_each_value() {
    let s = parse("0,15,30,45 * * * * quarter").unwrap();
    assert_eq!(s.minute, Field::List(vec![0, 15, 30, 45]));
    assert!(closure_cron::matches_time(&s, 15, 0, 1, 1, 1));
    assert!(!closure_cron::matches_time(&s, 14, 0, 1, 1, 1));
}

#[test]
fn range_field_matches_inclusive_endpoints() {
    let s = parse("* 9-17 * * * workhours").unwrap();
    assert_eq!(s.hour, Field::Range(9, 17));
    assert!(closure_cron::matches_time(&s, 0, 9, 1, 1, 1));
    assert!(closure_cron::matches_time(&s, 0, 17, 1, 1, 1));
    assert!(!closure_cron::matches_time(&s, 0, 8, 1, 1, 1));
    assert!(!closure_cron::matches_time(&s, 0, 18, 1, 1, 1));
}

#[test]
fn scheduler_skips_non_matching_job() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let mut s = closure_cron::Scheduler::new();
    let count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&count);
    let spec = parse("30 8 * * * cmd").unwrap();
    s.add(spec, move || {
        c.fetch_add(1, Ordering::SeqCst);
    });
    s.tick(0, 9, 1, 1, 1);
    s.tick(30, 9, 1, 1, 1);
    s.tick(31, 8, 1, 1, 1);
    assert_eq!(count.load(Ordering::SeqCst), 0);
}
