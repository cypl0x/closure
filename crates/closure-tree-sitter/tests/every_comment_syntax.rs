//! The comment prefix for every language family this crate knows.
//!
//! `line_comment` exists because commenting a line is a per-language
//! fact and getting it wrong corrupts the file rather than annoying
//! the user — the doc comment says it outright: "putting org's `#` in
//! front of a line of JSON does not comment it, it breaks it."
//!
//! Six of its seven arms had never been reached. That is the worst
//! possible distribution for this function: the arms are the whole
//! content, each is one line, and a language listed under the wrong one
//! is a data-loss bug for anybody who comments a line in that language.
//!
//! So every alias in every arm is checked, not one per arm. The aliases
//! are the likely mistake — `hs` sitting with `haskell`, `elisp` with
//! `emacs-lisp` — because they are added later and by hand.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_tree_sitter::line_comment;

/// Every language the crate claims, grouped by what it comments with.
const FAMILIES: &[(&str, &[&str])] = &[
    (
        "//",
        &[
            "rust",
            "rs",
            "javascript",
            "js",
            "node",
            "typescript",
            "ts",
            "tsx",
            "jsonc",
            "c",
            "cpp",
            "c++",
            "cxx",
            "java",
            "go",
            "golang",
            "scala",
            "kotlin",
            "swift",
            "zig",
            "dart",
            "php",
            "css",
            "scss",
            "jq",
        ],
    ),
    (
        "#",
        &[
            "nix",
            "shell",
            "sh",
            "bash",
            "zsh",
            "fish",
            "python",
            "py",
            "ruby",
            "rb",
            "perl",
            "r",
            "yaml",
            "yml",
            "toml",
            "ini",
            "conf",
            "make",
            "makefile",
            "dockerfile",
            "awk",
            "tcl",
            "elixir",
            "julia",
            "org",
        ],
    ),
    (
        ";;",
        &["lisp", "emacs-lisp", "elisp", "clojure", "scheme", "racket"],
    ),
    ("--", &["sql", "haskell", "hs", "lua", "elm", "ada"]),
    ("\"", &["vim", "vimscript"]),
    ("%", &["erlang", "latex", "tex", "matlab"]),
    (";", &["asm", "nasm"]),
];

#[test]
fn every_language_gets_the_prefix_its_family_uses() {
    for (prefix, langs) in FAMILIES {
        for lang in *langs {
            assert_eq!(
                line_comment(lang),
                Some(*prefix),
                "`{lang}` should comment with `{prefix}`"
            );
        }
    }
}

#[test]
fn no_language_appears_in_two_families() {
    // A duplicate would make the answer depend on match order, which
    // is exactly the kind of thing that survives a rename.
    let mut seen: Vec<&str> = Vec::new();
    for (_, langs) in FAMILIES {
        for lang in *langs {
            assert!(!seen.contains(lang), "`{lang}` is listed twice");
            seen.push(lang);
        }
    }
}

#[test]
fn a_language_it_does_not_know_gets_no_prefix_rather_than_a_guess() {
    // The important refusal. Guessing `#` for an unknown language is
    // how you break a JSON file, and returning None lets the caller
    // decline to comment at all.
    for unknown in [
        "",
        "json",
        "html",
        "xml",
        "brainfuck",
        "COBOL",
        "rust-analyzer",
    ] {
        assert_eq!(line_comment(unknown), None, "`{unknown}` got a prefix");
    }
}

#[test]
fn json_specifically_gets_nothing_while_jsonc_gets_slashes() {
    // The example the doc comment gives, asserted directly: plain JSON
    // has no comment syntax at all and `jsonc` does.
    assert_eq!(line_comment("json"), None);
    assert_eq!(line_comment("jsonc"), Some("//"));
}

#[test]
fn the_lookup_is_case_sensitive_and_says_so_by_refusing() {
    // Org writes `#+BEGIN_SRC rust`, lowercase, and that is what
    // reaches here. Accepting `Rust` would be a second spelling of the
    // same key with nothing keeping the two in step.
    assert_eq!(line_comment("Rust"), None);
    assert_eq!(line_comment("PYTHON"), None);
}
