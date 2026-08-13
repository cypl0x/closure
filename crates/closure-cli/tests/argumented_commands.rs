//! The subcommands that take an argument beyond the vault or the file.
//!
//! The read-only and writing passes covered the shapes `<VAULT>` and
//! `<FILE>`. What was left is everything needing a second argument — an
//! id, a needle, a path, an index — which is most of the remaining
//! unexecuted arms in `main.rs`.
//!
//! Two claims each, and the second is the one worth having: the command
//! succeeds on a real fixture, and it *fails* rather than panicking when
//! the argument names something that is not there. Every one of these
//! looks something up, and a wrong id is the ordinary typo.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::process::Command;

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_closure");
const ID: &str = "01CLIARGS00000000001";
const CHILD: &str = "01CLIARGS00000000002";

fn vault() -> TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        format!(
            "#+TITLE: Notes\n\
             * TODO Ship it :work:\n\
             :PROPERTIES:\n:ID: {ID}\n:END:\n\
             a body mentioning closure and [[id:{CHILD}]]\n\
             #+BEGIN_SRC sh :tangle out.sh\necho hi\n#+END_SRC\n\
             ** A child\n\
             :PROPERTIES:\n:ID: {CHILD}\n:END:\n"
        ),
    )
    .expect("write");
    d
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("spawn closure")
}

fn assert_no_panic(what: &str, out: &std::process::Output) {
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("panicked at"), "`{what}` panicked:\n{err}");
}

#[test]
fn the_commands_that_take_an_id_run() {
    let d = vault();
    let file = d.path().join("notes.org").to_string_lossy().into_owned();
    let dir = d.path().to_string_lossy().into_owned();
    for args in [
        vec!["subtree", &file, ID],
        vec!["subtree-bytes", &file, ID],
        vec!["subtree-stats", &file, ID],
        vec!["backlinks", &dir, CHILD],
        vec!["body", &file, ID],
        vec!["tags-of", &file, ID],
        vec!["find-id", &dir, ID],
        vec!["path-of", &dir, ID],
    ] {
        let out = run(&args);
        assert_no_panic(&args.join(" "), &out);
    }
}

#[test]
fn an_id_that_is_not_there_fails_rather_than_crashing() {
    // The shape that finds a panic. `.expect("the headline")` on a
    // lookup is the commonest way one gets in.
    let d = vault();
    let file = d.path().join("notes.org").to_string_lossy().into_owned();
    let dir = d.path().to_string_lossy().into_owned();
    let missing = "01NOSUCH000000000000";
    for args in [
        vec!["subtree", &file, missing],
        vec!["subtree-bytes", &file, missing],
        vec!["subtree-stats", &file, missing],
        vec!["backlinks", &dir, missing],
        vec!["body", &file, missing],
        vec!["tags-of", &file, missing],
        vec!["find-id", &dir, missing],
        vec!["path-of", &dir, missing],
    ] {
        let out = run(&args);
        assert_no_panic(&args.join(" "), &out);
    }
}

#[test]
fn the_export_and_import_commands_run() {
    let d = vault();
    let file = d.path().join("notes.org").to_string_lossy().into_owned();
    let dir = d.path().to_string_lossy().into_owned();
    for args in [
        vec!["export-md", &file],
        vec!["export-html", &dir],
        vec!["id", &file],
        vec!["ids", &file],
        vec!["db", &dir],
        vec!["widgets", &dir],
        vec!["stats", &dir],
        vec!["query", &dir],
        vec!["rank", &dir],
    ] {
        let out = run(&args);
        assert_no_panic(&args.join(" "), &out);
    }
}

#[test]
fn markdown_survives_a_round_trip_through_the_cli() {
    // Not just "it ran": org -> md -> org has to come back parseable,
    // which is the claim `export-md` is actually making.
    let d = vault();
    let file = d.path().join("notes.org").to_string_lossy().into_owned();
    let md = run(&["export-md", &file]);
    assert!(
        md.status.success(),
        "{}",
        String::from_utf8_lossy(&md.stderr)
    );
    let md_path = d.path().join("round.md");
    fs::write(&md_path, &md.stdout).expect("write md");
    let back = run(&["import-md", &md_path.to_string_lossy()]);
    assert_no_panic("import-md", &back);
    assert!(
        back.status.success(),
        "{}",
        String::from_utf8_lossy(&back.stderr)
    );
    let org_path = d.path().join("round.org");
    fs::write(&org_path, &back.stdout).expect("write org");
    let check = run(&["parse", &org_path.to_string_lossy()]);
    assert!(
        check.status.success(),
        "org -> md -> org does not parse:\n{}",
        String::from_utf8_lossy(&back.stdout)
    );
}

#[test]
fn search_finds_a_word_and_says_nothing_about_one_that_is_absent() {
    let d = vault();
    let dir = d.path().to_string_lossy().into_owned();
    let hit = run(&["search", &dir, "closure"]);
    assert_no_panic("search", &hit);
    assert!(
        String::from_utf8_lossy(&hit.stdout).contains("closure"),
        "search found nothing for a word that is there"
    );
    let miss = run(&["search", &dir, "zzqqzz-not-in-any-vault"]);
    assert_no_panic("search", &miss);
    assert!(
        String::from_utf8_lossy(&miss.stdout).trim().is_empty(),
        "search invented a hit"
    );
}

#[test]
fn tangle_writes_the_block_to_its_target() {
    let d = vault();
    let dir = d.path().to_string_lossy().into_owned();
    let out = run(&["tangle", &dir, "notes.org"]);
    assert_no_panic("tangle", &out);
    assert!(
        d.path().join("out.sh").exists(),
        "`:tangle out.sh` wrote nothing: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn config_and_check_config_read_a_real_one() {
    let d = vault();
    let cfg = d.path().join("config.org");
    fs::write(
        &cfg,
        "* Settings\n#+BEGIN_SRC closure-config\nwrap = true\n#+END_SRC\n",
    )
    .expect("write");
    for cmd in ["config", "check-config"] {
        let out = run(&[cmd, &cfg.to_string_lossy()]);
        assert_no_panic(cmd, &out);
        assert!(out.status.success(), "`{cmd}` rejected a valid config");
    }
}

#[test]
fn check_config_rejects_a_bad_one() {
    // The half that matters: a config command that accepts anything is
    // not checking.
    let d = vault();
    let cfg = d.path().join("bad.org");
    fs::write(
        &cfg,
        "* Settings\n#+BEGIN_SRC closure-config\noutline_width = wide\n#+END_SRC\n",
    )
    .expect("write");
    let out = run(&["check-config", &cfg.to_string_lossy()]);
    assert_no_panic("check-config", &out);
    assert!(!out.status.success(), "a bad config was accepted");
}
