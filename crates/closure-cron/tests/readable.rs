//! "[#C] top notch UI for job scheduler (cron like syntax?)" and
//! "implement the job scheduler ('cron') stack".
//!
//! The Jobs pane printed `format!("{:?}", job.spec)` — a Rust Debug
//! dump — where the schedule should be:
//!
//!     CronSpec { minute: Exact(0), hour: Exact(9), dom: Any, … }
//!
//! Nobody reads that, and it is not the thing they wrote either. A
//! schedule has two readable forms and the pane had neither: the cron
//! expression it came from, and a sentence saying when it fires.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_cron::parse;

#[test]
fn a_spec_prints_as_the_expression_it_came_from() {
    // Round-tripping the five fields is what makes the pane show you
    // what you wrote rather than what the parser made of it.
    for src in [
        "0 9 * * * agenda",
        "*/15 * * * * sync-now",
        "30 6 1 * * archive",
        "0 0 * * 0 backup",
        "0 8,12,18 * * * agenda",
        "0 9-17 * * * ping",
    ] {
        let spec = parse(src).expect(src);
        let printed = closure_cron::expression(&spec);
        let want = src.rsplit_once(' ').expect("a command").0;
        assert_eq!(printed, want, "for {src}");
    }
}

#[test]
fn a_spec_also_reads_as_a_sentence() {
    // The pane has room for one line, and "every day at 09:00" is what
    // a person checks against what they meant.
    let cases = [
        ("0 9 * * * agenda", "every day at 09:00"),
        ("*/15 * * * * sync", "every 15 minutes"),
        ("0 0 * * 0 backup", "every Sunday at 00:00"),
    ];
    for (src, want) in cases {
        let spec = parse(src).expect(src);
        let said = closure_cron::describe(&spec);
        assert_eq!(said, want, "for {src}");
    }
}

#[test]
fn a_schedule_nobody_has_a_sentence_for_falls_back_to_the_expression() {
    // Better an expression than a wrong sentence: the fallback must be
    // the thing that is always true.
    let spec = parse("5 3 12 6 2 odd").expect("parses");
    let said = closure_cron::describe(&spec);
    assert!(
        said.contains("5 3 12 6 2"),
        "it invented a sentence for a schedule it does not have words for: {said}"
    );
}

#[test]
fn describing_never_panics_on_anything_that_parsed() {
    // I5, over the shapes the parser accepts.
    for src in [
        "* * * * * a",
        "59 23 31 12 6 a",
        "0 0 1 1 0 a",
        "*/1 */1 */1 */1 */1 a",
        "1,2,3 4-5 * * * a",
    ] {
        let spec = parse(src).expect(src);
        let _ = closure_cron::describe(&spec);
        let _ = closure_cron::expression(&spec);
    }
}

#[test]
fn a_line_that_is_not_a_spec_is_an_error_not_a_shrug() {
    // Strict on purpose, and the strictness is right: a typo in a cron
    // line should be reported, not skipped, or a job silently never
    // runs.
    //
    // What was wrong is who called it. The shell handed it a whole org
    // *document*, which begins `* Something` — two fields, not a spec —
    // so the parse failed and the caller's `.ok()` dropped every job in
    // the file. The Jobs pane was empty in any vault with a headline in
    // it, which is all of them. Fixed by parsing the `#+BEGIN_SRC cron`
    // blocks, which is where jobs are actually declared.
    assert!(closure_cron::parse_jobs("* Jobs\n0 9 * * * agenda\n").is_err());
    assert!(closure_cron::parse_jobs("0 9 * * * agenda\n").is_ok());
}
