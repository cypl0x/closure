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
fn sniff_tui_renders_flow_and_block_allow_chords() {
    let v = vault();
    fs::write(
        v.path().join("config.org"),
        "#+BEGIN_SRC closure-config\nsniffer_blocklist = ads.*\n#+END_SRC\n",
    )
    .expect("config");
    let out = ok(&[
        "sniff",
        "--tui",
        "--config",
        v.path().join("config.org").to_str().unwrap(),
        "ads.example:443 TCP",
    ]);
    assert!(out.contains("ads.example"), "flow listed: {out}");
    assert!(
        out.contains("block") && out.contains("allow"),
        "block/allow offered: {out}"
    );
    // The block/allow detail rows carry their keybinding in brackets.
    assert!(out.contains('['), "chords shown: {out}");
}

#[test]
fn pkg_list_and_lock_over_local_registry() {
    let reg = tempfile::tempdir().expect("tmp");
    fs::write(
        reg.path().join("strings.org"),
        "#+BEGIN_SRC closure-package\nname = strings\nversion = 1.0.7\n#+END_SRC\n",
    )
    .expect("w");
    fs::write(
        reg.path().join("greeter.org"),
        "#+BEGIN_SRC closure-package\nname = greeter\nversion = 2.0.0\ndep = strings >=1.0.0\ncommand = greet\n#+END_SRC\n",
    )
    .expect("w");

    let list = ok(&["pkg", "list", reg.path().to_str().unwrap()]);
    assert!(
        list.contains("greeter") && list.contains("strings"),
        "list: {list}"
    );
    assert!(list.contains("greet"), "shows provided commands: {list}");

    let manifest = reg.path().join("greeter.org");
    let lock = ok(&[
        "pkg",
        "lock",
        manifest.to_str().unwrap(),
        reg.path().to_str().unwrap(),
    ]);
    assert!(
        lock.contains("strings 1.0.7"),
        "locked transitive dep: {lock}"
    );
    // closure.lock written next to the manifest.
    let written = fs::read_to_string(reg.path().join("closure.lock")).expect("lock written");
    assert!(written.contains("strings 1.0.7"));
}

#[test]
fn widgets_lists_vault_definitions() {
    let v = vault();
    fs::write(
        v.path().join("w.org"),
        "#+BEGIN: closure-widget :name banner\nhi\n#+END:\n",
    )
    .expect("write widget");
    let out = ok(&["widgets", v.path().to_str().unwrap()]);
    assert!(
        out.contains("banner") && out.contains("w.org"),
        "got: {out}"
    );
}

#[test]
fn ui_matrix_prints_node_kinds() {
    let out = ok(&["ui-matrix"]);
    assert!(out.contains("Pane") && out.contains("Palette"));
    assert!(out.contains("TUI") && out.contains("WEB"));
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
fn acp_initialize_over_stdin() {
    let v = vault();
    let mut child = Command::new(BIN)
        .args(["acp", v.path().to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn acp");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"agent/card\"}\n")
        .unwrap();
    let out = child.wait_with_output().expect("acp output");
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("\"id\":1") && s.contains("tools"),
        "acp card reply: {s}"
    );
}

#[test]
fn a2a_initialize_over_stdin() {
    let v = vault();
    let mut child = Command::new(BIN)
        .args(["a2a", v.path().to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn a2a");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n")
        .unwrap();
    let out = child.wait_with_output().expect("a2a output");
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("closure"), "a2a initialize reply: {s}");
}

#[test]
fn lsp_initialize_framed_over_stdin() {
    let v = vault();
    let mut child = Command::new(BIN)
        .args(["lsp", v.path().to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn lsp");
    let body = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}";
    let frame = format!("Content-Length: {}\r\n\r\n{body}", body.len());
    child
        .stdin
        .take()
        .unwrap()
        .write_all(frame.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("lsp output");
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("Content-Length:") && s.contains("documentSymbolProvider"),
        "lsp framed initialize reply: {s}"
    );
}

#[test]
fn lsp_framed_rename_persists_then_exits() {
    // Drives the full stdio loop: initialize, a server-authoritative
    // rename, shutdown, exit — and checks the rename hit disk.
    let d = tempfile::tempdir().expect("tempdir");
    let id = "01HXAAAAAAAAAAAAAAAAAAAAAA";
    fs::write(
        d.path().join("a.org"),
        format!("* Old title\n:PROPERTIES:\n:ID: {id}\n:END:\n"),
    )
    .expect("write");

    let frame = |j: &str| format!("Content-Length: {}\r\n\r\n{j}", j.len());
    let mut input = String::new();
    input.push_str(&frame(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}",
    ));
    input.push_str(&frame(
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"textDocument/rename\",\
         \"params\":{\"textDocument\":{\"uri\":\"file://a.org\"},\
         \"position\":{\"line\":0,\"character\":2},\"newName\":\"New title\"}}",
    ));
    input.push_str(&frame(
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"shutdown\"}",
    ));
    input.push_str(&frame("{\"jsonrpc\":\"2.0\",\"method\":\"exit\"}"));

    let mut child = Command::new(BIN)
        .args(["lsp", d.path().to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn lsp");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("lsp output");
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("renameProvider"), "advertises rename: {s}");
    assert!(s.contains("\"id\":2"), "framed rename response: {s}");

    // The rename was applied server-side and persisted to disk.
    let txt = fs::read_to_string(d.path().join("a.org")).expect("reread");
    assert!(txt.contains("New title"), "rename persisted: {txt}");
    assert!(!txt.contains("Old title"), "old title gone: {txt}");
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

#[test]
fn file_inspection_commands_run() {
    let v = vault();
    let f = v.path().join("notes.org");
    let fp = f.to_str().unwrap();
    for cmd in [
        "outline",
        "tree",
        "wc",
        "drawers",
        "tables",
        "leaves",
        "roots",
        "stats-file",
    ] {
        let _ = ok(&[cmd, fp]);
    }
    let _ = ok(&["head", fp, "--limit", "1"]);
}

#[test]
fn vault_query_commands_run() {
    let v = vault();
    let vp = v.path().to_str().unwrap();
    for cmd in [
        "todo-list",
        "archived",
        "comment-list",
        "scheduled",
        "deadlines",
    ] {
        let _ = ok(&[cmd, vp]);
    }
    let g = ok(&["grep", vp, "parser"]);
    assert!(g.contains("notes.org"), "grep prints matching paths: {g}");
    let _ = ok(&["recent", vp, "--limit", "2"]);
}

#[test]
fn outline_shows_headlines() {
    let v = vault();
    let out = ok(&["outline", v.path().join("notes.org").to_str().unwrap()]);
    assert!(out.contains("Ship parser") && out.contains("Personal wiki"));
}

#[test]
fn scheduled_lists_the_scheduled_headline() {
    let v = vault();
    let out = ok(&["scheduled", v.path().to_str().unwrap()]);
    assert!(out.contains("Ship parser"), "scheduled: {out}");
}

// C1a: `closure eval` is default-deny too. With no config.org alongside
// the file, a shell block must NOT run; the output never carries its
// stdout. Adding `eval_trust = shell` to config.org unblocks it.
#[test]
fn eval_is_default_deny_without_config() {
    let d = tempfile::tempdir().expect("tempdir");
    let f = d.path().join("a.org");
    fs::write(&f, "#+BEGIN_SRC shell\necho ran-c1a\n#+END_SRC\n").expect("write");
    let out = run(&["eval", f.to_str().unwrap()]);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !combined.contains("ran-c1a"),
        "untrusted block must not execute: {combined}"
    );
    assert!(
        combined.contains("blocked"),
        "tells the user why: {combined}"
    );
}

#[test]
fn eval_runs_when_trusted_in_config() {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("config.org"),
        "#+BEGIN_SRC closure-config\neval_trust = shell\n#+END_SRC\n",
    )
    .expect("config");
    let f = d.path().join("a.org");
    fs::write(&f, "#+BEGIN_SRC shell\necho ran-c1a\n#+END_SRC\n").expect("write");
    let out = ok(&["eval", f.to_str().unwrap()]);
    assert!(out.contains("ran-c1a"), "trusted block runs: {out}");
}

#[test]
fn clock_reports_clocked_minutes() {
    let v = vault();
    std::fs::write(
        v.path().join("clocked.org"),
        "* Deep work\n:LOGBOOK:\nCLOCK: [2024-01-01 Mon 09:00]--[2024-01-01 Mon 10:30] =>  1:30\n:END:\n",
    )
    .unwrap();
    let out = ok(&["clock-report", v.path().to_str().unwrap()]);
    assert!(out.contains("1:30"), "duration column: {out}");
    assert!(out.contains("Deep work"), "clocked headline: {out}");
}

#[test]
fn history_replay_reapplies_journal_commands() {
    let v = vault();
    // A block with a stable on-disk id (replay addresses blocks by id).
    let id = "01HXCCCCCCCCCCCCCCCCCCCCCC";
    std::fs::write(
        v.path().join("stable.org"),
        format!("* Fixed target\n:PROPERTIES:\n:ID: {id}\n:END:\n"),
    )
    .unwrap();
    std::fs::write(
        v.path().join("journal.org"),
        format!("* [1700000000] cmd: cmd=rename&id={id}&arg=Replayed+title\n"),
    )
    .unwrap();
    let out = ok(&[
        "history",
        v.path().to_str().unwrap(),
        "--replay",
        "--dry-run",
    ]);
    assert!(out.contains("would replay cmd"), "dry run lists: {out}");
    let out = ok(&["history", v.path().to_str().unwrap(), "--replay"]);
    assert!(out.contains("replayed cmd"), "{out}");
    let after = closure_store::Vault::open(v.path()).unwrap();
    assert!(
        after.find_by_title("Replayed title").is_some(),
        "replay applied"
    );
}
