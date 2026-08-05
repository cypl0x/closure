//! "Add (almost) ALL of the tree-sitter grammars."
//!
//! Four were bundled — bash, rust, python, json — so a `#+BEGIN_SRC
//! nix` block in a vault made of nix config highlighted like plain
//! text. Twenty now, chosen for what turns up in an org file rather
//! than for coverage of crates.io.
//!
//! Two lists of languages live in this crate and they must not drift:
//! the grammar registry, and the line-comment table that
//! `toggle-line-comment` reads. A language the comment table knows and
//! the registry does not is a `#+BEGIN_SRC` block that comments
//! correctly and highlights as prose.

#![cfg(feature = "tree-sitter")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_tree_sitter::TsHighlighter;

/// Every name the registry answers to, with the source it should be
/// able to parse.
const LANGUAGES: &[(&str, &str)] = &[
    ("bash", "echo hi\n"),
    ("sh", "echo hi\n"),
    ("shell", "echo hi\n"),
    ("zsh", "echo hi\n"),
    ("rust", "fn main() {}\n"),
    ("rs", "fn main() {}\n"),
    ("python", "def f():\n    return 1\n"),
    ("py", "def f():\n    return 1\n"),
    ("json", "{\"a\": 1}\n"),
    ("nix", "{ pkgs, ... }: { home.packages = []; }\n"),
    ("javascript", "const a = 1;\n"),
    ("js", "const a = 1;\n"),
    ("node", "const a = 1;\n"),
    ("typescript", "const a: number = 1;\n"),
    ("ts", "const a: number = 1;\n"),
    ("tsx", "const a = <div/>;\n"),
    ("c", "int main(void) { return 0; }\n"),
    ("cpp", "int main() { return 0; }\n"),
    ("c++", "int main() { return 0; }\n"),
    ("go", "package main\nfunc main() {}\n"),
    ("java", "class A { }\n"),
    ("haskell", "main = putStrLn \"hi\"\n"),
    ("ruby", "puts 'hi'\n"),
    ("lua", "print('hi')\n"),
    ("html", "<p>hi</p>\n"),
    ("css", "a { color: red; }\n"),
    ("toml", "a = 1\n"),
    ("yaml", "a: 1\n"),
    ("markdown", "# hi\n"),
];

#[test]
fn every_language_the_registry_names_has_a_grammar() {
    for (name, _) in LANGUAGES {
        assert!(
            TsHighlighter::for_language(name).is_some(),
            "`{name}` is claimed and has no grammar"
        );
    }
}

#[test]
fn every_grammar_parses_a_line_of_its_own_language() {
    // Bundling a grammar and never loading it is how an ABI mismatch
    // ships: the crate resolves, the build passes, and the parser
    // returns nothing at the first `#+BEGIN_SRC`.
    use closure_tree_sitter::Highlighter as _;
    for (name, src) in LANGUAGES {
        let h = TsHighlighter::for_language(name).expect("a grammar");
        let spans = h.highlight(src);
        assert!(
            !spans.is_empty(),
            "`{name}` produced no spans for {src:?} — the grammar loaded and parsed nothing"
        );
    }
}

#[test]
fn a_language_nobody_bundled_says_so_rather_than_guessing() {
    assert!(TsHighlighter::for_language("brainfuck").is_none());
    assert!(TsHighlighter::for_language("").is_none());
}

/// The languages with no way to comment a single line. Not an
/// oversight: there is no such prefix in JSON, HTML or Markdown.
const NO_LINE_COMMENT: &[&str] = &["json", "html", "markdown", "md"];

#[test]
fn the_comment_table_and_the_registry_agree() {
    // Both lists live in this crate and answer questions about the
    // same `#+BEGIN_SRC` block. A language one knows and the other
    // does not is a block that highlights right and comments wrong —
    // `#+BEGIN_SRC rs` was exactly that, commented with org's `#`.
    for (name, _) in LANGUAGES {
        if NO_LINE_COMMENT.contains(name) {
            assert!(
                closure_tree_sitter::line_comment(name).is_none(),
                "`{name}` has no line comment and one was invented for it"
            );
            continue;
        }
        assert!(
            closure_tree_sitter::line_comment(name).is_some(),
            "`{name}` has a grammar and no comment token — `gcc` in that \
             block would fall back to org's `#`"
        );
    }
}
