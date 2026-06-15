//! End-to-end CLI tests: spawn the real `closure` binary on temp
//! vaults and assert exit status + output. Covers the cmd_* glue in
//! main.rs that unit tests can't reach. Hermetic (no network); the
//! `ask` test uses the echo provider via config.org.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::process::{Command, Stdio};

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_closure");

fn vault() -> TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        "* TODO Ship parser :work:\n\
         SCHEDULED: <2026-06-20>\n\
         * Personal wiki\n\
         #+BEGIN_SRC sh\necho hello-from-block\n#+END_SRC\n\
         * DONE Write spec\n",
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

fn ok(args: &[&str]) -> String {
    let out = run(args);
    assert!(
        out.status.success(),
        "`{args:?}` failed: {}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn parse_prints_headline_summary() {
    let v = vault();
    let f = v.path().join("notes.org");
    let out = ok(&["parse", f.to_str().unwrap()]);
    assert!(out.contains("headline"), "got: {out}");
}

#[test]
fn stats_runs() {
    let v = vault();
    let _ = ok(&["stats", v.path().to_str().unwrap()]);
}

#[test]
fn shells_prints_matrix() {
    let out = ok(&["shells"]);
    assert!(out.contains("TUI") && out.contains("CLI"));
}

#[test]
fn spec_prints_primitives() {
    let out = ok(&["spec"]);
    assert!(out.to_lowercase().contains("parse") || !out.is_empty());
}

#[test]
fn default_config_prints_block() {
    let out = ok(&["default-config"]);
    assert!(out.contains("closure-config"));
}

#[test]
fn capture_appends_and_persists() {
    let v = vault();
    let out = ok(&["capture", v.path().to_str().unwrap(), "Buy milk"]);
    assert!(out.to_lowercase().contains("captured") || out.contains("milk"));
    let inbox = fs::read_to_string(v.path().join("inbox.org")).expect("inbox written");
    assert!(inbox.contains("Buy milk"));
}

#[test]
fn view_renders_table() {
    let v = vault();
    let out = ok(&[
        "view",
        v.path().to_str().unwrap(),
        ":from all :columns title",
    ]);
    assert!(out.contains("Ship parser") && out.contains("title"));
}

#[test]
fn search_finds_matches() {
    let v = vault();
    let out = ok(&["search", v.path().to_str().unwrap(), "wiki"]);
    assert!(out.contains("Personal wiki"));
}

#[test]
fn agenda_lists_scheduled() {
    let v = vault();
    let out = ok(&["agenda", v.path().to_str().unwrap()]);
    assert!(
        out.contains("Ship parser"),
        "scheduled item in agenda: {out}"
    );
}

#[test]
fn graph_emits_dot() {
    let v = vault();
    let out = ok(&["graph", v.path().to_str().unwrap()]);
    assert!(out.contains("digraph"));
}

#[test]
fn dead_links_runs() {
    let v = vault();
    let _ = ok(&["dead-links", v.path().to_str().unwrap()]);
}

#[test]
fn tag_cloud_lists_tags() {
    let v = vault();
    let out = ok(&["tag-cloud", v.path().to_str().unwrap()]);
    assert!(out.contains("work"));
}

#[test]
fn todos_and_tags_and_paths_run() {
    let v = vault();
    let _ = ok(&["todos", v.path().to_str().unwrap()]);
    let _ = ok(&["tags", v.path().to_str().unwrap()]);
    let p = ok(&["paths", v.path().to_str().unwrap()]);
    assert!(p.contains("notes.org"));
}

#[test]
fn mcp_initialize_over_stdin() {
    let v = vault();
    let mut child = Command::new(BIN)
        .args(["mcp", v.path().to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn mcp");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n")
        .unwrap();
    let out = child.wait_with_output().expect("mcp output");
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("closure"), "mcp initialize reply: {s}");
}

#[test]
fn ask_with_echo_provider_runs_the_tool_loop() {
    let v = vault();
    fs::write(
        v.path().join("config.org"),
        "#+BEGIN_SRC closure-config\nllm_provider = echo\n#+END_SRC\n",
    )
    .expect("config");
    let out = ok(&[
        "ask",
        "what files exist",
        "--vault",
        v.path().to_str().unwrap(),
    ]);
    assert!(
        out.contains("what files exist") || out.contains("Vault tools"),
        "echo loop: {out}"
    );
}

#[test]
fn tangle_writes_target() {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("lit.org"),
        "* Code\n#+BEGIN_SRC sh :tangle out.sh\necho hi\n#+END_SRC\n",
    )
    .expect("write");
    let _ = ok(&["tangle", d.path().to_str().unwrap(), "lit.org"]);
    assert!(d.path().join("out.sh").exists(), "tangled file written");
}

#[test]
fn plugin_runs_executable() {
    let d = tempfile::tempdir().expect("tempdir");
    let exe = d.path().join("greet");
    fs::write(&exe, "#!/bin/sh\necho \"hi $1\"\n").expect("write");
    let mut perm = fs::metadata(&exe).unwrap().permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&exe, perm).unwrap();
    let manifest = d.path().join("p.manifest");
    fs::write(
        &manifest,
        "id = g\nname = G\napi_version = 1.0.0\ncommand = greet\n",
    )
    .expect("m");
    let out = ok(&[
        "plugin",
        manifest.to_str().unwrap(),
        exe.to_str().unwrap(),
        "world",
    ]);
    assert!(out.contains("hi world"));
}

#[test]
fn unknown_subcommand_fails() {
    let out = run(&["definitely-not-a-command"]);
    assert!(!out.status.success());
}
