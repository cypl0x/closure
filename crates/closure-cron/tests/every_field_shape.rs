//! Every shape a cron field can take, and every sentence `describe`
//! can produce.
//!
//! `parse_field` has five branches — `*`, `*/N`, `lo-hi`, a
//! comma-list, and a bare number — each with its own way of being
//! malformed, and the existing tests reached about half of them.
//! `describe` has a phrase per common schedule and had only ever been
//! asked for one.
//!
//! These matter more than their size suggests. A cron field that
//! parses wrongly does not fail: it produces a schedule that fires at
//! the wrong time, or never, and the only way anybody finds out is by
//! noticing something did not happen.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_cron::{
    CronError, Field, Job, Scheduler, describe, expression, jobs_matching, matches_time,
    next_match_today, parse, parse_jobs,
};

#[test]
fn a_star_is_any() {
    let s = parse("* * * * * echo hi").expect("parse");
    assert!(matches!(s.minute, Field::Any));
    assert!(matches!(s.hour, Field::Any));
    assert_eq!(s.command, "echo hi");
}

#[test]
fn a_step_is_a_step_and_zero_is_refused() {
    let s = parse("*/15 * * * * tidy").expect("parse");
    assert!(matches!(s.minute, Field::Step(15)));
    // `*/0` would be "every zero minutes", which is either never or
    // always depending on how the matcher is written — neither is a
    // schedule anybody meant.
    assert!(matches!(parse("*/0 * * * * x"), Err(CronError::Number(_))));
    assert!(matches!(
        parse("*/abc * * * * x"),
        Err(CronError::Number(_))
    ));
}

#[test]
fn a_range_keeps_both_ends() {
    let s = parse("0 9-17 * * * work").expect("parse");
    assert!(matches!(s.hour, Field::Range(9, 17)));
    assert!(matches!(parse("0 a-17 * * * x"), Err(CronError::Number(_))));
    assert!(matches!(parse("0 9-b * * * x"), Err(CronError::Number(_))));
}

#[test]
fn a_list_keeps_every_entry() {
    let s = parse("0 6,12,18 * * * ping").expect("parse");
    match &s.hour {
        Field::List(v) => assert_eq!(v, &[6, 12, 18]),
        other => panic!("not a list: {other:?}"),
    }
    assert!(matches!(
        parse("0 6,x,18 * * * y"),
        Err(CronError::Number(_))
    ));
}

#[test]
fn a_bare_number_is_exact() {
    let s = parse("30 8 * * * standup").expect("parse");
    assert!(matches!(s.minute, Field::Exact(30)));
    assert!(matches!(s.hour, Field::Exact(8)));
    assert!(matches!(parse("xx 8 * * * y"), Err(CronError::Number(_))));
}

#[test]
fn a_line_with_too_few_fields_is_a_shape_error() {
    // Five fields and a command. Four of anything is not a schedule,
    // and guessing which one is missing would be inventing one.
    for short in ["* * * *", "* *", "*", ""] {
        assert!(
            matches!(parse(short), Err(CronError::Shape(_))),
            "`{short}` was accepted"
        );
    }
}

#[test]
fn the_command_keeps_its_spaces() {
    // The command is the rest of the line, not the sixth word. A
    // schedule that ran only the first word of its command would be a
    // quiet and very confusing failure.
    let s = parse("0 9 * * * capture a thought with spaces").expect("parse");
    assert_eq!(s.command, "capture a thought with spaces");
}

#[test]
fn describe_says_every_n_minutes() {
    let s = parse("*/5 * * * * x").expect("parse");
    assert_eq!(describe(&s), "every 5 minutes");
}

#[test]
fn describe_says_every_day_at_a_time() {
    let s = parse("30 8 * * * x").expect("parse");
    assert_eq!(describe(&s), "every day at 08:30");
}

#[test]
fn describe_names_the_weekday() {
    let s = parse("0 9 * * 1 x").expect("parse");
    assert_eq!(describe(&s), "every Monday at 09:00");
}

#[test]
fn describe_falls_back_to_the_expression_when_it_has_no_phrase_for_it() {
    // A day-of-week out of range has no name, and inventing one would
    // be worse than showing the user what they typed.
    let s = parse("0 9 * * 9 x").expect("parse");
    assert_eq!(describe(&s), expression(&s));

    // And a shape with no phrase at all.
    let odd = parse("0 9 1 6 * x").expect("parse");
    assert_eq!(describe(&odd), expression(&odd));
}

#[test]
fn matches_time_answers_each_field_kind() {
    let any = parse("* * * * * x").expect("parse");
    assert!(matches_time(&any, 0, 0, 1, 1, 0));

    let exact = parse("30 8 * * * x").expect("parse");
    assert!(matches_time(&exact, 30, 8, 1, 1, 0));
    assert!(!matches_time(&exact, 31, 8, 1, 1, 0));

    let step = parse("*/15 * * * * x").expect("parse");
    assert!(matches_time(&step, 30, 8, 1, 1, 0));
    assert!(!matches_time(&step, 31, 8, 1, 1, 0));

    let range = parse("0 9-17 * * * x").expect("parse");
    assert!(matches_time(&range, 0, 12, 1, 1, 0));
    assert!(!matches_time(&range, 0, 18, 1, 1, 0));

    let list = parse("0 6,12,18 * * * x").expect("parse");
    assert!(matches_time(&list, 0, 12, 1, 1, 0));
    assert!(!matches_time(&list, 0, 13, 1, 1, 0));
}

#[test]
fn next_match_today_wraps_around_the_clock_rather_than_giving_up() {
    let s = parse("0 9 * * * x").expect("parse");
    assert_eq!(next_match_today(&s, 0, 8), Some((0, 9)));
    // Past it today: it wraps. I expected None here — "there is no
    // later time today" — and the name encourages that reading. The
    // doc comment is explicit that the window is the next 24 hours,
    // and wrapping is the more useful answer for "when does this run
    // next", so the test pins the documented behaviour and this
    // comment is here because the name will mislead the next person
    // too.
    assert_eq!(next_match_today(&s, 0, 10), Some((0, 9)));
}

#[test]
fn next_match_today_gives_up_when_nothing_can_ever_match() {
    // The None branch, which needs a spec no clock can satisfy rather
    // than merely a time that has passed: hour 30 does not exist.
    let s = parse("0 30 * * * x").expect("parse");
    assert_eq!(next_match_today(&s, 0, 8), None);
}

#[test]
fn parse_jobs_reads_a_block_and_skips_what_is_not_a_job() {
    let content = "# a comment\n\
                   \n\
                   0 9 * * * capture morning\n\
                   30 17 * * * capture evening\n";
    let jobs = parse_jobs(content).expect("parse jobs");
    assert_eq!(jobs.len(), 2, "{jobs:?}");
    assert_eq!(jobs[0].spec.command, "capture morning");
}

#[test]
fn parse_jobs_reports_a_line_it_cannot_read() {
    assert!(parse_jobs("0 9 * * * fine\nnot a cron line\n").is_err());
}

#[test]
fn jobs_matching_returns_only_the_ones_due() {
    let jobs = parse_jobs("0 9 * * * morning\n30 17 * * * evening\n").expect("parse");
    let due = jobs_matching(&jobs, 0, 9, 1, 1, 0);
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].spec.command, "morning");

    assert!(
        jobs_matching(&jobs, 45, 3, 1, 1, 0).is_empty(),
        "something fired at 03:45 that was not scheduled for it"
    );
}

#[test]
fn a_job_can_be_built_directly_and_matches_the_same_way() {
    let spec = parse("0 9 * * * x").expect("parse");
    let job = Job::new(spec, "run me");
    assert!(job.matches(0, 9, 1, 1, 0));
    assert!(!job.matches(1, 9, 1, 1, 0));
    assert_eq!(job.command, "run me");
}

#[test]
fn a_default_scheduler_is_an_empty_one() {
    let mut s = Scheduler::default();
    // Nothing registered, so a tick at any time does nothing and must
    // not panic.
    s.tick(0, 9, 1, 1, 0);
}

#[test]
fn the_scheduler_fires_only_what_is_due() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let morning = Arc::new(AtomicUsize::new(0));
    let evening = Arc::new(AtomicUsize::new(0));
    let (m, e) = (Arc::clone(&morning), Arc::clone(&evening));

    let mut s = Scheduler::new();
    s.add(parse("0 9 * * * x").expect("parse"), move || {
        m.fetch_add(1, Ordering::SeqCst);
    });
    s.add(parse("30 17 * * * y").expect("parse"), move || {
        e.fetch_add(1, Ordering::SeqCst);
    });

    s.tick(0, 9, 1, 1, 0);
    assert_eq!(morning.load(Ordering::SeqCst), 1);
    assert_eq!(
        evening.load(Ordering::SeqCst),
        0,
        "the 17:30 job fired at 09:00"
    );

    s.tick(30, 17, 1, 1, 0);
    assert_eq!(morning.load(Ordering::SeqCst), 1);
    assert_eq!(evening.load(Ordering::SeqCst), 1);
}

#[test]
fn a_bad_field_is_reported_wherever_it_sits() {
    // `parse` propagates from five positions and only the first two
    // had ever failed in a test. A parser that validated the minute
    // and waved the month through would accept a schedule that can
    // never fire.
    for (line, which) in [
        ("x 9 * * * c", "minute"),
        ("0 x * * * c", "hour"),
        ("0 9 x * * c", "day of month"),
        ("0 9 * x * c", "month"),
        ("0 9 * * x c", "day of week"),
    ] {
        assert!(
            matches!(parse(line), Err(CronError::Number(_))),
            "a bad {which} was accepted: {line}"
        );
    }
}

#[test]
fn a_schedule_pinned_to_a_month_is_not_described_as_daily() {
    // `daily` is dom-is-any AND month-is-any, and each half has to be
    // able to say no on its own. With only one of them ever false, a
    // parenthesis in the wrong place would go unnoticed and a June-only
    // job would read as "every day at 09:00".
    let month_only = parse("0 9 * 6 * x").expect("parse");
    assert_eq!(describe(&month_only), expression(&month_only));

    let dom_only = parse("0 9 1 * * x").expect("parse");
    assert_eq!(describe(&dom_only), expression(&dom_only));
}

#[test]
fn a_scheduler_says_how_many_jobs_it_holds_rather_than_printing_them() {
    // The callbacks are boxed closures and cannot be shown, so Debug
    // reports the count. Worth asserting because a Debug that tried to
    // format the jobs would not compile, and one that said nothing at
    // all would make a scheduler indistinguishable from an empty one
    // in a log.
    let mut s = Scheduler::new();
    assert!(format!("{s:?}").contains('0'), "{s:?}");
    s.add(parse("0 9 * * * x").expect("parse"), || {});
    let shown = format!("{s:?}");
    assert!(shown.contains("Scheduler"), "{shown}");
    assert!(shown.contains('1'), "the job count is not in {shown}");
}
