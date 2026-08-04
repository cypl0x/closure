//! "API keys go through argv. `CurlProvider` builds
//! `curl -H "x-api-key: sk-..."` via `Command::new("curl").arg(...)`.
//! Any local user can read that from `/proc/<pid>/cmdline` while the
//! request is in flight. Same for the prompt body via `-d`, which
//! additionally will hit `E2BIG` on a long enough context. Both fix
//! the same way: `--config -` and feed headers + `-d @-` over stdin."
//!
//! Reported 2026-08-04 in the Opus 5 review. `/proc/<pid>/cmdline` is
//! world-readable on Linux, so the window is however long the request
//! takes — which for a streaming completion is the whole answer.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_llm::curl_config;

#[test]
fn the_key_and_the_body_are_in_the_config_not_the_arguments() {
    let cfg = curl_config(
        "https://api.example.com/v1/messages",
        &["x-api-key: sk-secret".to_owned()],
        "{\"prompt\":\"hello\"}",
    );
    assert!(cfg.contains("sk-secret"), "{cfg}");
    assert!(cfg.contains("url = "), "{cfg}");
    assert!(cfg.contains("request = \"POST\""), "{cfg}");
}

#[test]
fn quotes_and_backslashes_in_the_body_survive() {
    // The body is JSON, so it is made of the two characters curl's
    // config parser treats specially. A prompt with a quote in it
    // would otherwise truncate the request at that quote — and the
    // model would answer half a question, which reads as the model
    // being bad rather than as a bug.
    let body = r#"{"prompt":"say \"hi\" and a backslash \\ here"}"#;
    let cfg = curl_config("https://x", &[], body);
    let line = cfg
        .lines()
        .find(|l| l.starts_with("data = "))
        .expect("a data line");
    let quoted = line.strip_prefix("data = ").unwrap();
    assert_eq!(unquote(quoted), body, "the body did not survive quoting");
}

#[test]
fn a_newline_in_the_body_does_not_split_the_config() {
    // A config file is line-oriented; a raw newline in a value would
    // end the directive and turn the rest of the prompt into curl
    // options.
    let body = "{\"prompt\":\"line one\nline two\"}";
    let cfg = curl_config("https://x", &[], body);
    assert_eq!(
        cfg.lines().filter(|l| l.starts_with("data = ")).count(),
        1,
        "the body split the config across lines:\n{cfg}"
    );
    let line = cfg.lines().find(|l| l.starts_with("data = ")).unwrap();
    assert_eq!(unquote(line.strip_prefix("data = ").unwrap()), body);
}

#[test]
fn every_header_gets_its_own_line() {
    let cfg = curl_config(
        "https://x",
        &[
            "content-type: application/json".to_owned(),
            "x-api-key: sk-secret".to_owned(),
        ],
        "{}",
    );
    assert_eq!(
        cfg.lines().filter(|l| l.starts_with("header = ")).count(),
        2
    );
}

/// Undo curl's config quoting, so a test reads what curl would.
fn unquote(s: &str) -> String {
    let inner = s.trim_start_matches('"').trim_end_matches('"');
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}
