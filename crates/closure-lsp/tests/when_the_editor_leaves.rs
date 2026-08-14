//! The text dispatcher when the editor goes away, and the symbols it
//! offers when the file is odd.
//!
//! Third crate with the same gap and the same reason it matters: every
//! test writes into a `Vec<u8>`, a `Vec` cannot fail, so the arms
//! mapping write failures to `LspError::Transport` had never run. For
//! an LSP the far end leaving is not an exception — it is what happens
//! every time the editor closes, which is how most sessions end.
//!
//! `document_symbols` is the outline an editor draws in its sidebar,
//! and its awkward inputs had no tests: a headline with no title, one
//! that is only stars, and the stripping of TODO keywords, priority
//! cookies and tags — each of which appearing in the sidebar is a
//! visible wart in somebody's editor rather than a crash.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::io::{self, Write};

use closure_core::{Registry, RenameHeadline};
use closure_lsp::{LspError, Outcome, document_symbols, resolve_line, run};

struct BrokenPipe;

impl Write for BrokenPipe {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "the editor closed",
        ))
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "the editor closed",
        ))
    }
}

struct FailingRead;

impl io::Read for FailingRead {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("the pipe dropped mid-line"))
    }
}

fn registry() -> Registry {
    let mut r = Registry::new();
    r.register(Box::new(RenameHeadline::new_placeholder()));
    r
}

#[test]
fn a_write_that_fails_is_reported_for_a_known_command() {
    let r = registry();
    let err = run(&r, &b"rename-headline x\n"[..], &mut BrokenPipe)
        .expect_err("a closed editor was ignored");
    assert!(matches!(err, LspError::Transport(_)), "{err:?}");
}

#[test]
fn a_write_that_fails_is_reported_for_an_unknown_one_too() {
    let r = registry();
    let err = run(&r, &b"no-such-command\n"[..], &mut BrokenPipe)
        .expect_err("a closed editor was ignored");
    assert!(matches!(err, LspError::Transport(_)), "{err:?}");
}

#[test]
fn a_read_that_fails_is_not_end_of_input() {
    let r = registry();
    let mut out: Vec<u8> = Vec::new();
    let err = run(&r, std::io::BufReader::new(FailingRead), &mut out)
        .expect_err("a dropped pipe looked like a clean exit");
    assert!(matches!(err, LspError::Transport(_)), "{err:?}");
}

#[test]
fn blank_lines_and_comments_get_no_reply() {
    let r = registry();
    for quiet in ["", "  ", "\t", "# a comment"] {
        assert_eq!(resolve_line(&r, quiet), Outcome::Skip, "{quiet:?}");
    }
    let mut out: Vec<u8> = Vec::new();
    run(&r, &b"\n# comment\n   \n"[..], &mut out).expect("run");
    assert!(
        out.is_empty(),
        "quiet lines answered: {:?}",
        String::from_utf8_lossy(&out)
    );
}

#[test]
fn a_known_and_an_unknown_command_are_told_apart() {
    let r = registry();
    assert_eq!(
        resolve_line(&r, "rename-headline arg"),
        Outcome::Found("rename-headline".to_owned())
    );
    assert_eq!(
        resolve_line(&r, "nonsense"),
        Outcome::Unknown("nonsense".to_owned())
    );
}

// === document symbols ===

#[test]
fn a_symbol_is_the_bare_title_with_the_decoration_stripped() {
    // What the editor draws in its sidebar. A keyword, cookie or tag
    // leaking through is a visible wart in somebody's editor.
    let syms = document_symbols("* TODO [#A] Ship the parser :work:urgent:\n");
    assert_eq!(syms.len(), 1, "{syms:?}");
    assert_eq!(syms[0].name, "Ship the parser");
    assert_eq!(syms[0].level, 1);
    assert_eq!(syms[0].line, 0);
}

#[test]
fn levels_and_line_numbers_follow_the_file() {
    let syms = document_symbols("* One\nbody\n** Two\n*** Three\n* Four\n");
    let shape: Vec<(&str, u8, u32)> = syms
        .iter()
        .map(|s| (s.name.as_str(), s.level, s.line))
        .collect();
    assert_eq!(
        shape,
        vec![
            ("One", 1, 0),
            ("Two", 2, 2),
            ("Three", 3, 3),
            ("Four", 1, 4)
        ]
    );
}

#[test]
fn a_headline_with_no_title_still_appears() {
    // `*` alone is a headline org accepts. Dropping it from the
    // sidebar makes the outline disagree with the file, and every
    // symbol after it appears to be at the wrong place in the tree.
    let syms = document_symbols("* \n** A child\n");
    assert_eq!(syms.len(), 2, "a titleless headline was dropped: {syms:?}");
    assert_eq!(syms[0].level, 1);
    assert_eq!(syms[1].name, "A child");
}

#[test]
fn stars_inside_a_line_do_not_make_a_headline() {
    // Bold text, a multiplication, a list bullet — none of them start
    // an outline node, and an editor sidebar full of them is useless.
    for src in [
        "not *bold* a headline\n",
        "  * an indented bullet\n",
        "a * b\n",
    ] {
        assert!(
            document_symbols(src).is_empty(),
            "{src:?} produced a symbol"
        );
    }
}

#[test]
fn a_file_with_no_headlines_has_no_symbols() {
    assert!(document_symbols("").is_empty());
    assert!(document_symbols("just prose\nand more\n").is_empty());
}

#[test]
fn a_keyword_that_is_not_one_stays_in_the_title() {
    // `* Ship it` has no keyword; the first word must not be eaten on
    // the assumption that it is one.
    let syms = document_symbols("* Ship the parser\n");
    assert_eq!(syms[0].name, "Ship the parser");

    // And a lowercase word that looks like a keyword is not one.
    let syms = document_symbols("* todo something\n");
    assert_eq!(syms[0].name, "todo something");
}
