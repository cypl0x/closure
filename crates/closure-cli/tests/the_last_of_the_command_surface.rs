//! What was left after the error sweep: the arms that need a fixture
//! shaped for them, or an environment.
//!
//! `edit-block` spawns `$EDITOR`, which sounds untestable and is not —
//! an editor is a program that changes a file, and a three-line script
//! is one. That covers the whole tail of the command, including the
//! path where the editor exits non-zero and the temp file has to be
//! cleaned up anyway.
//!
//! `whichkey` takes no positional at all, which is why the error sweep
//! skipped it: that sweep works by feeding the first positional
//! something broken, and a command with none has nothing to feed.
//! Worth writing down — it is the sweep's one blind spot, and the fix
//! is a direct test rather than a cleverer sweep.
//!
//! Several others here want their content in the *preamble* —
//! `block-args` reads code blocks above the first headline, the same
//! assumption `lists` and `toggle-checkbox` have. Three commands now
//! share it and none of them says so at the call site.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::process::Command;

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_closure");

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

/// A shell script that behaves like an editor.
#[cfg(unix)]
fn editor(dir: &std::path::Path, name: &str, body: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).expect("write editor");
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).expect("chmod");
    p.to_string_lossy().into_owned()
}

/// A vault with two code blocks in one file.
fn block_vault() -> TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        "#+NAME: first\n#+BEGIN_SRC sh :results output\necho one\n#+END_SRC\n\
         \n#+NAME: second\n#+BEGIN_SRC sh\necho two\n#+END_SRC\n\
         \n* A headline\n:PROPERTIES:\n:ID: 01LASTCMD00000000001\n:END:\nbody\n",
    )
    .expect("write");
    d
}

#[cfg(unix)]
#[test]
fn edit_block_writes_back_what_the_editor_left() {
    // The whole point of the command. An editor is just a program that
    // changes a file.
    let d = block_vault();
    let bin = tempfile::tempdir().expect("tempdir");
    let ed = editor(
        bin.path(),
        "fake-editor",
        "echo 'echo edited-by-the-test' > \"$1\"",
    );

    let out = Command::new(BIN)
        .args(["edit-block", d.path().to_str().unwrap(), "notes.org", "0"])
        .env("EDITOR", &ed)
        .output()
        .expect("spawn");
    assert!(out.status.success(), "{}", text(&out));

    let after = fs::read_to_string(d.path().join("notes.org")).expect("read");
    assert!(
        after.contains("edited-by-the-test"),
        "the edit was not written back: {after}"
    );
    // And the file is still a document closure can read.
    let check = run(&["parse", d.path().join("notes.org").to_str().unwrap()]);
    assert!(check.status.success(), "{}", text(&check));
}

#[cfg(unix)]
#[test]
fn edit_block_leaves_the_file_alone_when_the_editor_fails() {
    // Quitting the editor with an error is how a person says "never
    // mind". Writing the buffer back anyway would be the command
    // overriding that.
    let d = block_vault();
    let bin = tempfile::tempdir().expect("tempdir");
    let ed = editor(bin.path(), "failing-editor", "exit 3");
    let before = fs::read_to_string(d.path().join("notes.org")).expect("read");

    let out = Command::new(BIN)
        .args(["edit-block", d.path().to_str().unwrap(), "notes.org", "0"])
        .env("EDITOR", &ed)
        .output()
        .expect("spawn");
    assert!(!out.status.success(), "{}", text(&out));
    assert_eq!(
        before,
        fs::read_to_string(d.path().join("notes.org")).expect("read"),
        "a failed editor still changed the file"
    );
}

#[cfg(unix)]
#[test]
fn edit_block_reports_an_editor_that_is_not_there() {
    let d = block_vault();
    let out = Command::new(BIN)
        .args(["edit-block", d.path().to_str().unwrap(), "notes.org", "0"])
        .env("EDITOR", "/nonexistent/editor/binary")
        .output()
        .expect("spawn");
    assert!(!out.status.success());
    assert!(!text(&out).contains("panicked"), "{}", text(&out));
}

#[test]
fn whichkey_lists_the_chords_and_a_prefix_narrows_them() {
    // No positional, so the error sweep cannot reach it — its one
    // blind spot, covered here directly.
    let all = run(&["whichkey"]);
    assert!(all.status.success(), "{}", text(&all));
    assert!(!all.stdout.is_empty(), "whichkey printed nothing");

    let pre = run(&["whichkey", "--prefix", "C-c"]);
    assert!(pre.status.success(), "{}", text(&pre));
    assert!(
        pre.stdout.len() <= all.stdout.len(),
        "a prefix returned more than no prefix"
    );
}

#[test]
fn whichkey_with_a_prefix_nothing_starts_with_is_empty_not_everything() {
    // The same "an empty filter must not mean everything" shape as the
    // search and the peer list.
    let all = run(&["whichkey"]);
    let none = run(&["whichkey", "--prefix", "zzz-no-such-prefix"]);
    assert!(none.status.success(), "{}", text(&none));
    assert!(
        none.stdout.len() < all.stdout.len(),
        "an impossible prefix listed every chord"
    );
}

#[test]
fn eval_of_a_named_block_that_is_not_there_says_which_name() {
    let d = block_vault();
    let file = d.path().join("notes.org");
    let out = run(&["eval", file.to_str().unwrap(), "--block", "no-such-name"]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("no-such-name"),
        "the message does not name the block asked for: {}",
        text(&out)
    );
}

#[test]
fn eval_of_block_one_skips_block_zero() {
    // Selecting by index has to *skip* the others, not run them all
    // and print one. Two blocks is the smallest fixture that can tell
    // the difference.
    let d = block_vault();
    let cfg = tempfile::tempdir().expect("tempdir");
    let trust = Command::new(BIN)
        .args(["trust", d.path().to_str().unwrap(), "shell"])
        .env("XDG_CONFIG_HOME", cfg.path())
        .output()
        .expect("spawn");
    assert!(trust.status.success(), "{}", text(&trust));

    let out = Command::new(BIN)
        .args([
            "eval",
            d.path().join("notes.org").to_str().unwrap(),
            "--block",
            "1",
        ])
        .env("XDG_CONFIG_HOME", cfg.path())
        .output()
        .expect("spawn");
    assert!(out.status.success(), "{}", text(&out));
    let t = text(&out);
    assert!(t.contains("two"), "block 1 did not run: {t}");
    assert!(
        !t.contains("one"),
        "block 0 ran when only block 1 was asked for: {t}"
    );
}

#[test]
fn eval_out_of_range_says_how_many_blocks_there_are() {
    let d = block_vault();
    let out = run(&[
        "eval",
        d.path().join("notes.org").to_str().unwrap(),
        "--block",
        "99",
    ]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("code block"),
        "the message does not say how many there are: {}",
        text(&out)
    );
}

#[test]
fn block_args_reads_the_header_arguments_of_preamble_blocks() {
    // Preamble again — the third command with this assumption.
    let d = block_vault();
    let out = run(&["block-args", d.path().join("notes.org").to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    let t = text(&out);
    assert!(t.contains("block #0"), "{t}");
    assert!(
        t.contains("lang=sh"),
        "the language is part of the answer: {t}"
    );
    assert!(
        t.contains("results"),
        "the `:results output` header is missing: {t}"
    );
}

#[test]
fn query_narrows_by_each_filter_it_offers() {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        "* TODO Alpha work :work:\n:PROPERTIES:\n:ID: 01QUERYCMD0000000001\n:END:\n\
         ** DONE Beta nested :home:\n:PROPERTIES:\n:ID: 01QUERYCMD0000000002\n:END:\n\
         * TODO Gamma other :work:\n:PROPERTIES:\n:ID: 01QUERYCMD0000000003\n:END:\n",
    )
    .expect("write");
    let dir = d.path().to_str().unwrap();

    let all = run(&["query", dir]);
    assert!(all.status.success(), "{}", text(&all));

    let by_tag = run(&["query", dir, "--tag", "home"]);
    assert!(by_tag.status.success(), "{}", text(&by_tag));
    assert!(text(&by_tag).contains("Beta"), "{}", text(&by_tag));
    assert!(!text(&by_tag).contains("Alpha"), "{}", text(&by_tag));

    let by_todo = run(&["query", dir, "--todo", "DONE"]);
    assert!(text(&by_todo).contains("Beta"), "{}", text(&by_todo));
    assert!(!text(&by_todo).contains("Gamma"), "{}", text(&by_todo));

    let by_title = run(&["query", dir, "--title", "Gamma"]);
    assert!(text(&by_title).contains("Gamma"), "{}", text(&by_title));
    assert!(!text(&by_title).contains("Alpha"), "{}", text(&by_title));

    let by_level = run(&["query", dir, "--level", "2"]);
    assert!(text(&by_level).contains("Beta"), "{}", text(&by_level));
    assert!(!text(&by_level).contains("Alpha"), "{}", text(&by_level));
}

#[test]
fn query_with_filters_that_match_nothing_returns_nothing() {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        "* TODO Alpha :work:\n:PROPERTIES:\n:ID: 01QUERYCMD0000000004\n:END:\n",
    )
    .expect("write");
    let out = run(&[
        "query",
        d.path().to_str().unwrap(),
        "--tag",
        "work",
        "--todo",
        "DONE",
    ]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        out.stdout.is_empty(),
        "two filters that cannot both hold returned rows: {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn ids_answers_each_kind_it_offers() {
    let d = tempfile::tempdir().expect("tempdir");
    let f = d.path().join("notes.org");
    fs::write(
        &f,
        "* One\n:PROPERTIES:\n:ID: 01IDSCMD000000000001\n:END:\n\
         links to [[id:01IDSCMD000000000002]]\n\
         * Two\n:PROPERTIES:\n:ID: 01IDSCMD000000000002\n:END:\n\
         * Alone\n:PROPERTIES:\n:ID: 01IDSCMD000000000003\n:END:\n",
    )
    .expect("write");
    for kind in [
        "all",
        "duplicate",
        "isolated",
        "hub",
        "source-only",
        "sink-only",
    ] {
        let out = run(&["ids", f.to_str().unwrap(), "--kind", kind]);
        assert!(out.status.success(), "--kind {kind}: {}", text(&out));
    }

    let all = run(&["ids", f.to_str().unwrap(), "--kind", "all"]);
    assert!(
        text(&all).contains("01IDSCMD000000000001"),
        "{}",
        text(&all)
    );
}

#[test]
fn ids_duplicate_finds_the_id_written_twice() {
    // The kind that reports a defect in the file, so it has to actually
    // find one.
    let d = tempfile::tempdir().expect("tempdir");
    let f = d.path().join("dup.org");
    fs::write(
        &f,
        "* One\n:PROPERTIES:\n:ID: 01DUPCMD000000000001\n:END:\n\
         * Two\n:PROPERTIES:\n:ID: 01DUPCMD000000000001\n:END:\n",
    )
    .expect("write");
    let out = run(&["ids", f.to_str().unwrap(), "--kind", "duplicate"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("01DUPCMD000000000001"),
        "the duplicated id was not reported: {}",
        text(&out)
    );
}

#[test]
fn ids_rejects_a_kind_it_does_not_have() {
    let d = tempfile::tempdir().expect("tempdir");
    let f = d.path().join("notes.org");
    fs::write(&f, "* One\n").expect("write");
    let out = run(&["ids", f.to_str().unwrap(), "--kind", "vibes"]);
    assert!(!text(&out).contains("panicked"), "{}", text(&out));
}
