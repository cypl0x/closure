//! "That's ~40 commands that are one query with a sort flag and a
//! limit. Each one is public API you now have to keep working."
//!
//! `largest-file`, `smallest-file`, `busiest-file`, `quietest-file`,
//! `rank-files`, `rank-bytes`, `rank-todos`, `rank-links`,
//! `rank-words`, `deepest-leaf`, `largest-subtree`, `longest-body`,
//! `most-tagged`, `most-propertied`, `most-linked`,
//! `highest-priority`, `empty-files` — seventeen commands that are one
//! command with `--by`, `--limit` and `--asc`. And `hub-ids`,
//! `isolated-ids`, `source-only-ids`, `sink-only-ids`,
//! `duplicate-ids`, `all-ids` — six that are one with `--kind`.
//!
//! Nothing in the repo called any of them. They are gone, and the
//! capability is not.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

fn vault() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("big.org"),
        "* One\n:PROPERTIES:\n:ID: rank-one\n:END:\nplenty of words in this body here\n\
         ** Two :alpha:beta:\n[[id:rank-one]]\n* TODO Three\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("small.org"), "* Only\n").unwrap();
    std::fs::write(dir.path().join("empty.org"), "no headlines at all\n").unwrap();
    dir
}

fn run(args: &[&str]) -> (String, bool) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_closure"))
        .args(args)
        .output()
        .expect("run");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.success(),
    )
}

#[test]
fn rank_orders_files_by_the_metric_you_name() {
    let dir = vault();
    let v = dir.path().to_str().unwrap();
    let (out, ok) = run(&["rank", v, "--by", "headlines"]);
    assert!(ok, "{out}");
    let first = out.lines().next().expect("a line");
    assert!(first.contains("big.org"), "{out}");
}

#[test]
fn asc_is_the_other_end_of_the_same_list() {
    let dir = vault();
    let v = dir.path().to_str().unwrap();
    let (down, _) = run(&["rank", v, "--by", "bytes"]);
    let (up, _) = run(&["rank", v, "--by", "bytes", "--asc"]);
    let down: Vec<&str> = down.lines().collect();
    let up: Vec<&str> = up.lines().collect();
    assert_eq!(down.len(), up.len(), "{down:?} vs {up:?}");
    assert_eq!(down.first(), up.last(), "ascending is not the reverse");
}

#[test]
fn limit_is_how_the_superlatives_are_spelled_now() {
    // `largest-file` was `rank --by bytes --limit 1`.
    let dir = vault();
    let (out, ok) = run(&[
        "rank",
        dir.path().to_str().unwrap(),
        "--by",
        "bytes",
        "--limit",
        "1",
    ]);
    assert!(ok, "{out}");
    assert_eq!(out.lines().count(), 1, "{out}");
    assert!(out.contains("big.org"), "{out}");
}

#[test]
fn the_headline_metrics_work_on_a_file() {
    let dir = vault();
    let file = dir.path().join("big.org");
    let file = file.to_str().unwrap();
    for by in ["tags", "properties", "links", "depth", "words", "priority"] {
        let (out, ok) = run(&["rank", file, "--by", by, "--limit", "1"]);
        assert!(ok, "--by {by} failed: {out}");
    }
    let (out, _) = run(&["rank", file, "--by", "tags", "--limit", "1"]);
    assert!(
        out.contains("Two"),
        "the most-tagged headline is Two: {out}"
    );
}

#[test]
fn ids_replaces_the_six_id_commands() {
    let dir = vault();
    let file = dir.path().join("big.org");
    let file = file.to_str().unwrap();
    for kind in [
        "all",
        "duplicate",
        "isolated",
        "hub",
        "source-only",
        "sink-only",
    ] {
        let (out, ok) = run(&["ids", file, "--kind", kind]);
        assert!(ok, "--kind {kind} failed: {out}");
    }
    let (out, _) = run(&["ids", file, "--kind", "all"]);
    assert!(out.contains("rank-one"), "{out}");
}

#[test]
fn the_commands_it_replaced_are_gone() {
    for gone in [
        "largest-file",
        "smallest-file",
        "busiest-file",
        "quietest-file",
        "rank-files",
        "rank-bytes",
        "rank-todos",
        "rank-links",
        "rank-words",
        "deepest-leaf",
        "largest-subtree",
        "longest-body",
        "most-tagged",
        "most-propertied",
        "most-linked",
        "highest-priority",
        "empty-files",
        "hub-ids",
        "isolated-ids",
        "source-only-ids",
        "sink-only-ids",
        "duplicate-ids",
        "all-ids",
    ] {
        let (_, ok) = run(&[gone, "--help"]);
        assert!(!ok, "`{gone}` is still a command");
    }
}
