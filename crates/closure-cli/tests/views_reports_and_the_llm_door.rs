//! The reporting commands, and what the LLM commands do with no key.
//!
//! `view`, `rank`, `lists`, `clock`, `validate` and `set-planning` are
//! the last of the local surface. `ask` and `chat` are the LLM door:
//! they need a provider, which a hermetic test cannot have — but the
//! path that matters most is the one where there is no key, because
//! that is what every user hits first and it must be a sentence rather
//! than a panic or a hang.
//!
//! No test here talks to a network. `ask` is given an empty API key
//! environment on purpose, and asserted to fail *quickly and legibly*.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::process::Command;
use std::time::{Duration, Instant};

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_closure");
const ID: &str = "01VIEWCMD00000000001";

fn run(args: &[&str]) -> std::process::Output {
    Command::new(BIN).args(args).output().expect("spawn")
}

fn text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn vault() -> TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        format!(
            "* TODO Ship the parser :work:\n\
             SCHEDULED: <2026-06-20 Sat>\n\
             :PROPERTIES:\n:ID: {ID}\n:EFFORT: 3h\n:END:\n\
             a body with a [[https://example.com][link]]\n\
             CLOCK: [2026-06-19 Fri 09:00]--[2026-06-19 Fri 10:30] =>  1:30\n\
             - [ ] an unchecked item\n\
             - [X] a checked one\n\
             \n* DONE Something else :home:\n\
             :PROPERTIES:\n:ID: 01VIEWCMD00000000002\n:END:\n\
             another body\n"
        ),
    )
    .expect("write");
    d
}

#[test]
fn view_renders_the_columns_it_was_asked_for() {
    let d = vault();
    let out = run(&[
        "view",
        d.path().to_str().unwrap(),
        ":from tag:work :columns title,todo :sort title",
    ]);
    assert!(out.status.success(), "{}", text(&out));
    let t = text(&out);
    assert!(t.contains("Ship the parser"), "{t}");
    assert!(
        !t.contains("Something else"),
        "`:from tag:work` returned a row tagged home: {t}"
    );
}

#[test]
fn view_with_no_params_still_produces_a_table() {
    let d = vault();
    let out = run(&["view", d.path().to_str().unwrap()]);
    assert!(!text(&out).contains("panicked"), "{}", text(&out));
}

#[test]
fn view_with_a_formula_column_runs_it_and_names_it() {
    // The formula is a shell command per row. Hermetic: it is `wc -c`
    // on what closure hands it, not anything from outside.
    let d = vault();
    let out = run(&[
        "view",
        d.path().to_str().unwrap(),
        ":columns title",
        "--formula",
        "wc -c",
        "--formula-name",
        "size",
    ]);
    if out.status.success() {
        assert!(text(&out).contains("size"), "{}", text(&out));
    } else {
        assert!(!text(&out).contains("panicked"), "{}", text(&out));
    }
}

#[test]
fn view_rejects_params_it_cannot_parse_rather_than_guessing() {
    let d = vault();
    let out = run(&["view", d.path().to_str().unwrap(), ":columns"]);
    assert!(!text(&out).contains("panicked"), "{}", text(&out));
}

#[test]
fn rank_orders_files_and_headlines_by_each_measure() {
    let d = vault();
    let dir = d.path().to_str().unwrap();
    let file = d.path().join("notes.org");
    for by in ["headlines", "bytes", "todos", "links", "words"] {
        let out = run(&["rank", dir, "--by", by]);
        assert!(out.status.success(), "rank --by {by}: {}", text(&out));
    }
    for by in ["tags", "properties", "links", "depth", "words", "priority"] {
        let out = run(&["rank", file.to_str().unwrap(), "--by", by]);
        assert!(out.status.success(), "rank --by {by}: {}", text(&out));
    }
}

#[test]
fn rank_rejects_a_measure_it_does_not_have() {
    let d = vault();
    let out = run(&["rank", d.path().to_str().unwrap(), "--by", "vibes"]);
    assert!(!out.status.success(), "ranked by a measure that is not one");
}

#[test]
fn lists_finds_the_checkboxes() {
    // In the *preamble*. `lists` reads the text above the first
    // headline, the same place `toggle-checkbox` counts its Nth item —
    // my first fixture put the list under a headline and the command
    // correctly found nothing. Documented here because the two
    // commands share the assumption and neither says so at the call
    // site.
    let d = tempfile::tempdir().expect("tempdir");
    let file = d.path().join("lists.org");
    fs::write(
        &file,
        "- [ ] an unchecked item\n- [X] a checked one\n\n* A headline\nbody\n",
    )
    .expect("write");
    let out = run(&["lists", file.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    let t = text(&out);
    assert!(t.contains("unchecked item"), "{t}");
    assert!(
        t.contains("[X]"),
        "the checked state is part of the answer: {t}"
    );
}

#[test]
fn lists_on_a_file_with_no_lists_says_nothing_and_succeeds() {
    let d = tempfile::tempdir().expect("tempdir");
    let file = d.path().join("plain.org");
    fs::write(&file, "* Just a headline\nand a body\n").expect("write");
    let out = run(&["lists", file.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        out.stdout.is_empty(),
        "{:?}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn clock_totals_the_time_in_the_logbook_drawer() {
    // A bare `CLOCK:` line is not what this reads: `cmd_clock` walks
    // LOGBOOK drawers. The distinction matters because org writes
    // clock entries both ways and only one of them is a drawer.
    let d = tempfile::tempdir().expect("tempdir");
    let file = d.path().join("clocked.org");
    fs::write(
        &file,
        "* Work\n:LOGBOOK:\n\
         CLOCK: [2026-06-19 Fri 09:00]--[2026-06-19 Fri 10:30] =>  1:30\n\
         :END:\nbody\n",
    )
    .expect("write");
    let out = run(&["clock", file.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        !out.stdout.is_empty(),
        "the drawer's 90 minutes produced no report"
    );
}

#[test]
fn clock_on_a_file_with_no_logbook_is_quiet() {
    let d = vault();
    let out = run(&["clock", d.path().join("notes.org").to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
}

#[test]
fn validate_accepts_a_good_file_and_reports_on_a_broken_one() {
    let d = vault();
    let good = run(&["validate", d.path().join("notes.org").to_str().unwrap()]);
    assert!(good.status.success(), "{}", text(&good));

    // A duplicate id is the thing ids exist to prevent (I2).
    let bad_dir = tempfile::tempdir().expect("tempdir");
    let bad = bad_dir.path().join("bad.org");
    fs::write(
        &bad,
        format!(
            "* One\n:PROPERTIES:\n:ID: {ID}\n:END:\n\
             * Two\n:PROPERTIES:\n:ID: {ID}\n:END:\n"
        ),
    )
    .expect("write");
    let out = run(&["validate", bad.to_str().unwrap()]);
    assert!(!text(&out).contains("panicked"), "{}", text(&out));
}

#[test]
fn set_planning_writes_each_stamp_and_clears_it_again() {
    let d = vault();
    let file = d.path().join("notes.org");
    let path = file.to_str().unwrap();

    let out = run(&["set-planning", path, ID, "--deadline", "<2026-09-01 Tue>"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        fs::read_to_string(&file)
            .expect("read")
            .contains("DEADLINE: <2026-09-01 Tue>"),
        "the deadline was not written"
    );

    // Empty clears — and clearing has to leave a file that still parses,
    // since removing a line is where a rewriter drops the one after it.
    let out = run(&["set-planning", path, ID, "--deadline", ""]);
    assert!(out.status.success(), "{}", text(&out));
    let after = fs::read_to_string(&file).expect("read");
    assert!(
        !after.contains("DEADLINE:"),
        "the deadline was not cleared: {after}"
    );
    let check = run(&["parse", path]);
    assert!(check.status.success(), "{}", text(&check));
}

#[test]
fn set_planning_fails_on_an_id_that_is_not_there() {
    let d = vault();
    let file = d.path().join("notes.org");
    let before = fs::read_to_string(&file).expect("read");
    let out = run(&[
        "set-planning",
        file.to_str().unwrap(),
        "01NOSUCHIDATALL000000",
        "--scheduled",
        "<2026-09-01 Tue>",
    ]);
    assert!(!out.status.success(), "{}", text(&out));
    assert_eq!(before, fs::read_to_string(&file).expect("read"));
}

#[test]
fn ask_without_a_key_says_so_instead_of_hanging_or_panicking() {
    // What every user meets first. Three things have to be true: it
    // stops, it is not a panic, and the message mentions the key —
    // "error: request failed" would send somebody looking at their
    // network instead of their environment.
    let started = Instant::now();
    let out = Command::new(BIN)
        .args(["ask", "what is in my vault"])
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env("CLOSURE_LLM_LIVE", "0")
        .output()
        .expect("spawn");
    let elapsed = started.elapsed();

    assert!(!out.status.success(), "asked without a key and succeeded");
    assert!(!text(&out).contains("panicked"), "{}", text(&out));
    assert!(
        elapsed < Duration::from_secs(30),
        "took {elapsed:?} to say there is no key"
    );
    let t = text(&out).to_lowercase();
    assert!(
        t.contains("key") || t.contains("api") || t.contains("provider"),
        "the message does not point at the missing key: {}",
        text(&out)
    );
}

#[test]
fn chat_with_a_closed_stdin_exits_rather_than_waiting_forever() {
    // The same claim the mcp/acp/lsp tests make: a session that hangs
    // on EOF hangs whatever launched it.
    let started = Instant::now();
    let out = Command::new(BIN)
        .args(["chat"])
        .env_remove("ANTHROPIC_API_KEY")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("spawn");
    assert!(!text(&out).contains("panicked"), "{}", text(&out));
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "chat did not return on a closed stdin"
    );
}
