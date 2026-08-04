//! Vault-level code block evaluation: run a block, attach
//! #+RESULTS: span-preserving, keep disk and memory in sync.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use closure_store::Vault;
use tempfile::TempDir;

/// A vault, and the user's own trust store beside it.
///
/// C1a: eval is default-deny. These tests exercise execution, so the
/// *user* opts in to the languages they use. The opt-in used to be a
/// `config.org` inside the vault, which meant the vault authorised its
/// own code — the 2026-08-04 review's first finding, and the reason
/// the grant is now written where a vault cannot reach it.
fn write_vault(files: &[(&str, &str)]) -> (TempDir, TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = tempfile::tempdir().expect("tempdir");
    let store = home.path().join("trust.org");
    for lang in ["shell", "python"] {
        closure_store::grant_eval_trust_at(&store, dir.path(), lang).expect("grant");
    }
    for (name, body) in files {
        fs::write(dir.path().join(name), body).expect("write");
    }
    (dir, home)
}

/// Open a vault against the store `write_vault` made for it.
fn open(dir: &TempDir, home: &TempDir) -> Vault {
    Vault::open(dir.path())
        .expect("open")
        .with_trust_store(&home.path().join("trust.org"))
}

fn file(td: &TempDir) -> PathBuf {
    td.path().join("a.org")
}

#[test]
fn eval_block_attaches_results_on_disk_and_in_memory() {
    let (td, home) = write_vault(&[(
        "a.org",
        "* H\n#+BEGIN_SRC shell\necho ran\n#+END_SRC\ntail\n",
    )]);
    let mut v = open(&td, &home);
    let out = v.eval_block(&file(&td), 0).expect("eval");
    assert_eq!(out.trim(), "ran");
    let disk = fs::read_to_string(file(&td)).expect("read");
    assert!(disk.contains("#+RESULTS:\n: ran\n"));
    assert!(disk.contains("tail\n"), "bytes after block survive (I1)");
    let mem = v.document(&file(&td)).expect("doc").source();
    assert_eq!(mem, disk);
}

#[test]
fn eval_block_respects_var_header_args() {
    let (td, home) = write_vault(&[(
        "a.org",
        "#+BEGIN_SRC shell :var who=world\necho \"hi $who\"\n#+END_SRC\n",
    )]);
    let mut v = open(&td, &home);
    let out = v.eval_block(&file(&td), 0).expect("eval");
    assert_eq!(out.trim(), "hi world");
}

#[test]
fn eval_block_silent_skips_results() {
    let (td, home) = write_vault(&[(
        "a.org",
        "#+BEGIN_SRC shell :results silent\necho quiet\n#+END_SRC\n",
    )]);
    let before = fs::read_to_string(file(&td)).expect("read");
    let mut v = open(&td, &home);
    let out = v.eval_block(&file(&td), 0).expect("eval");
    assert_eq!(out.trim(), "quiet");
    assert_eq!(
        fs::read_to_string(file(&td)).expect("read"),
        before,
        "silent leaves the file untouched"
    );
}

#[test]
fn eval_block_default_deny_runs_nothing() {
    // No grant anywhere => nothing trusted => a shell block must NOT
    // execute. Note what is *not* here: a config.org. Writing one in
    // the vault would once have been enough to make this pass.
    let dir = tempfile::tempdir().expect("tempdir");
    let home = tempfile::tempdir().expect("tempdir");
    let f = dir.path().join("a.org");
    fs::write(&f, "* H\n#+BEGIN_SRC shell\necho ran\n#+END_SRC\ntail\n").expect("write");
    let before = fs::read_to_string(&f).expect("read");
    let mut v = Vault::open(dir.path())
        .expect("open")
        .with_trust_store(&home.path().join("trust.org"));
    let err = v.eval_block(&f, 0).expect_err("must be blocked");
    assert!(
        matches!(err, closure_store::VaultError::EvalBlocked { .. }),
        "blocked decision, got {err:?}"
    );
    assert_eq!(
        fs::read_to_string(&f).expect("read"),
        before,
        "blocked block runs nothing and writes no #+RESULTS:"
    );
}

#[test]
fn eval_block_untrusted_language_is_blocked() {
    // The user trusts only shell; a python block is denied.
    let dir = tempfile::tempdir().expect("tempdir");
    let home = tempfile::tempdir().expect("tempdir");
    let store = home.path().join("trust.org");
    closure_store::grant_eval_trust_at(&store, dir.path(), "shell").expect("grant");
    let f = dir.path().join("a.org");
    fs::write(&f, "#+BEGIN_SRC python\nprint(1)\n#+END_SRC\n").expect("write");
    let mut v = Vault::open(dir.path())
        .expect("open")
        .with_trust_store(&store);
    assert!(matches!(
        v.eval_block(&f, 0),
        Err(closure_store::VaultError::EvalBlocked { .. })
    ));
}

// C1c: a trusted `wasm` block runs sandboxed (no host imports) and its
// `run` export's i32 becomes the result. Feature-gated.
#[cfg(feature = "wasmtime")]
#[test]
fn eval_block_runs_trusted_wasm_sandboxed() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\neval_trust = wasm\n#+END_SRC\n",
    )
    .expect("config");
    let f = dir.path().join("a.org");
    fs::write(
        &f,
        "#+BEGIN_SRC wasm\n(module (func (export \"run\") (result i32) i32.const 42))\n#+END_SRC\n",
    )
    .expect("write");
    let mut v = open(&dir, &home);
    let out = v.eval_block(&f, 0).expect("eval");
    assert_eq!(out.trim(), "42");
}

#[test]
fn eval_block_out_of_range_errors() {
    let (td, home) = write_vault(&[("a.org", "* no blocks\n")]);
    let mut v = open(&td, &home);
    assert!(v.eval_block(&file(&td), 0).is_err());
}

#[test]
fn eval_block_unknown_language_errors() {
    let (td, home) = write_vault(&[("a.org", "#+BEGIN_SRC brainfuck\n+++\n#+END_SRC\n")]);
    let mut v = open(&td, &home);
    assert!(v.eval_block(&file(&td), 0).is_err());
}

#[test]
fn eval_block_reeval_replaces_results() {
    let (td, home) = write_vault(&[("a.org", "#+BEGIN_SRC shell\necho once\n#+END_SRC\n")]);
    let mut v = open(&td, &home);
    v.eval_block(&file(&td), 0).expect("eval");
    v.eval_block(&file(&td), 0).expect("re-eval");
    let disk = fs::read_to_string(file(&td)).expect("read");
    assert_eq!(disk.matches("#+RESULTS:").count(), 1, "no duplication");
}

#[test]
fn set_block_content_replaces_and_persists() {
    let (td, home) = write_vault(&[(
        "a.org",
        "* H\n#+BEGIN_SRC shell\necho old\n#+END_SRC\ntail\n",
    )]);
    let mut v = open(&td, &home);
    v.set_block_content(&file(&td), 0, "echo new\n")
        .expect("set");
    let disk = fs::read_to_string(file(&td)).expect("read");
    assert!(disk.contains("echo new\n"));
    assert!(!disk.contains("echo old"));
    assert!(disk.contains("tail\n"), "neighbour bytes survive (I1)");
    let mem = v.document(&file(&td)).expect("doc").source();
    assert_eq!(mem, disk);
}

#[test]
fn set_block_content_out_of_range_errors() {
    let (td, home) = write_vault(&[("a.org", "* no blocks\n")]);
    let mut v = open(&td, &home);
    assert!(v.set_block_content(&file(&td), 0, "x\n").is_err());
}

#[test]
fn tangle_writes_blocks_to_targets() {
    let (td, home) = write_vault(&[(
        "lit.org",
        "#+BEGIN_SRC shell :tangle build.sh\necho one\n#+END_SRC\n\
         * H\n#+BEGIN_SRC python :tangle app.py\nprint('hi')\n#+END_SRC\n\
         #+BEGIN_SRC shell\necho untangled\n#+END_SRC\n",
    )]);
    let v = open(&td, &home);
    let written = v.tangle(&td.path().join("lit.org")).expect("tangle");
    assert_eq!(written.len(), 2, "only :tangle blocks written");
    assert_eq!(
        fs::read_to_string(td.path().join("build.sh")).expect("build.sh"),
        "echo one\n"
    );
    assert_eq!(
        fs::read_to_string(td.path().join("app.py")).expect("app.py"),
        "print('hi')\n"
    );
}

#[test]
fn tangle_concatenates_blocks_to_same_target() {
    let (td, home) = write_vault(&[(
        "lit.org",
        "#+BEGIN_SRC shell :tangle out.sh\nfirst\n#+END_SRC\n\
         #+BEGIN_SRC shell :tangle out.sh\nsecond\n#+END_SRC\n",
    )]);
    let v = open(&td, &home);
    v.tangle(&td.path().join("lit.org")).expect("tangle");
    assert_eq!(
        fs::read_to_string(td.path().join("out.sh")).expect("out.sh"),
        "first\nsecond\n"
    );
}

#[test]
fn tangle_no_targets_writes_nothing() {
    let (td, home) = write_vault(&[("lit.org", "#+BEGIN_SRC shell\necho x\n#+END_SRC\n")]);
    let v = open(&td, &home);
    assert!(
        v.tangle(&td.path().join("lit.org"))
            .expect("tangle")
            .is_empty()
    );
}

#[test]
fn tangle_unknown_file_errors() {
    let (td, home) = write_vault(&[("lit.org", "* x\n")]);
    let v = open(&td, &home);
    assert!(v.tangle(&td.path().join("nope.org")).is_err());
}
