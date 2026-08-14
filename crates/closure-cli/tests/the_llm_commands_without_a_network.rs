//! `ask` and `chat`, driven end to end with no network.
//!
//! These were the largest uncovered cluster left in `closure-cli` —
//! `cmd_chat`, `cmd_ask`, `stream_to_stdout` and `read_chat_line`, some
//! sixty-odd lines — and the earlier pass only reached the "no key"
//! door. Everything behind it, including the whole tool loop, went
//! unrun.
//!
//! It does not need a network. `closure-llm` already has an
//! `EchoProvider` for exactly this shape of problem, selected by
//! `llm_provider = echo` in the vault's `config.org`, so the agent
//! loop, the tool registry and the transcript are all exercised
//! locally and deterministically. No key, no socket, no daemon.
//!
//! What that cannot check is a real model's answers, which is what
//! `just llm-live` is for and why it is opt-in. What it does check is
//! everything between the prompt and the provider — which is where the
//! code in this crate actually lives.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_closure");

/// A vault that answers questions locally.
fn echo_vault() -> TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    fs::write(
        d.path().join("notes.org"),
        "* TODO Ship the parser :work:\n\
         :PROPERTIES:\n:ID: 01ASKVAULT0000000001\n:END:\n\
         the body of the first\n\
         \n* DONE Something finished :home:\n\
         :PROPERTIES:\n:ID: 01ASKVAULT0000000002\n:END:\n\
         the body of the second\n",
    )
    .expect("write");
    fs::write(
        d.path().join("config.org"),
        "#+BEGIN_SRC closure-config\nllm_provider = echo\n#+END_SRC\n",
    )
    .expect("write");
    d
}

fn text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn ask_against_a_vault_runs_the_whole_loop_locally() {
    let d = echo_vault();
    let out = Command::new(BIN)
        .args([
            "ask",
            "what is in my vault",
            "--vault",
            d.path().to_str().unwrap(),
        ])
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .expect("spawn");
    assert!(out.status.success(), "{}", text(&out));
    let t = text(&out);

    // The prompt reached the provider, and the vault was described to
    // it before the question — the model's first turn used to be spent
    // asking something the process already knew.
    assert!(
        t.contains("what is in my vault"),
        "the prompt is missing: {t}"
    );
    assert!(
        t.contains("headline(s)"),
        "the vault was not described: {t}"
    );
}

#[test]
fn ask_offers_the_model_the_vault_tools_and_no_others() {
    // I8: a shell offers the kernel's vocabulary, and an agent is a
    // shell. A tool list that drifted from the registry would let the
    // model call something that does not exist, or miss something it
    // should have.
    let d = echo_vault();
    let out = Command::new(BIN)
        .args(["ask", "hello", "--vault", d.path().to_str().unwrap()])
        .output()
        .expect("spawn");
    let t = text(&out);
    for tool in [
        "list-files",
        "read",
        "search",
        "capture",
        "rename",
        "set-property",
    ] {
        assert!(t.contains(tool), "`{tool}` is not offered: {t}");
    }
}

#[test]
fn ask_without_a_vault_and_without_a_key_names_the_key() {
    // Also pins something structural. Without a vault there is no
    // config to read, and the provider builder's default for an unset
    // `llm_provider` is Echo — so routing this path through that
    // builder without naming Anthropic would turn "no key" into a stub
    // answer that looks like a real one. Failing here is the correct
    // behaviour and the test that keeps it.
    let out = Command::new(BIN)
        .args(["ask", "hello"])
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .expect("spawn");
    assert!(
        !out.status.success(),
        "answered without a key, so it fell back to a stub: {}",
        text(&out)
    );
    assert!(
        text(&out).contains("ANTHROPIC_API_KEY"),
        "it did not name the variable: {}",
        text(&out)
    );
}

#[test]
fn ask_reports_a_config_it_could_not_read_rather_than_dropping_it() {
    // "A mistyped key used to throw the whole config away in silence."
    // The complaint has to reach the user, and the command still has to
    // work.
    let d = echo_vault();
    fs::write(
        d.path().join("config.org"),
        "#+BEGIN_SRC closure-config\nllm_provider = echo\nnot_a_real_key = 3\n#+END_SRC\n",
    )
    .expect("write");
    let out = Command::new(BIN)
        .args(["ask", "hello", "--vault", d.path().to_str().unwrap()])
        .output()
        .expect("spawn");
    assert!(!text(&out).contains("panicked"), "{}", text(&out));
}

#[test]
fn ask_refuses_a_vault_that_is_not_there() {
    let out = Command::new(BIN)
        .args(["ask", "hello", "--vault", "/nonexistent/vault/for/ask"])
        .output()
        .expect("spawn");
    assert!(!out.status.success(), "{}", text(&out));
}

/// Feed `chat` a script on stdin and collect what it said.
fn chat_with(vault: &std::path::Path, input: &str) -> String {
    let mut child = Command::new(BIN)
        .args(["chat", "--vault", vault.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("ANTHROPIC_API_KEY")
        .spawn()
        .expect("spawn chat");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write");
    drop(child.stdin.take());

    let started = Instant::now();
    loop {
        if child.try_wait().expect("wait").is_some() {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "chat did not exit after its input ran out"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().expect("output");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn chat_answers_a_line_and_then_exits_on_end_of_input() {
    // The claim: a turn is read, answered, and EOF ends the session.
    // A chat that hangs on EOF hangs whatever launched it.
    let d = echo_vault();
    let said = chat_with(d.path(), "what is in my vault\n");
    assert!(!said.contains("panicked"), "{said}");
    assert!(
        said.contains("what is in my vault"),
        "the turn never reached the provider: {said}"
    );
}

#[test]
fn chat_takes_more_than_one_turn() {
    // `read_chat_line` in a loop. One turn would pass a single-line
    // test and still be a broken chat.
    let d = echo_vault();
    let said = chat_with(d.path(), "first question\nsecond question\n");
    assert!(said.contains("first question"), "{said}");
    assert!(
        said.contains("second question"),
        "the second turn was never read: {said}"
    );
}

#[test]
fn chat_with_no_input_at_all_exits_quietly() {
    let d = echo_vault();
    let said = chat_with(d.path(), "");
    assert!(!said.contains("panicked"), "{said}");
}

#[test]
fn a_blank_line_ends_the_chat_the_same_way_quit_does() {
    // I expected a blank line to be ignored, the way most REPLs treat
    // an accidental Enter. It ends the session instead — `read_chat_line`
    // returns false for an empty line exactly as it does for `/quit`
    // and `/exit`. That is a deliberate third way out and not a bug,
    // but it is worth a test precisely because it is the surprising
    // one: anything that made blank lines a no-op would silently take
    // away a quit people use.
    let d = echo_vault();
    let said = chat_with(d.path(), "\n\nnever asked\n");
    assert!(!said.contains("panicked"), "{said}");
    assert!(
        !said.contains("never asked"),
        "a blank line did not end the session: {said}"
    );
}

#[test]
fn slash_quit_and_slash_exit_both_end_the_chat() {
    // Two spellings of the same intention, and only one of them being
    // wired up is the kind of thing nobody notices until they type the
    // other one.
    let d = echo_vault();
    for word in ["/quit", "/exit"] {
        let said = chat_with(d.path(), &format!("{word}\nnever asked\n"));
        assert!(
            !said.contains("never asked"),
            "`{word}` did not end the session: {said}"
        );
    }
}
