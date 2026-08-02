//! Optional tree-sitter integration for syntax highlighting inside
//! `#+BEGIN_SRC` code blocks.
//!
//! The full tree-sitter C grammar pulls unsafe code and a complicated
//! build; the crate currently exposes the abstract API contract so
//! shells and the kernel can already integrate against it. A real
//! grammar loader (bundled vs. feature-flagged per language) lands
//! once the policy is picked.

#![forbid(unsafe_code)]

/// Coarse highlight kind. Concrete grammars map their tokens to one
/// of these so shells can render with a small, stable palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightKind {
    /// Identifiers, function names, type names.
    Identifier,
    /// Reserved keywords.
    Keyword,
    /// String, number, bool, char literals.
    Literal,
    /// Comments.
    Comment,
    /// Operators and punctuation.
    Punctuation,
    /// Plain text (default fallback).
    Plain,
}

/// One highlight span: a byte range plus a highlight kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Highlight {
    /// Inclusive byte start.
    pub start: usize,
    /// Exclusive byte end.
    pub end: usize,
    /// Classification.
    pub kind: HighlightKind,
}

/// Highlighter implementation contract.
pub trait Highlighter {
    /// Language identifier this highlighter supports.
    fn language(&self) -> &str;
    /// Compute highlights for `source`. The returned spans must be
    /// non-overlapping and cover `[0, source.len())` without gaps so
    /// shells can fold them into a string-buffer renderer without
    /// re-scanning.
    fn highlight(&self, source: &str) -> Vec<Highlight>;
}

/// Default no-op highlighter: classifies the whole input as `Plain`.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoOpHighlighter;

impl Highlighter for NoOpHighlighter {
    #[allow(clippy::unnecessary_literal_bound)]
    fn language(&self) -> &str {
        "plain"
    }

    fn highlight(&self, source: &str) -> Vec<Highlight> {
        // Always produce exactly one span covering [0, len) (including the
        // degenerate 0-length span for empty input). This preserves the
        // pre-existing observable behavior for NoOpHighlighter on "".
        vec![Highlight {
            start: 0,
            end: source.len(),
            kind: HighlightKind::Plain,
        }]
    }
}

/// Dependency-free keyword-based highlighter.
///
/// Provides a pluggable default implementation of [`Highlighter`] for
/// common languages used in org `#+BEGIN_SRC` blocks (rust, python, shell).
/// Always available with no extra dependencies or unsafe code.
///
/// Spans are guaranteed gap-free and cover the entire source (see
/// trait contract). Use [`KeywordHighlighter::for_language`] for runtime
/// selection with sensible fallbacks.
#[derive(Debug, Clone)]
pub struct KeywordHighlighter {
    lang: &'static str,
}

impl KeywordHighlighter {
    /// Highlighter tuned for Rust (fn, let, etc. + "..." literals).
    #[must_use]
    pub const fn rust() -> Self {
        Self { lang: "rust" }
    }

    /// Highlighter tuned for POSIX shell / bash (keywords + # comments).
    #[must_use]
    pub const fn shell() -> Self {
        Self { lang: "shell" }
    }

    /// Highlighter tuned for Python (def, return, etc. + "..." literals).
    #[must_use]
    pub const fn python() -> Self {
        Self { lang: "python" }
    }

    /// Select by language name (case-insensitive).
    ///
    /// Known: "rust"/"rs", "python"/"py", "shell"/"sh"/"bash"/"zsh".
    /// Unknown languages fall back to a plain highlighter (language name "plain").
    #[must_use]
    pub fn for_language(name: &str) -> Self {
        let n = name.to_ascii_lowercase();
        match n.as_str() {
            "rust" | "rs" => Self::rust(),
            "python" | "py" => Self::python(),
            "shell" | "sh" | "bash" | "zsh" => Self::shell(),
            // closure's own configuration language. "config.org syntax
            // — It's just a src block. Which kind of syntax is this?"
            // It is this one, and now something knows it.
            "closure-config" | "closure_config" => Self::closure_config(),
            _ => Self { lang: "plain" },
        }
    }

    /// Highlighter for closure's own `closure-config` blocks.
    #[must_use]
    pub const fn closure_config() -> Self {
        Self {
            lang: "closure-config",
        }
    }

    /// `closure-config` is line-oriented: a comment, or `key = value`.
    ///
    /// A pass of its own rather than more cases in the token scanner,
    /// because the meaning here is positional — the same word is a key
    /// before the `=` and part of a value after it, which a scanner
    /// that classifies words in isolation cannot say.
    fn highlight_config(source: &str) -> Vec<Highlight> {
        let mut spans = Vec::new();
        let mut at = 0usize;
        for line in source.split_inclusive('\n') {
            let end = at + line.len();
            let trimmed = line.trim_start();
            let indent = line.len() - trimmed.len();
            if trimmed.starts_with('#') {
                // Whole line, including a commented-out setting: the
                // generated file comments out every key without a
                // default, and painting those as live config would say
                // the opposite of what they mean.
                spans.push(Highlight {
                    start: at,
                    end,
                    kind: HighlightKind::Comment,
                });
            } else if let Some(eq) = line.find('=') {
                // The key is the word, not the word and the space
                // before the `=`: a highlight that runs into the
                // padding makes the column look ragged.
                let key_end = line[..eq].trim_end().len();
                spans.push(Highlight {
                    start: at,
                    end: at + indent,
                    kind: HighlightKind::Plain,
                });
                spans.push(Highlight {
                    start: at + indent,
                    end: at + key_end,
                    kind: HighlightKind::Keyword,
                });
                spans.push(Highlight {
                    start: at + key_end,
                    end: at + eq,
                    kind: HighlightKind::Plain,
                });
                spans.push(Highlight {
                    start: at + eq,
                    end: at + eq + 1,
                    kind: HighlightKind::Plain,
                });
                spans.push(Highlight {
                    start: at + eq + 1,
                    end,
                    kind: HighlightKind::Literal,
                });
            } else {
                spans.push(Highlight {
                    start: at,
                    end,
                    kind: HighlightKind::Plain,
                });
            }
            at = end;
        }
        spans.retain(|s| s.start < s.end);
        spans
    }

    fn is_keyword(&self, word: &str) -> bool {
        match self.lang {
            "rust" => matches!(
                word,
                "fn" | "let"
                    | "mut"
                    | "pub"
                    | "use"
                    | "mod"
                    | "struct"
                    | "enum"
                    | "impl"
                    | "trait"
                    | "if"
                    | "else"
                    | "match"
                    | "loop"
                    | "for"
                    | "while"
                    | "return"
                    | "break"
                    | "continue"
                    | "const"
                    | "static"
                    | "type"
                    | "where"
                    | "async"
                    | "await"
                    | "move"
                    | "ref"
                    | "self"
                    | "Self"
                    | "true"
                    | "false"
            ),
            "python" => matches!(
                word,
                "def"
                    | "class"
                    | "if"
                    | "elif"
                    | "else"
                    | "for"
                    | "while"
                    | "return"
                    | "yield"
                    | "import"
                    | "from"
                    | "as"
                    | "pass"
                    | "break"
                    | "continue"
                    | "try"
                    | "except"
                    | "finally"
                    | "with"
                    | "lambda"
                    | "and"
                    | "or"
                    | "not"
                    | "in"
                    | "is"
                    | "None"
                    | "True"
                    | "False"
                    | "global"
                    | "nonlocal"
            ),
            "shell" => matches!(
                word,
                "if" | "then"
                    | "else"
                    | "elif"
                    | "fi"
                    | "for"
                    | "while"
                    | "do"
                    | "done"
                    | "case"
                    | "esac"
                    | "function"
                    | "return"
                    | "local"
                    | "export"
                    | "alias"
                    | "echo"
                    | "cd"
                    | "exit"
            ),
            _ => false,
        }
    }
}

impl Highlighter for KeywordHighlighter {
    fn language(&self) -> &str {
        self.lang
    }

    #[allow(clippy::too_many_lines)]
    fn highlight(&self, source: &str) -> Vec<Highlight> {
        if source.is_empty() {
            return vec![];
        }
        if self.lang == "closure-config" {
            return Self::highlight_config(source);
        }

        let bytes = source.as_bytes();
        let mut spans: Vec<Highlight> = Vec::new();
        let mut i = 0usize;

        let mut in_string: Option<u8> = None; // " or '
        let mut in_comment = false;

        while i < bytes.len() {
            let b = bytes[i];

            if in_comment {
                // consume until newline (or end)
                let start = i;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                spans.push(Highlight {
                    start,
                    end: i,
                    kind: HighlightKind::Comment,
                });
                in_comment = false;
                continue;
            }

            if let Some(quote) = in_string {
                let start = i;
                // consume until matching quote (simple; \" is treated as content for our test needs)
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == quote {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                spans.push(Highlight {
                    start,
                    end: i,
                    kind: HighlightKind::Literal,
                });
                in_string = None;
                continue;
            }

            // not in string/comment
            if b == b'"' || b == b'\'' {
                in_string = Some(b);
                // do not advance i here; the string arm will handle
                continue;
            }

            // line comments (lang specific starters)
            let starts_comment = match self.lang {
                "shell" | "python" => b == b'#',
                "rust" => {
                    // support // comments (common even if not in every test)
                    b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/'
                }
                _ => false,
            };
            if starts_comment {
                in_comment = true;
                continue;
            }

            // word / identifier / keyword
            if b.is_ascii_alphanumeric() || b == b'_' {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let word = &source[start..i];
                let kind = if self.is_keyword(word) {
                    HighlightKind::Keyword
                } else {
                    HighlightKind::Identifier
                };
                spans.push(Highlight {
                    start,
                    end: i,
                    kind,
                });
                continue;
            }

            // punctuation / operators / whitespace -> Plain (or Punctuation)
            let start = i;
            // group consecutive non-word chars as one span for cleanliness
            while i < bytes.len()
                && !(bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_')
                && bytes[i] != b'"'
                && bytes[i] != b'\''
                && !matches!(self.lang, "shell" | "python" if bytes[i] == b'#')
                && !(self.lang == "rust"
                    && bytes[i] == b'/'
                    && i + 1 < bytes.len()
                    && bytes[i + 1] == b'/')
            {
                i += 1;
            }
            // but if we advanced 0, force at least one byte
            if i == start {
                i += 1;
            }
            spans.push(Highlight {
                start,
                end: i,
                kind: HighlightKind::Plain,
            });
        }

        // Post-process: merge adjacent same-kind spans and guarantee full coverage + no gaps.
        // (Our scanner should already produce contiguous, but we normalize defensively.)
        let mut out: Vec<Highlight> = Vec::with_capacity(spans.len());
        for s in spans {
            // Merge adjacent same-kind spans (intentional adjacent check, not a bug).
            #[allow(clippy::suspicious_operation_groupings, clippy::collapsible_if)]
            if let Some(last) = out.last_mut()
                && last.kind == s.kind
                && last.end == s.start
            {
                last.end = s.end;
                continue;
            }
            // fill any hypothetical tiny gap with Plain (should not happen)
            #[allow(clippy::collapsible_if)]
            if let Some(last) = out.last()
                && last.end < s.start
            {
                out.push(Highlight {
                    start: last.end,
                    end: s.start,
                    kind: HighlightKind::Plain,
                });
            }
            out.push(s);
        }

        // Ensure starts at 0 and ends at len (fill if needed)
        #[allow(clippy::unnecessary_map_or)]
        if out.first().is_none_or(|s| s.start != 0) {
            let first_start = out.first().map_or(source.len(), |s| s.start);
            if first_start > 0 {
                out.insert(
                    0,
                    Highlight {
                        start: 0,
                        end: first_start,
                        kind: HighlightKind::Plain,
                    },
                );
            }
        }
        if let Some(last) = out.last()
            && last.end < source.len()
        {
            out.push(Highlight {
                start: last.end,
                end: source.len(),
                kind: HighlightKind::Plain,
            });
        }
        if out.is_empty() && !source.is_empty() {
            out.push(Highlight {
                start: 0,
                end: source.len(),
                kind: HighlightKind::Plain,
            });
        }

        out
    }
}

/// Real tree-sitter highlighter over a C grammar (V6, opt-in
/// `tree-sitter` feature).
///
/// Parses `source` with a genuine grammar and maps leaf/keyword/string/
/// comment nodes to [`HighlightKind`], filling inter-token gaps with
/// `Plain` so the [`Highlighter`] coverage contract holds. Default builds
/// never compile this (non-hermetic C grammar); the dep-free
/// [`KeywordHighlighter`] stays the hermetic default.
#[cfg(feature = "tree-sitter")]
#[derive(Clone)]
pub struct TsHighlighter {
    language: String,
    ts_language: tree_sitter::Language,
}

#[cfg(feature = "tree-sitter")]
impl TsHighlighter {
    /// A highlighter for `name`, or `None` if no grammar is bundled for
    /// it. Bundled grammars (D5): `bash`/`sh`/`shell`, `rust`/`rs`,
    /// `python`/`py`, `json`.
    #[must_use]
    pub fn for_language(name: &str) -> Option<Self> {
        let ts_language: tree_sitter::Language = match name {
            "bash" | "sh" | "shell" => tree_sitter_bash::LANGUAGE.into(),
            "rust" | "rs" => tree_sitter_rust::LANGUAGE.into(),
            "python" | "py" => tree_sitter_python::LANGUAGE.into(),
            "json" => tree_sitter_json::LANGUAGE.into(),
            _ => return None,
        };
        Some(Self {
            language: name.to_owned(),
            ts_language,
        })
    }
}

/// The whole-node highlight class for `kind` (comment / string / number),
/// or `None` to descend into children.
#[cfg(feature = "tree-sitter")]
fn ts_unit_kind(kind: &str) -> Option<HighlightKind> {
    // Node-kind names vary per grammar: bash uses `comment`, Rust uses
    // `line_comment`/`block_comment`; strings are `string`/`string_literal`;
    // numbers are `number` (json) or `integer_literal`/`float_literal`
    // (rust). Match by substring / suffix so one mapping spans all bundled
    // grammars. Whole-node units stop descent (the span is the literal).
    if kind.contains("comment") {
        Some(HighlightKind::Comment)
    } else if kind.contains("string") || kind == "number" || kind.ends_with("_literal") {
        Some(HighlightKind::Literal)
    } else {
        None
    }
}

/// The class for a leaf token of `kind` (named or anonymous).
#[cfg(feature = "tree-sitter")]
fn ts_leaf_kind(kind: &str, is_named: bool) -> HighlightKind {
    if is_named {
        match kind {
            "variable_name" | "command_name" | "word" => HighlightKind::Identifier,
            _ => HighlightKind::Plain,
        }
    } else if !kind.is_empty() && kind.chars().all(char::is_alphabetic) {
        // Anonymous tokens are the literal text: alphabetic → keyword,
        // otherwise an operator/punctuation token.
        HighlightKind::Keyword
    } else {
        HighlightKind::Punctuation
    }
}

#[cfg(feature = "tree-sitter")]
fn ts_collect(node: tree_sitter::Node, out: &mut Vec<(usize, usize, HighlightKind)>) {
    if let Some(kind) = ts_unit_kind(node.kind()) {
        out.push((node.start_byte(), node.end_byte(), kind));
        return;
    }
    if node.child_count() == 0 {
        out.push((
            node.start_byte(),
            node.end_byte(),
            ts_leaf_kind(node.kind(), node.is_named()),
        ));
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        ts_collect(child, out);
    }
}

#[cfg(feature = "tree-sitter")]
impl Highlighter for TsHighlighter {
    fn language(&self) -> &str {
        &self.language
    }

    fn highlight(&self, source: &str) -> Vec<Highlight> {
        let plain = || {
            vec![Highlight {
                start: 0,
                end: source.len(),
                kind: HighlightKind::Plain,
            }]
        };
        let mut parser = tree_sitter::Parser::new();
        if parser.set_language(&self.ts_language).is_err() {
            return plain();
        }
        let Some(tree) = parser.parse(source, None) else {
            return plain();
        };
        let mut leaves: Vec<(usize, usize, HighlightKind)> = Vec::new();
        ts_collect(tree.root_node(), &mut leaves);
        leaves.sort_by_key(|(s, _, _)| *s);

        let mut spans = Vec::new();
        let mut pos = 0;
        for (start, end, kind) in leaves {
            if start < pos {
                continue; // defensive: ignore any overlap
            }
            if start > pos {
                spans.push(Highlight {
                    start: pos,
                    end: start,
                    kind: HighlightKind::Plain,
                });
            }
            spans.push(Highlight { start, end, kind });
            pos = end;
        }
        if pos < source.len() {
            spans.push(Highlight {
                start: pos,
                end: source.len(),
                kind: HighlightKind::Plain,
            });
        }
        if spans.is_empty() {
            return plain();
        }
        spans
    }
}
