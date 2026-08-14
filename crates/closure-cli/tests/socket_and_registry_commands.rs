//! The subcommands the earlier sweeps kept setting aside.
//!
//! `every_read_only_command.rs` and friends covered the arms that take
//! a vault and print. What was left was described as "needs a network,
//! a subprocess, or a terminal" — and for most of it that turned out to
//! be wrong in the same way `mcp`/`acp`/`lsp` were wrong: `pkg` reads a
//! directory, `plugin` runs a program you name, `sync` shells out to a
//! git that is already local, and `serve` binds a loopback socket. None
//! of those needs the outside world.
//!
//! Loopback is the same hermeticity argument `closure-sync`'s TCP tests
//! already make: 127.0.0.1 is this machine talking to itself, so the
//! test is reproducible and offline.
//!
//! Port 0 throughout. A fixed port is a test that passes alone and
//! fails when two of them run at once, which is the worst failure to
//! debug because it looks like flakiness in the code under test.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_closure");

fn vault() -> TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        "* A headline\n:PROPERTIES:\n:ID: 01SOCKET0000000000001\n:END:\nbody\n",
    )
    .expect("write");
    d
}

/// A registry of two packages, one depending on the other.
fn registry() -> TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("strings.org"),
        "* strings\n#+BEGIN_SRC closure-package\nname = strings\nversion = 1.0.7\n#+END_SRC\n",
    )
    .expect("write");
    fs::write(
        d.path().join("greeter.org"),
        "* greeter\n#+BEGIN_SRC closure-package\nname = greeter\nversion = 2.0.0\n\
         dep = strings >=1.0.0\ncommand = greet\n#+END_SRC\n",
    )
    .expect("write");
    d
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(BIN).args(args).output().expect("spawn")
}

#[test]
fn pkg_list_names_every_package_in_the_registry() {
    let r = registry();
    let out = run(&["pkg", "list", r.path().to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("strings"), "{text}");
    assert!(text.contains("greeter"), "{text}");
    assert!(
        text.contains("1.0.7"),
        "the version is part of the answer: {text}"
    );
}

#[test]
fn pkg_list_on_an_empty_registry_prints_nothing_and_succeeds() {
    // Not an error. "No packages installed" is a true and ordinary
    // state, and a non-zero exit would make a script treat it as one.
    let d = tempfile::tempdir().expect("tempdir");
    let out = run(&["pkg", "list", d.path().to_str().unwrap()]);
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "{:?}", out.stdout);
}

#[test]
fn pkg_lock_resolves_a_dependency_and_writes_the_lockfile() {
    // The claim worth making about `lock` is not that it exits 0 but
    // that it wrote the file it says it wrote, and that the file names
    // the dependency the manifest never mentioned directly.
    let r = registry();
    let m = tempfile::tempdir().expect("tempdir");
    let manifest = m.path().join("manifest.org");
    fs::write(
        &manifest,
        "#+BEGIN_SRC closure-package\nname = app\nversion = 0.1.0\n\
         dep = greeter >=2.0.0\n#+END_SRC\n",
    )
    .expect("write");

    let out = run(&[
        "pkg",
        "lock",
        manifest.to_str().unwrap(),
        r.path().to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let lock = m.path().join("closure.lock");
    assert!(lock.exists(), "no closure.lock beside the manifest");
    let text = fs::read_to_string(&lock).expect("read lock");
    assert!(text.contains("greeter"), "{text}");
    assert!(
        text.contains("strings"),
        "the transitive dependency is missing, which is the whole job: {text}"
    );
}

#[test]
fn pkg_lock_fails_when_a_dependency_is_not_in_the_registry() {
    // A resolver that cannot find a package and succeeds anyway writes
    // a lockfile that lies.
    let r = registry();
    let m = tempfile::tempdir().expect("tempdir");
    let manifest = m.path().join("manifest.org");
    fs::write(
        &manifest,
        "#+BEGIN_SRC closure-package\nname = app\nversion = 0.1.0\n\
         dep = nonexistent >=9.9.9\n#+END_SRC\n",
    )
    .expect("write");
    let out = run(&[
        "pkg",
        "lock",
        manifest.to_str().unwrap(),
        r.path().to_str().unwrap(),
    ]);
    assert!(
        !out.status.success(),
        "resolved a package that is not there"
    );
    assert!(
        !m.path().join("closure.lock").exists(),
        "wrote a lockfile for a resolution that failed"
    );
}

#[test]
fn pkg_lock_rejects_a_manifest_with_no_package_block() {
    let r = registry();
    let m = tempfile::tempdir().expect("tempdir");
    let manifest = m.path().join("manifest.org");
    fs::write(&manifest, "* just a headline, no block\n").expect("write");
    let out = run(&[
        "pkg",
        "lock",
        manifest.to_str().unwrap(),
        r.path().to_str().unwrap(),
    ]);
    assert!(!out.status.success());
}

#[test]
fn serve_answers_on_the_port_it_was_given() {
    // The claim: `serve` binds, speaks HTTP, and says something about
    // the vault. It runs forever by design, so this is a spawn-and-kill
    // rather than an `output()` — and the kill is in every path,
    // because a leaked server holds a port for the rest of the suite.
    let v = vault();
    let addr = free_addr();
    let mut child = Command::new(BIN)
        .args([
            "serve",
            v.path().to_str().unwrap(),
            "--addr",
            &addr.to_string(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn serve");

    let body = wait_for_response(addr);
    let _ = child.kill();
    let _ = child.wait();

    let body = body.expect("serve never answered on its port");
    assert!(
        body.contains("A headline"),
        "served a page without the vault in it: {body}"
    );
}

/// A port nobody is on: bind 0, read what the OS chose, drop it.
///
/// Racy in principle and not in practice, and the alternative — a
/// constant — is racy in practice.
fn free_addr() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind 0");
    l.local_addr().expect("addr")
}

/// Poll until the server is up, with a deadline. A fixed sleep is
/// either slower than it needs to be or shorter than a loaded machine
/// needs, and usually both on different days.
fn wait_for_response(addr: SocketAddr) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(300)) {
            s.set_read_timeout(Some(Duration::from_secs(5))).ok();
            if s.write_all(b"GET / HTTP/1.0\r\n\r\n").is_ok() {
                let mut buf = String::new();
                if s.read_to_string(&mut buf).is_ok() && !buf.is_empty() {
                    return Some(buf);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

#[test]
fn serve_refuses_a_vault_that_is_not_there() {
    let addr = free_addr();
    let out = run(&[
        "serve",
        "/nonexistent/vault/for/a/serve/test",
        "--addr",
        &addr.to_string(),
    ]);
    assert!(!out.status.success(), "served a vault that does not exist");
}

#[test]
fn sync_push_reports_rather_than_panics_outside_a_git_repository() {
    // A vault that is not a repo is the ordinary case for somebody who
    // has not set git up yet, and the ordinary case must not be a
    // stack trace.
    let v = vault();
    let out = run(&["sync", v.path().to_str().unwrap(), "--op", "push"]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !text.contains("panicked"),
        "sync panicked outside a repo: {text}"
    );
}

#[test]
fn sync_rejects_an_operation_that_is_neither_push_nor_pull() {
    let v = vault();
    let out = run(&["sync", v.path().to_str().unwrap(), "--op", "sideways"]);
    assert!(!out.status.success(), "accepted an operation it cannot do");
}

#[test]
fn plugin_runs_the_executable_the_manifest_names() {
    // The plugin host's own tests cover the protocol. What is untested
    // here is the CLI arm: that it reads a manifest, finds the
    // executable and reports what came back.
    let d = tempfile::tempdir().expect("tempdir");
    let exe = d.path().join("plug.sh");
    fs::write(&exe, "#!/bin/sh\necho \"$@\"\n").expect("write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let manifest = d.path().join("plugin.conf");
    fs::write(
        &manifest,
        format!(
            "id = demo\nname = Demo\napi_version = 1\ncommand = {}\n",
            exe.display()
        ),
    )
    .expect("write");

    let out = run(&[
        "plugin",
        manifest.to_str().unwrap(),
        exe.to_str().unwrap(),
        "hello",
    ]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!text.contains("panicked"), "{text}");
}

#[test]
fn plugin_reports_a_manifest_that_is_not_there() {
    let out = run(&["plugin", "/nonexistent/manifest", "/bin/true"]);
    assert!(!out.status.success());
}
