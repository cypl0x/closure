//! Shell-agnostic launcher state core for closure's GUI shells.
//!
//! Pure, GPU-free state: the browse/filter list, detail/preview pane,
//! command palette, capture/rename/add edit surfaces, and input-mode
//! awareness. gpui, egui, and any future GUI consume this ONE core so
//! behaviour is identical and fully unit-testable without a window
//! (the vision's decoupled engine/shell). Kernel-agnostic (I7,
//! consumes Vault + closure-query); mutations route through the
//! [`Shell`] / vault commands (I8).

#![forbid(unsafe_code)]

use std::path::PathBuf;

use closure_config::InputMode;
pub use closure_sniffer::Action as FlowAction;
use closure_store::Vault;
pub use closure_tree_sitter::HighlightKind;

/// Fold the keyword highlighter's byte ranges into owned segments.
///
/// Produces `(text, kind)` segments a GUI can colour without
/// re-scanning, via the dependency-free
/// [`closure_tree_sitter::KeywordHighlighter`] (hermetic — no
/// tree-sitter C grammar). Empty source yields no spans; otherwise the
/// segments concatenate back to `source` exactly.
#[must_use]
pub fn highlight_spans(source: &str, lang: &str) -> Vec<(String, HighlightKind)> {
    use closure_tree_sitter::{Highlighter as _, KeywordHighlighter};
    if source.is_empty() {
        return Vec::new();
    }
    KeywordHighlighter::for_language(lang)
        .highlight(source)
        .into_iter()
        .filter(|h| h.start < h.end)
        .map(|h| (source[h.start..h.end].to_owned(), h.kind))
        .collect()
}

/// One segment of a headline body: free-form prose or a fenced code
/// block (carrying its normalised language). Lets a GUI render code
/// blocks distinct from prose without re-parsing the fences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodySegment {
    /// Prose lines (everything outside a `#+BEGIN_SRC … #+END_SRC`).
    Prose(String),
    /// A code block: `lang` is the normalised fence language (`"plain"`
    /// when none), `text` the block's inner content.
    Code {
        /// Normalised fence language (matches [`highlight_spans`]).
        lang: String,
        /// Inner content of the block (between the fences).
        text: String,
    },
}

/// Normalise a fence language token to the names [`highlight_spans`]
/// understands (`sh`/`bash`/`zsh` → `shell`, `py` → `python`, `rs` →
/// `rust`, empty → `plain`).
fn normalise_lang(token: &str) -> String {
    match token.to_ascii_lowercase().as_str() {
        "" => "plain".to_owned(),
        "sh" | "bash" | "zsh" => "shell".to_owned(),
        "py" => "python".to_owned(),
        "rs" => "rust".to_owned(),
        other => other.to_owned(),
    }
}

/// A `#+BEGIN_SRC … #+END_SRC` block located by line in a buffer —
/// what `C-c C-c` runs ([`code_block_at`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferBlock {
    /// The `#+BEGIN_SRC` line, 0-based.
    pub begin: usize,
    /// The `#+END_SRC` line, 0-based.
    pub end: usize,
    /// The fence's language token, as written (`sh`, not `shell`) —
    /// the trust gate canonicalises both sides itself.
    pub lang: String,
    /// The rest of the header line: `:var x=1`, `:results silent`.
    pub args: String,
    /// The block's content, newline-terminated per line.
    pub program: String,
}

/// The block `line` is inside, fences included, or `None` in prose.
///
/// Line-based rather than [`segment_body`]-based on purpose: this
/// answers *which* block the cursor is in and where its fences are, so
/// the results can be written back next to it.
///
/// An unterminated fence is not a block: mid-edit, the writer has not
/// decided where it ends, and running to the end of the buffer would
/// run whatever they type next.
#[must_use]
pub fn code_block_at(text: &str, line: usize) -> Option<BufferBlock> {
    let lines: Vec<&str> = text.lines().collect();
    let mut begin: Option<usize> = None;
    for (i, raw) in lines.iter().enumerate() {
        let head = raw.trim_start();
        if begin.is_none() && head.to_ascii_uppercase().starts_with("#+BEGIN_SRC") {
            begin = Some(i);
        } else if let Some(start) = begin
            && head.to_ascii_uppercase().starts_with("#+END_SRC")
        {
            if (start..=i).contains(&line) {
                let header = lines[start].trim_start();
                let rest = header[header.find(' ').unwrap_or(header.len())..].trim_start();
                let (lang, args) = rest.split_once(' ').unwrap_or((rest, ""));
                let mut program = String::new();
                for body_line in &lines[start + 1..i] {
                    program.push_str(body_line);
                    program.push('\n');
                }
                return Some(BufferBlock {
                    begin: start,
                    end: i,
                    lang: lang.to_owned(),
                    args: args.to_owned(),
                    program,
                });
            }
            begin = None;
        }
    }
    None
}

/// A src block whose result is a picture rather than text.
///
/// "mermaid diagrams" and "(inline) LaTeX preview": both are org's
/// oldest trick — hand a block to an external program and look at what
/// comes back. The picture belongs under the block that produced it,
/// which is why the closing fence's line is carried here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramBlock {
    /// The `#+END_SRC` line the picture is painted under.
    pub line: usize,
    /// The canonical diagram language (`mermaid`, `latex`).
    pub lang: String,
    /// The block's source, fences excluded.
    pub src: String,
}

/// Every diagram block in `text`, in order.
///
/// Only the languages that make pictures ([`closure_eval::diagram_for`]).
/// A `shell` block still produces text and `#+RESULTS:` is still the
/// contract for those — nothing here changes that.
#[must_use]
pub fn diagram_blocks(text: &str) -> Vec<DiagramBlock> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut begin: Option<usize> = None;
    for (i, raw) in lines.iter().enumerate() {
        let head = raw.trim_start().to_ascii_uppercase();
        if begin.is_none() && head.starts_with("#+BEGIN_SRC") {
            begin = Some(i);
        } else if begin.is_some() && head.starts_with("#+END_SRC") {
            // `begin` is cleared *here* and not in the pattern: taking
            // it while matching would clear it on every ordinary line
            // inside the block, so no block ever reached its fence.
            let start = begin.take().unwrap_or(i);
            let header = lines[start].trim_start();
            let rest = header[header.find(' ').unwrap_or(header.len())..].trim_start();
            let lang = rest.split_whitespace().next().unwrap_or("");
            if let Some(kind) = closure_eval::diagram_for(lang) {
                let mut src = String::new();
                for body_line in &lines[start + 1..i] {
                    src.push_str(body_line);
                    src.push('\n');
                }
                out.push(DiagramBlock {
                    line: i,
                    lang: kind.language().to_owned(),
                    src,
                });
            }
        }
    }
    out
}

/// `text` with `results` attached as a `#+RESULTS:` block directly
/// after line `after` (a block's `#+END_SRC`), replacing the one
/// already there.
///
/// The shape is org's — `#+RESULTS:` then one `: ` line per output
/// line, `:` alone when a block printed nothing — and matches what
/// [`closure_org::rewrite_attach_results_to_code_block`] writes on the
/// saved-file path, so a run from the buffer and a run from the Blocks
/// list leave the same text behind.
#[must_use]
pub fn attach_results(text: &str, after: usize, results: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = lines[..=after.min(lines.len().saturating_sub(1))]
        .iter()
        .map(|l| (*l).to_owned())
        .collect();
    out.push("#+RESULTS:".to_owned());
    if results.is_empty() {
        out.push(":".to_owned());
    } else {
        for line in results.lines() {
            out.push(format!(": {line}"));
        }
    }
    // Whatever the previous run left is replaced, not stacked up.
    let mut rest = lines[(after + 1).min(lines.len())..].iter().peekable();
    if rest.peek().is_some_and(|l| {
        l.trim_start()
            .to_ascii_uppercase()
            .starts_with("#+RESULTS:")
    }) {
        rest.next();
        while rest
            .peek()
            .is_some_and(|l| *l == &":" || l.starts_with(": "))
        {
            rest.next();
        }
    }
    out.extend(rest.map(|l| (*l).to_owned()));
    let mut joined = out.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

/// Split a headline body into prose / code-block [`BodySegment`]s.
///
/// Consecutive prose lines coalesce into one [`BodySegment::Prose`];
/// each `#+BEGIN_SRC [lang] … #+END_SRC` becomes a
/// [`BodySegment::Code`] (fence match is case-insensitive). An
/// unterminated block captures to the end. Empty input yields no
/// segments.
#[must_use]
pub fn segment_body(body: &str) -> Vec<BodySegment> {
    let mut out: Vec<BodySegment> = Vec::new();
    let mut prose: Vec<&str> = Vec::new();
    let mut lines = body.lines();

    let flush_prose = |prose: &mut Vec<&str>, out: &mut Vec<BodySegment>| {
        if !prose.is_empty() {
            out.push(BodySegment::Prose(prose.join("\n")));
            prose.clear();
        }
    };

    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if trimmed.to_ascii_uppercase().starts_with("#+BEGIN_SRC") {
            flush_prose(&mut prose, &mut out);
            let lang = normalise_lang(trimmed.split_whitespace().nth(1).unwrap_or(""));
            let mut code: Vec<&str> = Vec::new();
            for cl in lines.by_ref() {
                if cl
                    .trim_start()
                    .to_ascii_uppercase()
                    .starts_with("#+END_SRC")
                {
                    break;
                }
                code.push(cl);
            }
            out.push(BodySegment::Code {
                lang,
                text: code.join("\n"),
            });
        } else {
            prose.push(line);
        }
    }
    flush_prose(&mut prose, &mut out);
    out
}

/// Selection state (parity with egui Shell for consistent multi-UI model).
#[derive(Debug, Clone, Default)]
pub struct Selection {
    /// Currently-selected file, if any.
    pub file: Option<PathBuf>,
    /// Index of the selected headline within the file.
    pub headline: usize,
}

/// Kernel-side shell state for gpui (I7: only L2, no direct org spans).
/// Reuses Vault + commands for mutations (I8).
pub struct Shell {
    /// The loaded vault.
    pub vault: Vault,
    /// Current selection.
    pub selection: Selection,
}

/// The file a capture lands in when nothing is selected — org's
/// refile target, the one place a thought with no home goes.
///
/// Re-exported from [`closure_store`] rather than spelled again: the
/// shells tell the user where their capture is going, and a second
/// spelling of it would be a second answer.
pub use closure_store::CAPTURE_FILE;

/// One step on the path a capture will file into: the file at the
/// head, then each headline down to the target.
///
/// Shells render these as breadcrumbs and hand a click back through
/// [`ModalApp::pick_capture_crumb`], so the same list is both the
/// answer to "where is this going" and the way to change it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureCrumb {
    /// Block id of the headline, or `None` for the file crumb — and
    /// for a headline that carries no `:ID:`, which cannot be filed
    /// under and so is shown but not offered.
    pub id: Option<String>,
    /// File name or headline title, as shown.
    pub label: String,
    /// Whether this is the step the capture will actually file into.
    /// Exactly one crumb in a list is active.
    pub active: bool,
}

/// A file name and the headline chain under it: each step's block id
/// (absent when the headline carries no `:ID:`) and its title.
type CaptureChain = (String, Vec<(Option<String>, String)>);

/// The file name and the headline chain (outermost first) above and
/// including `id`, or `None` when the id is not in the vault.
///
/// Read from the document rather than from the visible rows: a filter
/// or a fold can hide an ancestor from the outline, and the path a
/// capture takes does not change because you cannot currently see part
/// of it.
fn capture_chain(shell: &Shell, id: &str) -> Option<CaptureChain> {
    let bid = closure_core::BlockId::from_existing(id);
    let (_, path) = shell.vault.find_by_id(&bid)?;
    let file = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let doc = shell.vault.document(path)?;
    let target = doc.headline_by_id(&bid)?.path().to_vec();
    // Every headline carries its outline path, so an ancestor is one
    // whose path is a prefix of the target's — no tree walk, and the
    // order falls out of the depth.
    let mut chain: Vec<(usize, Option<String>, String)> = doc
        .all_headlines()
        .filter(|h| {
            !h.path().is_empty() && h.path().len() <= target.len() && target.starts_with(h.path())
        })
        .map(|h| {
            (
                h.path().len(),
                Some(h.id().to_string()),
                h.title().to_owned(),
            )
        })
        .collect();
    chain.sort_by_key(|(depth, _, _)| *depth);
    Some((
        file,
        chain
            .into_iter()
            .map(|(_, id, title)| (id, title))
            .collect(),
    ))
}

impl Shell {
    /// Build a shell over an already-loaded vault.
    #[must_use]
    pub const fn new(vault: Vault) -> Self {
        Self {
            vault,
            selection: Selection {
                file: None,
                headline: 0,
            },
        }
    }

    /// Capture a new `TODO` entry into [`CAPTURE_FILE`] (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`] from the capture.
    pub fn capture(
        &mut self,
        title: &str,
    ) -> Result<closure_core::BlockId, closure_store::VaultError> {
        let template = closure_store::CaptureTemplate {
            target: std::path::PathBuf::from(CAPTURE_FILE),
            headline_prefix: "TODO ".to_owned(),
            body: String::new(),
        };
        self.vault.capture(&template, title)
    }

    /// Capture a new `TODO` entry at the top level of `file`, relative
    /// to the vault root (I8).
    ///
    /// [`Self::capture`] is this with [`CAPTURE_FILE`]: the inbox is
    /// where a thought goes when there is no better answer, not the
    /// only file a capture can reach. A missing file is created, the
    /// way org creates a capture target.
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`] from the capture.
    pub fn capture_into(
        &mut self,
        file: &str,
        title: &str,
    ) -> Result<closure_core::BlockId, closure_store::VaultError> {
        let template = closure_store::CaptureTemplate {
            target: std::path::PathBuf::from(file),
            headline_prefix: "TODO ".to_owned(),
            body: String::new(),
        };
        self.vault.capture(&template, title)
    }

    /// Set a body, filing headlines typed into it as children (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    /// Every child of `id`, verbatim — what a body editor showing the
    /// whole subtree puts under the prose (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn children_source(
        &self,
        id: &closure_core::BlockId,
    ) -> Result<String, closure_store::VaultError> {
        self.vault.children_source(id)
    }

    /// Replace everything under `id` — body and children — with what
    /// the buffer holds (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn set_subtree(
        &mut self,
        id: &closure_core::BlockId,
        body: &str,
        children_src: &str,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.set_subtree(id, body, children_src)
    }

    /// Set a body, filing headlines typed into it as children (I8).
    ///
    /// [`Self::set_subtree`] is the one to reach for when the buffer
    /// shows the children too — this one can add them but never remove
    /// them.
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn set_body_with_children(
        &mut self,
        id: &closure_core::BlockId,
        body: &str,
        children_src: &str,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.set_body_with_children(id, body, children_src)
    }

    /// Capture a new `TODO` as the last child of `parent` (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`] from the capture.
    pub fn capture_under(
        &mut self,
        parent: &closure_core::BlockId,
        title: &str,
    ) -> Result<closure_core::BlockId, closure_store::VaultError> {
        self.vault.capture_under(parent, "TODO ", title)
    }

    /// Select `path` and reset the headline cursor.
    pub fn select_file(&mut self, path: Option<std::path::PathBuf>) {
        self.selection.file = path;
        self.selection.headline = 0;
    }

    /// Vault-wide fuzzy headline search, best 20 matches first.
    #[must_use]
    pub fn fuzzy_search(&self, q: &str) -> Vec<(std::path::PathBuf, String)> {
        let mut scored: Vec<(u32, std::path::PathBuf, String)> = vec![];
        for (p, doc) in self.vault.iter() {
            for h in doc.all_headlines() {
                if let Some(sc) = closure_query::fuzzy_score(q, h.title()) {
                    scored.push((sc, p.to_path_buf(), h.title().to_owned()));
                }
            }
        }
        scored.sort_by_key(|(sc, _, _)| std::cmp::Reverse(*sc));
        scored
            .into_iter()
            .map(|(_, p, t)| (p, t))
            .take(20)
            .collect()
    }

    /// Rename a headline through the kernel command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn rename_headline(
        &mut self,
        id: &closure_core::BlockId,
        title: &str,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.rename_headline(id, title)
    }

    /// Remove a subtree through the kernel command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn remove_subtree(
        &mut self,
        id: &closure_core::BlockId,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.remove_subtree(id)
    }

    /// Cut the subtree rooted at `id`: onto the kill ring, then out of
    /// the document, through the kernel command (I8).
    ///
    /// What `delete` does. Dropping the text on the floor left undo as
    /// the only way back, and undo is not a way to *move* something.
    ///
    /// # Errors
    ///
    /// [`closure_store::VaultError::UnknownId`] when nothing has that
    /// id; otherwise the [`closure_store::Vault::cut`] contract.
    pub fn cut_subtree(
        &mut self,
        id: &closure_core::BlockId,
    ) -> Result<(), closure_store::VaultError> {
        let path = self
            .vault
            .find_by_id(id)
            .map(|(_, p)| p.to_path_buf())
            .ok_or_else(|| closure_store::VaultError::UnknownId(id.as_str().to_owned()))?;
        self.vault.cut(&path, id)
    }

    /// Paste the kill ring's top as the sibling after `after` (I8).
    ///
    /// # Errors
    ///
    /// [`closure_store::VaultError::UnknownId`] when nothing has that
    /// id; otherwise the [`closure_store::Vault::paste`] contract,
    /// which refuses an empty ring.
    pub fn paste_subtree(
        &mut self,
        after: &closure_core::BlockId,
    ) -> Result<(), closure_store::VaultError> {
        let path = self
            .vault
            .find_by_id(after)
            .map(|(_, p)| p.to_path_buf())
            .ok_or_else(|| closure_store::VaultError::UnknownId(after.as_str().to_owned()))?;
        self.vault.paste(&path, after)
    }

    /// What the assistant is told about this vault before its task.
    ///
    /// The tool loop used to receive a bare instruction: no idea which
    /// vault, how large, or what is in it. So the model's first turn
    /// went on asking — a round trip and an API call to learn
    /// something the process already knew.
    ///
    /// Shape only, never contents. The assistant reads notes through
    /// tools, and those are gated by `llm_tools`; a preamble that
    /// quietly pasted body text in would route around that gate
    /// entirely, which is not a thing a convenience should be able to
    /// do.
    ///
    /// Short on purpose: context that costs more than it saves is a
    /// tax on every prompt.
    #[must_use]
    pub fn assistant_context(&self) -> String {
        let files = self.vault.iter().count();
        let headlines: usize = self
            .vault
            .iter()
            .map(|(_, doc)| doc.all_headlines().count())
            .sum();
        let name = self.vault.root().file_name().map_or_else(
            || self.vault.root().display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        format!(
            "VAULT: {name} — {files} file(s), {headlines} headline(s). \
             Plain org files; read them with the tools below rather than guessing."
        )
    }

    /// Merge one block of a peer's replica into the vault, returning
    /// how many changes it made.
    ///
    /// The merge used to skip any id `find_by_id` could not resolve,
    /// so edits to headlines both machines already shared arrived and
    /// anything *created* on the peer was dropped in silence — the
    /// half of syncing anybody notices first, made worse by the status
    /// line then reporting the edits it *had* applied.
    ///
    /// An id we do not have is not an error and not a conflict: it is
    /// a note somebody else wrote. It is created, keeping the peer's
    /// id — the id is the identity, so minting a fresh one would make
    /// the next round treat it as different again and write a second
    /// copy.
    ///
    /// Applying what is already true costs nothing: a sync that
    /// dirties every note on every round is a sync that fights git and
    /// the file watcher.
    pub fn apply_peer_block(&mut self, id: &str, title: Option<&str>, body: Option<&str>) -> usize {
        let bid = closure_core::BlockId::from_existing(id);
        let Some((headline, _)) = self.vault.find_by_id(&bid) else {
            return usize::from(self.create_peer_block(&bid, title, body));
        };
        let (current_title, current_body) =
            (headline.title().to_owned(), headline.body_text().to_owned());
        let mut applied = 0usize;
        if let Some(title) = title
            && title != current_title
            && self.rename_headline(&bid, title).is_ok()
        {
            applied += 1;
        }
        if let Some(body) = body
            && body != current_body
            && self.set_body(&bid, body).is_ok()
        {
            applied += 1;
        }
        applied
    }

    /// Write a headline the peer has and this vault does not.
    ///
    /// It lands in the capture file, which is already where a thought
    /// with no home goes — a note arriving over the network has no
    /// more of a parent than one you typed into the capture bar.
    fn create_peer_block(
        &mut self,
        id: &closure_core::BlockId,
        title: Option<&str>,
        body: Option<&str>,
    ) -> bool {
        // Neither a title nor a body is not a note; a replica can
        // carry an id that nothing has said anything about yet.
        if title.is_none() && body.is_none() {
            return false;
        }
        let title = title.unwrap_or("(untitled)");
        let body = body.unwrap_or_default();
        let entry = format!(
            "* {title}\n:PROPERTIES:\n:ID: {}\n:END:\n{body}",
            id.as_str()
        );
        let relative = std::path::Path::new(CAPTURE_FILE);
        let full = self.vault.root().join(relative);
        let result = if full.is_file() {
            let mut text = std::fs::read_to_string(&full).unwrap_or_default();
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&entry);
            std::fs::write(&full, text)
                .map(|()| full.clone())
                .map_err(closure_store::VaultError::Io)
        } else {
            self.vault.create_file(relative, &entry)
        };
        if result.is_ok() {
            // The file changed underneath the index, so the next
            // lookup has to see it — without this the same block
            // arrives again on the next round and is written twice.
            let _ = self.vault.reload_incremental();
        }
        result.is_ok()
    }

    /// Paste arbitrary org `text` as the sibling after `after`.
    ///
    /// [`Self::paste_subtree`] can only replay what closure itself
    /// cut. This is the door for everything else — a subtree copied
    /// out of a browser, another window, another editor — which is the
    /// inward half of "sync with system clipboard (two way)" for the
    /// outline.
    ///
    /// # Errors
    ///
    /// [`closure_store::VaultError::UnknownId`] when nothing has that
    /// id; otherwise the [`closure_store::Vault::paste_text`] contract.
    pub fn paste_org_after(
        &mut self,
        after: &closure_core::BlockId,
        text: &str,
    ) -> Result<(), closure_store::VaultError> {
        let path = self
            .vault
            .find_by_id(after)
            .map(|(_, p)| p.to_path_buf())
            .ok_or_else(|| closure_store::VaultError::UnknownId(after.as_str().to_owned()))?;
        self.vault.paste_text(&path, after, text)
    }

    /// The top of the kill ring, if anything has been cut or copied.
    #[must_use]
    pub fn ring_top(&self) -> Option<&str> {
        self.vault.ring_top()
    }

    /// Add a child headline under `parent`, `prefix` in front of the
    /// title, through the kernel command (I8).
    ///
    /// [`Self::capture_under`] is this with org's `TODO ` prefix
    /// fixed. A new headline is not always a task — `C-RET` makes a
    /// plain one and `C-S-RET` makes the TODO — so the keyword has to
    /// be the caller's to decide.
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn add_child(
        &mut self,
        parent: &closure_core::BlockId,
        prefix: &str,
        title: &str,
    ) -> Result<closure_core::BlockId, closure_store::VaultError> {
        self.vault.capture_under(parent, prefix, title)
    }

    /// Add a sibling headline after `after_id` through the kernel
    /// command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn add_sibling(
        &mut self,
        after_id: &closure_core::BlockId,
        title: &str,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.add_sibling(after_id, title)
    }

    /// Insert a new sibling headline above another (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn add_sibling_before(
        &mut self,
        before_id: &closure_core::BlockId,
        title: &str,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.add_sibling_before(before_id, title)
    }

    /// Promote a headline one level through the kernel command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn promote(&mut self, id: &closure_core::BlockId) -> Result<(), closure_store::VaultError> {
        self.vault.promote(id)
    }

    /// Demote a headline one level through the kernel command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn demote(&mut self, id: &closure_core::BlockId) -> Result<(), closure_store::VaultError> {
        self.vault.demote(id)
    }

    /// Move `id`'s subtree right after `after`'s through the kernel
    /// command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn move_after(
        &mut self,
        id: &closure_core::BlockId,
        after: &closure_core::BlockId,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.move_after(id, after)
    }

    /// Replace a headline's body text through the kernel command (I8).
    /// This is the GUI's org-edit-special commit path.
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn set_body(
        &mut self,
        id: &closure_core::BlockId,
        body: &str,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.set_body(id, body)
    }

    /// Set or overwrite a `:KEY: value` property through the kernel
    /// command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn set_property(
        &mut self,
        id: &closure_core::BlockId,
        key: &str,
        value: &str,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.set_property(id, key, value)
    }

    /// Set or clear the TODO keyword through the kernel command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn set_todo(
        &mut self,
        id: &closure_core::BlockId,
        keyword: Option<&str>,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.set_todo(id, keyword)
    }

    /// Set or clear the priority cookie through the kernel command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn set_priority(
        &mut self,
        id: &closure_core::BlockId,
        priority: Option<char>,
    ) -> Result<(), closure_store::VaultError> {
        self.vault.set_priority(id, priority)
    }

    /// Replace the tag list through the kernel command (I8).
    ///
    /// # Errors
    ///
    /// Propagates [`closure_store::VaultError`].
    pub fn set_tags(
        &mut self,
        id: &closure_core::BlockId,
        tags: &[String],
    ) -> Result<(), closure_store::VaultError> {
        self.vault.set_tags(id, tags)
    }
}

/// Adapter for gpui embedder (parity with egui).
pub trait ShellAdapter {
    /// Render one frame from `shell` state.
    fn frame(&mut self, shell: &Shell);
    /// Feed one chord stroke into `shell`.
    fn input(&mut self, shell: &mut Shell, chord: &str);
}

/// Headless for tests (I7, no real GPU/window needed for invariants).
#[derive(Debug, Default)]
pub struct HeadlessAdapter {
    /// Number of frames rendered.
    pub frames: u64,
    /// Last chord fed in.
    pub last_chord: Option<String>,
}

impl ShellAdapter for HeadlessAdapter {
    fn frame(&mut self, _shell: &Shell) {
        self.frames += 1;
    }
    fn input(&mut self, _shell: &mut Shell, chord: &str) {
        self.last_chord = Some(chord.to_owned());
    }
}

/// One rendered row in the gpui browse/search list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Stable block id (`:ID:`, I2) — the edit target.
    pub id: String,
    /// File the headline lives in (display path).
    pub path: String,
    /// Headline title.
    pub title: String,
    /// Outline level (1-based).
    pub level: u8,
    /// TODO keyword, if any.
    pub todo: Option<String>,
    /// Byte ranges of [`Self::title`] the outline filter matched, for
    /// the shell to paint.
    ///
    /// The palette has told you *why* a row is in its list since it
    /// became a picker; the outline scored its rows with the same fuzzy
    /// matcher and threw the positions away, so filtering the tree gave
    /// you a shorter list and no reason for it. Empty when nothing is
    /// being filtered — the whole document is not a list of candidates.
    pub matches: Vec<(usize, usize)>,
    /// org's priority cookie letter (`[#A]` → `'A'`), if any.
    ///
    /// The one piece of a headline that says *do this first*, and the
    /// outline is where you decide what to do first — so an urgent task
    /// and an unprioritised one were the same row until the parser's
    /// answer was carried here instead of dropped.
    pub priority: Option<char>,
    /// Whether this headline's subtree is folded (`:VISIBILITY: folded`).
    ///
    /// Carried on the row because the outline needs it for every
    /// visible row on every frame, and the fold walk in `derive_rows`
    /// has already computed it — asking the vault again per row per
    /// frame is the same answer at wheel speed.
    pub folded: bool,
    /// Whether the headline has a subtree at all — read from the
    /// document, not from the rows, so a folded parent (whose children
    /// are not in the list) still says yes.
    ///
    /// A fold arrow on a row with nothing under it is an affordance
    /// that does nothing when clicked.
    pub has_children: bool,
}

/// org's own spelling of a priority letter: `'A'` → `"[#A]"`.
///
/// One place, because it is what you would type to make one and what
/// every surface should show — a cookie spelled two ways is two
/// cookies to the reader.
#[must_use]
pub fn priority_cookie(letter: char) -> String {
    format!("[#{letter}]")
}

/// How urgent a priority letter is, higher being louder.
///
/// `A` outranks `B` outranks `C`, and org's letters run past `C` for
/// anyone who configures more of them, so the rank is the distance from
/// the end of the alphabet rather than a table of three. Not an
/// uppercase letter, not a priority: rank zero.
///
/// It lives here rather than in a shell so that every shell agrees
/// about which of two tasks is the urgent one.
#[must_use]
pub const fn priority_rank(letter: char) -> u8 {
    if letter.is_ascii_uppercase() {
        b'Z' + 1 - (letter as u8)
    } else {
        0
    }
}

/// One agenda row for the GUI agenda pane, flags precomputed
/// against an injected today so shells stay hermetic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgendaRow {
    /// Date in YYYY-MM-DD form.
    pub date: String,
    /// SCHEDULED or DEADLINE.
    pub kind: String,
    /// Headline title.
    pub title: String,
    /// True when date equals the injected today.
    pub is_today: bool,
    /// True when date lies before the injected today.
    pub is_overdue: bool,
}

/// The per-line git marks of one file, and what they were read for:
/// the vault revision and the path. Both have to match for a cached
/// answer to still be the right answer.
type FringeMemo = (
    u64,
    std::path::PathBuf,
    Vec<(usize, closure_store::LineChange)>,
);

/// Full preview of the selected headline for the detail pane.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Detail {
    /// Headline title.
    pub title: String,
    /// TODO keyword, if any.
    pub todo: Option<String>,
    /// Priority letter, if any.
    pub priority: Option<char>,
    /// Tags, in order.
    pub tags: Vec<String>,
    /// `SCHEDULED:` timestamp, if any.
    pub scheduled: Option<String>,
    /// `DEADLINE:` timestamp, if any.
    pub deadline: Option<String>,
    /// `:KEY: value` property pairs.
    pub properties: Vec<(String, String)>,
    /// Body text below the headline.
    pub body: String,
    /// Everything *under* the headline: its children, at every depth,
    /// as they read in the file.
    ///
    /// A headline whose content is its children — which is most of them
    /// in an outline — used to preview as blank, so you had to open the
    /// editor to find out whether there was anything there. Kept apart
    /// from [`Self::body`] so a shell can style the two differently and
    /// so every existing reader of the body keeps meaning what it
    /// meant.
    ///
    /// Property drawers are stripped: unlike the editor's buffer this
    /// is never written back, so the four lines per child that carry
    /// its id are pure noise here.
    pub children: String,
    /// File the headline lives in (display path).
    pub path: String,
    /// Stable block id (`:ID:`), so a header can show it as its own
    /// field instead of as one line of a grey property drawer.
    pub id: String,
    /// Outline level (1-based) — "indentation level" in the report.
    pub level: u8,
    /// Non-empty lines of body, children included.
    pub lines: usize,
    /// Words of body, children included.
    pub words: usize,
    /// The day this note was created, `YYYY-MM-DD`, from its own id.
    ///
    /// Free: a closure id *is* a ULID and a ULID's first ten characters
    /// are the millisecond it was minted, so this needs no new property
    /// in anybody's file. `None` for an id that is not one.
    pub created: Option<String>,
    /// When the file was last written, `YYYY-MM-DD`.
    pub modified: Option<String>,
}

impl Detail {
    /// Build the detail for `h` in `path`, children included.
    ///
    /// One constructor, because there were two: the legacy pane and
    /// the modal one each assembled a `Detail` by hand, so a field
    /// added for one of them was a field the other did not have.
    #[must_use]
    pub fn of(h: &closure_core::DocHeadline, path: &std::path::Path, children: String) -> Self {
        let body = closure_org::unescape_body(h.body_text());
        // Counted from what the pane actually shows — the body and
        // everything under it — rather than from the body alone, which
        // would say "0 lines" for a headline whose content is its
        // children, and most of them are.
        let counted = format!("{body}\n{children}");
        let id = h.id().to_string();
        Self {
            title: h.title().to_owned(),
            todo: h.todo().map(ToOwned::to_owned),
            priority: h.priority(),
            tags: h.tags().to_vec(),
            scheduled: h.scheduled().map(ToOwned::to_owned),
            deadline: h.deadline().map(ToOwned::to_owned),
            properties: h.properties().to_vec(),
            lines: counted.lines().filter(|l| !l.trim().is_empty()).count(),
            words: counted.split_whitespace().count(),
            level: h.level(),
            created: ulid_date(&id),
            modified: std::fs::metadata(path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| civil_date(d.as_secs())),
            id,
            body,
            children,
            path: path.display().to_string(),
        }
    }
}

/// What a prompt is for, as a colour role a shell can resolve.
///
/// A powerline of one colour is a stripe: the tone is what makes a
/// rename read differently from a search and both differently from
/// the `:` line, which can run anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PromptTone {
    /// Narrowing something that is already on screen.
    Filter,
    /// Writing a name or a value into the document.
    Edit,
    /// Choosing where something goes.
    Target,
    /// The `:` line — it runs commands, so it is the loud one.
    Command,
}

/// The words around a prompt's field: what it is, and what it will do.
///
/// It was a match inside the gpui painter, so the terminal and the web
/// shell had no way to show the same prompt. The arrows and the font
/// weights stay in the shell that can draw them; the words are the
/// same everywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptChrome {
    /// What this prompt is — "search", "rename", "new sibling TODO".
    ///
    /// No trailing punctuation: in a powerline the separator is the
    /// arrow, and a `:` inside a coloured block reads as a typo.
    pub label: String,
    /// What it will do, or what it has found. Empty when there is
    /// nothing useful to say.
    pub hint: String,
    /// Which colour role the label's segment takes.
    pub tone: PromptTone,
    /// A glyph for the segment, in the shell's icon font.
    ///
    /// "very tiny search icon": the old one was a magnifier inside the
    /// label string, so it was drawn at the label's size and weight.
    /// Its own field is what lets a shell paint it at the size an icon
    /// needs.
    pub icon: &'static str,
    /// The TODO keyword this prompt will apply, if it applies one.
    ///
    /// "new sibling TODO … should color the word TODO just in the same
    /// color as TODO is in the headline tree view". It used to be three
    /// characters inside [`Self::label`], and a shell cannot colour
    /// part of a string it is handed whole — exactly why the icon
    /// needed a field of its own, one report earlier.
    ///
    /// The vault's first configured keyword rather than the literal
    /// `TODO`, so a vault that declares `NEXT` says NEXT and paints it
    /// in NEXT's colour.
    pub keyword: Option<String>,
}

/// One cell of the which-key panel: a group heading, or a binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhichKeyCell {
    /// The name of the group whose bindings follow.
    Heading(String),
    /// A chord and the command it runs.
    Entry {
        /// The keys to press.
        chord: String,
        /// What they do.
        command: String,
    },
}

/// Lay the which-key groups out as newspaper columns, each at most
/// `height` cells tall.
///
/// A column per group is what made the panel scroll: six groups, six
/// columns, and "Command" holds more bindings than the other five put
/// together — so its column ran off the bottom of the window while the
/// five beside it stood half empty. Doom flows one flat list into
/// balanced columns, which is why nothing there scrolls.
///
/// The group headings survive, because they are what makes the panel
/// skimmable and Doom's has none. That costs one rule: a heading may
/// not be the last cell of a column, since a heading whose group is in
/// the next column is worse than no heading.
#[must_use]
pub fn which_key_columns(
    groups: &[(String, Vec<(String, String)>)],
    height: usize,
) -> Vec<Vec<WhichKeyCell>> {
    let mut cols: Vec<Vec<WhichKeyCell>> = Vec::new();
    let mut col: Vec<WhichKeyCell> = Vec::new();
    // A panel too short for a heading and one binding under it is a
    // panel, not a hang: nothing can be laid out, so nothing is.
    if height < 2 {
        return Vec::new();
    }
    for (name, entries) in groups {
        if entries.is_empty() {
            continue;
        }
        // A heading needs at least one of its own entries beside it.
        if col.len() + 2 > height {
            cols.push(std::mem::take(&mut col));
        }
        col.push(WhichKeyCell::Heading(name.clone()));
        for (chord, command) in entries {
            if col.len() == height {
                cols.push(std::mem::take(&mut col));
            }
            col.push(WhichKeyCell::Entry {
                chord: chord.clone(),
                command: command.clone(),
            });
        }
    }
    if !col.is_empty() {
        cols.push(col);
    }
    cols
}

/// The marker that opens the next item of the list `line` belongs to,
/// or `None` when there is no list to continue.
///
/// org's rules, which Doom inherits. The indentation comes with it, so
/// a nested list survives being typed; a counter counts up; a checkbox
/// starts unticked, because a new item is not done whatever the one
/// above it says.
///
/// `None` for an *empty* item, which is what ends a list: without that
/// every list finishes with a stray bullet to go back and delete.
#[must_use]
pub fn list_continuation(line: &str) -> Option<String> {
    let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    let rest = &line[indent.len()..];
    let (marker, after) = split_list_marker(rest)?;
    // What follows the marker, checkbox included: an item with nothing
    // in it is the end of the list.
    let content = after
        .strip_prefix("[ ] ")
        .or_else(|| after.strip_prefix("[X] "))
        .or_else(|| after.strip_prefix("[x] "))
        .or_else(|| after.strip_prefix("[-] "))
        .unwrap_or(after);
    if content.trim().is_empty() {
        return None;
    }
    let box_part = if after.starts_with("[ ] ")
        || after.starts_with("[X] ")
        || after.starts_with("[x] ")
        || after.starts_with("[-] ")
    {
        "[ ] "
    } else {
        ""
    };
    // A counter counts; a bullet repeats.
    let digits: String = marker.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return Some(format!("{indent}{marker}{box_part}"));
    }
    let sep = marker.chars().nth(digits.len()).unwrap_or('.');
    let n: usize = digits.parse().ok()?;
    Some(format!("{indent}{}{sep} {box_part}", n + 1))
}

/// Split `rest` into its list marker (with the trailing space) and
/// what follows, or `None` when it does not open a list item.
fn split_list_marker(rest: &str) -> Option<(String, &str)> {
    let mut chars = rest.chars();
    let first = chars.next()?;
    if matches!(first, '-' | '+') && chars.next() == Some(' ') {
        return Some((rest[..2].to_owned(), &rest[2..]));
    }
    if first.is_ascii_digit() {
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let after = &rest[digits..];
        if (after.starts_with('.') || after.starts_with(')')) && after[1..].starts_with(' ') {
            return Some((rest[..digits + 2].to_owned(), &rest[digits + 2..]));
        }
    }
    None
}

/// Renumber the ordered list containing `line`, in place.
///
/// Counting is per depth: a nested list is a different list, so it
/// starts at one and the outer one does not skip because of it. The
/// separator the list already uses is kept — `1)` and `1.` are both
/// org, and a list does not change style halfway down because
/// something was inserted into it.
#[must_use]
pub fn renumber_list(text: &str, line: usize) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let Some(start) = lines.get(line) else {
        return text.to_owned();
    };
    let indent_of = |l: &str| l.len() - l.trim_start().len();
    let depth = indent_of(start);
    if split_list_marker(start.trim_start())
        .is_none_or(|(m, _)| !m.starts_with(|c: char| c.is_ascii_digit()))
    {
        return text.to_owned();
    }
    // The run of items at this depth around `line`, and every nested
    // list inside it, counted separately.
    let mut counters: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut inside = false;
    for (i, raw) in lines.iter().enumerate() {
        let ind = indent_of(raw);
        let marker = split_list_marker(raw.trim_start());
        let numbered = marker
            .as_ref()
            .is_some_and(|(m, _)| m.starts_with(|c: char| c.is_ascii_digit()));
        if !numbered && !raw.trim().is_empty() && ind <= depth {
            // Out of the list: prose at or above its depth ends it.
            if inside && i > line {
                out.extend(lines[i..].iter().map(|l| (*l).to_owned()));
                return out.join("\n");
            }
            counters.clear();
            inside = false;
            out.push((*raw).to_owned());
            continue;
        }
        if !numbered {
            out.push((*raw).to_owned());
            continue;
        }
        inside = true;
        let Some((m, after)) = marker else {
            out.push((*raw).to_owned());
            continue;
        };
        let digits = m.len() - 2;
        let sep = m.chars().nth(digits).unwrap_or('.');
        // A deeper list restarts once its parent moves on, so the
        // counters below this depth are dropped before this one steps.
        counters.retain(|k, _| *k <= ind);
        let n = counters.entry(ind).or_insert(0);
        *n += 1;
        let n = *n;
        out.push(format!("{}{n}{sep} {after}", &raw[..ind]));
    }
    out.join("\n")
}

/// Does this directory hold a vault?
///
/// A vault is a directory of org files, usually a tree of them.
/// Pointing closure at one that has none is a mistake worth naming
/// rather than an empty window with no explanation in it.
#[must_use]
pub fn looks_like_vault(dir: &std::path::Path) -> bool {
    fn any_org(dir: &std::path::Path, depth: usize) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("org"))
            {
                return true;
            }
            // A few levels is enough to recognise a vault; walking a
            // whole home directory to answer a dialog is not.
            if depth > 0 && path.is_dir() && any_org(&path, depth - 1) {
                return true;
            }
        }
        false
    }
    dir.is_dir() && any_org(dir, 3)
}

/// Every palette entry as `(label, canonical, section, description)`.
#[must_use]
pub fn palette_entries_raw() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    PALETTE_COMMANDS.to_vec()
}

/// Every palette entry as `(label, description)`.
#[must_use]
pub fn palette_descriptions() -> Vec<(&'static str, &'static str)> {
    PALETTE_COMMANDS
        .iter()
        .map(|(label, _, _, desc)| (*label, *desc))
        .collect()
}

/// Every palette entry as `(label, section)`.
#[must_use]
pub fn palette_sections_of() -> Vec<(&'static str, &'static str)> {
    PALETTE_COMMANDS
        .iter()
        .map(|(label, _, section, _)| (*label, *section))
        .collect()
}

/// The palette's section names, in order.
#[must_use]
pub fn palette_section_names() -> Vec<&'static str> {
    PALETTE_SECTIONS.to_vec()
}

/// Every command the palette knows, by canonical name.
///
/// The registry as a list, so a property can be asserted over *all* of
/// them rather than over the three that happened to be reported —
/// "Do I have to experience this for every new command?"
#[must_use]
pub fn palette_command_names() -> Vec<&'static str> {
    PALETTE_COMMANDS.iter().map(|(_, c, ..)| *c).collect()
}

/// When a ULID was minted, `YYYY-MM-DD HH:MM:SS` UTC, or `None` if `id` is
/// not one.
///
/// The first ten characters are the millisecond since the epoch in
/// Crockford base32, most significant first — which is the whole
/// reason ULIDs sort by time, and the reason this costs nothing.
#[must_use]
pub fn ulid_date(id: &str) -> Option<String> {
    if id.len() != 26 {
        return None;
    }
    let mut ms: u64 = 0;
    for c in id.chars().take(10) {
        let v = crockford_value(c)?;
        ms = ms.checked_mul(32)?.checked_add(u64::from(v))?;
    }
    Some(civil_date(ms / 1000))
}

/// One Crockford base32 digit, or `None` for a character that is not
/// one. `I`, `L`, `O` and `U` are the ambiguous ones it excludes.
fn crockford_value(c: char) -> Option<u8> {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let up = c.to_ascii_uppercase() as u8;
    ALPHABET
        .iter()
        .position(|&a| a == up)
        .and_then(|i| u8::try_from(i).ok())
}

/// `YYYY-MM-DD HH:MM:SS` for a count of seconds since the Unix epoch.
///
/// The calendar is [`civil_from_days`] — Howard Hinnant's, so a date
/// does not pull in a calendar crate. This divides the days away and
/// keeps the remainder.
///
/// The time of day is here rather than at the call sites because both
/// callers wanted it and neither could add it: the days are what this
/// divides away. "The panel currently just show the date in ISO8601
/// format. Please add the time to both."
fn civil_date(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX);
    let (y, m, d) = civil_from_days(days);
    let rest = secs % 86_400;
    let (hh, mm, ss) = (rest / 3600, (rest % 3600) / 60, rest % 60);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}")
}

/// Seconds since the epoch as `(minute, hour, day, month, weekday)`,
/// UTC.
///
/// The same calendar [`civil_date`] reads, [`civil_from_days`]; only
/// the weekday is not in it. That is `(days + 4) % 7`: 1970-01-01 was
/// a Thursday, and cron counts Sunday as 0.
fn civil_parts(secs: u64) -> (u8, u8, u8, u8, u8) {
    let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX);
    let rest = secs % 86_400;
    let (_y, m, d) = civil_from_days(days);
    let dow = (days + 4).rem_euclid(7);
    (
        u8::try_from(rest % 3600 / 60).unwrap_or(0),
        u8::try_from(rest / 3600).unwrap_or(0),
        u8::try_from(d).unwrap_or(1),
        u8::try_from(m).unwrap_or(1),
        u8::try_from(dow).unwrap_or(0),
    )
}

/// Whether the row at `i` is the first one its file contributes to the
/// outline.
///
/// The outline is every `*.org` under the vault in one flat list, and
/// "it's quite hard to see where a file ends or starts, due to the flat
/// hierachy". A rule above the row that starts a file is the one place
/// a divider can go without a row of its own — the list is a uniform
/// list whose indices *are* the selection, so an extra row would shift
/// every chord that counts.
///
/// Colouring each file differently was the other suggestion, and the
/// one to refuse: colour already carries outline depth, and a second
/// meaning on the same channel leaves neither readable.
#[must_use]
pub fn starts_file(rows: &[Row], i: usize) -> bool {
    let Some(row) = rows.get(i) else {
        return false;
    };
    i == 0 || rows[i - 1].path != row.path
}

/// A keybinding-bearing action attached to an actionable view node.
///
/// Constructing one *requires* a chord, so an actionable node can never
/// lack its keybinding — the vision's "every UI element shows its
/// keybinding" rule made type-level (V1). The only constructor,
/// [`Action::new`], returns `None` when the active mode binds no chord to
/// the command, so a command-without-chord cannot be represented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    command: String,
    chord: String,
    /// Every chord that runs it, primary first. A command reachable two
    /// ways whose second way is never shown is a command reachable one
    /// way, so the affordance carries the whole list and the painter
    /// decides how much room it has.
    chords: Vec<String>,
}

impl Action {
    /// An action for `command` in `mode`. `None` when no chord is bound
    /// (the source of truth is [`closure_input::chord_for_command`], I4).
    #[must_use]
    pub fn new(mode: closure_config::InputMode, command: impl Into<String>) -> Option<Self> {
        let command = command.into();
        Self::in_keymap(&closure_input::keymap_with(mode, &[]), command)
    }

    /// The same, against an explicit keymap — the mode's plus whatever
    /// `config.org` said about it.
    ///
    /// The palette went through [`Self::new`] and so showed the chord
    /// the *table* carries, which after a rebind is the one key that no
    /// longer works.
    #[must_use]
    pub fn in_keymap(keys: &[(String, String)], command: impl Into<String>) -> Option<Self> {
        let command = command.into();
        let chords: Vec<String> = keys
            .iter()
            .filter(|(_, cmd)| *cmd == command)
            .map(|(chord, _)| chord.clone())
            .collect();
        chords.first().map(|chord| Self {
            chord: chord.clone(),
            chords: chords.clone(),
            command,
        })
    }

    /// Every chord that runs it, primary first. Empty when this mode
    /// binds none.
    #[must_use]
    pub fn chords(&self) -> &[String] {
        &self.chords
    }

    /// An action for a command this mode has no chord for.
    ///
    /// The original contract was that an `Action` always carries a
    /// chord, "so a command-without-chord cannot be represented". That
    /// is backwards for the palette, which is precisely where you go
    /// for a command you have no chord for — and it was silently
    /// dropping every such command from the list. Rewritten 2026-08-02
    /// after "command palette issues": `sync-export` and its
    /// neighbours were unreachable by name in the one surface built
    /// for reaching things by name.
    #[must_use]
    pub fn unbound(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            chord: String::new(),
            chords: Vec::new(),
        }
    }

    /// The canonical command name.
    #[must_use]
    pub fn command(&self) -> &str {
        &self.command
    }

    /// The chord bound to it (non-empty by construction).
    #[must_use]
    pub fn chord(&self) -> &str {
        &self.chord
    }
}

/// A headline row in a [`Node::Rows`] list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    /// Stable block id (I2).
    pub id: String,
    /// Headline title.
    pub title: String,
    /// Outline level (1-based).
    pub level: u8,
    /// TODO keyword, if any.
    pub todo: Option<String>,
    /// Leading status glyph (icon-as-data, G5a) — e.g. a todo-state
    /// marker. The embedder maps it to a real icon.
    pub icon: Option<String>,
    /// Metadata chips (G5a) — tags, a priority letter, … shown after the
    /// title as Notion-style badges.
    pub badges: Vec<String>,
}

impl RowView {
    /// A row with no icon and no badges (the common case).
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        level: u8,
        todo: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            level,
            todo,
            icon: None,
            badges: Vec::new(),
        }
    }

    /// Set the leading icon glyph (G5a).
    #[must_use]
    pub fn with_icon(mut self, icon: Option<String>) -> Self {
        self.icon = icon;
        self
    }

    /// Set the metadata badge chips (G5a).
    #[must_use]
    pub fn with_badges(mut self, badges: Vec<String>) -> Self {
        self.badges = badges;
        self
    }
}

/// A labelled field in a [`Node::Detail`] pane; `action` present iff the
/// field is actionable (and then it carries its chord, V1 invariant).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldView {
    /// Field label (`title`, `todo`, `tags`, a property key, …).
    pub label: String,
    /// Field value, rendered as text.
    pub value: String,
    /// The action triggered by activating the field, if any.
    pub action: Option<Action>,
}

/// A command row in a [`Node::Palette`]; always actionable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItemView {
    /// Display label.
    pub label: String,
    /// The command + its chord.
    pub action: Action,
}

/// A node of the declarative view tree (V1).
///
/// A pure description of a screen that any embedder renders: the engine
/// emits the tree, the shell draws it (the Flutter engine/embedder
/// split). Deterministic (I6) — `view` is a pure function of state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A labelled region containing child nodes.
    Pane {
        /// Region label.
        title: String,
        /// Child nodes, in render order.
        children: Vec<Self>,
    },
    /// The headline list with the selected index.
    Rows {
        /// Visible rows.
        rows: Vec<RowView>,
        /// Index of the selected row.
        selected: usize,
    },
    /// The detail pane: a list of (maybe-actionable) fields.
    Detail {
        /// Fields, in display order.
        fields: Vec<FieldView>,
    },
    /// A text-entry surface (capture / rename / body / tags / property).
    Input {
        /// What is being edited.
        label: String,
        /// Current buffer contents.
        buffer: String,
    },
    /// The command palette (which-key list).
    Palette {
        /// Offered commands.
        items: Vec<PaletteItemView>,
        /// Index of the highlighted item.
        cursor: usize,
    },
    /// The always-on which-key hint line.
    Hints {
        /// Rendered hint line.
        line: String,
    },
    /// An expanded composite widget (V2c): its name + the already-expanded
    /// content (a `closure-widget` block resolved via `closure-query`).
    Widget {
        /// Widget name.
        name: String,
        /// Expanded content.
        content: String,
    },
    /// Inert text.
    Text(String),
    /// A multi-pane split layout (G1a): child panes arranged along an
    /// axis. The foundation for a real editor surface (sidebar + main +
    /// detail). Renderers lay the panes out along [`SplitDir`]; the
    /// hermetic guarantee is the pane *set + order + axis*, not pixels.
    Split {
        /// Axis the panes are arranged along.
        direction: SplitDir,
        /// Child panes, in render order.
        panes: Vec<Self>,
    },
    /// A modal overlay (G1b): a titled layer floating above the base
    /// surface — the command palette, a confirm dialog, a prompt. The
    /// embedder paints it as a focused dialog; the hermetic guarantee is
    /// the title + body content, not the dimming/animation.
    Modal {
        /// Dialog heading (also the accessible label).
        title: String,
        /// The content shown inside the overlay.
        body: Box<Self>,
    },
    /// A transient notification (G1c): the severity-classed toast a shell
    /// flashes for an async outcome (sync done, eval failed, …). The
    /// embedder decides the placement/auto-dismiss; the hermetic guarantee
    /// is the severity + message.
    Toast {
        /// Severity, driving styling + the a11y live-region politeness.
        level: ToastLevel,
        /// The message shown.
        text: String,
    },
}

/// Severity of a [`Node::Toast`] notification (G1c).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToastLevel {
    /// Neutral information.
    Info,
    /// A completed action.
    Success,
    /// A non-fatal caution.
    Warning,
    /// A failure.
    Error,
}

impl ToastLevel {
    /// Stable lowercase tag for serialisation / CSS class suffixes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    /// The ARIA live-region role: assertive `alert` for warning/error,
    /// polite `status` for info/success.
    #[must_use]
    pub const fn aria_role(self) -> &'static str {
        match self {
            Self::Warning | Self::Error => "alert",
            Self::Info | Self::Success => "status",
        }
    }
}

/// The axis a [`Node::Split`] arranges its panes along (G1a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SplitDir {
    /// Panes side by side, left to right.
    Row,
    /// Panes stacked, top to bottom.
    Column,
}

impl SplitDir {
    /// Stable lowercase tag for serialisation / CSS class suffixes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Row => "row",
            Self::Column => "column",
        }
    }
}

/// Build a [`Node::Split`] from an axis and its panes (G1a).
#[must_use]
pub const fn split_node(direction: SplitDir, panes: Vec<Node>) -> Node {
    Node::Split { direction, panes }
}

/// Build a [`Node::Modal`] overlay from a title and its body (G1b).
#[must_use]
pub fn modal_node(title: impl Into<String>, body: Node) -> Node {
    Node::Modal {
        title: title.into(),
        body: Box::new(body),
    }
}

/// Build a [`Node::Toast`] notification from a severity and message (G1c).
#[must_use]
pub fn toast_node(level: ToastLevel, text: impl Into<String>) -> Node {
    Node::Toast {
        level,
        text: text.into(),
    }
}

/// An `#rrggbb` colour token (G2). Holds a static hex string; the shells
/// map it to a native colour (CSS value, ratatui `Color::Rgb`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub &'static str);

impl Color {
    /// The `#rrggbb` hex string.
    #[must_use]
    pub const fn hex(self) -> &'static str {
        self.0
    }

    /// Parse the hex to an `(r, g, b)` byte triple. Malformed input
    /// resolves to black — never a panic (I5), so a bad theme token can't
    /// crash a render.
    #[must_use]
    pub fn rgb(self) -> (u8, u8, u8) {
        let h = self.0.strip_prefix('#').unwrap_or(self.0);
        if h.len() != 6 || !h.is_ascii() {
            return (0, 0, 0);
        }
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
        (byte(0), byte(2), byte(4))
    }
}

/// A semantic colour slot in a [`Theme`] palette (G2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorRole {
    /// Foreground / primary text.
    Fg,
    /// Background / surface.
    Bg,
    /// Accent / interactive highlight.
    Accent,
    /// De-emphasised / secondary text.
    Muted,
    /// Selection / active-row highlight.
    Selection,
    /// Error severity.
    Error,
    /// Warning severity.
    Warning,
    /// Success severity.
    Success,
    /// Second-level heading (doom-vibrant outline-2 magenta).
    Heading2,
    /// Third-level heading (doom-vibrant outline-3 violet).
    Heading3,
    /// Fourth-level heading. doom-themes derives outline-4 by
    /// lightening blue, which is what keeps the hue order readable once
    /// the cycle repeats.
    Heading4,
    /// Fifth-level heading (lightened magenta, doom's outline-5).
    Heading5,
    /// Inline/source code (doom-vibrant org-code orange).
    Code,
}

/// The named colour slots of a theme (G2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Foreground / primary text.
    pub fg: Color,
    /// Background / surface.
    pub bg: Color,
    /// Accent / interactive highlight.
    pub accent: Color,
    /// De-emphasised / secondary text.
    pub muted: Color,
    /// Selection / active-row highlight.
    pub selection: Color,
    /// Error severity.
    pub error: Color,
    /// Warning severity.
    pub warning: Color,
    /// Success severity.
    pub success: Color,
    /// Second-level heading colour.
    pub heading2: Color,
    /// Third-level heading colour.
    pub heading3: Color,
    /// Fourth-level heading colour.
    pub heading4: Color,
    /// Fifth-level heading colour.
    pub heading5: Color,
    /// Inline/source code colour.
    pub code: Color,
}

/// The spacing scale of a theme, in pixels (G2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spacing {
    /// Base spacing unit.
    pub unit_px: u16,
    /// Inter-element gap.
    pub gap_px: u16,
}

/// The typography of a theme (G2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Typography {
    /// Body font stack.
    pub font_family: &'static str,
    /// Monospace font stack (code blocks).
    pub mono_family: &'static str,
    /// Base font size, in pixels.
    pub base_px: u16,
}

/// A step on the type scale — what a shell asks for instead of a
/// number.
///
/// There was no scale: eighty-five literal sizes through the painter,
/// `10.0` here and `11.0` next to it, so "the chrome is too small" had
/// eighty-five places to be fixed and no way to stay fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeStep {
    /// Prose, and everything that is the content rather than about it.
    Body,
    /// Chrome the eye uses constantly: the rail, the header, the
    /// footer. Within a pixel of the body, because it is read as often.
    Ui,
    /// Annotations beside content — a file name, a chord chip.
    Small,
    /// Counters and badges. Small, never illegible.
    Tiny,
}

impl Typography {
    /// The pixel size of `step`, from the one size a theme declares.
    ///
    /// Derived rather than declared per step so that a theme raising
    /// `base_px` — as high-contrast does — moves the whole scale with
    /// it, instead of leaving fixed sizes underneath a larger body.
    #[must_use]
    pub const fn step_px(self, step: TypeStep) -> u16 {
        match step {
            TypeStep::Body => self.base_px,
            TypeStep::Ui => self.base_px.saturating_sub(1),
            TypeStep::Small => self.base_px.saturating_sub(2),
            TypeStep::Tiny => self.base_px.saturating_sub(3),
        }
    }
}

/// Split a CSS-shaped font stack into family names, best first.
///
/// The stacks are spelled the way CSS spells them because the web tier
/// drops them straight into a `font-family` rule. Every native toolkit
/// wants one family plus an ordered fallback list, and the gpui shell
/// used to hand the *whole string* to gpui's `font_family()` — asking
/// the platform for a font literally called
/// `"JetBrains Mono, ui-monospace, monospace"`, getting nothing, and
/// falling back to whatever the platform chose. Empty names are dropped
/// so a ragged stack cannot ask for a font with no name.
#[must_use]
pub fn font_stack(stack: &str) -> Vec<&str> {
    stack
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect()
}

/// A declarative, typed theme: palette + spacing + typography as data
/// (G2). Resolved from the free-form `config.theme` string via
/// [`Theme::from_name`]; each shell maps the tokens to its native style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Stable lowercase theme name (`dark` / `light` / `high-contrast`).
    pub name: &'static str,
    /// Colour palette.
    pub palette: Palette,
    /// Spacing scale.
    pub spacing: Spacing,
    /// Typography.
    pub typography: Typography,
}

impl Theme {
    /// The default dark theme.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            name: "dark",
            palette: Palette {
                fg: Color("#cdd6f4"),
                bg: Color("#1e1e2e"),
                accent: Color("#89b4fa"),
                muted: Color("#6c7086"),
                selection: Color("#45475a"),
                error: Color("#f38ba8"),
                warning: Color("#f9e2af"),
                success: Color("#a6e3a1"),
                heading2: Color("#cba6f7"),
                heading3: Color("#b4befe"),
                heading4: Color("#a6c8fa"),
                heading5: Color("#ddc4fb"),
                code: Color("#fab387"),
            },
            spacing: Spacing {
                unit_px: 8,
                gap_px: 4,
            },
            typography: Typography {
                font_family: "Maple Mono NF, JetBrains Mono, ui-monospace, monospace",
                mono_family: "Maple Mono NF, JetBrains Mono, ui-monospace, monospace",
                base_px: 14,
            },
        }
    }

    /// The light theme.
    #[must_use]
    pub const fn light() -> Self {
        Self {
            name: "light",
            palette: Palette {
                fg: Color("#4c4f69"),
                bg: Color("#eff1f5"),
                accent: Color("#1e66f5"),
                muted: Color("#9ca0b0"),
                selection: Color("#ccd0da"),
                error: Color("#d20f39"),
                warning: Color("#df8e1d"),
                success: Color("#40a02b"),
                heading2: Color("#8839ef"),
                heading3: Color("#7287fd"),
                heading4: Color("#3b7fd4"),
                heading5: Color("#a86ef4"),
                code: Color("#fe640b"),
            },
            spacing: Spacing {
                unit_px: 8,
                gap_px: 4,
            },
            typography: Typography {
                font_family: "Maple Mono NF, JetBrains Mono, ui-monospace, monospace",
                mono_family: "Maple Mono NF, JetBrains Mono, ui-monospace, monospace",
                base_px: 14,
            },
        }
    }

    /// The high-contrast theme: pure white on black, larger base size.
    #[must_use]
    pub const fn high_contrast() -> Self {
        Self {
            name: "high-contrast",
            palette: Palette {
                fg: Color("#ffffff"),
                bg: Color("#000000"),
                accent: Color("#ffff00"),
                muted: Color("#c0c0c0"),
                selection: Color("#0000ff"),
                error: Color("#ff0000"),
                warning: Color("#ffaa00"),
                success: Color("#00ff00"),
                heading2: Color("#ff00ff"),
                heading3: Color("#00ffff"),
                heading4: Color("#7fbfff"),
                heading5: Color("#ff7fff"),
                code: Color("#ffa500"),
            },
            spacing: Spacing {
                unit_px: 8,
                gap_px: 4,
            },
            typography: Typography {
                font_family: "Maple Mono NF, JetBrains Mono, ui-monospace, monospace",
                mono_family: "Maple Mono NF, JetBrains Mono, ui-monospace, monospace",
                base_px: 16,
            },
        }
    }

    /// The Doom Emacs `doom-vibrant` palette (the user's colorscheme;
    /// gui-color face values). Org face mapping: TODO green, DONE
    /// muted, date yellow, code orange, outline-1 blue / outline-2
    /// magenta / outline-3 violet.
    #[must_use]
    pub const fn doom_vibrant() -> Self {
        Self {
            name: "doom-vibrant",
            palette: Palette {
                fg: Color("#bbc2cf"),
                bg: Color("#242730"),
                accent: Color("#51afef"),
                muted: Color("#62686e"),
                selection: Color("#3d4451"),
                error: Color("#ff665c"),
                warning: Color("#fcce7b"),
                success: Color("#7bc275"),
                heading2: Color("#c57bdb"),
                heading3: Color("#a991f1"),
                // doom-themes' outline-4 and outline-5 are
                // `(doom-lighten blue 0.25)` and
                // `(doom-lighten magenta 0.25)` — the same hues coming
                // round again, lighter, so depth stays legible past
                // three. Blended toward white by a quarter.
                heading4: Color("#7cc3f3"),
                heading5: Color("#d39ce4"),
                code: Color("#e69055"),
            },
            spacing: Spacing {
                unit_px: 8,
                gap_px: 4,
            },
            typography: Typography {
                font_family: "Maple Mono NF, JetBrains Mono, ui-monospace, monospace",
                mono_family: "Maple Mono NF, JetBrains Mono, ui-monospace, monospace",
                base_px: 14,
            },
        }
    }

    /// Resolve a theme from the free-form `config.theme` string
    /// (case-insensitive): `light`, `high-contrast`/`hc`,
    /// `doom-vibrant`/`vibrant`, else `dark`.
    #[must_use]
    pub const fn from_name(name: &str) -> Self {
        if name.eq_ignore_ascii_case("light") {
            Self::light()
        } else if name.eq_ignore_ascii_case("high-contrast") || name.eq_ignore_ascii_case("hc") {
            Self::high_contrast()
        } else if name.eq_ignore_ascii_case("doom-vibrant") || name.eq_ignore_ascii_case("vibrant")
        {
            Self::doom_vibrant()
        } else {
            Self::dark()
        }
    }

    /// The colour for a semantic [`ColorRole`].
    #[must_use]
    pub const fn color(&self, role: ColorRole) -> Color {
        match role {
            ColorRole::Fg => self.palette.fg,
            ColorRole::Bg => self.palette.bg,
            ColorRole::Accent => self.palette.accent,
            ColorRole::Muted => self.palette.muted,
            ColorRole::Selection => self.palette.selection,
            ColorRole::Error => self.palette.error,
            ColorRole::Warning => self.palette.warning,
            ColorRole::Success => self.palette.success,
            ColorRole::Heading2 => self.palette.heading2,
            ColorRole::Heading3 => self.palette.heading3,
            ColorRole::Heading4 => self.palette.heading4,
            ColorRole::Heading5 => self.palette.heading5,
            ColorRole::Code => self.palette.code,
        }
    }
}

/// Build a [`Node::Widget`] from a name and its expanded content (V2c).
#[must_use]
pub fn widget_node(name: impl Into<String>, content: impl Into<String>) -> Node {
    Node::Widget {
        name: name.into(),
        content: content.into(),
    }
}

/// Marker shown on a folded row (`:VISIBILITY: folded`).
const FOLD_MARKER: &str = "▸";

/// Whether the headline is folded — the org-standard
/// `:VISIBILITY: folded` property (the same one Emacs org-mode reads),
/// so fold state lives in the document and persists between runs.
fn headline_is_folded(h: &closure_core::DocHeadline) -> bool {
    h.properties()
        .iter()
        .any(|(k, v)| k == "VISIBILITY" && v == "folded")
}

/// The outline, as rows: every headline in the vault, in document
/// order, minus the ones a fold is hiding.
///
/// A `:VISIBILITY: folded` headline hides its descendants — but only in
/// the listing. A live query searches into folds, the way org's isearch
/// does, so a non-empty `filter` walks past the fold rule entirely and
/// sorts by fuzzy score instead.
///
/// One walk, two callers ([`App::rows`] and [`ModalApp::derive_rows`]):
/// they were the same code twice, and a fold rule that differs between
/// the launcher and the window is a fold rule nobody can reason about.
fn outline_rows(shell: &Shell, filter: &str) -> Vec<Row> {
    let mut scored: Vec<(u32, Row)> = Vec::new();
    for (p, doc) in shell.vault.iter() {
        let headlines: Vec<_> = doc.all_headlines().collect();
        // One path per document, not one per headline: this ran
        // `Display` formatting machinery for every row in the vault to
        // rebuild a string that is the same for all of a file's
        // headlines. Cloning it is a memcpy.
        let path = p.display().to_string();
        let mut hide_below: Option<u8> = None;
        for (i, h) in headlines.iter().enumerate() {
            // The fold state is needed twice: to hide descendants here,
            // and by the outline to draw the arrow. Compute it once and
            // carry it on the row.
            let folded = headline_is_folded(h);
            if filter.is_empty() {
                if let Some(limit) = hide_below {
                    if h.level() > limit {
                        continue;
                    }
                    hide_below = None;
                }
                if folded {
                    hide_below = Some(h.level());
                }
            }
            let score = if filter.is_empty() {
                Some(0)
            } else {
                closure_query::fuzzy_score(filter, h.title())
            };
            if let Some(sc) = score {
                scored.push((
                    sc,
                    Row {
                        id: h.id().to_string(),
                        path: path.clone(),
                        title: h.title().to_owned(),
                        level: h.level(),
                        matches: if filter.is_empty() {
                            Vec::new()
                        } else {
                            closure_query::match_spans(filter, h.title())
                        },
                        todo: h.todo().map(ToOwned::to_owned),
                        priority: h.priority(),
                        folded,
                        // Document order is outline order, so the next
                        // headline is a child exactly when it is deeper.
                        // Read from the document rather than the rows:
                        // a folded parent's children are not in the list
                        // at all, and it must not lose its own arrow the
                        // moment the arrow is used.
                        has_children: headlines
                            .get(i + 1)
                            .is_some_and(|next| next.level() > h.level()),
                    },
                ));
            }
        }
    }
    if !filter.is_empty() {
        scored.sort_by_key(|(sc, _)| std::cmp::Reverse(*sc));
    }
    scored.into_iter().map(|(_, r)| r).collect()
}

// ---- Q3-V4: civil dates, without a clock or a calendar crate -------
//
// Everything here is pure arithmetic over `(year, month, day)`. The
// core never reads a clock — the shells inject today
// ([`ModalApp::set_today`]) — so every calendar in every test is
// reproducible, and a picker cannot drift between two frames of the
// same second.

/// Days since 1970-01-01 for a civil date (Howard Hinnant's
/// `days_from_civil`, public domain; the same algorithm
/// `closure_org`'s clock arithmetic uses).
#[must_use]
pub const fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date `days` after 1970-01-01 — the inverse of
/// [`days_from_civil`].
#[must_use]
pub const fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (y, m as u32, d as u32)
}

/// How many days a month has, leap years included.
#[must_use]
pub const fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        // April, June, September, November — and anything that is not
        // a month at all, which a caller cannot produce from a parsed
        // date but must not be a panic if one ever does (I5).
        _ => 30,
    }
}

/// Org's three-letter weekday for a civil date — what goes inside the
/// stamp, in the same spelling org itself writes.
#[must_use]
pub fn weekday_name(y: i64, m: u32, d: u32) -> &'static str {
    const NAMES: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    // 1970-01-01 was a Thursday, so the epoch day indexes NAMES.
    let days = days_from_civil(y, i64::from(m), i64::from(d));
    NAMES[usize::try_from(days.rem_euclid(7)).unwrap_or(0)]
}

/// Monday-based weekday index (0 = Monday) for a civil date — the
/// column a day sits in on a calendar that starts the week on Monday,
/// as org-mode's agenda does.
#[must_use]
pub fn weekday_index(y: i64, m: u32, d: u32) -> usize {
    let days = days_from_civil(y, i64::from(m), i64::from(d));
    // 1970-01-01 was a Thursday = column 3.
    usize::try_from((days + 3).rem_euclid(7)).unwrap_or(0)
}

/// Split `YYYY-MM-DD` into its parts, or `None` when it is not one.
///
/// Strict on purpose: this is what decides whether a *typed* date is
/// accepted, and a date that is nearly right is the one that silently
/// files a task in the wrong year.
#[must_use]
pub fn parse_ymd(text: &str) -> Option<(i64, u32, u32)> {
    let mut parts = text.trim().splitn(3, '-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) {
        return None;
    }
    Some((y, m, d))
}

/// The org stamp for a date and an optional repeater/warning tail:
/// `<2026-07-30 Thu>`, `<2026-07-30 Thu +1w>`.
#[must_use]
pub fn org_stamp(y: i64, m: u32, d: u32, tail: &str) -> String {
    let day = weekday_name(y, m, d);
    let tail = tail.trim();
    if tail.is_empty() {
        format!("<{y:04}-{m:02}-{d:02} {day}>")
    } else {
        format!("<{y:04}-{m:02}-{d:02} {day} {tail}>")
    }
}

/// Now, from the system clock, as `YYYY-MM-DD HH:MM` (UTC) — what a
/// shell without an injected clock stamps a `CLOCK:` entry with.
#[must_use]
pub fn now_local() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let (y, m, d) = civil_from_days(days);
    let minutes = (secs % 86_400) / 60;
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        minutes / 60,
        minutes % 60
    )
}

/// Today, from the system clock, as `YYYY-MM-DD` (UTC).
///
/// The core never calls this — [`ModalApp::set_today`] is how a shell
/// says what day it is, which is what keeps every calendar in every
/// test reproducible. It lives here so the shells that *do* have to ask
/// the clock all ask it the same way.
#[must_use]
pub fn today_local() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// A month as a picker draws it: seven columns, Monday first, blanks
/// where the month has not started or has ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateGrid {
    /// Year on show.
    pub year: i64,
    /// Month on show, 1-based.
    pub month: u32,
    /// Which planning field is being set (`SCHEDULED` / `DEADLINE`).
    pub field: String,
    /// The selected day as `YYYY-MM-DD`.
    pub selected: String,
    /// Weeks of seven days; `None` is a blank cell.
    pub weeks: Vec<Vec<Option<u32>>>,
    /// What the user has typed instead, if anything.
    pub typed: String,
}

/// Which planning field a date picker is filling (Q3-V4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanField {
    /// `SCHEDULED:` — when you will start.
    Scheduled,
    /// `DEADLINE:` — when it is due.
    Deadline,
}

impl PlanField {
    /// The org keyword this field writes.
    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Scheduled => "SCHEDULED",
            Self::Deadline => "DEADLINE",
        }
    }
}

/// The live date-picker session: which headline, which field, where
/// the cursor is, and whatever has been typed over it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DatePickSession {
    /// Headline being planned.
    id: String,
    /// Field being written.
    field: PlanField,
    /// Cursor date.
    date: (i64, u32, u32),
    /// Typed date, which wins over the cursor when it parses.
    typed: String,
}

// ---- Q3-V5: checkboxes and their cookies ---------------------------

/// Tick or untick a checkbox list line (`- [ ] x` ↔ `- [X] x`), or
/// `None` when the line has no checkbox on it.
///
/// Org writes `[X]`; `[x]` and the half-state `[-]` are read as ticked
/// too, because other tools write those and a file is not ours alone.
#[must_use]
pub fn toggle_checkbox_line(line: &str) -> Option<String> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    let bullet = ["- ", "+ ", "* "]
        .into_iter()
        .find(|b| rest.starts_with(b))?;
    let after_bullet = &rest[bullet.len()..];
    let box_at = line.len() - after_bullet.len();
    let checked = if after_bullet.starts_with("[ ]") {
        "[X]"
    } else if after_bullet.starts_with("[X]")
        || after_bullet.starts_with("[x]")
        || after_bullet.starts_with("[-]")
    {
        "[ ]"
    } else {
        return None;
    };
    let mut out = line.to_owned();
    out.replace_range(box_at..box_at + 3, checked);
    Some(out)
}

/// Whether a list line carries a ticked checkbox.
#[must_use]
fn checkbox_state(line: &str) -> Option<bool> {
    let rest = line.trim_start();
    let after = ["- ", "+ ", "* "]
        .into_iter()
        .find_map(|b| rest.strip_prefix(b))?;
    if after.starts_with("[ ]") {
        Some(false)
    } else if after.starts_with("[X]") || after.starts_with("[x]") {
        Some(true)
    } else {
        None
    }
}

/// `(ticked, total)` over the checkbox lines of `text`.
#[must_use]
pub fn checkbox_counts(text: &str) -> (usize, usize) {
    let mut done = 0;
    let mut total = 0;
    for line in text.lines() {
        if let Some(state) = checkbox_state(line) {
            total += 1;
            done += usize::from(state);
        }
    }
    (done, total)
}

/// Rewrite the `[n/m]` and `[p%]` cookies on any line that owns
/// checkboxes beneath it, leaving everything else byte-identical.
///
/// A cookie belongs to the lines indented under it — org's rule — so a
/// list with sub-lists counts each level against its own cookie.
#[must_use]
pub fn recount_cookies(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let Some(cookie) = cookie_span(line) else {
            out.push((*line).to_owned());
            continue;
        };
        let indent = line.len() - line.trim_start().len();
        let mut done = 0;
        let mut total = 0;
        for below in &lines[i + 1..] {
            let below_indent = below.len() - below.trim_start().len();
            if !below.trim().is_empty() && below_indent <= indent {
                break;
            }
            if let Some(state) = checkbox_state(below) {
                total += 1;
                done += usize::from(state);
            }
        }
        if total == 0 {
            out.push((*line).to_owned());
            continue;
        }
        let replacement = if line[cookie.clone()].ends_with("%]") {
            format!("[{}%]", done * 100 / total)
        } else {
            format!("[{done}/{total}]")
        };
        let mut rewritten = (*line).to_owned();
        rewritten.replace_range(cookie, &replacement);
        out.push(rewritten);
    }
    let mut joined = out.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

/// The byte range of a `[n/m]`, `[/]`, `[p%]` or `[%]` cookie in
/// `line`, if it has one.
#[must_use]
pub fn cookie_span(line: &str) -> Option<std::ops::Range<usize>> {
    let mut from = 0;
    while let Some(open) = line[from..].find('[') {
        let start = from + open;
        let close = line[start..].find(']')?;
        let end = start + close + 1;
        let inner = &line[start + 1..end - 1];
        let is_cookie = inner
            .strip_suffix('%')
            .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
            || inner.split_once('/').is_some_and(|(a, b)| {
                a.chars().all(|c| c.is_ascii_digit()) && b.chars().all(|c| c.is_ascii_digit())
            });
        if is_cookie {
            return Some(start..end);
        }
        from = end;
    }
    None
}

/// A vault file's path as the user names it: relative to the vault
/// root, or unchanged when it lies outside (which nothing in a loaded
/// vault does, but a path is data and this is not the place to panic —
/// I5).
fn vault_relative(shell: &Shell, path: &std::path::Path) -> std::path::PathBuf {
    path.strip_prefix(shell.vault.root())
        .unwrap_or(path)
        .to_path_buf()
}

/// Whether the row with block id `id` is currently folded — the
/// renderer's source for the `▸` marker and the fold-arrow click.
#[must_use]
pub fn is_row_folded(shell: &Shell, id: &str) -> bool {
    row_is_folded(shell, id)
}

/// Whether the row with block id `id` is currently folded.
fn row_is_folded(shell: &Shell, id: &str) -> bool {
    let bid = closure_core::BlockId::from_existing(id);
    shell
        .vault
        .find_by_id(&bid)
        .is_some_and(|(h, _)| headline_is_folded(h))
}

/// Flip the `:VISIBILITY:` property on `id` between `folded` and `all`
/// through the registry (I8, undoable I3). Returns the new folded state,
/// or `None` when the write failed.
fn toggle_visibility(shell: &mut Shell, id: &closure_core::BlockId) -> Option<bool> {
    let folded = shell
        .vault
        .find_by_id(id)
        .is_some_and(|(h, _)| headline_is_folded(h));
    let next = if folded { "all" } else { "folded" };
    shell
        .set_property(id, "VISIBILITY", next)
        .ok()
        .map(|()| !folded)
}

/// Map a TODO keyword to a leading status glyph (G5a): `DONE`-like
/// keywords get a filled marker, open ones a hollow marker, anything else
/// a diamond.
fn todo_glyph(keyword: &str) -> &'static str {
    todo_glyph_for(keyword)
}

/// Does this TODO keyword mean the work is finished?
///
/// Three places used to decide this independently, and they disagreed:
/// the glyph and the outline row counted `CANCELLED` and `KILL` as
/// finished, the body highlighter counted only `DONE`, so one headline
/// was a settled green dot in the tree and an alarm-red word in the
/// buffer. Which keywords mean finished is a property of org and of
/// the user's `todo_keywords`, not of whichever painter is running.
///
/// An unrecognised keyword is *not* finished — the safe way round,
/// because a task shown as open is one you look at again and a task
/// shown as done is one you lose.
#[must_use]
pub fn keyword_is_done(keyword: &str) -> bool {
    matches!(keyword, "DONE" | "CANCELLED" | "KILL")
}

/// The status glyph for a TODO keyword: filled when it is finished.
///
/// Derived from [`keyword_is_done`] rather than from a second list, so
/// the dot and the colour cannot drift apart again.
#[must_use]
pub fn todo_glyph_for(keyword: &str) -> &'static str {
    if keyword_is_done(keyword) {
        "●"
    } else if keyword.is_empty() {
        "·"
    } else {
        "○"
    }
}

/// The byte range of the TODO keyword `text` opens with, if any.
///
/// "In the prompt TODO is just white text" — a field cannot colour
/// what it cannot locate, and a headline being typed has no stars in
/// front of it yet, so the body highlighter (which needs them) cannot
/// answer. Only a leading, whole, uppercase word counts: `TODOS` is a
/// plural and `buy milk TODO` is a sentence that ends in shouting.
#[must_use]
pub fn leading_keyword(text: &str) -> Option<(usize, usize)> {
    let word = text.split_whitespace().next()?;
    if !text.starts_with(word) {
        return None;
    }
    let known = keyword_is_done(word)
        || matches!(word, "TODO" | "NEXT" | "WAIT" | "HOLD" | "PROJ" | "STRT");
    known.then_some((0, word.len()))
}

/// Overlay ephemeral peer presence onto rows (Q11-C3).
///
/// A `◉ <peer>` badge is appended to the row whose id a peer is
/// focused on. Pure — presence never touches document state or the
/// undo tree; every shell that renders badges (G5a) shows it with no
/// new code.
#[must_use]
pub fn with_presence(mut rows: Vec<RowView>, presence: &[(String, String)]) -> Vec<RowView> {
    for row in &mut rows {
        for (peer, block) in presence {
            if row.id == *block {
                row.badges.push(format!("◉ {peer}"));
            }
        }
    }
    rows
}

/// Deterministic seeded vault generator (Q12-B1).
///
/// `files` org files of `headlines_per_file` level-1 headlines with
/// stable `:ID:`s, varied TODO/tags/bodies — the big-vault fixture
/// without committing thousands of files. Same `(files, headlines,
/// seed)` always yields identical bytes (I6); every file parses and
/// roundtrips (I1, tested). Pure xorshift, no dependency.
#[must_use]
pub fn gen_vault(files: usize, headlines_per_file: usize, seed: u64) -> Vec<(String, String)> {
    use std::fmt::Write as _;
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).max(1);
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let words = [
        "parser", "vault", "agenda", "widget", "kernel", "backlink", "capture", "formula",
    ];
    let mut out = Vec::with_capacity(files);
    for f in 0..files {
        let mut src = String::new();
        for h in 0..headlines_per_file {
            let r = next();
            let todo = match r % 3 {
                0 => "TODO ",
                1 => "DONE ",
                _ => "",
            };
            let ri = usize::try_from(r % 1_000_003).unwrap_or(0);
            let w1 = words[(ri / 3) % words.len()];
            let w2 = words[(ri / 7) % words.len()];
            let tag = if r % 5 == 0 { " :work:" } else { "" };
            let _ = writeln!(src, "* {todo}{w1} {w2} {f}-{h}{tag}");
            let _ = writeln!(src, ":PROPERTIES:");
            let _ = writeln!(
                src,
                ":ID: 01H{:023X}",
                (u128::from(r) << 16) | ((f as u128) << 8) | h as u128
            );
            let _ = writeln!(src, ":END:");
            let _ = writeln!(src, "body {w2} line {}", r % 97);
        }
        out.push((format!("gen-{f:03}.org"), src));
    }
    out
}

/// Build the default browse [`ViewTree`](Node) from a borrowed vault
/// (V3a).
///
/// Every headline becomes a row (vault iteration order), selection 0,
/// plus a hint line. Borrow-friendly (no [`Shell`] ownership) so callers
/// like the LLM `view-render` tool can snapshot the screen. Rows carry an
/// icon (TODO glyph) + badges (tags) as data (G5a).
#[must_use]
pub fn browse_view(vault: &closure_store::Vault) -> Node {
    let rows: Vec<RowView> = vault
        .iter()
        .flat_map(|(_p, doc)| {
            doc.all_headlines().map(|h| {
                RowView::new(
                    h.id().to_string(),
                    h.title(),
                    h.level(),
                    h.todo().map(ToOwned::to_owned),
                )
                .with_icon(h.todo().map(|t| todo_glyph(t).to_owned()))
                // The cookie first: a shell painting badges left to
                // right should put "do this first" before the tags,
                // which is where org puts it on the line itself.
                .with_badges(
                    h.priority()
                        .map(priority_cookie)
                        .into_iter()
                        .chain(h.tags().iter().map(ToOwned::to_owned))
                        .collect(),
                )
            })
        })
        .collect();
    let line = format!("[Notion] {} headlines — type: filter", rows.len());
    Node::Pane {
        title: "closure".to_owned(),
        children: vec![Node::Rows { rows, selected: 0 }, Node::Hints { line }],
    }
}

/// Escape a string for a JSON double-quoted literal (dep-free).
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Serialise a [`ViewTree`](Node) to compact JSON (V13), dep-free (no
/// `serde`).
///
/// Each node object carries its kind tag (`k`), its
/// [`aria_role`](Node::aria_role), and its data — enough for a tiny
/// client-side renderer to rebuild the `DOM`, so a self-contained HTML
/// export can render the declarative tree with no server and no toolchain.
#[must_use]
// One flat arm per `Node` kind — exhaustive by design (V1c); splitting
// the match would only hide the one-to-one kind→JSON mapping.
#[allow(clippy::too_many_lines)]
pub fn view_to_json(node: &Node) -> String {
    use std::fmt::Write as _;
    let role = json_str(node.aria_role());
    match node {
        Node::Pane { title, children } => {
            let kids: Vec<String> = children.iter().map(view_to_json).collect();
            format!(
                "{{\"k\":\"pane\",\"role\":{role},\"title\":{},\"children\":[{}]}}",
                json_str(title),
                kids.join(",")
            )
        }
        Node::Rows { rows, selected } => {
            let items = rows.iter().fold(String::new(), |mut s, r| {
                if !s.is_empty() {
                    s.push(',');
                }
                let todo = r
                    .todo
                    .as_deref()
                    .map_or_else(|| "null".to_owned(), json_str);
                let icon = r
                    .icon
                    .as_deref()
                    .map_or_else(|| "null".to_owned(), json_str);
                let badges = r
                    .badges
                    .iter()
                    .map(|b| json_str(b))
                    .collect::<Vec<_>>()
                    .join(",");
                let _ = write!(
                    s,
                    "{{\"title\":{},\"todo\":{todo},\"level\":{},\"icon\":{icon},\"badges\":[{badges}]}}",
                    json_str(&r.title),
                    r.level
                );
                s
            });
            format!("{{\"k\":\"rows\",\"role\":{role},\"selected\":{selected},\"rows\":[{items}]}}")
        }
        Node::Detail { fields } => {
            let items = fields.iter().fold(String::new(), |mut s, f| {
                if !s.is_empty() {
                    s.push(',');
                }
                let chord = f
                    .action
                    .as_ref()
                    .map_or_else(|| "null".to_owned(), |a| json_str(a.chord()));
                let _ = write!(
                    s,
                    "{{\"label\":{},\"value\":{},\"chord\":{chord}}}",
                    json_str(&f.label),
                    json_str(&f.value)
                );
                s
            });
            format!("{{\"k\":\"detail\",\"role\":{role},\"fields\":[{items}]}}")
        }
        Node::Input { label, buffer } => format!(
            "{{\"k\":\"input\",\"role\":{role},\"label\":{},\"buffer\":{}}}",
            json_str(label),
            json_str(buffer)
        ),
        Node::Palette { items, cursor } => {
            let its = items.iter().fold(String::new(), |mut s, it| {
                if !s.is_empty() {
                    s.push(',');
                }
                let _ = write!(
                    s,
                    "{{\"label\":{},\"chord\":{}}}",
                    json_str(&it.label),
                    json_str(it.action.chord())
                );
                s
            });
            format!("{{\"k\":\"palette\",\"role\":{role},\"cursor\":{cursor},\"items\":[{its}]}}")
        }
        Node::Hints { line } => {
            format!(
                "{{\"k\":\"hints\",\"role\":{role},\"line\":{}}}",
                json_str(line)
            )
        }
        Node::Widget { name, content } => format!(
            "{{\"k\":\"widget\",\"role\":{role},\"name\":{},\"content\":{}}}",
            json_str(name),
            json_str(content)
        ),
        Node::Text(t) => format!(
            "{{\"k\":\"text\",\"role\":{role},\"text\":{}}}",
            json_str(t)
        ),
        Node::Split { direction, panes } => {
            let kids: Vec<String> = panes.iter().map(view_to_json).collect();
            format!(
                "{{\"k\":\"split\",\"role\":{role},\"dir\":{},\"panes\":[{}]}}",
                json_str(direction.as_str()),
                kids.join(",")
            )
        }
        Node::Modal { title, body } => format!(
            "{{\"k\":\"modal\",\"role\":{role},\"title\":{},\"body\":{}}}",
            json_str(title),
            view_to_json(body)
        ),
        Node::Toast { level, text } => format!(
            "{{\"k\":\"toast\",\"role\":{role},\"level\":{},\"text\":{}}}",
            json_str(level.as_str()),
            json_str(text)
        ),
    }
}

/// Serialise a [`ViewTree`](Node) to a compact, LLM-readable snapshot
/// (V3a).
///
/// Captures what is on screen — panes, the selected row, visible rows,
/// detail fields, the focused input, palette, and widgets. Deterministic.
#[must_use]
pub fn serialize_view(node: &Node) -> String {
    let mut out = String::new();
    serialize_node(node, 0, &mut out);
    out
}

fn serialize_node(node: &Node, depth: usize, out: &mut String) {
    use std::fmt::Write as _;
    let pad = "  ".repeat(depth);
    match node {
        Node::Pane { title, children } => {
            let _ = writeln!(out, "{pad}PANE {title}");
            for c in children {
                serialize_node(c, depth + 1, out);
            }
        }
        Node::Rows { rows, selected } => {
            let _ = writeln!(out, "{pad}ROWS selected={selected} count={}", rows.len());
            for (i, r) in rows.iter().enumerate() {
                let mark = if i == *selected { '>' } else { ' ' };
                let todo = r
                    .todo
                    .as_deref()
                    .map_or_else(String::new, |t| format!("{t} "));
                let icon = r
                    .icon
                    .as_deref()
                    .map_or_else(String::new, |g| format!("{g} "));
                let badges = if r.badges.is_empty() {
                    String::new()
                } else {
                    format!("  :{}:", r.badges.join(":"))
                };
                let _ = writeln!(out, "{pad}  {mark} {icon}{todo}{}{badges}", r.title);
            }
        }
        Node::Detail { fields } => {
            let _ = writeln!(out, "{pad}DETAIL");
            for f in fields {
                let act = f
                    .action
                    .as_ref()
                    .map_or_else(String::new, |a| format!(" [{}]", a.chord()));
                let _ = writeln!(out, "{pad}  {}: {}{act}", f.label, f.value);
            }
        }
        Node::Input { label, buffer } => {
            let _ = writeln!(out, "{pad}INPUT {label}=\"{buffer}\"");
        }
        Node::Palette { items, cursor } => {
            let _ = writeln!(out, "{pad}PALETTE cursor={cursor} count={}", items.len());
            for it in items {
                let _ = writeln!(out, "{pad}  [{}] {}", it.action.chord(), it.label);
            }
        }
        Node::Hints { line } => {
            let _ = writeln!(out, "{pad}HINTS {line}");
        }
        Node::Widget { name, content } => {
            let _ = writeln!(out, "{pad}WIDGET {name}");
            for l in content.lines() {
                let _ = writeln!(out, "{pad}  {l}");
            }
        }
        Node::Text(t) => {
            let _ = writeln!(out, "{pad}TEXT {t}");
        }
        Node::Split { direction, panes } => {
            let _ = writeln!(out, "{pad}SPLIT {}", direction.as_str());
            for p in panes {
                serialize_node(p, depth + 1, out);
            }
        }
        Node::Modal { title, body } => {
            let _ = writeln!(out, "{pad}MODAL {title}");
            serialize_node(body, depth + 1, out);
        }
        Node::Toast { level, text } => {
            let _ = writeln!(out, "{pad}TOAST {} {text}", level.as_str());
        }
    }
}

/// Type-level shell capabilities (V11).
///
/// Applies the Yesod "turn runtime bugs into compile-time errors" rule to
/// the shell/capability matrix: a shell may only invoke a capability it
/// *statically* declares via [`Supports`]. The sibling
/// [`NodeKind`]/`ui_matrix` data describes what each shell renders at
/// run-time; this makes a *wrong* invocation a compile error.
pub mod caps {
    mod sealed {
        pub trait Sealed {}
    }

    /// A capability marker (sealed — only the markers below implement it).
    pub trait Capability: sealed::Sealed {}

    macro_rules! capability {
        ($(#[$m:meta])* $name:ident) => {
            $(#[$m])*
            #[derive(Debug, Clone, Copy)]
            pub struct $name;
            impl sealed::Sealed for $name {}
            impl Capability for $name {}
        };
    }

    capability!(/// Read/browse the vault.
        Browse);
    capability!(/// Mutating edits (rename/add/delete/set).
        Edit);
    capability!(/// org-capture creation.
        Capture);
    capability!(/// Fuzzy / full-text search.
        Search);
    capability!(/// Babel / eval / tangle.
        Eval);
    capability!(/// Notion-style database views.
        Database);

    /// `S: Supports<C>` iff shell `S` provides capability `C`. A shell
    /// type implements it once per capability it offers.
    pub trait Supports<C: Capability> {}

    /// Compile-time proof that shell `S` supports capability `C`.
    ///
    /// A no-op at run-time; its only purpose is the bound `S: Supports<C>`,
    /// so a shell that does not declare a capability cannot call into it.
    ///
    /// Supported combinations compile:
    /// ```
    /// use closure_shell_core::caps::{capability_gate, TuiShell, Edit};
    /// capability_gate::<TuiShell, Edit>();
    /// ```
    ///
    /// An unsupported one does **not** — a whole class of shell/capability
    /// mismatch bugs cannot be written:
    /// ```compile_fail
    /// use closure_shell_core::caps::{capability_gate, ReadOnlyWebShell, Edit};
    /// capability_gate::<ReadOnlyWebShell, Edit>();
    /// ```
    pub const fn capability_gate<S: Supports<C>, C: Capability>() {}

    /// The full-featured TUI shell.
    #[derive(Debug, Clone, Copy)]
    pub struct TuiShell;
    impl Supports<Browse> for TuiShell {}
    impl Supports<Edit> for TuiShell {}
    impl Supports<Capture> for TuiShell {}
    impl Supports<Search> for TuiShell {}
    impl Supports<Eval> for TuiShell {}
    impl Supports<Database> for TuiShell {}

    /// The read-only web shell (browse + search; no editing).
    #[derive(Debug, Clone, Copy)]
    pub struct ReadOnlyWebShell;
    impl Supports<Browse> for ReadOnlyWebShell {}
    impl Supports<Search> for ReadOnlyWebShell {}
}

/// One captured network flow + the action decided for it (V7a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SniffEvent {
    /// Candidate string (`"<host>:<port> <proto>"`).
    pub candidate: String,
    /// The action a rule decided, if any.
    pub action: Option<closure_sniffer::Action>,
    /// *Which* rule decided it. The action says a request was blocked;
    /// this says by what, which is the question that follows.
    pub rule: Option<closure_sniffer::Rule>,
    /// `path:line` of the capture log entry it was read from — the
    /// answer to "what was actually recorded", which a verdict you did
    /// not expect always raises. `None` for a flow captured live,
    /// which has not been written down yet.
    pub source: Option<String>,
}

/// One host and how much of the traffic was to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTraffic {
    /// The host, without its port.
    pub host: String,
    /// How many flows went to it.
    pub flows: usize,
    /// How many of those a rule blocked.
    pub blocked: usize,
}

/// One captured flow, taken apart.
///
/// "inspect" from the snitcher's feature list. A candidate is three
/// facts wearing one string — a host, a port and a protocol — and a
/// filter over that string cannot tell a port from a hostname with
/// digits in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowDetail {
    /// The candidate exactly as captured.
    pub candidate: String,
    /// The host part, without the port.
    pub host: String,
    /// The port, when the candidate carried one.
    pub port: Option<u16>,
    /// The protocol word, or empty when there was none.
    pub protocol: String,
    /// What was decided.
    pub action: Option<closure_sniffer::Action>,
    /// And by which rule — `user-N` for one you added this session.
    pub rule: Option<closure_sniffer::Rule>,
}

/// The flow one line of `network.org` records, if it records one.
///
/// The log's shape is `* <ts> host=<host> proto=<proto>`, written by
/// [`closure_sniffer::log_capture_to_org`]. Anything else in the file
/// is somebody's note about their network, which is a thing a vault is
/// allowed to contain.
fn capture_candidate(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix('*')?.trim_start();
    let field = |key: &str| {
        let at = rest.find(key)?;
        rest[at + key.len()..]
            .split_whitespace()
            .next()
            .filter(|v| !v.is_empty())
    };
    let host = field("host=")?;
    // The candidate a rule matches against is `host:port protocol` —
    // the same shape `record` is given live, so one flow reads the
    // same whether it came off the wire or out of the log. The log
    // said `proto=tcp` and the pane showed `protocol —`, because this
    // dropped it.
    Some(field("proto=").map_or_else(|| host.to_owned(), |proto| format!("{host} {proto}")))
}

/// Headless state for the interactive sniffer surface (V7a).
///
/// A pure state machine over [`closure_sniffer::CaptureBackend`]: a live
/// event list, a cursor, a substring filter, and per-flow allow/block
/// toggles that mutate the blocklist rules. Unit-testable without a
/// terminal — the same pattern as the launcher [`App`]; a shell (V7b)
/// renders it as a [`ViewTree`](Node).
#[derive(Debug, Clone, Default)]
pub struct SnifferApp {
    events: Vec<SniffEvent>,
    selected: usize,
    filter: String,
    rules: Vec<closure_sniffer::Rule>,
    /// The vault's configured rules, as of the last [`Self::load`].
    /// Kept so [`Self::debug`] can say what was tried when nothing
    /// matched — "no rule matched" is only useful beside the rules.
    considered: Vec<closure_sniffer::Rule>,
}

impl SnifferApp {
    /// A fresh sniffer surface.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Capture `candidate`, deciding its action via `backend` (and any
    /// user rules already added). Appended to the event list.
    pub fn record(&mut self, candidate: &str, backend: &impl closure_sniffer::CaptureBackend) {
        // A rule you added this session wins over the backend's, and
        // the *rule* is kept beside the action so the pane can say
        // which one it was.
        let user = closure_sniffer::match_first(candidate, &self.rules).cloned();
        let rule = user.or_else(|| backend.match_rule(candidate));
        let action = rule
            .as_ref()
            .map(|r| r.action)
            .or_else(|| backend.match_action(candidate));
        self.events.push(SniffEvent {
            candidate: candidate.to_owned(),
            action,
            rule,
            source: None,
        });
    }

    /// Every captured event, in capture order.
    #[must_use]
    pub fn events(&self) -> &[SniffEvent] {
        &self.events
    }

    /// Read the flows out of the vault's `network.org`, replacing what
    /// is in the pane. Returns how many there were.
    ///
    /// This is where the pane's contents come from. There was no
    /// source at all before: nothing in either shell ever called
    /// [`Self::record`], so the empty state told you to go and run
    /// `closure sniff` — a screen whose first instruction is to use a
    /// different program — while `log_capture_to_org` wrote an
    /// org-native log that nobody read.
    ///
    /// Reading the vault rather than holding a socket is the point: it
    /// needs no privileges, it survives a restart, and a flow you
    /// captured is a headline you can search, tag and link like any
    /// other. The verdict is not stored with it — it is whatever the
    /// rules say now, so blocking a host re-decides the flows already
    /// on screen.
    pub fn load(&mut self, vault: &closure_store::Vault) -> usize {
        self.events.clear();
        self.selected = 0;
        // Where each flow came from, kept beside it: when a verdict
        // surprises you the question is what was actually recorded,
        // and a pane that cannot answer it sends you to `grep`.
        let rows: Vec<(String, String)> = vault
            .iter()
            .filter(|(path, _)| path.file_name().is_some_and(|n| n == "network.org"))
            .flat_map(|(path, doc)| {
                let where_from = path.display().to_string();
                doc.source()
                    .lines()
                    .enumerate()
                    .filter_map(|(i, line)| {
                        Some((capture_candidate(line)?, format!("{where_from}:{}", i + 1)))
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        // The vault's own `sniffer_blocklist`, which is what decides a
        // flow unless you have overridden it by hand this session. It
        // was not consulted at all: the pane read four flows and said
        // "nothing matched it" beside a host the config blocks.
        let configured: Vec<closure_sniffer::Rule> =
            closure_config::Config::load_reporting(&vault.root().join(closure_config::CONFIG_FILE))
                .0
                .sniffer_blocklist
                .unwrap_or_default()
                .into_iter()
                .map(|pattern| closure_sniffer::Rule {
                    id: format!("block-{pattern}"),
                    pattern,
                    action: closure_sniffer::Action::Block,
                })
                .collect();
        for (candidate, source) in rows {
            let rule = closure_sniffer::match_first(&candidate, &self.rules)
                .or_else(|| closure_sniffer::match_first(&candidate, &configured))
                .cloned();
            let action = rule.as_ref().map(|r| r.action);
            self.events.push(SniffEvent {
                candidate,
                action,
                rule,
                source: Some(source),
            });
        }
        self.considered = configured;
        self.events.len()
    }

    /// Everything known about flow `i`, for when its verdict surprises
    /// you: what was recorded, where, what it was matched against, and
    /// which rule decided it — or, when none did, the rules that were
    /// tried and did not.
    ///
    /// "no rule matched" is a different sentence from "allowed", and
    /// the pane could say neither.
    #[must_use]
    pub fn debug(&self, i: usize) -> Option<Vec<String>> {
        let event: SniffEvent = (*self.filtered().get(i)?).clone();
        let mut out = vec![format!("candidate   {}", event.candidate)];
        if let Some(source) = &event.source {
            out.push(format!("recorded in {source}"));
        }
        if let Some(rule) = &event.rule {
            out.push(format!("decided by  {} ({})", rule.id, rule.pattern));
            out.push(format!("action      {:?}", rule.action));
        } else {
            out.push("decided by  no rule matched — allowed by default".to_owned());
            let tried: Vec<&str> = self
                .rules
                .iter()
                .chain(self.considered.iter())
                .map(|r| r.pattern.as_str())
                .collect();
            out.push(if tried.is_empty() {
                "tried       no rules are configured".to_owned()
            } else {
                format!("tried       {}", tried.join(", "))
            });
        }
        Some(out)
    }

    /// Who this machine talks to and how much: one row per host,
    /// busiest first.
    ///
    /// "network graph". The log records a host per flow and nothing
    /// else — no process, no peer-to-peer edge — so what there is to
    /// draw is a distribution rather than a topology. Drawing edges
    /// that were never measured would be a picture of nothing.
    #[must_use]
    pub fn graph(&self) -> Vec<HostTraffic> {
        let mut by_host: Vec<HostTraffic> = Vec::new();
        for event in &self.events {
            let host = event
                .candidate
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .rsplit_once(':')
                .map_or(event.candidate.as_str(), |(h, _)| h)
                .trim_matches(['[', ']'])
                .to_owned();
            let blocked = usize::from(event.action == Some(closure_sniffer::Action::Block));
            if let Some(row) = by_host.iter_mut().find(|r| r.host == host) {
                row.flows += 1;
                row.blocked += blocked;
            } else {
                by_host.push(HostTraffic {
                    host,
                    flows: 1,
                    blocked,
                });
            }
        }
        // Busiest first; ties by name so the list does not shuffle
        // between frames.
        by_host.sort_by(|a, b| b.flows.cmp(&a.flows).then_with(|| a.host.cmp(&b.host)));
        by_host
    }

    /// The blocklist rules the toggles have added (for persistence to the
    /// `sniffer_blocklist` config).
    #[must_use]
    pub fn rules(&self) -> &[closure_sniffer::Rule] {
        &self.rules
    }

    /// Set the substring filter applied to [`Self::filtered`].
    pub fn set_filter(&mut self, filter: &str) {
        self.filter.clear();
        self.filter.push_str(filter);
        self.selected = 0;
    }

    /// Events whose candidate contains the filter (all when empty).
    #[must_use]
    pub fn filtered(&self) -> Vec<&SniffEvent> {
        self.events
            .iter()
            .filter(|e| self.filter.is_empty() || e.candidate.contains(&self.filter))
            .collect()
    }

    /// Move the cursor to filtered-row `i` (clamped).
    pub fn select(&mut self, i: usize) {
        let last = self.filtered().len().saturating_sub(1);
        self.selected = i.min(last);
    }

    /// The cursor index into [`Self::filtered`].
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }

    /// Add a rule for the selected flow and re-decide every event.
    fn rule_for_selected(&mut self, action: closure_sniffer::Action) {
        let Some(candidate) = self
            .filtered()
            .get(self.selected)
            .map(|e| e.candidate.clone())
        else {
            return;
        };
        let rule = closure_sniffer::Rule {
            id: format!("user-{}", self.rules.len()),
            pattern: candidate.clone(),
            action,
        };
        self.rules.push(rule.clone());
        // Re-decide the matching events under the new user rule — and
        // record *which* rule, or the pane goes on naming the backend
        // rule you have just overridden.
        for e in &mut self.events {
            if e.candidate == candidate {
                e.action = Some(action);
                e.rule = Some(rule.clone());
            }
        }
    }

    /// Block the selected flow (adds a `Block` rule).
    pub fn block_selected(&mut self) {
        self.rule_for_selected(closure_sniffer::Action::Block);
    }

    /// Allow the selected flow (adds an `Allow` rule, overriding any
    /// backend block).
    pub fn allow_selected(&mut self) {
        self.rule_for_selected(closure_sniffer::Action::Allow);
    }

    /// One flow, taken apart: host, port, protocol, and the rule that
    /// decided it.
    ///
    /// Indexes the *filtered* list, which is the list on screen — the
    /// row you are looking at is the row you get.
    #[must_use]
    pub fn inspect(&self, i: usize) -> Option<FlowDetail> {
        let event: SniffEvent = (*self.filtered().get(i)?).clone();
        let (address, protocol) = event
            .candidate
            .split_once(' ')
            .map_or((event.candidate.as_str(), ""), |(a, p)| (a, p.trim()));
        // IPv6 wears its own brackets — `[::1]:443` — so the port is
        // whatever follows the last colon *outside* them.
        let (host, port) = address.rsplit_once(':').map_or((address, None), |(h, p)| {
            p.parse::<u16>().map_or((address, None), |n| (h, Some(n)))
        });
        Some(FlowDetail {
            candidate: event.candidate.clone(),
            host: host.trim_matches(['[', ']']).to_owned(),
            port,
            protocol: protocol.to_owned(),
            action: event.action,
            rule: event.rule,
        })
    }

    /// A description of the selected flow (candidate + action), or `None`.
    #[must_use]
    pub fn detail(&self) -> Option<String> {
        let e = self.filtered().get(self.selected).copied()?;
        let action = e
            .action
            .map_or_else(|| "(no rule)".to_owned(), |a| format!("{a:?}"));
        Some(format!("{}\naction: {action}", e.candidate))
    }

    /// The declarative [`Node`] tree for the sniffer surface (V7b): the
    /// filtered flow list, a detail pane offering block/allow (each
    /// carrying its chord, the V1 invariant), and the which-key hints.
    #[must_use]
    pub fn view(&self, mode: closure_config::InputMode) -> Node {
        let rows: Vec<RowView> = self
            .filtered()
            .iter()
            .map(|e| {
                RowView::new(
                    e.candidate.clone(),
                    e.candidate.clone(),
                    1,
                    e.action.map(|a| format!("{a:?}")),
                )
            })
            .collect();
        let mut children = vec![Node::Rows {
            rows,
            selected: self.selected,
        }];
        if let Some(e) = self.filtered().get(self.selected).copied() {
            let mut fields = vec![FieldView {
                label: "flow".to_owned(),
                value: e.candidate.clone(),
                action: None,
            }];
            fields.push(FieldView {
                label: "action".to_owned(),
                value: e
                    .action
                    .map_or_else(|| "(no rule)".to_owned(), |a| format!("{a:?}")),
                action: None,
            });
            // Inspect, in the tree both shells render from: the list
            // says a flow was blocked and this says by what. Without
            // it the question you actually have has no answer on
            // screen in either shell.
            if let Some(flow) = self.inspect(self.selected) {
                fields.push(FieldView {
                    label: "host".to_owned(),
                    value: flow.host,
                    action: None,
                });
                if let Some(port) = flow.port {
                    fields.push(FieldView {
                        label: "port".to_owned(),
                        value: port.to_string(),
                        action: None,
                    });
                }
                if !flow.protocol.is_empty() {
                    fields.push(FieldView {
                        label: "protocol".to_owned(),
                        value: flow.protocol,
                        action: None,
                    });
                }
                fields.push(FieldView {
                    label: "decided by".to_owned(),
                    value: flow.rule.map_or_else(
                        // Not "no rule": nothing matched it, which is
                        // *why* it is allowed.
                        || "nothing matched it".to_owned(),
                        |r| format!("{} ({})", r.id, r.pattern),
                    ),
                    action: None,
                });
            }
            fields.push(FieldView {
                label: "block".to_owned(),
                value: String::new(),
                action: Action::new(mode, "block-flow"),
            });
            fields.push(FieldView {
                label: "allow".to_owned(),
                value: String::new(),
                action: Action::new(mode, "allow-flow"),
            });
            children.push(Node::Detail { fields });
        }
        children.push(Node::Hints {
            line: format!("[{mode:?}] sniffer — {} flows", self.events.len()),
        });
        Node::Pane {
            title: "sniffer".to_owned(),
            children,
        }
    }
}

/// Interactive 3-way conflict resolution surface (V9b).
///
/// Lists the [`FieldConflict`](closure_crdt::FieldConflict)s detected by
/// [`closure_crdt::conflicts`] and applies the user's ours/theirs choice
/// through the vault command path (undoable, I3/I8). Rendered as a
/// [`ViewTree`](Node) with the `resolve-ours`/`resolve-theirs` chords.
#[derive(Debug, Clone)]
pub struct ConflictApp {
    conflicts: Vec<closure_crdt::FieldConflict>,
    selected: usize,
    input_mode: closure_config::InputMode,
}

impl ConflictApp {
    /// Build a resolver over `conflicts` for input `mode`.
    #[must_use]
    pub const fn new(
        conflicts: Vec<closure_crdt::FieldConflict>,
        mode: closure_config::InputMode,
    ) -> Self {
        Self {
            conflicts,
            selected: 0,
            input_mode: mode,
        }
    }

    /// The outstanding conflicts.
    #[must_use]
    pub fn conflicts(&self) -> &[closure_crdt::FieldConflict] {
        &self.conflicts
    }

    /// The cursor index.
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }

    /// Move the cursor (clamped).
    pub fn select(&mut self, i: usize) {
        let last = self.conflicts.len().saturating_sub(1);
        self.selected = i.min(last);
    }

    /// Resolve the selected conflict to our value.
    ///
    /// # Errors
    ///
    /// Propagates the vault command error.
    pub fn resolve_ours(&mut self, shell: &mut Shell) -> Result<(), closure_store::VaultError> {
        let Some(c) = self.conflicts.get(self.selected) else {
            return Ok(());
        };
        let (block, field, value) = (c.block.clone(), c.field, c.ours.clone());
        self.apply(shell, &block, field, &value)
    }

    /// Resolve the selected conflict to their value.
    ///
    /// # Errors
    ///
    /// Propagates the vault command error.
    pub fn resolve_theirs(&mut self, shell: &mut Shell) -> Result<(), closure_store::VaultError> {
        let Some(c) = self.conflicts.get(self.selected) else {
            return Ok(());
        };
        let (block, field, value) = (c.block.clone(), c.field, c.theirs.clone());
        self.apply(shell, &block, field, &value)
    }

    /// Apply `value` to the conflicting field via the command path, then
    /// drop the now-resolved conflict.
    fn apply(
        &mut self,
        shell: &mut Shell,
        block: &closure_core::BlockId,
        field: closure_crdt::ConflictField,
        value: &str,
    ) -> Result<(), closure_store::VaultError> {
        match field {
            closure_crdt::ConflictField::Title => shell.rename_headline(block, value)?,
            closure_crdt::ConflictField::Body => shell.set_body(block, value)?,
        }
        self.conflicts.remove(self.selected);
        if self.selected >= self.conflicts.len() {
            self.selected = self.conflicts.len().saturating_sub(1);
        }
        Ok(())
    }

    /// The declarative [`Node`] tree: the conflict list + a detail pane
    /// showing base/ours/theirs with the resolve actions (chords carried
    /// per the V1 invariant).
    #[must_use]
    pub fn view(&self) -> Node {
        let m = self.input_mode;
        let rows: Vec<RowView> = self
            .conflicts
            .iter()
            .map(|c| {
                RowView::new(
                    c.block.to_string(),
                    format!("{:?}: {}", c.field, c.block),
                    1,
                    None,
                )
            })
            .collect();
        let mut children = vec![Node::Rows {
            rows,
            selected: self.selected,
        }];
        if let Some(c) = self.conflicts.get(self.selected) {
            children.push(Node::Detail {
                fields: vec![
                    FieldView {
                        label: "base".to_owned(),
                        value: c.base.clone().unwrap_or_default(),
                        action: None,
                    },
                    FieldView {
                        label: "ours".to_owned(),
                        value: c.ours.clone(),
                        action: Action::new(m, "resolve-ours"),
                    },
                    FieldView {
                        label: "theirs".to_owned(),
                        value: c.theirs.clone(),
                        action: Action::new(m, "resolve-theirs"),
                    },
                ],
            });
        }
        children.push(Node::Hints {
            line: format!("[{m:?}] {} conflicts", self.conflicts.len()),
        });
        Node::Pane {
            title: "conflicts".to_owned(),
            children,
        }
    }
}

/// The interaction state of an element, for focus-ring / hover / pressed /
/// disabled styling (G5b). Derived by [`Interactions::state_of`] with a
/// fixed precedence; the embedder maps it to native styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementState {
    /// No interaction.
    Normal,
    /// Pointer is over the element.
    Hovered,
    /// Element holds keyboard focus.
    Focused,
    /// Element is being pressed / activated.
    Active,
    /// Element is non-interactive (absorbing — wins over every other
    /// state).
    Disabled,
}

impl ElementState {
    /// A stable lowercase style class (P6) each shell maps to its native
    /// focus-ring / hover / pressed / disabled styling.
    #[must_use]
    pub const fn class(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Hovered => "hovered",
            Self::Focused => "focused",
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }
}

/// The interaction-state machine for a list of elements (G5b).
///
/// Tracks which element index is focused, hovered, and pressed, plus a set
/// of disabled indices. [`Self::state_of`] resolves these to a single
/// [`ElementState`] with precedence `Disabled > Active > Focused >
/// Hovered > Normal`. Pure + deterministic — every shell paints from this
/// one tested source, the pixels are the embedder's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Interactions {
    focused: Option<usize>,
    hovered: Option<usize>,
    pressed: Option<usize>,
    disabled: Vec<usize>,
}

impl Interactions {
    /// Give keyboard focus to element `i`.
    pub const fn focus(&mut self, i: usize) {
        self.focused = Some(i);
    }

    /// Drop keyboard focus.
    pub const fn blur(&mut self) {
        self.focused = None;
    }

    /// The currently focused element index, if any.
    #[must_use]
    pub const fn focused(&self) -> Option<usize> {
        self.focused
    }

    /// Set (or clear, with `None`) the hovered element.
    pub const fn hover(&mut self, i: Option<usize>) {
        self.hovered = i;
    }

    /// Begin pressing element `i` (becomes [`ElementState::Active`]).
    pub const fn press(&mut self, i: usize) {
        self.pressed = Some(i);
    }

    /// Release the press.
    pub const fn release(&mut self) {
        self.pressed = None;
    }

    /// Enable or disable element `i`.
    pub fn set_disabled(&mut self, i: usize, disabled: bool) {
        let present = self.disabled.iter().position(|&d| d == i);
        match (disabled, present) {
            (true, None) => self.disabled.push(i),
            (false, Some(p)) => {
                self.disabled.remove(p);
            }
            _ => {}
        }
    }

    /// Move focus to the next element (wrapping within `count`).
    pub fn focus_next(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        self.focused = Some(self.focused.map_or(0, |f| (f + 1) % count));
    }

    /// Move focus to the previous element (wrapping within `count`).
    pub fn focus_prev(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        self.focused = Some(self.focused.map_or(count - 1, |f| (f + count - 1) % count));
    }

    /// Resolve element `i`'s [`ElementState`] under the fixed precedence.
    #[must_use]
    pub fn state_of(&self, i: usize) -> ElementState {
        if self.disabled.contains(&i) {
            ElementState::Disabled
        } else if self.pressed == Some(i) {
            ElementState::Active
        } else if self.focused == Some(i) {
            ElementState::Focused
        } else if self.hovered == Some(i) {
            ElementState::Hovered
        } else {
            ElementState::Normal
        }
    }
}

/// The kind of a feedback item (G7): a severity, or a progress percentage
/// for a running long op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackKind {
    /// Neutral information.
    Info,
    /// A completed action.
    Success,
    /// A non-fatal caution.
    Warning,
    /// A failure.
    Error,
    /// A long op in progress, `0..=100`%.
    Progress(u8),
}

/// One entry in the [`Feedback`] queue (G7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackItem {
    /// Severity / progress.
    pub kind: FeedbackKind,
    /// Message (for progress, the operation label).
    pub text: String,
}

/// The async feedback surface (G7): a queue of typed notifications +
/// progress for long ops (sync / eval / llm).
///
/// Mutated as state — `notify` appends a severity message, `progress`
/// updates a labelled progress entry in place (so a running op reports
/// incrementally instead of stacking). [`Self::to_nodes`] renders every
/// item as a [`Node::Toast`] (G1c), so *every* shell already displays the
/// feedback with no per-shell notification code.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Feedback {
    items: Vec<FeedbackItem>,
}

impl Feedback {
    /// Append a severity notification.
    pub fn notify(&mut self, level: ToastLevel, text: impl Into<String>) {
        let kind = match level {
            ToastLevel::Info => FeedbackKind::Info,
            ToastLevel::Success => FeedbackKind::Success,
            ToastLevel::Warning => FeedbackKind::Warning,
            ToastLevel::Error => FeedbackKind::Error,
        };
        self.items.push(FeedbackItem {
            kind,
            text: text.into(),
        });
    }

    /// Report progress for the op labelled `label` — updates the existing
    /// progress entry in place, or starts one.
    pub fn progress(&mut self, label: impl Into<String>, percent: u8) {
        let label = label.into();
        if let Some(item) = self
            .items
            .iter_mut()
            .find(|i| i.text == label && matches!(i.kind, FeedbackKind::Progress(_)))
        {
            item.kind = FeedbackKind::Progress(percent);
        } else {
            self.items.push(FeedbackItem {
                kind: FeedbackKind::Progress(percent),
                text: label,
            });
        }
    }

    /// The current feedback items, oldest first.
    #[must_use]
    pub fn items(&self) -> &[FeedbackItem] {
        &self.items
    }

    /// Drop every item (e.g. once a batch of ops has settled).
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Render the queue as [`Node::Toast`] nodes (G7) — progress becomes a
    /// polite `Info` toast with the label + percentage.
    #[must_use]
    pub fn to_nodes(&self) -> Vec<Node> {
        self.items
            .iter()
            .map(|i| match i.kind {
                FeedbackKind::Info => toast_node(ToastLevel::Info, i.text.clone()),
                FeedbackKind::Success => toast_node(ToastLevel::Success, i.text.clone()),
                FeedbackKind::Warning => toast_node(ToastLevel::Warning, i.text.clone()),
                FeedbackKind::Error => toast_node(ToastLevel::Error, i.text.clone()),
                FeedbackKind::Progress(p) => {
                    toast_node(ToastLevel::Info, format!("{} {p}%", i.text))
                }
            })
            .collect()
    }
}

/// Compose a [`Feedback`] queue onto a base [`ViewTree`](Node) as toast
/// nodes (P6).
///
/// Every shell already renders [`Node::Toast`] (G1c), so this is how
/// notifications + progress reach every window from the one shared queue. A
/// `Pane` gets the toasts appended to its children; any other node is
/// wrapped in a `Pane` with the toasts after it. Empty feedback returns the
/// base unchanged.
#[must_use]
pub fn with_feedback(base: Node, feedback: &Feedback) -> Node {
    let toasts = feedback.to_nodes();
    if toasts.is_empty() {
        return base;
    }
    match base {
        Node::Pane {
            title,
            mut children,
        } => {
            children.extend(toasts);
            Node::Pane { title, children }
        }
        other => {
            let mut children = vec![other];
            children.extend(toasts);
            Node::Pane {
                title: String::new(),
                children,
            }
        }
    }
}

/// The kind of a [`Node`], for the type-level UI capability matrix (V1c).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// [`Node::Pane`].
    Pane,
    /// [`Node::Rows`].
    Rows,
    /// [`Node::Detail`].
    Detail,
    /// [`Node::Input`].
    Input,
    /// [`Node::Palette`].
    Palette,
    /// [`Node::Hints`].
    Hints,
    /// [`Node::Widget`].
    Widget,
    /// [`Node::Text`].
    Text,
    /// [`Node::Split`].
    Split,
    /// [`Node::Modal`].
    Modal,
    /// [`Node::Toast`].
    Toast,
}

impl Node {
    /// The semantic ARIA role for this node (V12a), so an embedder can
    /// emit screen-reader-navigable output. Derived per kind, like
    /// [`Self::kind`].
    #[must_use]
    pub const fn aria_role(&self) -> &'static str {
        match self {
            Self::Pane { .. } | Self::Widget { .. } => "region",
            Self::Rows { .. } => "list",
            Self::Detail { .. } | Self::Split { .. } => "group",
            Self::Modal { .. } => "dialog",
            Self::Toast { level, .. } => level.aria_role(),
            Self::Input { .. } => "textbox",
            Self::Palette { .. } => "listbox",
            Self::Hints { .. } => "status",
            Self::Text(_) => "note",
        }
    }

    /// The accessible label for this node, where it has a natural one
    /// (pane title, input label, widget name); `None` otherwise (V12a).
    #[must_use]
    pub fn aria_label(&self) -> Option<&str> {
        match self {
            Self::Pane { title, .. } | Self::Modal { title, .. } => Some(title),
            Self::Input { label, .. } => Some(label),
            Self::Widget { name, .. } => Some(name),
            _ => None,
        }
    }

    /// This node's [`NodeKind`].
    #[must_use]
    pub const fn kind(&self) -> NodeKind {
        match self {
            Self::Pane { .. } => NodeKind::Pane,
            Self::Rows { .. } => NodeKind::Rows,
            Self::Detail { .. } => NodeKind::Detail,
            Self::Input { .. } => NodeKind::Input,
            Self::Palette { .. } => NodeKind::Palette,
            Self::Hints { .. } => NodeKind::Hints,
            Self::Widget { .. } => NodeKind::Widget,
            Self::Text(_) => NodeKind::Text,
            Self::Split { .. } => NodeKind::Split,
            Self::Modal { .. } => NodeKind::Modal,
            Self::Toast { .. } => NodeKind::Toast,
        }
    }
}

/// Every node kind, in definition order (single source of truth).
pub const ALL_NODE_KINDS: &[NodeKind] = &[
    NodeKind::Pane,
    NodeKind::Rows,
    NodeKind::Detail,
    NodeKind::Input,
    NodeKind::Palette,
    NodeKind::Hints,
    NodeKind::Widget,
    NodeKind::Text,
    NodeKind::Split,
    NodeKind::Modal,
    NodeKind::Toast,
];

/// The floor every shell must render to host the launcher (I7 for UI):
/// a labelled region, a list, the which-key line, inert text.
pub const MINIMAL_NODE_KINDS: &[NodeKind] = &[
    NodeKind::Pane,
    NodeKind::Rows,
    NodeKind::Hints,
    NodeKind::Text,
];

/// Kinds the TUI renderer (`closure_tui::render_view`) handles — all of
/// them (its match is exhaustive, so this cannot silently drift: adding
/// a `NodeKind` without a render arm is a compile error).
pub const TUI_NODE_KINDS: &[NodeKind] = ALL_NODE_KINDS;

/// Kinds the web renderer (`closure_shell_web::render_view`) handles —
/// all of them, exhaustively.
pub const WEB_NODE_KINDS: &[NodeKind] = ALL_NODE_KINDS;

/// Kinds the GTK4 renderer (`closure_shell_gtk::widget_tree`) handles —
/// all of them, exhaustively (G3: its `match` over `Node` is total, so a
/// new kind is a compile error there too).
pub const GTK_NODE_KINDS: &[NodeKind] = ALL_NODE_KINDS;

/// Kinds the Qt6/QML renderer (`closure_shell_qt::qml_view`) handles —
/// all of them, exhaustively (G4).
pub const QT_NODE_KINDS: &[NodeKind] = ALL_NODE_KINDS;

/// The node kinds a shell does *not* render (`ALL_NODE_KINDS` minus
/// `kinds`). Empty for a complete renderer.
#[must_use]
pub fn missing_node_kinds(kinds: &[NodeKind]) -> Vec<NodeKind> {
    ALL_NODE_KINDS
        .iter()
        .copied()
        .filter(|k| !kinds.contains(k))
        .collect()
}

/// Render the shell × node-kind venn/diff table (code = single source of
/// truth; mirrors `closure shells`). `closure ui-matrix` prints it.
#[must_use]
pub fn ui_matrix_table() -> String {
    use std::fmt::Write as _;
    // `runs` is the column's honest status, not its completeness: GTK
    // and Qt map every node kind and neither is a program you can
    // start. A table of ticks with nothing to say so reads as though
    // they were peers of the shell you use all day.
    let shells: &[(&str, &[NodeKind], bool)] = &[
        ("MIN", MINIMAL_NODE_KINDS, false),
        ("TUI", TUI_NODE_KINDS, true),
        ("WEB", WEB_NODE_KINDS, false),
        ("GTK", GTK_NODE_KINDS, false),
        ("QT", QT_NODE_KINDS, false),
    ];
    let mut out =
        String::from("UI node-kind matrix (which shells render which ViewTree nodes)\n\n");
    let _ = write!(out, "{:<9}", "NodeKind");
    for (name, _, _) in shells {
        let _ = write!(out, " | {name}");
    }
    out.push('\n');
    for kind in ALL_NODE_KINDS {
        let _ = write!(out, "{:<9}", format!("{kind:?}"));
        for (_, set, _) in shells {
            let mark = if set.contains(kind) { " X " } else { "   " };
            let _ = write!(out, " | {mark}");
        }
        out.push('\n');
    }
    let mappings: Vec<&str> = shells
        .iter()
        .filter(|(name, _, runs)| !runs && *name != "MIN")
        .map(|(name, ..)| *name)
        .collect();
    out.push_str("\nLegend: X = renders this node kind. MIN = the floor (I7).\n");
    let _ = writeln!(
        out,
        "The shells you can start: gpui (the reference one) and TUI.\n\
         {} are a `ViewTree` mapping each — a function that returns a\n\
         widget tree, behind a feature gate CI does not build. Real\n\
         work, and not yet something you can run.",
        mappings.join(", ")
    );
    out
}

/// Which input surface the gpui shell is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Browsing/filtering the headline list.
    Browse,
    /// Typing the title of a new capture entry.
    Capture,
    /// Editing the selected headline's title.
    Rename,
    /// Typing the title of a new sibling after the selected headline.
    AddSibling,
    /// Slash command palette: fuzzy-pick a command (which-key list).
    Palette,
    /// Editing the selected headline's body in a multiline buffer
    /// (org-edit-special). Commit via [`App::commit_edit_body`].
    EditBody,
    /// Editing a `(key, value)` property on the selected headline.
    /// Commit via [`App::commit_property`].
    PropertyEdit,
    /// Editing the selected headline's tag list (space-separated).
    /// Commit via [`App::commit_tags`].
    TagsEdit,
}

/// Commands offered by the slash palette as `(display, canonical)`:
/// the launcher's which-key surface. The key hint shown beside each is
/// derived from the active mode's keymap (the single source of truth,
/// I4) via the canonical command name — never hardcoded here.
/// Palette commands as `(display, canonical, section, description)` (G6).
/// `section` groups them in the command palette; `description` is the
/// human one-liner shown beside the chord.
const PALETTE_COMMANDS: &[(&str, &str, &str, &str)] = &[
    (
        "first-file",
        "first-file",
        "Navigate",
        "Jump to the first headline",
    ),
    (
        "last-file",
        "last-file",
        "Navigate",
        "Jump to the last headline",
    ),
    (
        "half-page-down",
        "half-page-down",
        "Navigate",
        "Scroll down half a screen",
    ),
    (
        "half-page-up",
        "half-page-up",
        "Navigate",
        "Scroll up half a screen",
    ),
    (
        "jump-back",
        "jump-back",
        "Navigate",
        "Go back to where you were",
    ),
    (
        "jump-forward",
        "jump-forward",
        "Navigate",
        "Go forward again after jumping back",
    ),
    (
        "search",
        "search",
        "Navigate",
        "Filter the outline by headline title",
    ),
    (
        "search-headlines",
        "search-headlines",
        "Navigate",
        "Start a headline search from the outline",
    ),
    (
        "body-search",
        "body-search",
        "Navigate",
        "Find text inside the open note",
    ),
    (
        "backlinks",
        "backlinks",
        "Navigate",
        "Show what links to this headline",
    ),
    ("browse", "browse", "Navigate", "Go back to the outline"),
    (
        "recent-files",
        "recent-files",
        "Navigate",
        "Open a file you had open lately",
    ),
    (
        "find-file",
        "find-file",
        "Navigate",
        "Open a file by name, or make one that is not there yet",
    ),
    (
        "open-vault",
        "open-vault",
        "App",
        "Choose a different vault directory to work in",
    ),
    (
        "list-buffers",
        "list-buffers",
        "Navigate",
        "Switch between open buffers",
    ),
    (
        "next-buffer",
        "next-buffer",
        "Navigate",
        "Go to the next open buffer",
    ),
    (
        "prev-buffer",
        "prev-buffer",
        "Navigate",
        "Go to the previous open buffer",
    ),
    (
        "alternate-buffer",
        "alternate-buffer",
        "Navigate",
        "Swap back to the buffer before this one",
    ),
    (
        "close-buffer",
        "close-buffer",
        "Navigate",
        "Close this buffer, keeping unsaved text",
    ),
    (
        "close-buffer-force",
        "close-buffer-force",
        "Navigate",
        "Close this buffer and discard its edits",
    ),
    (
        "list-headlines",
        "list-headlines",
        "Navigate",
        "Pick a headline from anywhere in the vault",
    ),
    (
        "list-blocks",
        "list-blocks",
        "Navigate",
        "Pick a source block from anywhere in the vault",
    ),
    (
        "refile",
        "refile",
        "Edit",
        "Move this subtree under another headline",
    ),
    (
        "insert-link",
        "insert-link",
        "Edit",
        "Insert an org link, picking its type and destination",
    ),
    (
        "edit-body",
        "edit-body",
        "Edit",
        "Open this headline's body in the editor",
    ),
    (
        "edit-special",
        "edit-special",
        "Edit",
        "Open the source block under the cursor on its own",
    ),
    (
        "execute-block",
        "execute-block",
        "Edit",
        "Run the source block and keep its output",
    ),
    (
        "add-heading",
        "add-heading",
        "Edit",
        "Add a headline at the current level",
    ),
    (
        "promote",
        "promote",
        "Edit",
        "Move this headline one level out",
    ),
    (
        "demote",
        "demote",
        "Edit",
        "Move this headline one level in",
    ),
    (
        "move-subtree-up",
        "move-subtree-up",
        "Edit",
        "Move this subtree above its sibling",
    ),
    (
        "move-subtree-down",
        "move-subtree-down",
        "Edit",
        "Move this subtree below its sibling",
    ),
    (
        "archive",
        "archive",
        "Edit",
        "File this subtree away under the archive",
    ),
    (
        "paste-subtree",
        "paste-subtree",
        "Edit",
        "Paste the subtree you cut or copied",
    ),
    (
        "toggle-todo",
        "toggle-todo",
        "Edit",
        "Cycle this headline's TODO keyword forward",
    ),
    (
        "todo-back",
        "todo-back",
        "Edit",
        "Cycle this headline's TODO keyword backward",
    ),
    (
        "cycle-priority",
        "cycle-priority",
        "Edit",
        "Cycle this headline's priority cookie",
    ),
    (
        "priority-up",
        "priority-up",
        "Edit",
        "Raise this headline's priority one letter",
    ),
    (
        "priority-down",
        "priority-down",
        "Edit",
        "Lower this headline's priority one letter",
    ),
    (
        "toggle-checkbox",
        "toggle-checkbox",
        "Edit",
        "Tick or untick the checkbox on this line",
    ),
    (
        "edit-tags",
        "edit-tags",
        "Edit",
        "Edit this headline's tags",
    ),
    (
        "tag-picker",
        "tag-picker",
        "Edit",
        "Choose tags from the ones the vault already uses",
    ),
    (
        "edit-property",
        "edit-property",
        "Edit",
        "Add or change a property on this headline",
    ),
    (
        "schedule",
        "schedule",
        "Edit",
        "Set when you plan to start this",
    ),
    (
        "deadline",
        "deadline",
        "Edit",
        "Set when this has to be done",
    ),
    (
        "clock-in",
        "clock-in",
        "Edit",
        "Start counting time against this headline",
    ),
    ("clock-out", "clock-out", "Edit", "Stop counting time"),
    (
        "clock-goto",
        "clock-goto",
        "Edit",
        "Jump to the headline the clock is running on",
    ),
    (
        "clock-cancel",
        "clock-cancel",
        "Edit",
        "Throw away the running clock",
    ),
    ("undo", "undo", "Edit", "Undo the last change"),
    ("redo", "redo", "Edit", "Redo the change you just undid"),
    (
        "undo-history",
        "undo-history",
        "Edit",
        "Pick a point in this note's history to jump to",
    ),
    (
        "save-buffer",
        "save-buffer",
        "Edit",
        "Write the open buffer without closing it",
    ),
    (
        "toggle-file-view",
        "toggle-file-view",
        "View",
        "Switch between the outline and the whole file as one buffer",
    ),
    (
        "toggle-inline-images",
        "toggle-inline-images",
        "View",
        "Show or hide the pictures a note links to",
    ),
    (
        "preview-diagrams",
        "preview-diagrams",
        "View",
        "Draw the mermaid and LaTeX blocks in this buffer",
    ),
    (
        "toggle-trace",
        "toggle-trace",
        "View",
        "Time every keypress and log the slow ones",
    ),
    (
        "build-info",
        "build-info",
        "App",
        "Say which commit this binary was built from",
    ),
    (
        "open-config",
        "open-config",
        "App",
        "Open config.org, writing a default one if there is none",
    ),
    (
        "assistant-setup",
        "assistant-setup",
        "App",
        "Set up the assistant: provider, model, key variable, endpoint, tools",
    ),
    (
        "toggle-tree",
        "toggle-tree",
        "View",
        "Show or hide the headline tree beside the editor",
    ),
    (
        "toggle-wrap",
        "toggle-wrap",
        "View",
        "Fold long lines at the pane edge, or scroll them",
    ),
    (
        "toggle-which-key",
        "toggle-which-key",
        "View",
        "Show or hide the keybinding panel",
    ),
    (
        "agenda",
        "agenda",
        "View",
        "Everything scheduled or due, by date",
    ),
    ("journal", "journal", "View", "The vault's journal entries"),
    (
        "graph",
        "graph",
        "View",
        "Hubs, orphans and dead links in the vault",
    ),
    (
        "db-view",
        "db-view",
        "View",
        "Every headline in the vault as one list",
    ),
    (
        "messages",
        "messages",
        "View",
        "The log of everything the shell has said",
    ),
    (
        "dismiss-notifications",
        "dismiss-notifications",
        "View",
        "Clear the notifications on screen",
    ),
    ("palette", "palette", "App", "Run any command by name"),
    (
        "ex-command",
        "ex-command",
        "App",
        "Open the `:` command line",
    ),
    ("llm", "llm", "App", "Ask the assistant about this vault"),
    (
        "toggle-llm-render",
        "toggle-llm-render",
        "App",
        "Show the assistant's replies as org or as plain text",
    ),
    (
        "sniffer",
        "sniffer",
        "App",
        "Watch what the app talks to over the network",
    ),
    ("cron", "cron", "App", "Jobs the vault runs on a schedule"),
    (
        "sync",
        "sync",
        "Sync",
        "Pair with a peer and exchange changes",
    ),
    (
        "conflicts",
        "conflicts",
        "Sync",
        "Headlines two peers changed at once",
    ),
    (
        "resolve-ours",
        "resolve-ours",
        "Sync",
        "Keep this side's version of the conflict",
    ),
    (
        "resolve-theirs",
        "resolve-theirs",
        "Sync",
        "Keep the peer's version of the conflict",
    ),
    (
        "allow-flow",
        "allow-flow",
        "App",
        "Let this network flow through",
    ),
    ("block-flow", "block-flow", "App", "Stop this network flow"),
    (
        "debug-flow",
        "debug-flow",
        "App",
        "Show what was recorded about this flow and why it was decided",
    ),
    (
        "reload-flows",
        "reload-flows",
        "App",
        "Re-read the captured flows from network.org",
    ),
    ("next-file", "next-file", "Navigate", "Go to the next file"),
    (
        "prev-file",
        "prev-file",
        "Navigate",
        "Go to the previous file",
    ),
    ("open", "open-file", "Navigate", "Open the selected file"),
    ("capture", "capture", "Edit", "Capture a new entry"),
    (
        "add-sibling",
        "add-sibling",
        "Edit",
        "Add a sibling headline",
    ),
    (
        "manual",
        "manual",
        "App",
        "Every command and its keys, generated from the registry",
    ),
    (
        "describe-key",
        "describe-key",
        "App",
        "Press a key and be told what it runs",
    ),
    (
        "toggle-rail",
        "toggle-rail",
        "View",
        "Collapse the left rail to its icons, or open it again",
    ),
    (
        "trust-language",
        "trust-language",
        "App",
        "Let this vault run one language, in your config not the vault's",
    ),
    (
        "toggle-line-comment",
        "toggle-line-comment",
        "Edit",
        "Comment or uncomment the line, or the selection",
    ),
    (
        "add-sibling-above",
        "add-heading-above",
        "Edit",
        "Add a sibling headline above this one",
    ),
    (
        "add-todo-sibling",
        "add-todo-heading",
        "Edit",
        "Add a sibling headline as a TODO",
    ),
    (
        "add-child",
        "add-child-heading",
        "Edit",
        "Add a child headline",
    ),
    (
        "add-todo-child",
        "add-todo-child-heading",
        "Edit",
        "Add a child headline as a TODO",
    ),
    ("rename", "rename", "Edit", "Rename the headline"),
    ("delete", "delete", "Edit", "Delete the headline"),
    (
        "toggle-mark",
        "toggle-mark",
        "Edit",
        "Mark this headline, or take the mark off",
    ),
    ("unmark-all", "unmark-all", "Edit", "Clear every mark"),
    (
        "delete-marked",
        "delete-marked",
        "Edit",
        "Cut the marked headlines, or the one under the cursor",
    ),
    (
        "next-input-mode",
        "next-input-mode",
        "Mode",
        "Switch the input mode",
    ),
    (
        "set-input-mode",
        "set-input-mode",
        "Mode",
        "Switch straight to a named keymap: `:set-input-mode vim`",
    ),
    (
        "fold",
        "toggle-fold",
        "Navigate",
        "Fold or unfold the selected subtree",
    ),
    (
        "sync-export",
        "sync-export",
        "Sync",
        "Leave a bundle in the shared folder (sync_dir)",
    ),
    (
        "sync-import",
        "sync-import",
        "Sync",
        "Pick up bundles peers left in the shared folder",
    ),
    ("zoom-in", "zoom-in", "View", "Scale the text up one step"),
    (
        "zoom-out",
        "zoom-out",
        "View",
        "Scale the text down one step",
    ),
    ("zoom-reset", "zoom-reset", "View", "Back to 100%"),
    (
        "reload",
        "reload-shell",
        "App",
        "Start over: re-read the vault and config.org from disk",
    ),
    ("quit", "quit", "App", "Quit closure"),
];

/// Section order for the command palette (G6); sections render in this
/// order, empty ones dropped.
const PALETTE_SECTIONS: &[&str] = &["Navigate", "Edit", "View", "Mode", "Sync", "App"];

/// A command entry in the [`command_palette`] (G6): a label + human
/// description + its actionable chord.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntry {
    /// Display label.
    pub label: String,
    /// Human one-line description.
    pub description: String,
    /// The command + its chord.
    pub action: Action,
}

/// A titled group of palette entries (G6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteSection {
    /// Section heading.
    pub title: String,
    /// Entries in fuzzy-ranked order.
    pub items: Vec<PaletteEntry>,
}

/// The polished command palette (G6).
///
/// Every command grouped into sections, fuzzy-filtered + ranked by
/// `query`, each entry carrying a description + its chord in `mode`. Empty
/// sections are dropped. One hermetic source every GUI renders.
/// M-x rule (I4): every command bound in the mode's keymap appears —
/// curated entries keep their sections/descriptions, the rest land in
/// a final "Command" section labelled by their canonical name.
#[must_use]
pub fn command_palette(query: &str, mode: closure_config::InputMode) -> Vec<PaletteSection> {
    command_palette_with_history(query, mode, &[])
}

/// [`command_palette`], with the commands this session has already run
/// (most recent first) suggested in a `Recent` section at the top.
///
/// The curated sections are in a fixed order that is the same for
/// everybody forever, so the command you ran four times this hour sat
/// exactly as far down as the one you have never run. Recency is the
/// cheapest useful signal a launcher has, and every launcher worth
/// using leans on it.
///
/// It is a shortcut, not a move: a suggested command still appears in
/// the section it belongs to, because someone who knows where `rename`
/// lives must still find it there. The section is filtered by the same
/// query as the rest — a suggestion that ignores what you typed is
/// noise — and disappears when nothing in it matches.
#[must_use]
pub fn command_palette_with_history(
    query: &str,
    mode: closure_config::InputMode,
    recent: &[String],
) -> Vec<PaletteSection> {
    palette_in_keymap(query, &closure_input::keymap_with(mode, &[]), recent)
}

/// The palette built against an explicit keymap — what the app calls,
/// so a rebound chord shows up here the moment it shows up under your
/// fingers.
#[must_use]
pub fn palette_in_keymap(
    query: &str,
    keys: &[(String, String)],
    recent: &[String],
) -> Vec<PaletteSection> {
    let mut sections: Vec<PaletteSection> = PALETTE_SECTIONS
        .iter()
        .filter_map(|section| {
            let mut scored: Vec<(u32, PaletteEntry)> = PALETTE_COMMANDS
                .iter()
                .filter(|(.., sec, _)| sec == section)
                .filter_map(|(label, canonical, _, desc)| {
                    // Doom's `orderless`: `add sibling` finds
                    // `add-sibling` without you having to know where
                    // the hyphen goes.
                    // The label *or* the canonical name: a curated
                    // entry reads as "toggle wrap" and is called
                    // `toggle-wrap`, and somebody who knows the command
                    // name — a person with muscle memory, or an LLM
                    // calling it as a tool — types the name.
                    let score = if query.is_empty() {
                        Some(0)
                    } else {
                        closure_query::orderless_score(query, label)
                            .max(closure_query::orderless_score(query, canonical))
                    }?;
                    let action = Action::in_keymap(keys, *canonical)
                        .unwrap_or_else(|| Action::unbound(*canonical));
                    Some((
                        score,
                        PaletteEntry {
                            label: (*label).to_owned(),
                            description: (*desc).to_owned(),
                            action,
                        },
                    ))
                })
                .collect();
            if scored.is_empty() {
                return None;
            }
            scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
            Some(PaletteSection {
                title: (*section).to_owned(),
                items: scored.into_iter().map(|(_, e)| e).collect(),
            })
        })
        .collect();
    let curated: std::collections::BTreeSet<&str> =
        PALETTE_COMMANDS.iter().map(|(_, c, ..)| *c).collect();
    let mut rest: std::collections::BTreeSet<&str> = keys
        .iter()
        .map(|(_, cmd)| cmd.as_str())
        .filter(|cmd| !curated.contains(cmd))
        .collect();
    let mut scored: Vec<(u32, PaletteEntry)> = Vec::new();
    for cmd in std::mem::take(&mut rest) {
        let score = if query.is_empty() {
            Some(0)
        } else {
            closure_query::orderless_score(query, cmd)
        };
        if let Some(score) = score
            && let Some(action) = Action::in_keymap(keys, cmd)
        {
            scored.push((
                score,
                PaletteEntry {
                    label: cmd.to_owned(),
                    description: format!("Run {cmd}"),
                    action,
                },
            ));
        }
    }
    if !scored.is_empty() {
        scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
        sections.push(PaletteSection {
            title: "Command".to_owned(),
            items: scored.into_iter().map(|(_, e)| e).collect(),
        });
    }
    // History order, not score order: the point of the section is that
    // it answers "the thing I just did" before you have finished
    // typing, and re-ranking it by the filter would put a stranger
    // above the command you ran ten seconds ago.
    let suggestions: Vec<PaletteEntry> = recent
        .iter()
        .filter(|cmd| query.is_empty() || closure_query::orderless_score(query, cmd).is_some())
        // A command with no chord in this mode still belongs in the
        // palette — that is what the palette is for. A name that is
        // not a command at all does not: history is written by what
        // ran, but a vault carried between versions can hold a name
        // this build no longer has.
        .filter(|cmd| command_exists(cmd))
        .map(|cmd| palette_entry_for(cmd, keys))
        .collect();
    if !suggestions.is_empty() {
        // Promotion *moves* a command. Adding the section without
        // taking its members out of the sections below listed the thing
        // you had just run twice, with the same label and the same
        // chord — a list offering the same command twice is asking you
        // to work out how the two entries differ. A section left empty
        // by the move goes with it; a heading with nothing under it is
        // furniture.
        let promoted: std::collections::BTreeSet<&str> =
            suggestions.iter().map(|e| e.action.command()).collect();
        for section in &mut sections {
            section
                .items
                .retain(|e| !promoted.contains(e.action.command()));
        }
        sections.retain(|s| !s.items.is_empty());
        sections.insert(
            0,
            PaletteSection {
                title: "Recent".to_owned(),
                items: suggestions,
            },
        );
    }
    sections
}

/// The palette entry for one canonical command: its curated label and
/// description when it has them, its own name otherwise. `None` when
/// this mode cannot run it.
fn palette_entry_for(cmd: &str, keys: &[(String, String)]) -> PaletteEntry {
    let action = Action::in_keymap(keys, cmd).unwrap_or_else(|| Action::unbound(cmd));
    let curated = PALETTE_COMMANDS
        .iter()
        .find(|(_, canonical, ..)| *canonical == cmd);
    match curated {
        Some((label, _, _, desc)) => PaletteEntry {
            label: (*label).to_owned(),
            description: (*desc).to_owned(),
            action,
        },
        None => PaletteEntry {
            label: cmd.to_owned(),
            description: format!("Run {cmd}"),
            action,
        },
    }
}

/// Serialise a [`command_palette`] to a deterministic text snapshot (G6) —
/// golden-testable, and the form a shell can render verbatim.
#[must_use]
pub fn serialize_palette(sections: &[PaletteSection]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for s in sections {
        let _ = writeln!(out, "SECTION {}", s.title);
        for e in &s.items {
            let _ = writeln!(
                out,
                "  [{}] {} — {}",
                e.action.chord(),
                e.label,
                e.description
            );
        }
    }
    out
}

/// The contiguous run of table lines around line `line`, as a line
/// range, or `None` when that line is not part of a table.
///
/// "Table" is what org means by it: consecutive lines whose first
/// non-space character is a pipe. A blank line — or any prose —
/// separates two tables rather than joining them.
#[must_use]
pub fn table_bounds(text: &str, line: usize) -> Option<std::ops::Range<usize>> {
    let lines: Vec<&str> = text.lines().collect();
    let is_row = |l: &str| l.trim_start().starts_with('|');
    if !lines.get(line).is_some_and(|l| is_row(l)) {
        return None;
    }
    let mut start = line;
    while start > 0 && is_row(lines[start - 1]) {
        start -= 1;
    }
    let mut end = line + 1;
    while end < lines.len() && is_row(lines[end]) {
        end += 1;
    }
    Some(start..end)
}

/// Realign an org table: every column padded to its widest cell, and
/// every `|---+---|` rule redrawn to match.
///
/// This is the feature that makes plain-text tables bearable, and the
/// reason TAB in org does more than move. Cells are measured in
/// *characters*, so a non-ASCII cell still lines up. Idempotent —
/// pressing TAB twice must not keep growing the table — and it never
/// drops a cell from a half-typed row, only pads it out.
#[must_use]
pub fn align_table(table: &str) -> String {
    /// Split a row into its cells, dropping the outer pipes.
    fn cells(line: &str) -> Vec<&str> {
        let t = line.trim();
        let inner = t
            .strip_prefix('|')
            .map_or(t, |s| s.strip_suffix('|').unwrap_or(s));
        inner.split('|').map(str::trim).collect()
    }
    /// Does this cell read as a number? org's `%g`-ish test: a figure,
    /// with the punctuation figures come with.
    fn numeric(cell: &str) -> bool {
        let c = cell.trim();
        !c.is_empty()
            && c.chars().any(|c| c.is_ascii_digit())
            && c.chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '+' | '%' | 'e' | 'E'))
    }
    // One definition of a rule, shared with the table verbs: a second
    // copy here is how an empty row — all pipes and spaces — was
    // redrawn as a horizontal line by the realign that runs after
    // every edit, so inserting a row appeared to insert a rule.
    let is_rule = is_rule_row;
    let rows: Vec<&str> = table.lines().collect();
    if rows.is_empty() {
        return table.to_owned();
    }
    // Widths come from the content rows only; a rule has no content to
    // measure and would otherwise dictate the width.
    let mut widths: Vec<usize> = Vec::new();
    for row in rows.iter().filter(|r| !is_rule(r)) {
        for (i, cell) in cells(row).iter().enumerate() {
            let w = cell.chars().count();
            if i < widths.len() {
                widths[i] = widths[i].max(w);
            } else {
                widths.push(w);
            }
        }
    }
    // org right-aligns a column of figures so it lines up on its last
    // digit. The header is prose in every table anybody writes, and an
    // empty cell says nothing, so neither gets a vote — the question is
    // whether the cells that *have* content are numbers.
    let body_start = usize::from(rows.first().is_some_and(|r| !is_rule(r)));
    let right: Vec<bool> = widths
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let mut seen = 0_usize;
            let mut nums = 0_usize;
            for row in rows.iter().skip(body_start).filter(|r| !is_rule(r)) {
                let cell = cells(row).get(i).copied().unwrap_or("").trim().to_owned();
                if !cell.is_empty() {
                    seen += 1;
                    nums += usize::from(numeric(&cell));
                }
            }
            seen > 0 && nums * 2 > seen
        })
        .collect();
    let mut out = String::with_capacity(table.len());
    for row in rows {
        if is_rule(row) {
            out.push('|');
            for (i, w) in widths.iter().enumerate() {
                if i > 0 {
                    out.push('+');
                }
                for _ in 0..w + 2 {
                    out.push('-');
                }
            }
            out.push('|');
        } else {
            let row_cells = cells(row);
            out.push('|');
            for (i, w) in widths.iter().enumerate() {
                let cell = row_cells.get(i).copied().unwrap_or("");
                let pad = w.saturating_sub(cell.chars().count());
                out.push(' ');
                if right.get(i).copied().unwrap_or(false) {
                    for _ in 0..pad {
                        out.push(' ');
                    }
                }
                out.push_str(cell);
                if !right.get(i).copied().unwrap_or(false) {
                    for _ in 0..pad {
                        out.push(' ');
                    }
                }
                out.push_str(" |");
            }
        }
        out.push('\n');
    }
    out
}

/// Byte offset of the next cell's content on `row`, from `at`.
///
/// `None` when there is no next cell — at the last one, or off a
/// table row entirely; the caller decides whether that means wrapping
/// to the following row. The offset lands on the cell's *content*,
/// past the padding, because that is where you want to start typing.
#[must_use]
pub fn next_table_cell(row: &str, at: usize) -> Option<usize> {
    if !row.trim_start().starts_with('|') || at >= row.len() {
        return None;
    }
    let pipe = row[at..].find('|').map(|off| at + off)?;
    let rest = &row[pipe + 1..];
    // The trailing pipe of the row is not a cell boundary.
    if rest.trim().is_empty() {
        return None;
    }
    let lead = rest.len() - rest.trim_start().len();
    Some(pipe + 1 + lead)
}

/// An org link that points at a picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageLink {
    /// Byte range of the whole `[[…]]` in the text it was found in.
    pub range: std::ops::Range<usize>,
    /// The target as written, `file:` stripped — still relative to the
    /// vault if that is how it was written.
    pub path: String,
    /// The `[[target][description]]` half, when there is one; org calls
    /// it the description and a window can use it as alt text.
    pub description: Option<String>,
}

/// The file extensions a picture link is allowed to have.
///
/// A whitelist rather than "anything with an extension": `[[file:
/// notes.org]]` is a link to a document and painting it as a broken
/// image would be worse than leaving it as text.
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"];

/// Whether `path` names a file a shell could paint.
fn is_image_path(path: &str) -> bool {
    let Some((_, ext)) = path.rsplit_once('.') else {
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    IMAGE_EXTENSIONS.contains(&ext.as_str())
}

/// Every image link in `text`, in order.
///
/// Org has no separate image syntax: an image *is* a file link whose
/// target happens to be a picture, which is why nothing new goes in the
/// file format for this. Both spellings org accepts are read —
/// `[[file:x.png]]` and the bare `[[./x.png]]`.
#[must_use]
pub fn image_links(text: &str) -> Vec<ImageLink> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(open) = text[i..].find("[[").map(|off| i + off) {
        let Some(close) = text[open..].find("]]").map(|off| open + off) else {
            break;
        };
        let inner = &text[open + 2..close];
        let (target, description) = match inner.split_once("][") {
            Some((t, d)) => (t, Some(d.to_owned())),
            None => (inner, None),
        };
        let path = target.strip_prefix("file:").unwrap_or(target);
        if is_image_path(path) && !path.contains("://") {
            out.push(ImageLink {
                range: open..close + 2,
                path: path.to_owned(),
                description,
            });
        }
        i = close + 2;
    }
    out
}

/// A fresh file name for an image arriving on the clipboard.
///
/// A ULID, so two pastes in the same second are two files and the
/// directory sorts in the order they were pasted.
#[must_use]
pub fn asset_file_name(extension: &str) -> String {
    format!(
        "{}.{}",
        closure_core::BlockId::fresh().as_str(),
        extension.to_ascii_lowercase()
    )
}

/// Byte offset of the previous cell's content on `row`, from `at` —
/// `S-TAB`, the mirror of [`next_table_cell`].
#[must_use]
pub fn table_previous_cell(row: &str, at: usize) -> Option<usize> {
    if !row.trim_start().starts_with('|') {
        return None;
    }
    let at = at.min(row.len());
    // Every cell start on the row, in order; the answer is the last one
    // before the cursor's own.
    let mut starts = Vec::new();
    let mut cursor = 0usize;
    while let Some(next) = next_table_cell(row, cursor) {
        starts.push(next);
        cursor = next;
    }
    let first = row.find('|').map(|p| {
        let rest = &row[p + 1..];
        p + 1 + (rest.len() - rest.trim_start().len())
    })?;
    starts.insert(0, first);
    starts.iter().rev().find(|&&s| s < at).copied()
}

/// Byte offset where column `n`'s content starts on a table row,
/// clamped to the last cell there is.
fn cell_start(row: &str, n: usize) -> usize {
    let Some(pipe) = row.find('|') else {
        return 0;
    };
    let rest = &row[pipe + 1..];
    let mut at = pipe + 1 + (rest.len() - rest.trim_start().len());
    for _ in 0..n {
        match next_table_cell(row, at) {
            Some(next) => at = next,
            None => break,
        }
    }
    at
}

/// Which column (0-based) byte offset `at` sits in on a table row.
///
/// `None` off a table row. Counted by the pipes before the cursor,
/// which is what org means by "the column you are in" — the cell it
/// would move if you pressed `M-<right>`.
#[must_use]
pub fn table_column_at(row: &str, at: usize) -> Option<usize> {
    if !row.trim_start().starts_with('|') {
        return None;
    }
    let at = at.min(row.len());
    let before = row[..at].matches('|').count();
    Some(before.saturating_sub(1))
}

/// The cells of one org table row, outer pipes dropped and each
/// trimmed. A rule row (`|---+---|`) yields its dashes like any other,
/// so callers check [`is_rule_row`] first.
fn row_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let inner = t
        .strip_prefix('|')
        .map_or(t, |s| s.strip_suffix('|').unwrap_or(s));
    // A rule separates its columns with `+`, not `|`. Splitting one on
    // `|` alone reported a three-column table as having one column, so
    // every column verb rebuilt the rule at the wrong width: inserting
    // a column left `|---+---|` under a four-column table, and on
    // screen the rule stopped lining up with the rows it ruled.
    let sep = if is_rule_row(line) { '+' } else { '|' };
    inner.split(sep).map(|c| c.trim().to_owned()).collect()
}

/// Whether the line is an org table rule (`|---+---|`) rather than a
/// row of content.
fn is_rule_row(line: &str) -> bool {
    let t = line.trim();
    // A rule has to *have* a dash. Without that clause an empty row —
    // `|  |  |  |`, which is exactly what inserting a row makes — is
    // all pipes and spaces and so passed for a rule, and the realign
    // then redrew it as one: `M-S-<down>` appeared to insert a second
    // horizontal line instead of the line you were about to type in.
    t.starts_with('|') && t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | '+' | ' '))
}

/// Rebuild `text` with the table containing `line` replaced by `rows`,
/// realigned — the shared tail of every table edit below.
fn splice_table(text: &str, line: usize, rows: &[String]) -> String {
    let Some(span) = table_bounds(text, line) else {
        return text.to_owned();
    };
    let all: Vec<&str> = text.lines().collect();
    let mut out = String::with_capacity(text.len());
    for l in &all[..span.start] {
        out.push_str(l);
        out.push('\n');
    }
    if !rows.is_empty() {
        out.push_str(&align_table(&rows.join("\n")));
    }
    for l in &all[span.end..] {
        out.push_str(l);
        out.push('\n');
    }
    out
}

/// Apply `f` to every row of the table containing `line`, as cells.
/// `None` when `line` is not in a table, or when `f` refuses.
fn map_table<F>(text: &str, line: usize, f: F) -> Option<String>
where
    F: Fn(&mut Vec<String>) -> bool,
{
    let span = table_bounds(text, line)?;
    let all: Vec<&str> = text.lines().collect();
    let mut rows: Vec<String> = Vec::with_capacity(span.len());
    for l in all.iter().take(span.end).skip(span.start) {
        let mut cells = row_cells(l);
        if !f(&mut cells) {
            return None;
        }
        if is_rule_row(l) {
            // A rule is redrawn from the column widths by `align_table`,
            // so it only has to keep its *count* of columns right.
            rows.push(format!("|{}|", vec!["---"; cells.len()].join("+")));
        } else {
            rows.push(format!("| {} |", cells.join(" | ")));
        }
    }
    Some(splice_table(text, line, &rows))
}

/// `M-<left>` / `M-<right>` in a table: swap the column at `col` with
/// its neighbour, in every row. `None` at the edge, or off a table.
#[must_use]
pub fn table_move_column(text: &str, line: usize, col: usize, right: bool) -> Option<String> {
    let other = if right {
        col.checked_add(1)?
    } else {
        col.checked_sub(1)?
    };
    let width = table_bounds(text, line)
        .and_then(|span| text.lines().nth(span.start).map(|l| row_cells(l).len()))?;
    if col >= width || other >= width {
        return None;
    }
    map_table(text, line, |cells| {
        // A short row is padded rather than refused: a half-typed table
        // is still a table.
        while cells.len() <= col.max(other) {
            cells.push(String::new());
        }
        cells.swap(col, other);
        true
    })
}

/// `M-S-<right>` / `M-S-<left>`: insert an empty column before `col`,
/// or delete it, in every row.
#[must_use]
pub fn table_insert_column(text: &str, line: usize, col: usize) -> Option<String> {
    table_bounds(text, line)?;
    map_table(text, line, |cells| {
        let at = col.min(cells.len());
        cells.insert(at, String::new());
        true
    })
}

/// See [`table_insert_column`]. Refuses to delete the last column —
/// a table with no columns is not a table.
#[must_use]
pub fn table_delete_column(text: &str, line: usize, col: usize) -> Option<String> {
    let width = table_bounds(text, line)
        .and_then(|span| text.lines().nth(span.start).map(|l| row_cells(l).len()))?;
    if width <= 1 || col >= width {
        return None;
    }
    map_table(text, line, |cells| {
        if col < cells.len() {
            cells.remove(col);
        }
        true
    })
}

/// `M-<up>` / `M-<down>`: swap the row at `line` with its neighbour
/// inside the same table. `None` at either end.
#[must_use]
pub fn table_move_row(text: &str, line: usize, down: bool) -> Option<String> {
    let span = table_bounds(text, line)?;
    let other = if down {
        line.checked_add(1)?
    } else {
        line.checked_sub(1)?
    };
    if other < span.start || other >= span.end {
        return None;
    }
    let all: Vec<&str> = text.lines().collect();
    let mut rows: Vec<String> = all[span.clone()].iter().map(|l| (*l).to_owned()).collect();
    rows.swap(line - span.start, other - span.start);
    Some(splice_table(text, line, &rows))
}

/// `M-S-<down>`: an empty row above the one at `line`.
#[must_use]
pub fn table_insert_row(text: &str, line: usize) -> Option<String> {
    let span = table_bounds(text, line)?;
    let all: Vec<&str> = text.lines().collect();
    let width = row_cells(all[span.start]).len().max(1);
    let mut rows: Vec<String> = all[span.clone()].iter().map(|l| (*l).to_owned()).collect();
    // `" |".repeat(n)` already ends in a pipe, so the closing one made
    // an empty row one column wider than the table it went into.
    rows.insert(line - span.start, format!("|{}", " |".repeat(width)));
    Some(splice_table(text, line, &rows))
}

/// `M-S-<up>`: take the row at `line` out. A table whose last row goes
/// is gone, rather than left half there.
#[must_use]
pub fn table_kill_row(text: &str, line: usize) -> Option<String> {
    let span = table_bounds(text, line)?;
    let all: Vec<&str> = text.lines().collect();
    let mut rows: Vec<String> = all[span.clone()].iter().map(|l| (*l).to_owned()).collect();
    rows.remove(line - span.start);
    Some(splice_table(text, line, &rows))
}

/// `C-c -`: rule a line under the row at `line`.
#[must_use]
pub fn table_insert_hline(text: &str, line: usize) -> Option<String> {
    let span = table_bounds(text, line)?;
    let all: Vec<&str> = text.lines().collect();
    let width = row_cells(all[span.start]).len().max(1);
    let mut rows: Vec<String> = all[span.clone()].iter().map(|l| (*l).to_owned()).collect();
    rows.insert(
        line - span.start + 1,
        format!("|{}|", vec!["---"; width].join("+")),
    );
    Some(splice_table(text, line, &rows))
}

/// The byte range of the source-block *content* enclosing `at`, and
/// the block's language — the org-edit-special lookup.
///
/// `at` may sit anywhere in the block including on either fence line,
/// because point-on-`#+BEGIN_SRC` is how the command is usually
/// reached. The returned range covers the lines between the fences and
/// nothing else, so it can be replaced wholesale without disturbing
/// them. An unterminated block is not a block: a half-typed fence must
/// not swallow the rest of the buffer.
#[must_use]
pub fn enclosing_src_block(text: &str, at: usize) -> Option<(std::ops::Range<usize>, String)> {
    if at > text.len() {
        return None;
    }
    let mut open: Option<(usize, usize, String)> = None; // (fence start, content start, lang)
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let lower = line.trim_start().to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("#+begin_src") {
            let lang = rest.split_whitespace().next().unwrap_or("").to_owned();
            open = Some((offset, offset + line.len(), lang));
        } else if lower.starts_with("#+end_src")
            && let Some((fence_start, content_start, lang)) = open.take()
            && (fence_start..offset + line.len()).contains(&at)
        {
            return Some((content_start..offset, lang));
        }
        offset += line.len();
    }
    None
}

/// Whether `line` already carries `token` as a line comment.
fn is_commented(line: &str, token: &str) -> bool {
    let body = line.trim_start();
    // `#+begin_src` starts with `#` and is org *syntax*, not a comment.
    // Counting it as one made the first `gcc` on a fence line take the
    // `#` off and leave `+begin_src javascript` behind.
    body.starts_with(token) && !(token == "#" && body.starts_with("#+"))
}

/// Comment lines `first..=last` of `text`, or uncomment them when all
/// of them already carry the comment.
///
/// Returns the new text, the token used, and whether it came *off* —
/// or `None` when the range names nothing. Pure and shell-agnostic,
/// because both shells have the same chord and there is one right
/// answer for a given buffer.
///
/// The token comes from the enclosing `#+begin_src` block's language,
/// and from org itself outside one — which is the whole reason the
/// chord beats typing `#` yourself: in an org file the right comment
/// changes every few lines.
#[must_use]
pub fn toggle_comment_lines(text: &str, first: usize, last: usize) -> Option<(String, &str, bool)> {
    let lines: Vec<&str> = text.split('\n').collect();
    if first >= lines.len() {
        return None;
    }
    let last = last.min(lines.len() - 1);
    // Which language, asked at the first line rather than per line: a
    // selection that straddles a fence is one comment style, and the
    // one the user was looking at when they pressed it.
    let start: usize = lines[..first].iter().map(|l| l.len() + 1).sum();
    // Strictly inside: `enclosing_src_block` counts the fences as part
    // of the block, which is how `edit-special` is reached with the
    // point on one — but `#+begin_src javascript` is org's own syntax,
    // and `// #+begin_src` is neither language.
    let inside = enclosing_src_block(text, start).filter(|(content, _)| content.contains(&start));
    let token = match inside {
        // Inside a block, the block's language decides — and a
        // language with no line comment gets *no* comment. JSON, HTML
        // and Markdown have no prefix that comments one line, and
        // `# "key": 1,` is not commented JSON, it is broken JSON.
        // Falling back to org's `#` there corrupted the block.
        Some((_, lang)) => closure_tree_sitter::line_comment(&lang)?,
        // Outside one, org's own — which is also where an unknown
        // language lands, since it is still text in an org file.
        None => "#",
    };

    let body = &lines[first..=last];
    let interesting: Vec<&&str> = body.iter().filter(|l| !l.trim().is_empty()).collect();
    // Uncomment only when *every* line is commented — a selection with
    // one bare line in it is one you meant to comment.
    let off = !interesting.is_empty() && interesting.iter().all(|l| is_commented(l, token));
    // The shallowest line decides the column, so the block keeps its
    // shape instead of every line being flattened to the left.
    let indent = interesting
        .iter()
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    out.extend(lines[..first].iter().map(|l| (*l).to_owned()));
    for line in body {
        if line.trim().is_empty() {
            out.push((*line).to_owned());
        } else if off {
            let cut = line.find(token).unwrap_or(0);
            let rest = &line[cut + token.len()..];
            out.push(format!(
                "{}{}",
                &line[..cut],
                rest.strip_prefix(' ').unwrap_or(rest)
            ));
        } else {
            let at = indent.min(line.len());
            out.push(format!("{}{token} {}", &line[..at], &line[at..]));
        }
    }
    out.extend(lines[last + 1..].iter().map(|l| (*l).to_owned()));
    Some((out.join("\n"), token, off))
}

/// One entry of the Notion-style "/" block menu.
///
/// The insertion is *org text*, not a private block model: whatever is
/// picked is what lands in the file, so a block inserted in the GUI is
/// a block Emacs reads (I1). `cursor` is where the caret goes
/// afterwards, as a char offset into `text` — inside the code block,
/// after the checkbox, between the link brackets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockTemplate {
    /// Menu label.
    pub label: &'static str,
    /// The org text inserted at the cursor.
    pub text: &'static str,
    /// Caret position afterwards, in chars from the start of `text`.
    pub cursor: usize,
}

/// The block templates whose labels fuzzy-match `query`, best first;
/// an empty query lists all of them, and no match lists none.
#[must_use]
pub fn block_templates(query: &str) -> Vec<BlockTemplate> {
    // `cursor` counts chars, so each entry is written with its caret
    // target spelled out rather than a magic number.
    const fn t(label: &'static str, text: &'static str, cursor: usize) -> BlockTemplate {
        BlockTemplate {
            label,
            text,
            cursor,
        }
    }
    let all = [
        t("Heading", "** ", 3),
        t("To-do", "- [ ] ", 6),
        t("Done", "- [X] ", 6),
        t("Bullet", "- ", 2),
        t("Numbered", "1. ", 3),
        // Caret on the empty middle line of the block.
        t("Code", "#+BEGIN_SRC sh\n\n#+END_SRC", 15),
        t("Quote", "#+BEGIN_QUOTE\n\n#+END_QUOTE", 14),
        t("Example", "#+BEGIN_EXAMPLE\n\n#+END_EXAMPLE", 16),
        t("Table", "| ", 2),
        t("Link", "[[][]]", 2),
        t("Property", ":PROPERTIES:\n:KEY: value\n:END:", 14),
        t("Divider", "-----", 5),
    ];
    let mut scored: Vec<(u32, BlockTemplate)> = all
        .into_iter()
        .filter_map(|tpl| {
            let score = if query.is_empty() {
                Some(0)
            } else {
                closure_query::fuzzy_score(query, tpl.label)
            }?;
            Some((score, tpl))
        })
        .collect();
    scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    scored.into_iter().map(|(_, t)| t).collect()
}

/// What we know about a peer we have been handed a ticket for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerState {
    /// Its ticket is on file; nothing has been sent yet.
    Known,
    /// A round completed; `blocks` is what we know about afterwards.
    Synced {
        /// Blocks in our replica after the merge.
        blocks: usize,
    },
    /// The last attempt failed, with this reason. Shown, not swallowed.
    Failed(String),
}

/// A peer in the sync surface's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    /// Where it listens.
    pub addr: std::net::SocketAddr,
    /// Its verifying key — frames it sends must be signed with this.
    pub key: closure_sync::VerifyingKey,
    /// What happened last.
    pub state: PeerState,
}

/// A one-line text field: the text, and where in it the cursor is.
///
/// The overlays — capture, search, the pairing ticket, the tag and
/// property fields — were plain `String`s that only knew `push` and
/// `pop`. That is a field you can only edit at the end: no `C-a`, no
/// `C-e`, no fixing a typo in the middle without retyping the tail,
/// and no `C-w`. Every surface reimplementing that badly is worse than
/// one type doing it once, so this is the field itself and the
/// surfaces route their keys into it.
///
/// Positions are byte offsets on a char boundary; the motions step by
/// *character*, because a captured German line is the normal case and
/// half a `ß` is not a thing to leave behind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineInput {
    text: String,
    cursor: usize,
    /// What the last kill took out, for `C-y` to put back.
    kill: String,
}

impl LineInput {
    /// The text as typed so far.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Byte offset of the cursor within [`Self::text`].
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Replace the contents, cursor at the end — what restoring a
    /// rejected ticket or a remembered query wants.
    pub fn set_text(&mut self, text: &str) {
        self.text.clear();
        self.text.push_str(text);
        self.cursor = self.text.len();
    }

    /// Empty the field.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    /// Take the text out, leaving the field empty.
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    /// Whether anything has been typed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Insert one character at the cursor.
    pub fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Insert a run of text at the cursor — a paste.
    pub fn insert_str(&mut self, s: &str) {
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    /// Delete the character before the cursor.
    pub fn backspace(&mut self) {
        if let Some(prev) = self.prev_boundary() {
            self.text.replace_range(prev..self.cursor, "");
            self.cursor = prev;
        }
    }

    /// Delete the character under the cursor.
    pub fn delete(&mut self) {
        if let Some(next) = self.next_boundary() {
            self.text.replace_range(self.cursor..next, "");
        }
    }

    /// One character left.
    pub fn left(&mut self) {
        if let Some(prev) = self.prev_boundary() {
            self.cursor = prev;
        }
    }

    /// One character right.
    pub fn right(&mut self) {
        if let Some(next) = self.next_boundary() {
            self.cursor = next;
        }
    }

    /// To the start of the line (`C-a`, Home).
    pub const fn home(&mut self) {
        self.cursor = 0;
    }

    /// To the end of the line (`C-e`, End).
    pub const fn end(&mut self) {
        self.cursor = self.text.len();
    }

    /// Move to the start of the word before the cursor.
    pub fn word_backward(&mut self) {
        self.cursor = self.word_start();
    }

    /// Move to the end of the word after the cursor.
    pub fn word_forward(&mut self) {
        self.cursor = self.word_end();
    }

    /// Delete from the cursor to the end of the word after it (`M-d`).
    pub fn delete_word_forward(&mut self) {
        let end = self.word_end();
        if end > self.cursor {
            self.kill = self.text[self.cursor..end].to_owned();
            self.text.replace_range(self.cursor..end, "");
        }
    }

    /// Byte offset of the end of the word after the cursor.
    ///
    /// Whitespace between point and the word is stepped over; the word
    /// itself is where it stops. Emacs' `forward-word`, not vim's `w`,
    /// which lands on the *next* word and would swallow the space that
    /// lets you type a replacement in place.
    fn word_end(&self) -> usize {
        let rest = &self.text[self.cursor..];
        let mut at = 0usize;
        let word = |c: char| c.is_alphanumeric() || c == '_';
        for c in rest.chars() {
            if word(c) {
                break;
            }
            at += c.len_utf8();
        }
        for c in rest[at..].chars() {
            if !word(c) {
                break;
            }
            at += c.len_utf8();
        }
        self.cursor + at
    }

    /// Delete back to the previous whitespace (`C-w`, unix-word-rubout).
    ///
    /// The shell's rule, kept as the shell has it: this is the chord
    /// you press when you want the whole of `~/dev/closure` gone.
    pub fn delete_word_back(&mut self) {
        let start = self.word_start();
        self.kill = self.text[start..self.cursor].to_owned();
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    /// Delete back over one word (`M-DEL`, backward-kill-word, and the
    /// desktop's ctrl+backspace).
    ///
    /// "impossible to delete/kill a . or /": with only the
    /// whitespace rule, `~/dev/closure` was one word and the last
    /// segment could not be removed on its own. A word here is a run of
    /// alphanumerics, exactly as [`Self::word_end`] already had it
    /// going forwards — the two directions disagreeing is what made it
    /// feel arbitrary.
    pub fn delete_word_back_readline(&mut self) {
        let start = self.readline_word_start();
        self.kill = self.text[start..self.cursor].to_owned();
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    /// Start of the run before the cursor: whitespace skipped, then one
    /// run of *the same kind* — letters, or punctuation.
    ///
    /// Strict readline would take `dev/` off `~/dev/` in one press,
    /// which leaves the `/` undeletable by this chord and is exactly
    /// what was reported. Taking a run at a time makes the separator
    /// reachable, which is also what ctrl+backspace does in a browser
    /// and in VS Code. Plain prose is unaffected: with no punctuation
    /// the two rules agree.
    fn readline_word_start(&self) -> usize {
        let head = &self.text[..self.cursor];
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let mut at = head.len();
        for (i, c) in head.char_indices().rev() {
            if !c.is_whitespace() {
                break;
            }
            at = i;
        }
        let Some(kind) = head[..at].chars().next_back().map(word) else {
            return at;
        };
        for (i, c) in head[..at].char_indices().rev() {
            if c.is_whitespace() || word(c) != kind {
                break;
            }
            at = i;
        }
        at
    }

    /// Delete from the cursor to the start of the line (`C-u`).
    pub fn kill_to_start(&mut self) {
        self.kill = self.text[..self.cursor].to_owned();
        self.text.replace_range(..self.cursor, "");
        self.cursor = 0;
    }

    /// Delete from the cursor to the end of the line (`C-k`).
    pub fn kill_to_end(&mut self) {
        self.kill = self.text[self.cursor..].to_owned();
        self.text.truncate(self.cursor);
    }

    /// The text the last kill took out, if any.
    ///
    /// A kill that cannot be put back is a delete wearing a kill's
    /// name: `C-k` in a prompt used to drop the rest of the line on the
    /// floor while the same chord in the editor was recoverable.
    #[must_use]
    pub fn kill(&self) -> &str {
        &self.kill
    }

    /// Adopt a kill made in another field, so `C-y` can cross prompts.
    pub fn set_kill(&mut self, text: &str) {
        self.kill.clear();
        self.kill.push_str(text);
    }

    /// Put the last kill back at the cursor (`C-y`).
    pub fn yank(&mut self) {
        let kill = std::mem::take(&mut self.kill);
        self.insert_str(&kill);
        self.kill = kill;
    }

    /// Offer `key` to the field, reporting whether it was consumed.
    ///
    /// The surface keeps the keys that mean something to *it* —
    /// `enter`, `escape`, its own navigation — and hands the rest here,
    /// so every one-line field in the app answers to the same chords.
    pub fn key(&mut self, key: &str, ctrl: bool, alt: bool, text: Option<char>) -> bool {
        match key {
            "a" if ctrl => self.home(),
            "e" if ctrl => self.end(),
            "b" if ctrl => self.left(),
            "f" if ctrl => self.right(),
            "d" if ctrl => self.delete(),
            "k" if ctrl => self.kill_to_end(),
            "u" if ctrl => self.kill_to_start(),
            "w" if ctrl => self.delete_word_back(),
            "y" if ctrl => self.yank(),
            "backspace" if ctrl || alt => self.delete_word_back_readline(),
            "backspace" => self.backspace(),
            "delete" => self.delete(),
            // The desktop word ops, and readline's `M-d`. The body
            // editor has had these; a prompt had neither, which is
            // most of "the discrepancy between the editor and the
            // prompt makes it feel unsatisfying".
            "left" if ctrl || alt => self.word_backward(),
            "right" if ctrl || alt => self.word_forward(),
            // readline's own word motions. The arrow spellings were
            // here and these were not, so the chord an Emacs hand
            // actually reaches for did nothing.
            "b" if alt => self.cursor = self.readline_word_start(),
            "f" if alt => self.word_forward(),
            "d" if alt => self.delete_word_forward(),
            "left" => self.left(),
            "right" => self.right(),
            "home" => self.home(),
            "end" => self.end(),
            _ => {
                // A bare character is text; the same letter under a
                // modifier is a chord nobody bound, and typing its
                // letter would be a surprise.
                let Some(c) = text.filter(|_| !ctrl && !alt) else {
                    return false;
                };
                self.insert_char(c);
            }
        }
        true
    }

    /// Byte offset of the character boundary before the cursor.
    fn prev_boundary(&self) -> Option<usize> {
        self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
    }

    /// Byte offset of the boundary after the character at the cursor.
    fn next_boundary(&self) -> Option<usize> {
        self.text[self.cursor..]
            .chars()
            .next()
            .map(|c| self.cursor + c.len_utf8())
    }

    /// Where the word *being typed* starts — everything back to the
    /// last whitespace, with no run of spaces skipped.
    ///
    /// Deliberately not [`Self::word_start`], which steps over trailing
    /// whitespace because that is what deleting a word backwards has to
    /// do. A completion asked for on a boundary has nothing to work
    /// from, and stepping back over the space to find something would
    /// complete the word before the one the cursor is on.
    #[must_use]
    pub fn prefix_start(&self) -> usize {
        let head = &self.text[..self.cursor];
        head.rfind(char::is_whitespace).map_or(0, |i| {
            i + head[i..].chars().next().map_or(1, char::len_utf8)
        })
    }

    /// The word being typed, empty on a boundary.
    #[must_use]
    pub fn word_prefix(&self) -> &str {
        &self.text[self.prefix_start()..self.cursor]
    }

    /// Replace everything from `start` to the cursor with `text`, and
    /// leave the cursor at its end — what applying a completion does.
    pub fn replace_to_cursor(&mut self, start: usize, text: &str) {
        let start = start.min(self.cursor);
        if !self.text.is_char_boundary(start) {
            return;
        }
        self.text.replace_range(start..self.cursor, text);
        self.cursor = start + text.len();
    }

    /// Where the word before the cursor starts: the run of whitespace
    /// immediately behind it, and then the word behind that.
    fn word_start(&self) -> usize {
        let head = &self.text[..self.cursor];
        let trimmed = head.trim_end();
        let without_word = trimmed.trim_end_matches(|c: char| !c.is_whitespace());
        without_word.len()
    }
}

/// Keep the rows whose text the filter matches, in order.
///
/// One rule for every picker: Doom's `orderless`, so `beta child`
/// finds "Beta child" without the words being adjacent, and an empty
/// filter keeps everything.
fn filtered<T>(rows: Vec<T>, filter: &str, text: impl Fn(&T) -> String) -> Vec<T> {
    if filter.trim().is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|row| closure_query::orderless_score(filter, &text(row)).is_some())
        .collect()
}

/// What which-key lists inside a buffer, by mode.
///
/// The editor's vocabulary is not in `mode_keymap`: those chords are
/// resolved by the vim engine and the readline set rather than
/// dispatched as commands, so the keymap has nothing to say about them
/// and which-key had nothing to show but the outline's. This is that
/// vocabulary as data, which is also what the hint strip should be
/// reading rather than carrying prose.
///
/// A mode with no NORMAL is offered no NORMAL: "only show the
/// keybindings that are relevant to the corresponding mode".
/// Build a which-key group list from static pairs.
fn which_key_of(groups: &[(&str, &[(&str, &str)])]) -> Vec<(String, Vec<(String, String)>)> {
    groups
        .iter()
        .map(|(title, rows)| {
            (
                (*title).to_owned(),
                rows.iter()
                    .map(|(chord, label)| ((*chord).to_owned(), (*label).to_owned()))
                    .collect(),
            )
        })
        .collect()
}

/// A one-line prompt: the readline set, and the two keys that end it.
///
/// The outline's single-letter verbs are not merely unbound here —
/// `m`, `d`, `r` are the letters you are typing.
fn prompt_which_key() -> Vec<(String, Vec<(String, String)>)> {
    which_key_of(&[
        (
            "Edit",
            &[
                ("C-a", "start of line"),
                ("C-e", "end of line"),
                ("C-b", "back one character"),
                ("C-f", "forward one character"),
                ("M-b", "back one word"),
                ("M-f", "forward one word"),
                ("M-DEL", "kill the word behind"),
                ("C-k", "kill to end of line"),
                ("C-u", "kill to start of line"),
                ("C-y", "yank it back"),
            ],
        ),
        (
            "Prompt",
            &[
                ("M-p", "previous entry"),
                ("M-n", "next entry"),
                ("TAB", "complete"),
                ("RET", "accept"),
                ("ESC", "cancel"),
            ],
        ),
    ])
}

/// A floating picker: type to narrow it, walk it, run one, leave.
fn picker_which_key() -> Vec<(String, Vec<(String, String)>)> {
    which_key_of(&[
        (
            "Pick",
            &[
                ("type", "narrow the list"),
                ("C-n", "next"),
                ("C-p", "previous"),
                ("<down>", "next"),
                ("<up>", "previous"),
                ("RET", "run it"),
                ("ESC", "close"),
            ],
        ),
        (
            "Edit",
            &[
                ("C-a", "start of line"),
                ("C-e", "end of line"),
                ("M-DEL", "kill the word behind"),
                ("C-u", "kill to start of line"),
            ],
        ),
    ])
}

/// A picture, full size. One key, and saying so is the whole job.
fn image_which_key() -> Vec<(String, Vec<(String, String)>)> {
    which_key_of(&[("Picture", &[("ESC", "close")])])
}

/// The date picker's own arrows.
fn date_which_key() -> Vec<(String, Vec<(String, String)>)> {
    which_key_of(&[(
        "Date",
        &[
            ("h/l", "a day back or on"),
            ("k/j", "a week back or on"),
            ("<left>/<right>", "a day"),
            ("<up>/<down>", "a week"),
            ("RET", "take it"),
            ("ESC", "cancel"),
        ],
    )])
}

fn editor_which_key(mode: InputMode) -> Vec<(String, Vec<(String, String)>)> {
    let modal = matches!(mode, InputMode::Vim | InputMode::Doom | InputMode::Helix);
    let mut groups: Vec<(&str, Vec<(&str, &str)>)> = vec![(
        "Edit",
        vec![
            ("C-a", "start of line"),
            ("C-e", "end of line"),
            ("C-b", "back one character"),
            ("C-f", "forward one character"),
            ("M-b", "back one word"),
            ("M-f", "forward one word"),
            ("M-DEL", "kill the word behind"),
            ("C-w", "kill to the last space"),
            ("C-k", "kill to end of line"),
            ("C-u", "kill to start of line"),
            ("C-y", "yank it back"),
            ("C-n", "complete, then next candidate"),
            // Honest about both jobs, because it has two now: it walks
            // the popup while there is one and is `previous-line` the
            // rest of the time. Labelling it "previous completion"
            // would advertise the half you use less.
            ("C-p", "previous candidate, else up a line"),
            ("TAB", "expand a snippet, or accept"),
            ("M-TAB", "fold or unfold"),
            ("M-z", "wrap long lines"),
        ],
    )];
    if modal {
        groups.push((
            "Normal",
            vec![
                ("w b e", "by word"),
                ("f t %", "to a character, to a pair"),
                ("d c y", "delete, change, yank"),
                ("dd yy p", "whole lines"),
                ("diw caw", "inside, around"),
                ("g g", "first line"),
                ("g u", "lowercase"),
                ("g U", "uppercase"),
                ("g q", "reflow"),
                ("z a", "fold or unfold"),
                ("z z", "centre the line"),
                ("z t", "line to the top"),
                ("z b", "line to the bottom"),
                ("q a @a", "record and replay"),
                ("m a `a", "mark and jump"),
                ("/ n N", "search, next, previous"),
                ("v V", "visual, visual line"),
                ("u C-r", "undo, redo"),
                (". ", "repeat"),
                ("Esc", "back to NORMAL"),
            ],
        ));
    }
    groups.push((
        "Buffer",
        if modal {
            vec![
                ("C-s", "save and stay"),
                ("C-c C-c", "save and close"),
                ("C-c C-k", "discard"),
                (":w :q :wq", "write, quit, both"),
            ]
        } else {
            vec![
                ("C-x C-s", "save and stay"),
                ("C-c C-c", "save and close"),
                ("C-c C-k", "discard"),
                ("Esc", "close a clean buffer"),
            ]
        },
    ));
    groups
        .into_iter()
        .map(|(title, rows)| {
            (
                title.to_owned(),
                rows.into_iter()
                    .map(|(chord, what)| (chord.to_owned(), what.to_owned()))
                    .collect(),
            )
        })
        .collect()
}

/// Command names that were spelled another way before 2026-08-02.
///
/// The schema is verb first, and a bare noun opens the pane of that
/// name. Ninety-two commands had grown three shapes at once —
/// `toggle-fold` beside `checkbox-toggle`, `add-sibling` beside
/// `buffer-next`, `block-list` beside `move-subtree-up` — so guessing
/// the name of a command you had not used yet was a coin toss, which is
/// the whole of discoverability. "They should have sound names in a
/// similiar schema."
///
/// org lends its own word where closure had invented one: nothing in
/// org is called `eval-block`, and what it does is
/// `org-babel-execute-src-block`.
const COMMAND_ALIASES: &[(&str, &str)] = &[
    ("checkbox-toggle", "toggle-checkbox"),
    ("block-list", "list-blocks"),
    ("headline-list", "list-headlines"),
    ("buffer-list", "list-buffers"),
    ("buffer-next", "next-buffer"),
    ("buffer-prev", "prev-buffer"),
    ("buffer-close", "close-buffer"),
    ("buffer-close-force", "close-buffer-force"),
    ("buffer-alternate", "alternate-buffer"),
    ("eval-block", "execute-block"),
    ("search-start", "search"),
    ("search-headline-start", "search-headlines"),
    ("capture-start", "capture"),
    // "Which view?" — the outline, or the whole file as one buffer.
    ("toggle-view", "toggle-file-view"),
    // Which mode? The keymap, not the editor's vim mode, which is the
    // other thing "mode" means three lines away in the status bar.
    ("cycle-mode", "next-input-mode"),
    // "version" is what people type; `build-info` is what it is.
    ("version", "build-info"),
];

/// The commands that take an argument, and what the argument is.
///
/// A list rather than a guess. "Silently ignore the rest of the line"
/// is how a cron entry runs for a week doing something other than what
/// it says, and "hand it to whoever asks" is how a typo becomes a
/// headline called `--force`.
pub const COMMAND_ARGUMENTS: &[(&str, &str)] = &[
    ("capture", "<title>"),
    ("search", "<text>"),
    ("search-headlines", "<text>"),
    ("goto", "<id>"),
    ("rename", "<new title>"),
];

/// Split a command line into its name and the rest.
///
/// The one place a command line becomes a name and an argument. A name
/// with no space in it is unchanged, which is every chord: a chord *is*
/// a name, and everything else that runs a command — a cron line, the
/// `:` line, a key bound in config.org, an agent on the bridge — knows
/// something a bare name has nowhere to put.
#[must_use]
pub fn split_command(line: &str) -> (&str, &str) {
    let line = line.trim();
    line.split_once(' ')
        .map_or((line, ""), |(name, rest)| (name, rest.trim()))
}

/// What `name` calls its argument, if it takes one.
#[must_use]
pub fn command_argument(name: &str) -> Option<&'static str> {
    COMMAND_ARGUMENTS
        .iter()
        .find(|(cmd, _)| *cmd == name)
        .map(|(_, arg)| *arg)
}

/// The name a command answers to now, given any name it has ever had.
///
/// Every former spelling still resolves: a rename that breaks the chord
/// somebody typed yesterday, the `:` line in their muscle memory, or a
/// tool call an LLM has already learned costs more than the tidiness it
/// buys. An unknown name is returned unchanged, so this is safe to put
/// in front of every dispatch.
#[must_use]
pub fn canonical_command(name: &str) -> &str {
    COMMAND_ALIASES
        .iter()
        .find(|(was, _)| *was == name)
        .map_or(name, |(_, now)| *now)
}

/// Drive one field with the session's shared kill ring.
///
/// Every surface with a text field routes its unclaimed keys through
/// here. That is what makes `C-a`, `M-b`, the arrows and `C-k`/`C-y`
/// mean the same thing in all of them — "not for every new input field
/// the same keybinding problems" — and what lets a kill in one prompt
/// be yanked in the next. Reports whether the field took the key.
fn line_key(
    field: &mut LineInput,
    kill: &mut String,
    key: &str,
    ctrl: bool,
    alt: bool,
    text: Option<char>,
) -> bool {
    field.set_kill(kill);
    let claimed = field.key(key, ctrl, alt, text);
    kill.clear();
    kill.push_str(field.kill());
    claimed
}

/// Body lines a shell is assumed to be able to paint until it says
/// otherwise ([`ModalApp::set_body_viewport`]).
pub const BODY_VIEWPORT_DEFAULT: usize = 20;

/// One painted row of a wrapped body: a slice of a logical line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualLine {
    /// Which logical line (0-based) this row came from — what the
    /// gutter number refers to.
    pub logical: usize,
    /// Byte range within the whole body.
    pub start: usize,
    /// End of the range (exclusive).
    pub end: usize,
    /// Whether this is the first row of its logical line, so the gutter
    /// numbers each line once rather than once per row.
    pub first: bool,
}

/// Break `body` into visual rows at most `cols` characters wide.
///
/// The editor clips long lines and scrolls sideways instead, which was
/// a deliberate decision — wrapping desyncs the one-number gutter, the
/// fixed row height, and the arithmetic that turns pane height into a
/// line count. Wrapping is therefore opt-in (`wrap = true`), and this
/// is the piece that makes it honest: every row says which logical line
/// it belongs to and exactly which bytes it holds, so the gutter, the
/// cursor and the scroll all still have something exact to ask.
///
/// Breaks at the last space that fits; a word longer than the width is
/// broken rather than dropped, because a URL is the normal case. Never
/// loses a byte, and never splits a character.
#[must_use]
pub fn wrap_body(body: &str, cols: usize) -> Vec<VisualLine> {
    let cols = cols.max(1);
    let mut out = Vec::new();
    let mut line_start = 0usize;
    for (logical, line) in body.split('\n').enumerate() {
        let mut at = 0usize; // byte offset within `line`
        let mut first = true;
        loop {
            let rest = &line[at..];
            if rest.chars().count() <= cols {
                out.push(VisualLine {
                    logical,
                    start: line_start + at,
                    end: line_start + line.len(),
                    first,
                });
                break;
            }
            // The byte offset `cols` characters in — never mid-char.
            let hard = rest.char_indices().nth(cols).map_or(rest.len(), |(i, _)| i);
            // Prefer the last space that fits; a word wider than the
            // pane is broken instead of hanging off it. The space stays
            // at the end of the row it broke, so the rows are an exact
            // partition of the bytes — a cursor sitting on that space
            // has to be *somewhere*, and a row that owns no bytes is a
            // row the cursor can fall through.
            let cut = rest[..hard]
                .rfind(' ')
                .map_or(hard, |sp| if sp == 0 { hard } else { sp + 1 });
            out.push(VisualLine {
                logical,
                start: line_start + at,
                end: line_start + at + cut,
                first,
            });
            at += cut;
            first = false;
        }
        line_start += line.len() + 1;
    }
    out
}

/// Does this shell have a command by that name?
///
/// Approximate on purpose and erring towards yes: the true set is the
/// `match` in `run_command`, which is not enumerable, so the question
/// is asked of the two lists that *are* — the palette's registry and
/// the five keymaps. It exists to catch a typo in `config.org`, not to
/// gatekeep.
fn command_exists(command: &str) -> bool {
    let name = canonical_command(command);
    PALETTE_COMMANDS.iter().any(|(_, c, ..)| *c == name)
        || [
            InputMode::Doom,
            InputMode::Vim,
            InputMode::Emacs,
            InputMode::Helix,
            InputMode::Notion,
        ]
        .iter()
        .any(|m| {
            closure_input::mode_keymap(*m)
                .iter()
                .any(|(_, c)| *c == name)
        })
}

/// Every chord for `command`, as org markup — or a plain statement
/// that this mode does not bind it, never a guess.
///
/// A tutorial that teaches one of two keys teaches half a keymap, and
/// the half it drops is the one the reader would have preferred.
fn chord_list(mode: InputMode, command: &str) -> String {
    let chords = closure_input::chords_for_command(mode, command);
    if chords.is_empty() {
        return "(unbound in this mode)".to_owned();
    }
    chords
        .iter()
        .map(|c| format!("={c}="))
        .collect::<Vec<_>>()
        .join(" or ")
}

/// The tutorial closure writes into a vault, for the given input mode.
///
/// Every chord in it is looked up in the keymap the app dispatches
/// through (I4), so it cannot describe a binding that does not exist:
/// a hand-written tutorial is wrong the first time a chord moves, and
/// the person who finds out is a new user following it.
///
/// It is an org file because that is what closure is — the tutorial is
/// a note you can fold, search, edit and sync like any other.
#[must_use]
pub fn tutorial_org(mode: InputMode) -> String {
    let chord = chord_list;
    let modal = matches!(mode, InputMode::Vim | InputMode::Doom | InputMode::Helix);
    let mode_name = match mode {
        InputMode::Emacs => "emacs",
        InputMode::Vim => "vim",
        InputMode::Doom => "doom",
        InputMode::Helix => "helix",
        InputMode::Notion => "notion",
    };
    // Doom's identity is the leader, and a tutorial that never says the
    // word teaches the wrong app — even though every command below also
    // has a shorter chord of its own.
    let leader = if matches!(mode, InputMode::Doom) {
        "\n* The leader\n\
         =SPC= alone opens which-key: press it and wait, and the panel lists \
         what the next\nkey would do. =SPC :=  is the palette, =SPC f s= \
         saves, =SPC q q= quits, =SPC o a= the\nagenda. Everything under it \
         also has a shorter chord, which is what the rest of\nthis file \
         shows.\n"
    } else {
        ""
    };
    let editing = tutorial_editing(modal);
    let registers = tutorial_registers(modal);
    format!(
        "#+TITLE: closure — a tour\n\
         #+FILETAGS: :closure:tutorial:\n\
         \n\
         Written by closure itself for =input_mode = {mode_name}=. Every chord \
         below is read out\nof the keymap the app dispatches through, so it \
         cannot describe a binding that\ndoes not exist. Regenerate it after \
         changing modes: =closure init-vault .=\n\
         \n\
         {leader}\n* Moving around\n\
         The outline is the vault: every headline in every =.org= file, in one \
         list.\n\
         - {next} / {prev} — next and previous row\n\
         - {search} — search; type, then Enter selects the hit *in the \
         outline*\n\
         - {open} — open the selected item's body for editing\n\
         - {palette} — the command palette: every command, with its chord\n\
         - =Esc= — drop the selection (see /Capture/, below)\n\
         - {quit} — leave\n\
         \n\
         * Capture\n\
         {capture} opens one line. Enter files it; Shift+Enter adds a second \
         line, and\nthen the first line is the headline and the rest is its \
         body.\n\
         \n\
         A capture is filed *under whatever is selected*, which is what an \
         outliner is for.\nPress =Esc= first to select nothing, and it goes to \
         the top level of =inbox.org=.\nEither way the new item ends up \
         selected, so the next thing you do happens to it.\n\
         \n\
         * Editing a body\n\
         {editing}\n\
         \n\
         - =C-c C-c= writes the buffer and closes it; =C-s= or =:w= writes and stays\n\
         - =:q= closes the buffer — the app is =:qa=, the way vim closes a \
         window\n  and quits only on the last one\n\
         - =Esc= is the mode key and never closes: it means NORMAL, and \
         pressing it\n  twice is a reflex, not a decision. (In notion/emacs \
         mode there is no NORMAL,\n  so there it still closes an unchanged \
         buffer.)\n\
         - =:q!= throws the edit away on purpose\n\
         - opening another note keeps the one you were in, edits and all, \
         unsaved\n  until you write it\n\
         - =C-l= cycles the cursor line: middle, top, bottom (=zz= / =zt= / \
         =zb= do it directly)\n\
         - =C-+= / =C--= / =C-0= zoom the buffer text\n\
         - =/= searches inside the buffer; =u= undoes, =C-r= redoes\n\
         \n\
         Type =* Something= on a line of its own and it becomes a *child* of \
         the note you\nare editing, at the right depth — a =*= typed in a \
         =***= note is a =****=. Prose\nthat merely starts with a star \
         (=*bold*=, a =  * list item=) stays prose.\n\
         \n\
         {new_notes}\n\
         {registers}{reference}",
        leader = leader,
        next = chord(mode, "next-file"),
        prev = chord(mode, "prev-file"),
        search = chord(mode, "search"),
        open = chord(mode, "open-file"),
        palette = chord(mode, "palette"),
        quit = chord(mode, "quit"),
        capture = chord(mode, "capture"),
        new_notes = tutorial_new_notes(mode),
        registers = registers,
        reference = tutorial_reference(mode),
    )
}

/// The three ways to make a note, and the reason they are one answer.
///
/// Asked directly ("how do I create a new headline/sibling/subtree that
/// is still compatible with the P2P sync?"), and the honest answer is
/// about the `:ID:` rather than about any of the three chords.
fn tutorial_new_notes(mode: InputMode) -> String {
    let chord = |command: &str| chord_list(mode, command);
    format!(
        "* Three ways to make a note, and why they are the same\n\
         - {capture} — a capture, filed under the selection\n\
         - {sibling} — a sibling, right after the selected one, at its level\n\
         - =* Something= typed into a body — a child of the note being edited\n\
         \n\
         All three write an =:ID:= into the file. That property is what a \
         note *is* to\neverything above the parser: sync addresses blocks by \
         id, and so do =[[id:…]]=\nlinks, the undo tree and the cursor \
         memory. So all three are ordinary notes — they\nsync, they can be \
         linked to, and they are still the same note when the file is \
         read\nback tomorrow. A headline you add by hand in another editor \
         has no id until\nclosure touches it: it appears in the outline \
         immediately, and is stamped the\nfirst time you edit it.\n",
        capture = chord("capture"),
        sibling = chord("add-sibling"),
    )
}

/// What a buffer does when you open it, which is the one thing a modal
/// mode has to explain and a mouse-first one has to promise.
const fn tutorial_editing(modal: bool) -> &'static str {
    if modal {
        "This mode is *modal*: a buffer opens in NORMAL, where the keys are \
         commands.\n=i= starts typing, =Esc= stops. That is why pressing =d= \
         in a fresh buffer deletes\nsomething instead of typing a letter — it \
         is a command, not a mistake."
    } else {
        "This mode has no modes: a buffer opens ready to type, and every key \
         is text.\nThe editor is always in INSERT, which is what the mode \
         indicator says."
    }
}

/// Where yanked text goes — the registers in a modal mode, the system
/// clipboard in one without them.
const fn tutorial_registers(modal: bool) -> &'static str {
    if modal {
        "** Images\n\
         An image is an org link whose target is a picture — \
         =[[file:assets/shot.png]]= —\nso nothing new goes in the file. \
         Paste one from the clipboard (=C-v=) and it is\nwritten into the \
         vault's =assets/= and linked at the cursor, with a *relative* \
         path,\nso the note still resolves in Emacs and on the machine you \
         sync with.\n=assets_dir= in config.org moves the directory; \
         =toggle-inline-images= turns the\npictures off and leaves the links. \
         Images are painted in the note *preview*: the\nbuffer's lines are a \
         fixed height, which is what every viewport measurement is\nbuilt \
         on, so the editor shows the link and the preview shows the \
         picture.\n\
         \n\
         ** Tables\n\
         A line starting with =|= is a table. TAB realigns the whole thing \
         and steps to the\nnext cell, =S-TAB= back one. The rest is org's, \
         and each key does nothing outside a\ntable, so the outline command \
         it shares a chord with keeps it:\n\
         - =M-<left>= / =M-<right>= — move this column\n\
         - =M-<up>= / =M-<down>= — move this row\n\
         - =M-S-<left>= / =M-S-<right>= — delete / insert a column\n\
         - =M-S-<up>= / =M-S-<down>= — kill / insert a row\n\
         - =M--= — rule a line under this row (Emacs says =C-c -=; here =C-c= \
         is copy)\n\
         \n\
         ** Surround\n\
         Pairs, as an operator — evil-surround's vocabulary, and org's \
         emphasis markers\nare pairs too.\n\
         - =ysiw\"= wraps the word in quotes; =ys$)= to the end of the line, \
         =yss*= the line\n\
         - =S= in VISUAL wraps the selection\n\
         - =ds\"= takes a pair away, =cs\"'= swaps one for another\n\
         - =*=, =/=, =_=, ==, =~=, =+= are pairs, so =ysiw*= is bold and \
         =cs*/= makes it italic\n\
         - the closing bracket hugs and the opening one pads: =ysiw)= gives \
         =(word)=,\n  =ysiw(= gives =( word )=\n\
         (HTML-tag surrounds — vim's =t= — are not implemented.)\n\
         \n\
         ** Registers\n\
         Yanking and deleting put text somewhere, and that somewhere has a \
         name.\n\
         - =yy= copies a line into the unnamed register, =p= puts it back\n\
         - =\"ayy= copies into register =a=; =\"ap= puts *that* one back\n\
         - =dd= deletes a line — a delete fills the same register, so =p= \
         after =dd= moves it\n\
         - =q a= records a macro into register =a=, =q= stops, =@a= replays \
         it\n\
         The registers are the editor's, not the system's: =C-v= pastes what \
         the *desktop*\ncopied, which is a different clipboard and \
         deliberately so.\n"
    } else {
        "** The clipboard\n\
         =C-c= and =C-v= are the system clipboard, the way they are \
         everywhere else.\n"
    }
}

/// The second half of the tutorial: the per-command reference, and what
/// the files around it are.
// One long format string, which is what a tutorial is.
#[allow(clippy::too_many_lines)]
fn tutorial_reference(mode: InputMode) -> String {
    let chord = chord_list;
    format!(
        "* Tags, TODOs and properties\n\
         - {todo} — cycle the TODO keyword\n\
         - {tags} — edit tags (=:work:urgent:=)\n\
         - {property} — add or edit a property\n\
         - {fold} — fold or unfold the subtree\n\
         - {promote} / {demote} — change a headline's level\n\
         \n\
         * Undo\n\
         Every edit goes through the kernel, so every edit is undoable — \
         including the\nones a chord made by accident. {undo} undoes, {redo} \
         redoes, and {history}\nshows the tree of where you have been.\n\
         \n\
         * config.org\n\
         The file beside this one *is* the configuration: how you type, the \
         colours, which\nlanguages may run, where pairing listens. Delete a \
         line to get its default back.\n\
         \n\
         Two keys are worth reading twice:\n\
         - =eval_trust= is default-deny. A vault is something people can send \
         you, so no\n  code block runs — and =:!cmd= is refused — until you \
         list a language here.\n\
         \n\
         ** Running a source block\n\
         Put the cursor in a block and press =C-c C-c= — org's own \
         =C-c C-c= in a\nbuffer, which runs the block under the cursor and \
         writes what it printed back\ninto the note as =#+RESULTS:=. Press it \
         anywhere that is /not/ a block and it\nsaves and closes instead, \
         which is org's rule too.\n\
         \n\
         A block will not run until its language is trusted. This one is \
         refused:\n\
         \n\
         #+BEGIN_SRC shell\n\
         echo it works\n\
         #+END_SRC\n\
         \n\
         …until =config.org= says so. Open it, find the =closure-config= \
         block, and\nadd the language to =eval_trust=:\n\
         \n\
         : eval_trust = shell\n\
         \n\
         Several are a comma list — =eval_trust = shell, python= — and the \
         names are\nthe ones you write after =#+BEGIN_SRC=. Then =g != \
         reloads the config and the\nblock above runs.\n\
         - =llm_key_env= names an /environment variable/, never the key \
         itself, so this file\n  can be committed and synced without leaking \
         a credential.\n\
         \n\
         =last_place= is written for you when the window closes: the id of the \
         note you\nwere last in, so the next session opens on it rather than \
         on the first headline.\nThe note you last /edited/ wins over the one \
         the cursor was resting on. Delete\nthe line to start at the top \
         again.\n\
         \n\
         Org itself has no \"last modified\" property — neither does closure \
         invent one.\nThe conventions people use are a =:LAST_MODIFIED:= \
         property maintained by a save\nhook, or =org-log-done= logbook \
         entries; both are things you can add to your own\nfiles, and closure \
         will keep them like any other property. What closure knows\nabout \
         time is the journal and the undo tree, neither of which touches your \
         text.\n\
         \n\
         * Pairing with another machine\n\
         {sync} opens the pairing pane. It shows a *ticket*: one line naming \
         an address and\na public key. Hand it to the other person, paste \
         theirs, and both press listen.\n\
         \n\
         The ticket exchange is the trust anchor — anyone whose ticket you \
         paste can write\nto your vault, and a listener that trusts nobody \
         refuses to answer at all. On a\nmachine with more than one address (a \
         LAN *and* a VPN), set =sync_advertise= in\nconfig.org so the ticket \
         names the one your peer can actually reach.\n\
         \n\
         ** Through a folder instead of a socket\n\
         Set =sync_dir= in config.org to something both machines can see — a \
         Syncthing\nshare, a Dropbox, a mounted drive, a USB stick. \
         =sync-export= leaves a signed\nbundle in it; =sync-import= picks up \
         the ones your peers left and writes what\nconverged back into your \
         files. Neither machine has to be awake when the other\nis, and there \
         has to be no route between them.\n\
         \n\
         You still pair first: a bundle is merged only when it is signed by \
         someone whose\nticket you pasted. A shared folder is exactly as \
         trustworthy as everyone who can\nwrite to it, so it is verified \
         rather than believed.\n\
         \n\
         * Where the files are\n\
         Every note is a plain =.org= file in this directory. Nothing is in a \
         database; a\nsecond program reading the directory sees exactly what \
         closure sees. That is the\nwhole design — you can leave at any time \
         and take your notes with you.\n",
        todo = chord(mode, "toggle-todo"),
        tags = chord(mode, "edit-tags"),
        property = chord(mode, "edit-property"),
        fold = chord(mode, "toggle-fold"),
        promote = chord(mode, "promote"),
        demote = chord(mode, "demote"),
        undo = chord(mode, "undo"),
        redo = chord(mode, "redo"),
        history = chord(mode, "undo-history"),
        sync = chord(mode, "sync"),
    )
}

/// The lines a fold at `at` would cover: `(first, last)`, where `first`
/// stays visible and everything through `last` hides.
///
/// A `#+BEGIN_…` line folds through its `#+END_…`; a headline folds
/// through the line before the next headline at its level or shallower.
/// Standing *inside* a block folds the block, because the cursor is
/// usually in the thing you want out of the way. Anything else folds
/// nothing — better than guessing at a range.
/// Every property drawer in `text`, as fold ranges.
///
/// What a buffer starts with folded. The handle line stays visible, the
/// way a folded block keeps its `#+BEGIN_`: a fold you cannot see is a
/// disappearance.
fn drawer_folds(text: &str) -> Vec<(usize, usize)> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < lines.len() {
        if lines[at].trim().eq_ignore_ascii_case(":PROPERTIES:")
            && let Some((start, end)) = fold_range(&lines, at)
        {
            out.push((start, end));
            at = end + 1;
            continue;
        }
        at += 1;
    }
    out
}

fn fold_range(lines: &[&str], at: usize) -> Option<(usize, usize)> {
    /// The headline depth of a line, or 0 when it is not one.
    fn level(line: &str) -> usize {
        let rest = line.trim_start_matches('*');
        let stars = line.len() - rest.len();
        if stars > 0 && (rest.starts_with([' ', '\t']) || rest.is_empty()) {
            stars
        } else {
            0
        }
    }
    let line = lines.get(at)?;
    let stars = level(line);
    if stars > 0 {
        // Through to the line before the next headline at this level or
        // shallower — which is what a subtree is.
        let end = lines
            .iter()
            .enumerate()
            .skip(at + 1)
            .find(|(_, l)| matches!(level(l), s if s > 0 && s <= stars))
            .map_or(lines.len(), |(i, _)| i);
        return (end > at + 1).then_some((at, end - 1));
    }
    // A property drawer folds like a block: `:PROPERTIES:` through the
    // `:END:` that closes it. Same shape, and the same reason — four
    // lines of bookkeeping between you and the next thing you meant to
    // read.
    if line.trim().eq_ignore_ascii_case(":PROPERTIES:") {
        let end = lines
            .iter()
            .enumerate()
            .skip(at + 1)
            .find(|(_, l)| l.trim().eq_ignore_ascii_case(":END:"))
            .map(|(i, _)| i)?;
        return (end > at).then_some((at, end));
    }
    // Inside or on a block: walk back to its `#+BEGIN_`, then forward
    // to the matching `#+END_`.
    let opens_at = (0..=at)
        .rev()
        .find(|i| closure_org::block_delimiter_of(lines[*i]).is_some())?;
    let name = match closure_org::block_delimiter_of(lines[opens_at])? {
        closure_org::BlockDelimiter::Begin { name, .. } => name,
        closure_org::BlockDelimiter::End { .. } => return None,
    };
    let end = lines.iter().enumerate().skip(opens_at + 1).find(|(_, l)| {
        matches!(
            closure_org::block_delimiter_of(l),
            Some(closure_org::BlockDelimiter::End { name: n }) if n.eq_ignore_ascii_case(name)
        )
    })?;
    Some((opens_at, end.0))
}

/// One zoom step, as a ratio. Doom scales its font by an increment per
/// press; a ratio is the same idea in a world with no font table.
const ZOOM_STEP: f32 = 1.1;
/// Zoom ceiling (`1.1^15` ≈ 4.2×) and floor (`1.1^-7` ≈ 0.51×). Past
/// either end a "zoom level" stops being one: a wall of one glyph, or a
/// font nobody can read.
const ZOOM_MAX_STEPS: i8 = 15;
/// See [`ZOOM_MAX_STEPS`].
const ZOOM_MIN_STEPS: i8 = -7;

/// A key the body editor is holding until the rest of its chord
/// arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyPrefix {
    /// `z` — evil's viewport prefix (`zz` / `zt` / `zb`).
    Viewport,
    /// `C-c` — org's own prefix, waiting for `C-c` (accept) or `C-k`
    /// (abandon). Held here rather than resolved as a single stroke
    /// because a prefix is two keys and `window_chord` knows one.
    OrgAccept,
}

/// Where the cursor line should end up in the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyFraming {
    /// Middle of the pane (`zz`, and `C-l`'s first press).
    Centre,
    /// First visible line (`zt`).
    Top,
    /// Last visible line (`zb`).
    Bottom,
}

/// Where pairing listens when nothing has said otherwise: the closure
/// port, on every interface.
///
/// It used to be `127.0.0.1:7420`, which made every ticket a lie the
/// moment it left the machine — the peer dialled its own loopback and
/// reached itself. Binding wide is only half the fix; the other half is
/// that a network-facing listener refuses inbound rounds until it has
/// been given a peer to trust ([`SyncApp::inbound_ready`]), and that
/// nothing binds at all until the user asks to listen.
pub const DEFAULT_SYNC_BIND: std::net::SocketAddr =
    std::net::SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), 7420);

/// Collaboration state: our identity, the ticket we hand out, the
/// peers we have been given, and the replica that merges with them.
///
/// The socket work is deliberately *not* here — it blocks, and this
/// has to stay callable from a render thread. A shell dials the peer,
/// hands the outcome back through [`SyncApp::record_outcome`], and
/// merges what it received with [`SyncApp::merge_session`]. Everything
/// that decides *what* happens is here, and testable without a network.
pub struct SyncApp {
    name: String,
    /// What the ticket names — where a *peer* dials us. Not necessarily
    /// where we bind: `0.0.0.0` is bindable and undialable.
    addr: std::net::SocketAddr,
    /// The socket we open. `0.0.0.0:7420` accepts from the network;
    /// `127.0.0.1:…` keeps pairing on this machine.
    bind: std::net::SocketAddr,
    /// Which of our addresses to advertise, when the operator has said.
    /// `None` means detect it at bind time.
    advertise: Option<std::net::IpAddr>,
    signing: closure_sync::SigningKey,
    session: closure_sync::SyncSession,
    peers: Vec<Peer>,
    /// Bound listener, once a shell has asked to accept connections.
    listener: Option<std::sync::Arc<std::net::TcpListener>>,
    /// Where *we* are, broadcast to peers each round.
    local: Option<PeerAt>,
    /// Where each peer was when we last heard from it, one entry per
    /// peer — a position, not a log, so a peer that moves does not
    /// leave a ghost on the row it left.
    seen: Vec<PeerAt>,
    /// When each address last failed to answer, so an absent peer is
    /// not redialled on every frame.
    quiet_until: std::collections::HashMap<std::net::SocketAddr, std::time::Instant>,
    /// Rounds in flight, one per address.
    ///
    /// Both halves of a round are IO and neither needs the CRDT:
    /// connecting cost 60.2ms on the thread that draws the window, and
    /// the exchange after it cost 208ms against a host that accepts
    /// and then never speaks the protocol. Both wait on a worker; what
    /// stays here is the arithmetic that needs the session.
    in_flight: std::collections::HashMap<std::net::SocketAddr, Round>,
}

/// What a finished wire round hands back: their sync message and
/// their presence frame, or why neither arrived.
type WireResult = Result<(Vec<u8>, Vec<u8>), String>;

/// How far along a round with one peer is.
///
/// A tick advances it by one step and never waits: the frame loop gets
/// that for free, and anything calling [`SyncApp::sync_with`] once has
/// to call it again.
enum Round {
    /// Waiting for the peer to accept.
    Dialing(std::thread::JoinHandle<std::io::Result<std::net::TcpStream>>),
    /// Connected; our frame is out and theirs is being waited for.
    /// Yields their sync message and their presence frame.
    Exchanging(std::thread::JoinHandle<WireResult>),
}

impl std::fmt::Debug for Round {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Dialing(_) => "Dialing",
            Self::Exchanging(_) => "Exchanging",
        })
    }
}

/// One round on the wire: our frame out, theirs back, presence both
/// ways. Runs on a worker and touches no shared state.
///
/// Length-prefixed the way [`closure_sync::TcpSyncTransport`] frames
/// it, because it is the same wire — this is that round with the CRDT
/// taken out of it, so that the part needing the session can stay on
/// the thread that owns the session.
fn wire_round(stream: &mut std::net::TcpStream, ours: &[u8], presence: &[u8]) -> WireResult {
    use std::io::{Read as _, Write as _};
    fn send(stream: &mut std::net::TcpStream, frame: &[u8]) -> Result<(), String> {
        let len = u32::try_from(frame.len()).unwrap_or(0);
        stream
            .write_all(&len.to_le_bytes())
            .map_err(|e| e.to_string())?;
        stream.write_all(frame).map_err(|e| e.to_string())
    }
    fn recv(stream: &mut std::net::TcpStream) -> Result<Vec<u8>, String> {
        let mut len = [0u8; 4];
        stream.read_exact(&mut len).map_err(|e| e.to_string())?;
        let n = u32::from_le_bytes(len) as usize;
        let mut buf = vec![0u8; n];
        if n > 0 {
            stream.read_exact(&mut buf).map_err(|e| e.to_string())?;
        }
        Ok(buf)
    }
    send(stream, ours)?;
    let theirs = recv(stream)?;
    send(stream, presence)?;
    let their_presence = recv(stream)?;
    Ok((theirs, their_presence))
}

/// Where somebody is: which block, and which line inside it.
///
/// The shell's view of [`closure_sync::Presence`]. Ephemeral by
/// construction — never written to a file, never in the undo tree,
/// never merged into a replica.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerAt {
    /// Who.
    pub peer: String,
    /// Which block id.
    pub block: String,
    /// Which line inside that block's body.
    pub line: u32,
}

/// Which of this host's addresses a peer should be told to dial.
///
/// An explicit choice wins — on a machine that is on a LAN *and* a
/// mesh VPN, both addresses are "local" and only the operator knows
/// which one the peer can route to. Otherwise the bind address is the
/// answer, except when it is the unspecified one (`0.0.0.0` / `::`),
/// which means "every interface" to `bind` and nothing at all to
/// `connect`; then we ask the routing table which source address it
/// would use to leave this host.
fn advertised_ip(bind: std::net::IpAddr, advertise: Option<std::net::IpAddr>) -> std::net::IpAddr {
    if let Some(ip) = advertise {
        return ip;
    }
    if !bind.is_unspecified() {
        return bind;
    }
    detect_outbound_ip().unwrap_or(match bind {
        std::net::IpAddr::V4(_) => std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        std::net::IpAddr::V6(_) => std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
    })
}

/// The source address the kernel would use to reach the outside world.
///
/// A connected UDP socket is the portable way to ask: `connect` on a
/// datagram socket only fixes the peer and picks a route — no packet
/// is sent, nothing is resolved, and no name server is consulted, so
/// this stays honest on an offline machine (it simply returns `None`).
/// The target is in TEST-NET-3 (RFC 5737), an address reserved for
/// documentation precisely so that nothing real is ever implied.
fn detect_outbound_ip() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    socket.connect(("203.0.113.1", 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

impl std::fmt::Debug for SyncApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Hand-written so the signing key never reaches a log line;
        // `finish_non_exhaustive` says out loud that fields are held
        // back rather than forgotten.
        f.debug_struct("SyncApp")
            .field("name", &self.name)
            .field("addr", &self.addr)
            .field("bind", &self.bind)
            .field("peers", &self.peers.len())
            .finish_non_exhaustive()
    }
}

impl SyncApp {
    /// A fresh identity for replica `name`, listening on `addr`.
    ///
    /// The keypair is generated per session: pairing is an explicit act
    /// in both directions, so there is nothing to persist until the
    /// user decides there is.
    #[must_use]
    pub fn new(name: &str, addr: std::net::SocketAddr) -> Self {
        Self::with_bind(name, addr, None)
    }

    /// An identity that binds one address and advertises another.
    ///
    /// The two are the same on a single-homed host and different on
    /// every interesting one: a machine reachable over a mesh VPN binds
    /// `0.0.0.0` and hands out its `100.x` address, because that is the
    /// one its peer can route to. Passing `None` for `advertise` asks
    /// for detection — see [`advertised_ip`].
    ///
    /// The ticket is correct immediately, before anything is bound:
    /// pairing starts by pasting it, and a ticket that only becomes
    /// true after the user finds the "listen" button is a trap.
    #[must_use]
    pub fn with_bind(
        name: &str,
        bind: std::net::SocketAddr,
        advertise: Option<std::net::IpAddr>,
    ) -> Self {
        Self {
            name: name.to_owned(),
            addr: std::net::SocketAddr::new(advertised_ip(bind.ip(), advertise), bind.port()),
            bind,
            advertise,
            signing: closure_sync::generate_key(),
            session: closure_sync::SyncSession::new(name),
            peers: Vec::new(),
            listener: None,
            local: None,
            seen: Vec::new(),
            quiet_until: std::collections::HashMap::new(),
            in_flight: std::collections::HashMap::new(),
        }
    }

    /// Name this peer, as other peers will see it.
    ///
    /// Every shell called itself "local", so two peers on one screen
    /// were both `◉ local` — a badge that says somebody is here and
    /// refuses to say who.
    pub fn set_name(&mut self, name: &str) {
        if !name.is_empty() {
            name.clone_into(&mut self.name);
            self.session = closure_sync::SyncSession::new(name);
        }
    }

    /// Load this vault's signing identity, creating it on first use.
    ///
    /// Without this every launch minted a fresh key, which makes the
    /// whole pairing story hold only until you close the window: the
    /// ticket you handed someone stops matching you, `sync_peers` in
    /// config.org is stale the first time either side reopens, and a
    /// returning peer is not merely unrecognised but *refused*, since
    /// frames are verified against the trusted key.
    ///
    /// It lives under `.closure/` so it is not a note: it does not
    /// appear in the outline and is not committed as content. It
    /// belongs to the vault rather than to the binary, so two vaults on
    /// one machine cannot impersonate each other.
    ///
    /// A key file that will not parse is treated as no key — losing
    /// pairings is recoverable, a shell that refuses to open is not.
    pub fn load_identity(&mut self, vault_root: &std::path::Path) {
        let dir = vault_root.join(".closure");
        let path = dir.join("identity.key");
        if let Ok(bytes) = std::fs::read(&path)
            && let Ok(seed) = <[u8; 32]>::try_from(bytes.as_slice())
        {
            self.signing = closure_sync::SigningKey::from_bytes(&seed);
            return;
        }
        if std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::fs::write(&path, self.signing.to_bytes());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                // A signing key is a secret: readable by its owner and
                // nobody else, the same as an ssh private key.
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }

    /// Say where we are. Sent to every peer on the next round.
    pub fn set_local_presence(&mut self, block: &str, line: u32) {
        self.local = Some(PeerAt {
            peer: self.name.clone(),
            block: block.to_owned(),
            line,
        });
    }

    /// Record where a peer is. One entry per peer: presence is a
    /// position, not a history, so this replaces rather than appends —
    /// a peer that moves must not leave a ghost on the row it left.
    pub fn note_peer(&mut self, peer: &str, block: &str, line: u32) {
        let at = PeerAt {
            peer: peer.to_owned(),
            block: block.to_owned(),
            line,
        };
        match self.seen.iter_mut().find(|p| p.peer == at.peer) {
            Some(slot) => *slot = at,
            None => self.seen.push(at),
        }
    }

    /// Where every peer was when we last heard from it.
    #[must_use]
    pub fn peer_presence(&self) -> &[PeerAt] {
        &self.seen
    }

    /// Our own position, if we have said one.
    #[must_use]
    pub const fn local_presence(&self) -> Option<&PeerAt> {
        self.local.as_ref()
    }

    /// One live round against a peer: our document and our position
    /// out, theirs back, merged.
    ///
    /// This is the piece that did not exist. Pairing, the transport
    /// and the CRDT merge were all real and tested, and nothing ever
    /// dialled — the running shell only wrote bundle files into a
    /// shared folder when a key was pressed.
    ///
    /// # Errors
    ///
    /// The connection, or a malformed frame.
    ///
    /// *Advances* a round rather than performing one. The dial waits
    /// on a worker — it cost 60.2ms on the drawing thread, measured,
    /// and needs nothing from the CRDT session — so the first call
    /// starts the connection and returns `Err("dialing")`, and a later
    /// one finds the socket open and does the round. A frame loop gets
    /// this for free; anything calling it once has to call it again.
    pub fn sync_with(&mut self, addr: std::net::SocketAddr) -> Result<(), String> {
        /// How long to wait for a peer to accept.
        ///
        /// `TcpStream::connect` has *no* timeout, and this runs on the
        /// frame timer. A peer that refuses costs a millisecond; a peer
        /// that is merely absent, on a network that drops rather than
        /// refuses, costs the OS default — about two minutes with the
        /// window wedged. That is what "the app won't start with my
        /// vault" turned out to be.
        ///
        /// Long enough for a LAN or a mesh VPN, which answer in single
        /// -digit milliseconds; short enough that hitting it is a
        /// stutter rather than a hang.
        const DIAL: std::time::Duration = std::time::Duration::from_millis(60);
        /// How long to wait for a peer that *accepted* to say
        /// something.
        ///
        /// Connecting is not the only way to stall: a host with some
        /// other service on the port accepts instantly and then never
        /// speaks our protocol, and the old five seconds of patience
        /// were five seconds of frozen window. A peer that is really
        /// closure answers in single-digit milliseconds on a LAN.
        const READ_BUDGET: std::time::Duration = std::time::Duration::from_millis(200);
        if self
            .quiet_until
            .get(&addr)
            .is_some_and(|t| *t > std::time::Instant::now())
        {
            // Short on purpose: this lands in a peer row beside the
            // address, and a sentence there wraps the row into three
            // ragged lines.
            return Err("quiet".to_owned());
        }
        // Neither the dial nor the wire is this thread's business.
        //
        // `connect_timeout` costs its whole budget whenever a peer is
        // merely absent — 60.2ms measured — and the round after it
        // costs READ_BUDGET whenever a host accepts and then never
        // speaks the protocol: 208ms measured, thirteen dropped
        // frames. Both are IO and neither needs the CRDT.
        //
        // What does need the CRDT is only arithmetic: building our
        // frame from the session, and applying theirs. So the session
        // stays here and the socket goes to a worker.
        match self.in_flight.remove(&addr) {
            None => {
                self.in_flight.insert(
                    addr,
                    Round::Dialing(std::thread::spawn(move || {
                        std::net::TcpStream::connect_timeout(&addr, DIAL)
                    })),
                );
                // Not a failure: the answer is not in yet, and the
                // next tick collects it. Short, because it lands in a
                // peer row beside the address.
                Err("dialing".to_owned())
            }
            Some(Round::Dialing(handle)) => {
                if !handle.is_finished() {
                    self.in_flight.insert(addr, Round::Dialing(handle));
                    return Err("dialing".to_owned());
                }
                let mut stream = handle
                    .join()
                    .map_err(|_| "the dial thread panicked".to_owned())?
                    .map_err(|e| {
                        self.mark_quiet(addr);
                        e.to_string()
                    })?;
                stream
                    .set_read_timeout(Some(READ_BUDGET))
                    .map_err(|e| e.to_string())?;
                // Our half of the round, built here where the session
                // is, and handed over as bytes.
                let ours = closure_sync::SyncMessage::from_session(&self.session).to_bytes();
                let presence = self.presence_frame();
                self.in_flight.insert(
                    addr,
                    Round::Exchanging(std::thread::spawn(move || {
                        wire_round(&mut stream, &ours, &presence)
                    })),
                );
                Err("syncing".to_owned())
            }
            Some(Round::Exchanging(handle)) => {
                if !handle.is_finished() {
                    self.in_flight.insert(addr, Round::Exchanging(handle));
                    return Err("syncing".to_owned());
                }
                let (theirs, presence) = handle
                    .join()
                    .map_err(|_| "the sync thread panicked".to_owned())?
                    .inspect_err(|_| self.mark_quiet(addr))?;
                let message =
                    closure_sync::SyncMessage::from_bytes(&theirs).map_err(|e| e.to_string())?;
                self.session.apply_message(&message);
                self.absorb_presence(&presence);
                Ok(())
            }
        }
    }

    /// Where we are, as a frame for a peer. Empty when we have not
    /// said.
    fn presence_frame(&self) -> Vec<u8> {
        self.local.as_ref().map_or_else(Vec::new, |at| {
            closure_sync::Presence {
                peer: at.peer.clone(),
                block: at.block.clone(),
                line: at.line,
            }
            .encode()
        })
    }

    /// Take note of where a peer said it was.
    fn absorb_presence(&mut self, frame: &[u8]) {
        if let Ok(p) = closure_sync::Presence::decode(frame) {
            self.note_peer(&p.peer, &p.block, p.line);
        }
    }

    /// Back off from an address that did not answer.
    ///
    /// Even a bounded dial costs its budget every time it is tried, and
    /// the frame timer fires every 1.5s — so without this an absent
    /// peer stutters the window for as long as it is absent, which for
    /// a laptop that is simply elsewhere is all day.
    fn mark_quiet(&mut self, addr: std::net::SocketAddr) {
        /// Long enough that an absent peer is free, short enough that
        /// one coming back is picked up while you are still looking.
        const QUIET: std::time::Duration = std::time::Duration::from_secs(20);
        self.quiet_until
            .insert(addr, std::time::Instant::now() + QUIET);
    }

    /// Serve the connections already waiting, without blocking on one
    /// that never comes. Returns how many rounds it served.
    ///
    /// Non-blocking so a shell can call it from its frame loop and a
    /// quiet network costs nothing.
    pub fn serve_pending(&mut self) -> usize {
        /// How long to wait for a client that connected to say
        /// something. The dialling side's `READ_BUDGET`, from the
        /// other end of the same wire.
        const SERVE_BUDGET: std::time::Duration = std::time::Duration::from_millis(200);
        let Some(listener) = self.listener.clone() else {
            return 0;
        };
        if listener.set_nonblocking(true).is_err() {
            return 0;
        }
        let mut served = 0;
        while let Ok((mut stream, _)) = listener.accept() {
            // The accepted socket had no read timeout at all, so a
            // client that connects and then says nothing blocked this
            // thread — the one that draws the window — for as long as
            // it cared to. Anything able to reach the port could do
            // it. The same budget the dialling side uses: a peer that
            // is really closure answers in single-digit milliseconds.
            let ok = stream.set_nonblocking(false).is_ok()
                && stream.set_read_timeout(Some(SERVE_BUDGET)).is_ok()
                && closure_sync::TcpSyncTransport::stream_round_server(
                    &mut stream,
                    &mut self.session,
                )
                .is_ok()
                && self.exchange_presence(&mut stream, false).is_ok();
            if ok {
                served += 1;
            }
        }
        let _ = listener.set_nonblocking(false);
        served
    }

    /// Swap presence frames over an already-synced stream.
    ///
    /// Ordered by role, like the document round, so the two sides do
    /// not sit waiting on each other.
    fn exchange_presence(
        &mut self,
        stream: &mut std::net::TcpStream,
        we_speak_first: bool,
    ) -> Result<(), String> {
        use std::io::{Read as _, Write as _};
        fn send(stream: &mut std::net::TcpStream, frame: &[u8]) -> Result<(), String> {
            let len = u32::try_from(frame.len()).unwrap_or(0);
            stream
                .write_all(&len.to_le_bytes())
                .map_err(|e| e.to_string())?;
            stream.write_all(frame).map_err(|e| e.to_string())
        }
        fn recv(stream: &mut std::net::TcpStream) -> Result<Vec<u8>, String> {
            let mut len = [0u8; 4];
            stream.read_exact(&mut len).map_err(|e| e.to_string())?;
            let n = u32::from_le_bytes(len) as usize;
            let mut buf = vec![0u8; n];
            if n > 0 {
                stream.read_exact(&mut buf).map_err(|e| e.to_string())?;
            }
            Ok(buf)
        }
        let mine = self.local.as_ref().map_or_else(Vec::new, |at| {
            closure_sync::Presence {
                peer: at.peer.clone(),
                block: at.block.clone(),
                line: at.line,
            }
            .encode()
        });
        let theirs = if we_speak_first {
            send(stream, &mine)?;
            recv(stream)?
        } else {
            let theirs = recv(stream)?;
            send(stream, &mine)?;
            theirs
        };
        if let Ok(p) = closure_sync::Presence::decode(&theirs) {
            self.note_peer(&p.peer, &p.block, p.line);
        }
        Ok(())
    }

    /// Point the (not yet opened) socket somewhere else.
    ///
    /// A shell reads `config.org` after the state exists, so the
    /// addresses arrive late; the keypair must survive that, which is
    /// why this is a setter and not a fresh [`Self::with_bind`] — a new
    /// identity would invalidate a ticket the user may already have
    /// handed over. Once a listener is open the bind address is a fact
    /// rather than a preference and only the advertised one moves.
    pub fn rebind(&mut self, bind: std::net::SocketAddr, advertise: Option<std::net::IpAddr>) {
        self.advertise = advertise;
        if self.listener.is_none() {
            self.bind = bind;
        }
        self.addr =
            std::net::SocketAddr::new(advertised_ip(self.bind.ip(), advertise), self.bind.port());
    }

    /// Our replica name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Our verifying key — what a peer checks our frames against.
    #[must_use]
    pub fn public_key(&self) -> closure_sync::VerifyingKey {
        self.signing.verifying_key()
    }

    /// Our signing key, for a shell that is about to dial out.
    #[must_use]
    pub const fn signing_key(&self) -> &closure_sync::SigningKey {
        &self.signing
    }

    /// The one line to hand to whoever you want to sync with.
    #[must_use]
    pub fn ticket(&self) -> String {
        closure_sync::SyncTicket {
            addr: self.addr,
            pubkey: self.public_key(),
        }
        .encode()
    }

    /// Peers we have tickets for.
    #[must_use]
    pub fn peers(&self) -> &[Peer] {
        &self.peers
    }

    /// Keys we trust to sign frames — what the transport is given.
    #[must_use]
    pub fn trusted_keys(&self) -> Vec<closure_sync::VerifyingKey> {
        self.peers.iter().map(|p| p.key).collect()
    }

    /// Our replica, for reading and for shipping to a peer.
    #[must_use]
    pub const fn session(&self) -> &closure_sync::SyncSession {
        &self.session
    }

    /// The replica, mutably — what a shell folds local edits into, and
    /// what a test uses to build one big enough to matter.
    #[must_use]
    pub const fn session_mut(&mut self) -> &mut closure_sync::SyncSession {
        &mut self.session
    }

    /// Add a peer from a pasted ticket.
    ///
    /// # Errors
    ///
    /// The decode failure as a message, or a refusal when the ticket is
    /// our own — pointing a vault at itself would merge it with itself
    /// forever. Pasting the same peer twice is a no-op, not an error.
    pub fn add_peer(&mut self, ticket: &str) -> Result<(), String> {
        let parsed = closure_sync::SyncTicket::decode(ticket).map_err(|e| format!("{e}"))?;
        if parsed.pubkey == self.public_key() {
            return Err("that is our own ticket — hand it to the other peer".to_owned());
        }
        if self.peers.iter().any(|p| p.key == parsed.pubkey) {
            return Ok(());
        }
        self.peers.push(Peer {
            addr: parsed.addr,
            key: parsed.pubkey,
            state: PeerState::Known,
        });
        Ok(())
    }

    /// Drop the peer at `at`, returning its address.
    pub fn forget_peer(&mut self, at: usize) -> Option<std::net::SocketAddr> {
        (at < self.peers.len()).then(|| {
            let peer = self.peers.remove(at);
            self.quiet_until.remove(&peer.addr);
            peer.addr
        })
    }

    /// Record how the last round with `addr` went.
    pub fn record_outcome(&mut self, addr: std::net::SocketAddr, result: Result<usize, String>) {
        if let Some(peer) = self.peers.iter_mut().find(|p| p.addr == addr) {
            peer.state = match result {
                Ok(blocks) => PeerState::Synced { blocks },
                Err(e) => PeerState::Failed(e),
            };
        }
    }

    /// Fold the vault's current state into our replica.
    pub fn snapshot(&mut self, shell: &Shell) {
        for (_, doc) in shell.vault.iter() {
            self.session.record_local(doc);
        }
    }

    /// The file our bundle takes in a shared folder — one per replica,
    /// named after the key that signs it so two machines never collide
    /// and "not ours" is decidable without opening anything.
    fn bundle_name(key: &closure_sync::VerifyingKey) -> String {
        use std::fmt::Write as _;
        let mut hex = String::with_capacity(16);
        for b in key.to_bytes().iter().take(8) {
            let _ = write!(hex, "{b:02x}");
        }
        format!("{hex}.closure-sync")
    }

    /// Leave our replica in `dir` as a signed bundle — sync through a
    /// shared folder rather than a socket.
    ///
    /// Syncthing, a Dropbox, a USB stick, a mounted share: the two
    /// machines never have to be up at the same time or have a route
    /// between them. The bundle *is* the frame the socket would have
    /// sent, signed with the same key, so the trust anchor does not
    /// change — a folder is exactly as trustworthy as whoever can write
    /// to it, which is why the other side verifies rather than believes.
    ///
    /// Rewritten in place on every export: one file per replica, kept
    /// up to date, rather than a pile a folder syncer has to carry.
    ///
    /// # Errors
    ///
    /// The IO failure as a message.
    pub fn export_bundle(&self, dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
        let msg = closure_sync::SyncMessage::from_session(&self.session);
        let bytes = msg.to_signed_bytes(&self.signing);
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(Self::bundle_name(&self.public_key()));
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }

    /// Merge every bundle in `dir` that a paired peer signed.
    ///
    /// Returns how many bundles were actually merged and the
    /// divergences they brought, which are surfaced the same way the
    /// socket's are rather than resolved. Ours is skipped; anything
    /// unsigned, corrupt, or signed by a key whose ticket has not been
    /// pasted is skipped too — a half-written file is what a folder
    /// syncer *does* while it copies, and the next round has to still
    /// work.
    ///
    /// # Errors
    ///
    /// Only a directory that cannot be listed. An empty or absent one
    /// is a round that merged nothing, not a failure.
    pub fn import_bundles(
        &mut self,
        dir: &std::path::Path,
    ) -> Result<(usize, Vec<closure_crdt::FieldConflict>), String> {
        if !dir.exists() {
            return Ok((0, Vec::new()));
        }
        let ours = Self::bundle_name(&self.public_key());
        let trusted = self.trusted_keys();
        let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|e| e.to_str()) == Some("closure-sync")
                    && p.file_name().and_then(|n| n.to_str()) != Some(ours.as_str())
            })
            .collect();
        // Deterministic order: two bundles that touch the same field
        // must merge the same way twice.
        entries.sort();
        let mut merged = 0usize;
        let mut conflicts = Vec::new();
        for path in entries {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(msg) = closure_sync::SyncMessage::from_signed_bytes(&bytes, &trusted) else {
                continue;
            };
            conflicts.extend(self.session.receive_message_with_conflicts(&msg));
            merged += 1;
        }
        Ok((merged, conflicts))
    }

    /// Merge a peer's replica into ours, returning every divergence
    /// the automatic LWW would otherwise have resolved silently.
    pub fn merge_session(
        &mut self,
        other: &closure_sync::SyncSession,
    ) -> Vec<closure_crdt::FieldConflict> {
        self.session.receive_with_conflicts(other.outgoing())
    }

    /// Blocks our replica knows about.
    #[must_use]
    pub fn block_count(&self) -> usize {
        self.session.block_ids().count()
    }

    /// Write the converged replica back into the vault's files,
    /// returning how many fields actually changed.
    ///
    /// The replica converging is only half of a sync: until the merged
    /// state reaches the org files nothing the user can see has
    /// changed, and it is the vault — not the replica — that gets
    /// committed to git and opened in Emacs.
    ///
    /// Only blocks the vault already has are touched. A headline that
    /// exists solely in the peer's replica would need a file to live
    /// in and a place in the tree, and guessing either is worse than
    /// leaving it: convergence of *known* blocks is the promise.
    /// Writes go through the kernel commands like every other edit
    /// (I8), and a field that already matches is skipped, so a
    /// repeated round is a no-op rather than a rewrite of every file.
    pub fn apply_to_vault(&self, shell: &mut Shell) -> usize {
        // One merge, in `Shell`. This was a verbatim copy of the
        // window's loop, carrying the same defect: an id `find_by_id`
        // missed was skipped, so a headline created on the peer never
        // arrived — over a shared folder as well as over a socket. Two
        // copies of one rule is how a bug gets fixed once and survives.
        self.session
            .block_ids()
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .map(|id| {
                let title = self.session.title_of(&id).map(ToOwned::to_owned);
                let body = self.session.body_of(&id);
                shell.apply_peer_block(id.as_str(), title.as_deref(), body.as_deref())
            })
            .sum()
    }

    /// Bind a listener so a peer can dial *in*.
    ///
    /// Pairing needs both directions: handing over a ticket is useless
    /// if nothing answers at the address it names. Binding port 0 asks
    /// the OS for a free one, and the ticket is rewritten to the real
    /// address afterwards — a ticket naming a port nothing listens on
    /// is worse than no ticket.
    ///
    /// # Errors
    ///
    /// The bind failure as a message.
    pub fn listen(&mut self) -> Result<std::net::SocketAddr, String> {
        if let Some(listener) = &self.listener {
            return listener.local_addr().map_err(|e| format!("{e}"));
        }
        let listener = std::net::TcpListener::bind(self.bind).map_err(|e| format!("{e}"))?;
        let bound = listener.local_addr().map_err(|e| format!("{e}"))?;
        self.bind = bound;
        // The port is only knowable after the bind when it was 0, and
        // the address a peer dials is never the unspecified one we may
        // have just bound — so the ticket is recomputed from both.
        self.addr =
            std::net::SocketAddr::new(advertised_ip(bound.ip(), self.advertise), bound.port());
        self.listener = Some(std::sync::Arc::new(listener));
        Ok(bound)
    }

    /// The address our ticket names.
    #[must_use]
    pub const fn ticket_addr(&self) -> std::net::SocketAddr {
        self.addr
    }

    /// The socket we bind (or have bound) — where we listen, as opposed
    /// to [`Self::ticket_addr`], which is where a peer dials.
    #[must_use]
    pub const fn bind_addr(&self) -> std::net::SocketAddr {
        self.bind
    }

    /// Whether it is safe to accept an inbound round right now.
    ///
    /// The transport verifies every frame's signature, but a peer is
    /// only *authenticated* against a trusted set; an empty one is
    /// integrity-only mode, where any self-consistent signature is
    /// accepted. On loopback that set being empty means "anyone on this
    /// machine", which is the user. On `0.0.0.0` it would mean anyone
    /// who can reach the port — and an inbound round writes titles and
    /// bodies into the vault. So a network-facing listener answers only
    /// once it has been given someone to trust.
    ///
    /// # Errors
    ///
    /// The reason to show the user, naming the thing they have to do.
    pub fn inbound_ready(&self) -> Result<(), String> {
        if self.bind.ip().is_loopback() || !self.peers.is_empty() {
            return Ok(());
        }
        Err(format!(
            "listening on {} but trusting nobody — paste your peer's ticket first, \
             or nothing that dials in can be told apart from a stranger",
            self.bind
        ))
    }

    /// The bound listener, for a shell that wants to accept on its own
    /// thread.
    #[must_use]
    pub fn listener(&self) -> Option<std::sync::Arc<std::net::TcpListener>> {
        self.listener.clone()
    }
}

/// One entry in the activity rail: a pane the user can reach with one
/// click, named, keyed and carrying whatever it currently holds.
///
/// The rail exists because the `g`-prefixed chords were the *only* door
/// into the subsystems: pairing, in particular, had no clickable entry
/// point anywhere in any shell, so P2P was undiscoverable to anyone who
/// had not read the keymap. Derived data rather than render-time
/// assembly, so every shell offers the same destinations in the same
/// order with the same chords (I4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    /// Stable identifier (`"peers"`, `"sniffer"`, …) — what tests and
    /// shells match on, never the label.
    pub id: &'static str,
    /// One glyph, for a collapsed rail. Deliberately not a Nerd Font
    /// codepoint: the TUI and the web tier render this too.
    pub icon: &'static str,
    /// What it is, in words. A glyph alone is a guessing game.
    pub label: &'static str,
    /// Registry command a click runs (I8).
    pub command: &'static str,
    /// Chord bound to that command in the active mode — `None` only if
    /// the mode binds none, which `rail.rs` forbids.
    pub chord: Option<String>,
    /// Surface the command opens, so the rail can mark the current one.
    pub surface: ModalSurface,
    /// Live count, when there is something to count. `None` renders no
    /// badge at all rather than a `0`.
    pub badge: Option<String>,
    /// Whether that count is work waiting on the user (unresolved
    /// conflicts, blocked flows) rather than mere activity.
    pub urgent: bool,
    /// Whether this is the surface currently open.
    pub active: bool,
}

/// The rail's fixed running order, as
/// `(id, icon, label, command, surface)`.
///
/// A table rather than a builder: the order is what the user's muscle
/// memory learns, so it is one thing to read and one thing to change.
/// Icons are plain Unicode — the TUI and the web tier paint these too,
/// and a Nerd Font glyph would be a box in both.
const RAIL: &[(&str, &str, &str, &str, ModalSurface)] = &[
    ("outline", "⌂", "Outline", "browse", ModalSurface::Browse),
    ("agenda", "◷", "Agenda", "agenda", ModalSurface::Agenda),
    ("blocks", "⌗", "Blocks", "list-blocks", ModalSurface::Blocks),
    ("graph", "⁂", "Graph", "graph", ModalSurface::Graph),
    (
        "backlinks",
        "⟵",
        "Backlinks",
        "backlinks",
        ModalSurface::Backlinks,
    ),
    ("journal", "≡", "Journal", "journal", ModalSurface::Journal),
    ("cron", "⏱", "Jobs", "cron", ModalSurface::Cron),
    ("peers", "⇄", "Peers", "sync", ModalSurface::Sync),
    ("sniffer", "⇅", "Network", "sniffer", ModalSurface::Sniffer),
    ("assistant", "✦", "Assistant", "llm", ModalSurface::Llm),
    (
        "conflicts",
        "⚠",
        "Conflicts",
        "conflicts",
        ModalSurface::Conflicts,
    ),
    ("palette", "⌘", "Commands", "palette", ModalSurface::Palette),
];

/// How loudly a status-bar indicator should read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndicatorLevel {
    /// Nothing happening; muted.
    Idle,
    /// Doing something, or a permission is granted; accented.
    Active,
    /// Something is waiting on the user; warning colour.
    Warn,
}

/// One item in the bottom-right status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Indicator {
    /// Stable identifier (`"sniffer"`, `"llm"`, …) — what tests and
    /// shells match on, never the label.
    pub id: &'static str,
    /// Short text shown in the bar, usually a glyph plus a count.
    pub label: String,
    /// Longer explanation for a tooltip or the status line.
    pub tooltip: String,
    /// How loudly it should read.
    pub level: IndicatorLevel,
    /// Command a click runs, when there is one.
    pub command: Option<&'static str>,
    /// Chord bound to that command in the active mode.
    pub chord: Option<String>,
}

/// What a right-click landed on, selecting which context menu to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextTarget {
    /// An outline row: the structural commands.
    Row,
    /// The body editor or its preview: editing and code blocks.
    Body,
    /// A detail-pane field: the per-field edit commands.
    Detail,
}

/// The context menu for `target` in `mode`.
///
/// Derived from the mode's keymap, never hand-written per shell: every
/// entry carries the chord that actually runs it (I4), which is how
/// the mouse-only path stays a way to *discover* the keyboard one.
/// A command with no binding in `mode` is dropped rather than shown
/// with a blank chord — the menu shrinks instead of lying.
#[must_use]
pub fn context_menu(
    target: ContextTarget,
    mode: closure_config::InputMode,
) -> Vec<PaletteItemView> {
    let entries: &[(&str, &str)] = match target {
        ContextTarget::Row => &[
            ("Rename", "rename"),
            ("Cycle TODO", "toggle-todo"),
            ("Cycle priority", "cycle-priority"),
            ("Edit tags", "edit-tags"),
            ("Edit property", "edit-property"),
            ("Edit body", "edit-body"),
            ("Fold / unfold", "toggle-fold"),
            ("Promote", "promote"),
            ("Demote", "demote"),
            ("Move up", "move-subtree-up"),
            ("Move down", "move-subtree-down"),
            ("Add sibling", "add-sibling"),
            ("Backlinks", "backlinks"),
            ("Undo history", "undo-history"),
            ("Delete subtree", "delete"),
        ],
        ContextTarget::Body => &[
            ("Edit body", "edit-body"),
            ("Source blocks", "list-blocks"),
            ("Backlinks", "backlinks"),
            ("Undo", "undo"),
            ("Redo", "redo"),
        ],
        ContextTarget::Detail => &[
            ("Rename", "rename"),
            ("Cycle TODO", "toggle-todo"),
            ("Cycle priority", "cycle-priority"),
            ("Edit tags", "edit-tags"),
            ("Edit property", "edit-property"),
            ("Edit body", "edit-body"),
        ],
    };
    entries
        .iter()
        .filter_map(|(label, command)| {
            Action::new(mode, *command).map(|action| PaletteItemView {
                label: (*label).to_owned(),
                action,
            })
        })
        .collect()
}

/// The Notion-style slash command menu (G5c).
///
/// Every palette command whose name fuzzy-matches `query`, as actionable
/// [`PaletteItemView`]s carrying their chord in `mode` (empty query ⇒
/// all). Pure + deterministic — the "/" affordance as data, the same
/// command set every shell drives.
#[must_use]
pub fn slash_menu(query: &str, mode: closure_config::InputMode) -> Vec<PaletteItemView> {
    let mut scored: Vec<(u32, PaletteItemView)> = PALETTE_COMMANDS
        .iter()
        .filter_map(|(name, canonical, _, _)| {
            let score = if query.is_empty() {
                Some(0)
            } else {
                closure_query::fuzzy_score(query, name)
            }?;
            let action = Action::new(mode, *canonical)?;
            Some((
                score,
                PaletteItemView {
                    label: (*name).to_owned(),
                    action,
                },
            ))
        })
        .collect();
    scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    scored.into_iter().map(|(_, it)| it).collect()
}

/// The action behind the Notion block "+" insert affordance (G5c).
///
/// The `add-sibling` command in `mode`, carrying its chord (so the "+"
/// button shows its keybinding, V1). `None` if unbound.
#[must_use]
pub fn block_insert_action(mode: closure_config::InputMode) -> Option<Action> {
    Action::new(mode, "add-sibling")
}

/// A drag-to-reorder gesture (G5c) — a pure state machine.
///
/// [`Self::begin`] records the dragged index, [`Self::over`] the hover
/// target, and [`Self::drop`] yields `(from, to)` once (then resets). The
/// actual move is a registry command at the call site (I8); this only
/// tracks the gesture.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DragReorder {
    from: Option<usize>,
    to: Option<usize>,
}

impl DragReorder {
    /// Begin dragging the element at `i`.
    pub const fn begin(&mut self, i: usize) {
        self.from = Some(i);
    }

    /// Update the hover target to `i`.
    pub const fn over(&mut self, i: usize) {
        self.to = Some(i);
    }

    /// The row the gesture picked up, while it is still in flight.
    ///
    /// A drag is unusable without an insertion indicator, and a shell
    /// can only paint one if it can read the gesture mid-flight —
    /// [`Self::drop`] answers once, at the end, and then resets.
    #[must_use]
    pub const fn source(&self) -> Option<usize> {
        self.from
    }

    /// The row the pointer is currently over, or `None` until it has
    /// moved off the source.
    #[must_use]
    pub const fn target(&self) -> Option<usize> {
        self.to
    }

    /// Abandon the gesture.
    pub const fn cancel(&mut self) {
        self.from = None;
        self.to = None;
    }

    /// Complete the gesture: `(from, to)` if a drag was in progress (the
    /// target defaults to the source when the pointer never moved), then
    /// reset. `None` if no drag began.
    pub const fn drop(&mut self) -> Option<(usize, usize)> {
        let result = match self.from {
            Some(from) => Some((from, if let Some(t) = self.to { t } else { from })),
            None => None,
        };
        self.from = None;
        self.to = None;
        result
    }
}

/// The index order after a drag-reorder move (G5c).
///
/// Moves the element at `from` to position `to` in a list of `len`
/// (remove + re-insert). Out-of-range `from`/`to` yields the identity
/// order — never a panic (I5).
#[must_use]
pub fn reorder_indices(len: usize, from: usize, to: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    if from >= len || to >= len {
        return order;
    }
    let moved = order.remove(from);
    order.insert(to, moved);
    order
}

/// A typed key event from a shell's window (P1).
///
/// The named key, whether Ctrl is held, and the typed character (if
/// printable). Shells translate their native event into this and hand it
/// to [`App::dispatch`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyEvent {
    /// The key name (`"enter"`, `"escape"`, `"down"`, or the char as a
    /// string for a printable key).
    pub key: String,
    /// Whether Ctrl was held.
    pub ctrl: bool,
    /// The typed character, for printable keys.
    pub text: Option<char>,
}

impl KeyEvent {
    /// A key event from its parts.
    #[must_use]
    pub fn new(key: impl Into<String>, ctrl: bool, text: Option<char>) -> Self {
        Self {
            key: key.into(),
            ctrl,
            text,
        }
    }

    /// A named non-printable key (`enter`, `escape`, `down`, …).
    #[must_use]
    pub fn key(name: impl Into<String>) -> Self {
        Self::new(name, false, None)
    }

    /// A `Ctrl`-modified key (the chord leader the shells share).
    #[must_use]
    pub fn ctrl(name: impl Into<String>) -> Self {
        Self::new(name, true, None)
    }

    /// A printable typed character (`key` = the char, `text` = the char).
    #[must_use]
    pub fn char(c: char) -> Self {
        Self::new(c.to_string(), false, Some(c))
    }
}

/// Pure, GPU-free state core for the gpui shell.
///
/// All keyboard behaviour lives here so it is unit-testable without a
/// window (mirrors the TUI `App`). The gpui `Render` adapter (behind
/// the `gpui` feature) only translates key events into [`Self::on_key`]
/// and reads the accessors. Mutations route through [`Shell`], i.e.
/// kernel commands (I8); search reuses `closure_query` (I7).
#[derive(Debug)]
pub struct App {
    query: String,
    selected: usize,
    mode: Mode,
    capture_buf: String,
    rename_target: Option<String>,
    add_target: Option<String>,
    /// Block id whose body the `EditBody` surface is editing.
    edit_target: Option<String>,
    /// Multiline body buffer for the `EditBody` surface.
    body_buf: String,
    /// Block id whose property the `PropertyEdit` surface is editing.
    prop_target: Option<String>,
    /// Property key + value buffers for the `PropertyEdit` surface.
    prop_key: String,
    prop_value: String,
    /// Block id + space-separated buffer for the `TagsEdit` surface.
    tags_target: Option<String>,
    tags_buf: String,
    input_mode: closure_config::InputMode,
    palette_cursor: usize,
    status: String,
    quit: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// Fresh app in Browse mode with an empty query.
    #[must_use]
    pub fn new() -> Self {
        Self {
            query: String::new(),
            selected: 0,
            mode: Mode::Browse,
            capture_buf: String::new(),
            rename_target: None,
            add_target: None,
            edit_target: None,
            body_buf: String::new(),
            prop_target: None,
            prop_key: String::new(),
            prop_value: String::new(),
            tags_target: None,
            tags_buf: String::new(),
            input_mode: closure_config::InputMode::Notion,
            palette_cursor: 0,
            status: "browse — type to filter".to_owned(),
            quit: false,
        }
    }

    /// Palette rows `(command, key-hint)` matching the live palette
    /// filter (held in the capture buffer while in [`Mode::Palette`]),
    /// best fuzzy match first.
    #[must_use]
    pub fn palette_results(&self) -> Vec<(String, String)> {
        let q = &self.capture_buf;
        let mut scored: Vec<(u32, (String, String))> = PALETTE_COMMANDS
            .iter()
            .filter_map(|(name, canonical, _, _)| {
                let sc = if q.is_empty() {
                    Some(0)
                } else {
                    closure_query::fuzzy_score(q, name)
                };
                // Key hint from the active mode's keymap, not hardcoded.
                let key = self.chord_for(canonical).unwrap_or("—");
                sc.map(|s| (s, ((*name).to_owned(), key.to_owned())))
            })
            .collect();
        scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
        scored.into_iter().map(|(_, row)| row).collect()
    }

    /// Index of the highlighted palette row.
    #[must_use]
    pub const fn palette_cursor(&self) -> usize {
        self.palette_cursor
    }

    /// The active editing mode (label/which-key only — the GUI is a
    /// launcher shell, see ROADMAP Decisions).
    #[must_use]
    pub const fn input_mode(&self) -> closure_config::InputMode {
        self.input_mode
    }

    /// Set the active editing mode.
    pub const fn set_mode(&mut self, mode: closure_config::InputMode) {
        self.input_mode = mode;
    }

    fn cycle_mode(&mut self) {
        use closure_config::InputMode as M;
        self.input_mode = match self.input_mode {
            M::Notion => M::Emacs,
            M::Emacs => M::Vim,
            M::Vim => M::Doom,
            M::Doom => M::Helix,
            M::Helix => M::Notion,
        };
        self.set_status(&format!("mode: {:?}", self.input_mode));
    }

    /// Move the selection to row `i`, clamped to the current result
    /// set. Used by mouse clicks on a row.
    pub fn select(&mut self, i: usize, shell: &Shell) {
        let last = self.rows(shell).len().saturating_sub(1);
        self.selected = i.min(last);
    }

    /// Fold or unfold the selected subtree (`C-f` / palette `fold`).
    ///
    /// Writes the org-standard `:VISIBILITY:` property through the
    /// registry (I8, undoable I3), so the fold persists between program
    /// runs and is honoured by Emacs org-mode too.
    pub fn toggle_fold(&mut self, shell: &mut Shell) {
        let rows = self.rows(shell);
        let Some(row) = rows.get(self.selected) else {
            return;
        };
        let title = row.title.clone();
        let bid = closure_core::BlockId::from_existing(&row.id);
        match toggle_visibility(shell, &bid) {
            Some(true) => self.set_status(&format!("folded: {title}")),
            Some(false) => self.set_status(&format!("unfolded: {title}")),
            None => self.set_status(&format!("fold failed: {title}")),
        }
        self.selected = self.selected.min(self.rows(shell).len().saturating_sub(1));
    }

    /// Begin a capture (Notion "＋" affordance / `C-c`): switch to the
    /// capture surface with an empty title buffer.
    pub fn begin_capture(&mut self) {
        self.mode = Mode::Capture;
        self.capture_buf.clear();
        self.set_status("capture: type a title");
    }

    /// Begin adding a sibling after the selected row (Notion "＋" / `C-a`).
    /// No-op when there is no selection. Mouse + keyboard share this.
    pub fn begin_add_sibling(&mut self, shell: &Shell) {
        if let Some(row) = self.rows(shell).get(self.selected) {
            self.add_target = Some(row.id.clone());
            self.capture_buf.clear();
            self.mode = Mode::AddSibling;
            self.set_status("add sibling: type a title");
        }
    }

    /// Begin renaming the selected row (double-click / `C-r`), prefilling
    /// the buffer with its current title. No-op without a selection.
    pub fn begin_rename(&mut self, shell: &Shell) {
        if let Some(row) = self.rows(shell).get(self.selected) {
            self.rename_target = Some(row.id.clone());
            self.capture_buf.clear();
            self.capture_buf.push_str(&row.title);
            self.mode = Mode::Rename;
            self.set_status("rename: edit the title");
        }
    }

    /// Begin editing the selected headline's body (org-edit-special),
    /// prefilling the buffer with the current body. No-op without a
    /// selection. Commit with [`Self::commit_edit_body`] or cancel with
    /// Esc.
    pub fn begin_edit_body(&mut self, shell: &Shell) {
        let Some(row) = self.rows(shell).get(self.selected).cloned() else {
            return;
        };
        let body = self.detail(shell).map(|d| d.body).unwrap_or_default();
        self.edit_target = Some(row.id);
        self.body_buf = body;
        self.mode = Mode::EditBody;
        self.set_status("edit body — save to commit, Esc to cancel");
    }

    /// The body editor buffer (read).
    #[must_use]
    pub fn body_buffer(&self) -> &str {
        &self.body_buf
    }

    /// Mutable body buffer, for the egui multiline `TextEdit` to bind to
    /// (the widget mutates the buffer in place; commit reads it back).
    pub const fn body_buffer_mut(&mut self) -> &mut String {
        &mut self.body_buf
    }

    /// Commit the body editor buffer to the target headline through the
    /// kernel command (I8), then return to Browse. No-op if not editing.
    pub fn commit_edit_body(&mut self, shell: &mut Shell) {
        if let Some(id) = self.edit_target.take() {
            let bid = closure_core::BlockId::from_existing(&id);
            // Org bodies are newline-terminated; without a trailing
            // newline a following sibling headline would be absorbed.
            let mut body = closure_org::escape_body(&self.body_buf);
            if !body.is_empty() && !body.ends_with('\n') {
                body.push('\n');
            }
            match shell.set_body(&bid, &body) {
                Ok(()) => self.set_status("body saved"),
                Err(e) => self.status = format!("save failed: {e}"),
            }
        }
        self.body_buf.clear();
        self.mode = Mode::Browse;
    }

    /// Cancel body editing without writing.
    fn cancel_edit_body(&mut self) {
        self.edit_target = None;
        self.body_buf.clear();
        self.mode = Mode::Browse;
        self.set_status("edit cancelled");
    }

    /// Begin adding a new property to the selected headline (empty
    /// key+value form). No-op without a selection.
    pub fn begin_add_property(&mut self, shell: &Shell) {
        let Some(row) = self.rows(shell).get(self.selected).cloned() else {
            return;
        };
        self.prop_target = Some(row.id);
        self.prop_key.clear();
        self.prop_value.clear();
        self.mode = Mode::PropertyEdit;
        self.set_status("property — key + value, save to commit");
    }

    /// Begin editing an existing property `key` on the selected
    /// headline, prefilling its current value. No-op without a
    /// selection.
    pub fn begin_edit_property(&mut self, shell: &Shell, key: &str) {
        let Some(row) = self.rows(shell).get(self.selected).cloned() else {
            return;
        };
        let value = self
            .detail(shell)
            .and_then(|d| {
                d.properties
                    .into_iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v)
            })
            .unwrap_or_default();
        self.prop_target = Some(row.id);
        self.prop_key.clear();
        self.prop_key.push_str(key);
        self.prop_value = value;
        self.mode = Mode::PropertyEdit;
        self.set_status("property — edit value, save to commit");
    }

    /// Property key buffer (read) + its mutable form for the egui field.
    #[must_use]
    pub fn prop_key(&self) -> &str {
        &self.prop_key
    }
    /// Mutable property-key buffer for the egui text field.
    pub const fn prop_key_mut(&mut self) -> &mut String {
        &mut self.prop_key
    }
    /// Property value buffer (read).
    #[must_use]
    pub fn prop_value(&self) -> &str {
        &self.prop_value
    }
    /// Mutable property-value buffer for the egui text field.
    pub const fn prop_value_mut(&mut self) -> &mut String {
        &mut self.prop_value
    }

    /// Commit the property (key,value) to the target headline through
    /// the kernel command (I8), then return to Browse. No-op if not
    /// editing or the key is empty.
    pub fn commit_property(&mut self, shell: &mut Shell) {
        if let Some(id) = self.prop_target.take()
            && !self.prop_key.trim().is_empty()
        {
            let bid = closure_core::BlockId::from_existing(&id);
            match shell.set_property(&bid, self.prop_key.trim(), &self.prop_value) {
                Ok(()) => self.set_status("property saved"),
                Err(e) => self.status = format!("property save failed: {e}"),
            }
        }
        self.prop_key.clear();
        self.prop_value.clear();
        self.mode = Mode::Browse;
    }

    /// Cancel property editing without writing.
    fn cancel_property(&mut self) {
        self.prop_target = None;
        self.prop_key.clear();
        self.prop_value.clear();
        self.mode = Mode::Browse;
        self.set_status("property edit cancelled");
    }

    /// Cycle the selected headline's TODO keyword None -> TODO -> DONE
    /// -> None through the kernel command (I8). No-op without a
    /// selection.
    pub fn cycle_todo(&mut self, shell: &mut Shell) {
        let Some(row) = self.rows(shell).get(self.selected).cloned() else {
            return;
        };
        let next = match self.detail(shell).and_then(|d| d.todo) {
            None => Some("TODO"),
            Some(k) if k == "TODO" => Some("DONE"),
            Some(_) => None,
        };
        let bid = closure_core::BlockId::from_existing(&row.id);
        match shell.set_todo(&bid, next) {
            Ok(()) => self.set_status(next.map_or("todo cleared", |k| {
                if k == "TODO" {
                    "todo: TODO"
                } else {
                    "todo: DONE"
                }
            })),
            Err(e) => self.status = format!("todo failed: {e}"),
        }
    }

    /// Set (or clear) the selected headline's priority through the
    /// kernel command (I8). No-op without a selection.
    pub fn set_priority_cmd(&mut self, shell: &mut Shell, priority: Option<char>) {
        let Some(row) = self.rows(shell).get(self.selected).cloned() else {
            return;
        };
        let bid = closure_core::BlockId::from_existing(&row.id);
        match shell.set_priority(&bid, priority) {
            Ok(()) => self.set_status("priority updated"),
            Err(e) => self.status = format!("priority failed: {e}"),
        }
    }

    /// Begin editing the selected headline's tags (space-separated
    /// buffer prefilled with the current tags). No-op without a
    /// selection.
    pub fn begin_edit_tags(&mut self, shell: &Shell) {
        let Some(row) = self.rows(shell).get(self.selected).cloned() else {
            return;
        };
        let tags = self.detail(shell).map(|d| d.tags).unwrap_or_default();
        self.tags_target = Some(row.id);
        self.tags_buf = tags.join(" ");
        self.mode = Mode::TagsEdit;
        self.set_status("tags — space-separated, save to commit");
    }

    /// Tags buffer (read) + its mutable form for the egui text field.
    #[must_use]
    pub fn tags_buffer(&self) -> &str {
        &self.tags_buf
    }
    /// Mutable tags buffer for the egui text field.
    pub const fn tags_buffer_mut(&mut self) -> &mut String {
        &mut self.tags_buf
    }

    /// Property key buffer (read) for the `PropertyEdit` surface.
    #[must_use]
    pub fn property_key(&self) -> &str {
        &self.prop_key
    }

    /// Property value buffer (read) for the `PropertyEdit` surface.
    #[must_use]
    pub fn property_value(&self) -> &str {
        &self.prop_value
    }

    /// Commit the tags buffer (split on whitespace) to the target
    /// headline through the kernel command (I8), then return to Browse.
    pub fn commit_tags(&mut self, shell: &mut Shell) {
        if let Some(id) = self.tags_target.take() {
            let tags: Vec<String> = self
                .tags_buf
                .split_whitespace()
                .map(ToOwned::to_owned)
                .collect();
            let bid = closure_core::BlockId::from_existing(&id);
            match shell.set_tags(&bid, &tags) {
                Ok(()) => self.set_status("tags saved"),
                Err(e) => self.status = format!("tags save failed: {e}"),
            }
        }
        self.tags_buf.clear();
        self.mode = Mode::Browse;
    }

    /// Cancel tag editing without writing.
    fn cancel_tags(&mut self) {
        self.tags_target = None;
        self.tags_buf.clear();
        self.mode = Mode::Browse;
        self.set_status("tags edit cancelled");
    }

    /// Live filter query.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Highlighted row index.
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }

    /// Active input surface.
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// In-progress capture title.
    #[must_use]
    pub fn capture_buffer(&self) -> &str {
        &self.capture_buf
    }

    /// One-line status / feedback message.
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Whether the user asked to quit.
    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.quit
    }

    /// Which-key style hint line for the active mode (vision: every
    /// UI element shows its keybindings).
    #[must_use]
    pub fn key_hints(&self) -> String {
        let body = match self.mode {
            // Browse hints come from the keymap source of truth (I4), so
            // every shown chord is the real binding for the active mode,
            // never a hardcoded string (vision: every UI element shows
            // its keybinding).
            Mode::Browse => {
                return format!(
                    "[{:?}] type: filter   {}",
                    self.input_mode,
                    self.command_hints()
                );
            }
            Mode::Capture => "capture title — Enter: save   Esc: cancel",
            Mode::Rename => "rename — Enter: save   Esc: cancel",
            Mode::AddSibling => "add sibling — Enter: save   Esc: cancel",
            Mode::Palette => "command palette — type to filter   Enter: run   Esc: cancel",
            Mode::EditBody => "edit body — C-c C-c: save & close   C-s: save   Esc: cancel",
            Mode::PropertyEdit => "property — fill key + value   Save: commit   Esc: cancel",
            Mode::TagsEdit => "tags — space-separated   Save: commit   Esc: cancel",
        };
        format!("[{:?}] {body}", self.input_mode)
    }

    /// Which-key line for the active mode, built from
    /// [`closure_input::mode_keymap`] — the single keymap source of
    /// truth (I4). Shared shape with [`ModalApp::key_hints`].
    fn command_hints(&self) -> String {
        closure_input::mode_keymap(self.input_mode)
            .iter()
            .map(|(chord, cmd)| format!("{chord}:{cmd}"))
            .collect::<Vec<_>>()
            .join("  ")
    }

    /// The chord bound to `command` in the active mode, for labelling
    /// actionable widgets (the egui "＋" buttons) with their real key.
    #[must_use]
    pub fn chord_for(&self, command: &str) -> Option<&'static str> {
        closure_input::chord_for_command(self.input_mode, command)
    }

    /// Rows for the current query, each carrying its block id, level,
    /// and TODO keyword. Empty query lists every headline in file
    /// order; otherwise fuzzy matches, best first (reusing
    /// `closure_query`, I7).
    #[must_use]
    pub fn rows(&self, shell: &Shell) -> Vec<Row> {
        outline_rows(shell, &self.query)
    }

    /// The visible slice of rows for a viewport of `page` rows, plus
    /// its start offset, chosen so the selection stays on screen. Caps
    /// the number of rendered nodes for large vaults; stateless (offset
    /// derived from the selection each call).
    #[must_use]
    pub fn view_window(&self, shell: &Shell, page: usize) -> (usize, Vec<Row>) {
        let rows = self.rows(shell);
        if page == 0 || rows.len() <= page {
            return (0, rows);
        }
        let max_offset = rows.len() - page;
        let offset = self.selected.saturating_sub(page - 1).min(max_offset);
        let slice = rows[offset..offset + page].to_vec();
        (offset, slice)
    }

    /// Full preview of the currently-selected headline (resolved by
    /// its stable id through the vault index), for the detail pane.
    #[must_use]
    pub fn detail(&self, shell: &Shell) -> Option<Detail> {
        let rows = self.rows(shell);
        let row = rows.get(self.selected)?;
        let bid = closure_core::BlockId::from_existing(&row.id);
        let (h, path) = shell.vault.find_by_id(&bid)?;
        Some(Detail::of(h, path, String::new()))
    }

    /// The declarative [`Node`] tree describing the current screen (V1).
    ///
    /// Pure function of state — any embedder (TUI, web, egui, gpui)
    /// renders the same tree. Every actionable node carries its chord by
    /// construction ([`Action`]), so the "show keybinding everywhere"
    /// rule cannot be violated.
    #[must_use]
    pub fn view(&self, shell: &Shell) -> Node {
        let hints = Node::Hints {
            line: self.key_hints(),
        };
        let m = self.input_mode;
        let input_pane = |label: &str, buffer: &str| Node::Pane {
            title: label.to_owned(),
            children: vec![
                Node::Input {
                    label: label.to_owned(),
                    buffer: buffer.to_owned(),
                },
                hints.clone(),
            ],
        };
        match self.mode {
            Mode::Browse => {
                let rows = self
                    .rows(shell)
                    .into_iter()
                    .map(|r| {
                        let icon = r.todo.as_deref().map(|t| todo_glyph(t).to_owned());
                        let badges = if row_is_folded(shell, &r.id) {
                            vec![FOLD_MARKER.to_owned()]
                        } else {
                            Vec::new()
                        };
                        RowView::new(r.id, r.title, r.level, r.todo)
                            .with_icon(icon)
                            .with_badges(badges)
                    })
                    .collect();
                let mut children = vec![Node::Rows {
                    rows,
                    selected: self.selected,
                }];
                if let Some(d) = self.detail(shell) {
                    children.push(Self::detail_node(m, &d));
                }
                children.push(hints);
                Node::Pane {
                    title: "closure".to_owned(),
                    children,
                }
            }
            Mode::Palette => {
                let items = PALETTE_COMMANDS
                    .iter()
                    .filter_map(|(label, canonical, _, _)| {
                        let matches = self.capture_buf.is_empty()
                            || closure_query::fuzzy_score(&self.capture_buf, label).is_some();
                        if !matches {
                            return None;
                        }
                        Action::new(m, *canonical).map(|action| PaletteItemView {
                            label: (*label).to_owned(),
                            action,
                        })
                    })
                    .collect();
                Node::Pane {
                    title: "palette".to_owned(),
                    children: vec![
                        Node::Palette {
                            items,
                            cursor: self.palette_cursor,
                        },
                        hints,
                    ],
                }
            }
            Mode::Capture => input_pane("capture", &self.capture_buf),
            Mode::Rename => input_pane("rename", &self.capture_buf),
            Mode::AddSibling => input_pane("add sibling", &self.capture_buf),
            Mode::EditBody => input_pane("edit body", &self.body_buf),
            Mode::TagsEdit => input_pane("tags", &self.tags_buf),
            Mode::PropertyEdit => Node::Pane {
                title: "property".to_owned(),
                children: vec![
                    Node::Input {
                        label: "key".to_owned(),
                        buffer: self.prop_key.clone(),
                    },
                    Node::Input {
                        label: "value".to_owned(),
                        buffer: self.prop_value.clone(),
                    },
                    hints,
                ],
            },
        }
    }

    /// Build the detail pane node, attaching the click-to-edit action
    /// (with its chord, V1 invariant) to each editable field.
    fn detail_node(mode: closure_config::InputMode, d: &Detail) -> Node {
        let mut fields = vec![FieldView {
            label: "title".to_owned(),
            value: d.title.clone(),
            action: Action::new(mode, "rename"),
        }];
        fields.push(FieldView {
            label: "todo".to_owned(),
            value: d.todo.clone().unwrap_or_default(),
            action: Action::new(mode, "toggle-todo"),
        });
        if let Some(p) = d.priority {
            fields.push(FieldView {
                label: "priority".to_owned(),
                value: p.to_string(),
                action: Action::new(mode, "cycle-priority"),
            });
        }
        fields.push(FieldView {
            label: "tags".to_owned(),
            value: d.tags.join(" "),
            action: Action::new(mode, "edit-tags"),
        });
        for (k, v) in &d.properties {
            fields.push(FieldView {
                label: k.clone(),
                value: v.clone(),
                action: Action::new(mode, "edit-property"),
            });
        }
        fields.push(FieldView {
            label: "body".to_owned(),
            value: d.body.clone(),
            action: Action::new(mode, "edit-body"),
        });
        Node::Detail { fields }
    }

    /// Feed one key. `key` is the gpui key name (`"a"`, `"enter"`,
    /// `"backspace"`, `"escape"`, `"down"`, `"up"`, …); `ctrl` is the
    /// control modifier; `text` is the typed character when the key
    /// produced printable, unmodified input.
    /// The unified input→state→view step (P1): apply `event` via the
    /// mode-aware [`Self::on_key`], then return the fresh
    /// [`ViewTree`](Node). The ONE call every shell's window delegates to,
    /// so editing behaviour is the tested core, not per-shell key logic.
    pub fn dispatch(&mut self, shell: &mut Shell, event: &KeyEvent) -> Node {
        self.on_key(shell, &event.key, event.ctrl, event.text);
        self.view(shell)
    }

    /// Apply one key in the active mode (the low-level handler behind
    /// [`Self::dispatch`]): routes to the per-mode key handler, mutating
    /// through [`Shell`] (I8). Most shells call `dispatch` instead, which
    /// also returns the refreshed view.
    pub fn on_key(&mut self, shell: &mut Shell, key: &str, ctrl: bool, text: Option<char>) {
        if ctrl && key == "q" {
            self.quit = true;
            return;
        }
        match self.mode {
            Mode::Capture => self.on_capture_key(shell, key, text),
            Mode::Rename => self.on_rename_key(shell, key, text),
            Mode::AddSibling => self.on_add_key(shell, key, text),
            Mode::Palette => self.on_palette_key(shell, key, text),
            Mode::EditBody => self.on_editbody_key(shell, key, ctrl, text),
            Mode::PropertyEdit => self.on_property_key(key),
            Mode::TagsEdit => self.on_tags_key(key),
            Mode::Browse => self.on_browse_key(shell, key, ctrl, text),
        }
    }

    /// Tags editor keys for keyboard fallback: Esc cancels. The text
    /// field owns typing; commit is the Save affordance / `commit_tags`.
    fn on_tags_key(&mut self, key: &str) {
        if key == "escape" {
            self.cancel_tags();
        }
    }

    /// Property editor keys for keyboard fallback: Esc cancels. Typing
    /// the key/value uses the egui text fields (`prop_key_mut` /
    /// `prop_value_mut`); commit is the Save affordance / `commit_property`.
    fn on_property_key(&mut self, key: &str) {
        if key == "escape" {
            self.cancel_property();
        }
    }

    /// Body editor keys: Esc cancels, `C-<enter>` commits, plain Enter
    /// inserts a newline, Backspace deletes, printable chars append.
    /// (egui also binds a multiline `TextEdit` to `body_buffer_mut` + a
    /// Save button; this path makes it keyboard-drivable + testable.)
    fn on_editbody_key(&mut self, shell: &mut Shell, key: &str, ctrl: bool, text: Option<char>) {
        match key {
            "escape" => self.cancel_edit_body(),
            "enter" if ctrl => self.commit_edit_body(shell),
            "enter" => self.body_buf.push('\n'),
            "backspace" => {
                self.body_buf.pop();
            }
            _ => {
                if let Some(c) = text.filter(|_| !ctrl) {
                    self.body_buf.push(c);
                }
            }
        }
    }

    fn on_palette_key(&mut self, shell: &mut Shell, key: &str, text: Option<char>) {
        match key {
            "escape" => {
                self.mode = Mode::Browse;
                self.capture_buf.clear();
            }
            "down" => {
                let last = self.palette_results().len().saturating_sub(1);
                self.palette_cursor = (self.palette_cursor + 1).min(last);
            }
            "up" => self.palette_cursor = self.palette_cursor.saturating_sub(1),
            "backspace" => {
                self.capture_buf.pop();
                self.palette_cursor = 0;
            }
            "enter" => {
                let pick = self
                    .palette_results()
                    .get(self.palette_cursor)
                    .map(|(name, _)| name.clone());
                self.mode = Mode::Browse;
                self.capture_buf.clear();
                if let Some(cmd) = pick {
                    self.run_palette_command(shell, &cmd);
                }
            }
            _ => {
                if let Some(c) = text {
                    self.capture_buf.push(c);
                    self.palette_cursor = 0;
                }
            }
        }
    }

    /// Execute a command chosen from the palette, reusing the same
    /// surfaces the key bindings drive.
    fn run_palette_command(&mut self, shell: &mut Shell, cmd: &str) {
        let rows = self.rows(shell);
        match cmd {
            "next-file" => {
                let last = rows.len().saturating_sub(1);
                self.selected = (self.selected + 1).min(last);
            }
            "prev-file" => self.selected = self.selected.saturating_sub(1),
            "capture" => self.begin_capture(),
            "add-sibling" => self.begin_add_sibling(shell),
            "rename" => self.begin_rename(shell),
            "delete" => {
                if let Some(row) = rows.get(self.selected) {
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    let _ = shell.remove_subtree(&bid);
                    self.selected = self.selected.min(self.rows(shell).len().saturating_sub(1));
                }
            }
            "open" => {
                if let Some(row) = rows.get(self.selected) {
                    self.status = format!("{} — {}", row.path, row.title);
                }
            }
            "cycle-mode" => self.cycle_mode(),
            "fold" => self.toggle_fold(shell),
            "quit" => self.quit = true,
            _ => {}
        }
    }

    fn on_add_key(&mut self, shell: &mut Shell, key: &str, text: Option<char>) {
        match key {
            "escape" => {
                self.mode = Mode::Browse;
                self.capture_buf.clear();
                self.add_target = None;
                self.set_status("add cancelled");
            }
            "enter" => {
                if let Some(after) = self.add_target.take()
                    && !self.capture_buf.is_empty()
                {
                    let bid = closure_core::BlockId::from_existing(&after);
                    match shell.add_sibling(&bid, &self.capture_buf) {
                        Ok(()) => self.status = format!("added: {}", self.capture_buf),
                        Err(e) => self.status = format!("add failed: {e}"),
                    }
                }
                self.mode = Mode::Browse;
                self.capture_buf.clear();
            }
            "backspace" => {
                self.capture_buf.pop();
            }
            _ => {
                if let Some(c) = text {
                    self.capture_buf.push(c);
                }
            }
        }
    }

    fn on_rename_key(&mut self, shell: &mut Shell, key: &str, text: Option<char>) {
        match key {
            "escape" => {
                self.mode = Mode::Browse;
                self.capture_buf.clear();
                self.rename_target = None;
                self.set_status("rename cancelled");
            }
            "enter" => {
                if let Some(id) = self.rename_target.take()
                    && !self.capture_buf.is_empty()
                {
                    let bid = closure_core::BlockId::from_existing(&id);
                    match shell.rename_headline(&bid, &self.capture_buf) {
                        Ok(()) => self.status = format!("renamed to {}", self.capture_buf),
                        Err(e) => self.status = format!("rename failed: {e}"),
                    }
                }
                self.mode = Mode::Browse;
                self.capture_buf.clear();
            }
            "backspace" => {
                self.capture_buf.pop();
            }
            _ => {
                if let Some(c) = text {
                    self.capture_buf.push(c);
                }
            }
        }
    }

    fn set_status(&mut self, s: &str) {
        self.status.clear();
        self.status.push_str(s);
    }

    fn on_capture_key(&mut self, shell: &mut Shell, key: &str, text: Option<char>) {
        match key {
            "escape" => {
                self.mode = Mode::Browse;
                self.capture_buf.clear();
                self.set_status("capture cancelled");
            }
            "enter" => {
                if !self.capture_buf.is_empty() {
                    match shell.capture(&self.capture_buf) {
                        Ok(_) => self.status = format!("captured: {}", self.capture_buf),
                        Err(e) => self.status = format!("capture failed: {e}"),
                    }
                }
                self.mode = Mode::Browse;
                self.capture_buf.clear();
            }
            "backspace" => {
                self.capture_buf.pop();
            }
            _ => {
                if let Some(c) = text {
                    self.capture_buf.push(c);
                }
            }
        }
    }

    fn on_browse_key(&mut self, shell: &mut Shell, key: &str, ctrl: bool, text: Option<char>) {
        let rows = self.rows(shell);
        let last = rows.len().saturating_sub(1);
        match key {
            "c" if ctrl => self.begin_capture(),
            // Notion-style slash command: `/` on an empty filter opens
            // the command palette; mid-query it's a literal filter char.
            "/" if !ctrl && self.query.is_empty() => {
                self.mode = Mode::Palette;
                self.capture_buf.clear();
                self.palette_cursor = 0;
                self.set_status("command palette — type to filter, Enter to run");
            }
            "t" if ctrl => self.cycle_mode(),
            "f" if ctrl => self.toggle_fold(shell),
            "a" if ctrl => self.begin_add_sibling(shell),
            "r" if ctrl => self.begin_rename(shell),
            "d" if ctrl => {
                if let Some(row) = rows.get(self.selected) {
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    let title = row.title.clone();
                    match shell.remove_subtree(&bid) {
                        Ok(()) => {
                            self.status = format!("deleted: {title}");
                            self.selected =
                                self.selected.min(self.rows(shell).len().saturating_sub(1));
                        }
                        Err(e) => self.status = format!("delete failed: {e}"),
                    }
                }
            }
            "escape" => {
                self.query.clear();
                self.selected = 0;
                self.set_status("browse — type to filter");
            }
            "down" => self.selected = (self.selected + 1).min(last),
            "up" => self.selected = self.selected.saturating_sub(1),
            "n" if ctrl => self.selected = (self.selected + 1).min(last),
            "p" if ctrl => self.selected = self.selected.saturating_sub(1),
            "enter" => {
                if let Some(row) = rows.get(self.selected) {
                    self.status = format!("{} — {}", row.path, row.title);
                }
            }
            "backspace" => {
                self.query.pop();
                self.selected = 0;
            }
            _ => {
                if let Some(c) = text.filter(|_| !ctrl) {
                    self.query.push(c);
                    self.selected = 0;
                }
            }
        }
    }
}

/// Input surface for the modal command-surface experiment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalSurface {
    /// Keys are commands resolved against the active mode's keymap.
    Browse,
    /// A search overlay: typing filters, Enter picks, Esc cancels.
    Search,
    /// Typing the title of a new capture entry.
    Capture,
    /// Editing the selected headline's body (org-edit-special).
    EditBody,
    /// Read-only list of headlines linking to the selected one.
    Backlinks,
    /// Read-only agenda: scheduled/deadline entries across the vault.
    Agenda,
    /// Read-only list of every `#+BEGIN_SRC` block across the vault.
    Blocks,
    /// Editing the selected headline's tags (space-separated buffer).
    TagsEdit,
    /// Editing a property on the selected headline (`key value` buffer).
    PropertyEdit,
    /// Renaming the selected headline (single-line title buffer,
    /// prefilled with the current title).
    Rename,
    /// Typing the title of a new sibling inserted after the selected row.
    AddSibling,
    /// Fuzzy command palette over the shared [`command_palette`] source.
    Palette,
    /// Read-only undo-tree of the selected row's document (I3).
    UndoHistory,
    /// Every headline in the selected row's file, flat.
    Headlines,
    /// Notion-style database table over the whole vault.
    DbView,
    /// Full-text search over headline *bodies* (the outline search
    /// only sees titles).
    BodySearch,
    /// Captured network flows with their allow/block rules (X3).
    Sniffer,
    /// The assistant's setup screen: every `llm_*` option with what it
    /// is set to, edited here and written back into `config.org`.
    Settings,
    /// Typing the new value for one setting, prefilled with the old.
    Setting,
    /// CRDT field conflicts awaiting an ours/theirs decision.
    Conflicts,
    /// The vim-style `:` command line.
    Ex,
    /// The message log — every status line this session has shown,
    /// because the bottom line only ever holds the newest.
    Messages,
    /// org-edit-special: one source block, on its own, in its own
    /// language. `C-Enter` writes it back, `Esc` discards.
    EditBlock,
    /// Pairing and collaboration: our ticket, the peers we have, and
    /// what the last round with each of them did.
    Sync,
    /// The link graph: hubs, orphans, dead links.
    Graph,
    /// The recorded command journal.
    Journal,
    /// Scheduled jobs declared in the vault.
    Cron,
    /// The assistant: a transcript, a question field, and what the
    /// vault's `config.org` says is behind it.
    Llm,
    /// The whole file, as one buffer — the editor view. `C-Enter` (or
    /// `:w`) writes it back, `Esc` abandons it.
    EditFile,
    /// The open-buffer list: everything this session has a buffer for,
    /// most recently visited first, filtered as you type (Q1-B1).
    Buffers,
    /// The file picker: the files this vault has, the ones recent
    /// sessions were in first (Q1-B4).
    Files,
    /// Doom's `find-file`: walk the vault's directories, and make what
    /// is not there yet.
    FindFile,
    /// One picture, as large as the window will make it.
    ImageView,
    /// The date picker: a month grid over `SCHEDULED:` or `DEADLINE:`
    /// (Q3-V4).
    DatePick,
    /// The refile target picker: every other headline in the vault,
    /// filtered as you type (Q3-V1).
    Refile,
    /// The tag picker: every tag the vault uses, ticked where this
    /// headline carries it (Q3-V6).
    TagPick,
    /// The manual, read-only, generated on the way in.
    Manual,
    /// `C-h k`: waiting for the one key to describe.
    ///
    /// A surface rather than a prompt because it takes a *chord*, not
    /// text — the next stroke is the answer, and it must not be
    /// resolved as a command on the way.
    DescribeKey,
    /// `C-c C-l`: which kind of link this is, then where it goes, then
    /// what to call it. One surface rather than three, because the step
    /// is already written down — the type picked, and the destination
    /// settled — and a surface that repeats it would be a second owner
    /// of the same fact.
    InsertLink,
}

/// Which of the two shapes of the shell you are working in.
///
/// The GUI grew Notion-shaped: rows you click, a detail pane, a rail.
/// That is the right shape for a mouse and the wrong one for someone who
/// lives in Doom, where the *file* is the interface and the whole frame
/// is a buffer. Both are the same vault and the same commands; what
/// changes is what fills the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// The outline, the detail pane and the rail: rows you click.
    Clickable,
    /// The file itself, in one full-window buffer.
    Editor,
}

impl ViewMode {
    /// The view an input mode starts in.
    ///
    /// A modal mode is a statement about how you work: Vim, Doom and
    /// Helix users edit files. Notion and Emacs mode start on the
    /// outline — Emacs included, because its bindings here are the
    /// launcher's `C-c` chords rather than evil's.
    #[must_use]
    pub const fn for_input(mode: InputMode) -> Self {
        match mode {
            InputMode::Vim | InputMode::Doom | InputMode::Helix => Self::Editor,
            InputMode::Notion | InputMode::Emacs => Self::Clickable,
        }
    }
}

impl ModalSurface {
    /// Whether this surface has a one-line field the user types into.
    ///
    /// The list of them, once: [`ModalApp::prompt`] maps each to the
    /// buffer behind it and cannot also be the answer to "is this a
    /// prompt", which is what a shell asks when it decides whether to
    /// draw a field or flash one.
    #[must_use]
    pub const fn takes_text(self) -> bool {
        matches!(
            self,
            Self::Capture
                | Self::Rename
                | Self::AddSibling
                | Self::TagsEdit
                | Self::PropertyEdit
                | Self::Palette
                | Self::Search
                | Self::BodySearch
                | Self::Buffers
                | Self::Files
                | Self::TagPick
                | Self::Refile
                | Self::Headlines
                | Self::Blocks
                | Self::UndoHistory
                | Self::Messages
                | Self::Ex
                | Self::Sync
                | Self::Llm
                | Self::InsertLink
        )
    }

    /// Whether this surface is a text buffer rather than a pane.
    ///
    /// A buffer takes the whole window: the outline, the rail and the
    /// detail pane get out of its way, the way `org-edit-special` gets
    /// its own frame in Emacs. Editing a body in a third of the window,
    /// beside a list of the other headlines, is a preview — not a
    /// place to write in.
    #[must_use]
    pub const fn is_editor(self) -> bool {
        matches!(self, Self::EditBody | Self::EditBlock | Self::EditFile)
    }
}

/// One turn of the assistant transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatTurn {
    /// Whether the user said it (rather than the model).
    pub from_user: bool,
    /// What was said.
    pub text: String,
}

/// What the vault's `config.org` says about the assistant, and whether
/// it can actually be used right now.
///
/// "Configured" and "usable" are different questions: the key lives in
/// an environment variable, so a perfectly good config still fails at
/// 2am when the variable is not exported. Both are reported, and the
/// key's *value* never is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmStatus {
    /// Whether a question can be sent right now.
    pub ready: bool,
    /// Provider name from the config, if any.
    pub provider: Option<String>,
    /// Model identifier from the config, if any.
    pub model: Option<String>,
    /// Endpoint URL, when the config pins one.
    pub endpoint: Option<String>,
    /// A sentence for the pane: what is set, or what to add.
    pub detail: String,
}

/// Where an org-edit-special session came from, and therefore how it
/// writes back.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SpecialOrigin {
    /// A block reached from the Blocks list: it belongs to a file, so
    /// the writeback goes through the kernel's span-preserving
    /// `set_block_content` and the rest of the file stays untouched
    /// bytes (I1).
    File {
        /// File the block lives in.
        path: std::path::PathBuf,
        /// Index of the block within that file.
        index: usize,
    },
    /// A block reached from the body editor: it lives inside the
    /// buffer being edited, so it is spliced back into that buffer and
    /// the ordinary body commit carries it to disk. Writing through
    /// the file here would mean persisting the body twice, from two
    /// different states.
    Body {
        /// Byte range of the block's content within the body buffer.
        range: std::ops::Range<usize>,
        /// The body buffer as it was when the session opened.
        buffer: String,
        /// Cursor to restore into that buffer afterwards.
        cursor: usize,
    },
}

/// Which read-only list a generic list surface is showing (drives the
/// shared navigation handler).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListKind {
    Agenda,
}

/// Which of the two pickers a keystroke is going to (Q1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerKind {
    /// The open-buffer list.
    Buffers,
    /// The vault's files, recent first.
    Files,
}

/// Which field a single-line modal field-edit surface is editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldKind {
    Tags,
    Property,
    Rename,
    AddSibling,
}

/// What the title prompt will make when it is accepted: org's grid of
/// new-headline chords, as two flags.
///
/// | | sibling | child |
/// | plain | `M-RET` | `C-RET` |
/// | TODO | `M-S-RET` | `C-S-RET` |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NewHeading {
    /// A child of the selection rather than a sibling after it.
    pub child: bool,
    /// It arrives carrying org's `TODO` keyword.
    pub todo: bool,
    /// It goes above the selection rather than below it — Doom's
    /// `+org/insert-item-above` on `C-S-RET`.
    pub above: bool,
}

impl NewHeading {
    /// The flavour each of the four commands asks for.
    #[must_use]
    pub const fn for_command(cmd: &str) -> Self {
        // A `match` on `&str` is not const, and these four are worth
        // keeping in one place rather than spelling the flags at every
        // call site.
        Self {
            child: matches!(
                cmd.as_bytes(),
                b"add-child-heading" | b"add-todo-child-heading"
            ),
            todo: matches!(
                cmd.as_bytes(),
                b"add-todo-heading" | b"add-todo-child-heading"
            ),
            above: matches!(cmd.as_bytes(), b"add-heading-above"),
        }
    }

    /// What org writes in front of the title.
    #[must_use]
    pub const fn prefix(self) -> &'static str {
        if self.todo { "TODO " } else { "" }
    }
}

/// The body editor's vim-style mode (org-edit-special surface).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    /// Typing inserts at the cursor; `Esc` drops to [`Self::Normal`].
    Insert,
    /// `h`/`j`/`k`/`l` navigate, `i`/`a`/`o` insert, `x` deletes,
    /// `v` selects, `dd`/`yy`/`p` cut/copy/paste lines, `Esc` cancels
    /// the edit.
    Normal,
    /// Charwise selection from an anchor (`v`): motions extend, `y`
    /// yanks, `d`/`x` delete, `Esc` returns to Normal.
    Visual,
    /// Linewise selection from an anchor line (V): motions extend by
    /// whole lines, y yanks them, d/x delete them, Esc returns.
    VisualLine,
}

/// Vim's three character classes. `w`/`b`/`e` and the `iw` object stop
/// at every class boundary; the `W`/`B`/`E`/`iW` variants fold `Punct`
/// into `Word` so only blanks delimit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Blank,
    Word,
    Punct,
}

/// Classify `c` for a small (`big == false`) or big word motion.
/// The pair a surround char names, as `(open, close)`.
///
/// vim-surround's oldest convention is in here: the *closing* bracket
/// wraps tight and the *opening* one leaves a space inside, so `ysiw)`
/// gives `(word)` and `ysiw(` gives `( word )`. `b` and `B` are the
/// tight forms of `()` and `{}`. Everything else is its own mirror,
/// which covers the quotes and org's emphasis markers alike.
///
/// `None` is a char that names no pair — `t`, because HTML tags need a
/// name typed after them and a note-taking app is not where that
/// belongs.
const fn surround_pair(c: char) -> Option<(&'static str, &'static str)> {
    Some(match c {
        '(' => ("( ", " )"),
        ')' | 'b' => ("(", ")"),
        '{' => ("{ ", " }"),
        '}' | 'B' => ("{", "}"),
        '[' => ("[ ", " ]"),
        ']' => ("[", "]"),
        '<' => ("< ", " >"),
        '>' => ("<", ">"),
        '"' => ("\"", "\""),
        '\'' => ("'", "'"),
        '`' => ("`", "`"),
        '*' => ("*", "*"),
        '/' => ("/", "/"),
        '_' => ("_", "_"),
        '=' => ("=", "="),
        '~' => ("~", "~"),
        '+' => ("+", "+"),
        _ => return None,
    })
}

fn char_class(c: char, big: bool) -> CharClass {
    if c.is_whitespace() {
        CharClass::Blank
    } else if big || c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

/// How a motion's target combines with the cursor into an operator
/// range — vim's exclusive/inclusive/linewise distinction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MotionKind {
    /// `w`, `b`, `0`, `$`: the target byte itself stays.
    Exclusive,
    /// `e`, `f`, `t`: the char under the target is taken too.
    Inclusive,
    /// `j`, `k`, `G`, `gg`, `ip`: whole lines, newline included.
    Linewise,
}

/// What the editor is waiting for mid-chord. `d` alone is not an edit;
/// it is a promise that the next stroke names a range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    /// Nothing outstanding — the next stroke is a whole command.
    None,
    /// An operator (`d`/`c`/`y`/`>`/`<`) awaiting a motion, a text
    /// object, or its own doubled key (`dd`).
    Op(char),
    /// `g` typed, alone (`gg`) or under an operator (`dgg`).
    G(Option<char>),
    /// `i`/`a` typed; the next stroke names the object (`iw`, `a"`).
    /// `op == None` is Visual mode, where the object sets the selection.
    Obj { op: Option<char>, around: bool },
    /// `f`/`F`/`t`/`T` typed; the next stroke is the target char.
    Find { op: Option<char>, kind: char },
    /// `r` typed; the next stroke is the replacement char.
    Replace,
    /// `"` typed; the next stroke names the register the following
    /// yank/delete/paste uses.
    Register,
    /// `m` typed; the next stroke names the mark to set.
    Mark,
    /// `` ` `` or `'` typed; the next stroke names the mark to jump to.
    /// `linewise` is the `'` form, which lands on the line's first
    /// non-blank and makes an operator take whole lines.
    JumpMark { op: Option<char>, linewise: bool },
    /// `ys{motion}` (or VISUAL `S`) resolved to a range; the next
    /// stroke names the pair to wrap it in.
    SurroundWith { lo: usize, hi: usize },
    /// `g c` typed: evil-nerd-commenter's operator prefix, awaiting
    /// its doubled `c` (`gcc`, the current line). In Visual the
    /// selection is already the range, so `g c` acts at once.
    Comment,
    /// `ds` typed; the next stroke names the pair to take away.
    SurroundDelete,
    /// `cs` typed; the next stroke names the pair to replace, and the
    /// one after it what to replace it with.
    SurroundChange(Option<char>),
    /// `q` typed with nothing recording; the next stroke names the
    /// register to record into.
    RecordMacro,
    /// `@` typed; the next stroke names the macro to replay.
    RunMacro,
}

impl Pending {
    /// The stroke a shell should echo as "mid-chord", if any.
    const fn stroke(self) -> Option<char> {
        match self {
            Self::None => None,
            Self::Op(c) => Some(c),
            Self::G(_) => Some('g'),
            Self::Comment => Some('c'),
            Self::Obj { around: true, .. } => Some('a'),
            Self::Obj { around: false, .. } => Some('i'),
            Self::Find { kind, .. } => Some(kind),
            Self::Replace => Some('r'),
            Self::Register => Some('"'),
            Self::Mark => Some('m'),
            Self::JumpMark { linewise: true, .. } => Some('\''),
            Self::JumpMark {
                linewise: false, ..
            } => Some('`'),
            Self::SurroundWith { .. } | Self::SurroundDelete | Self::SurroundChange(_) => Some('s'),
            Self::RecordMacro => Some('q'),
            Self::RunMacro => Some('@'),
        }
    }
}

/// One step of a recorded macro: a modal stroke, or a run of text
/// typed while the macro was in INSERT.
///
/// Two kinds rather than one because a macro spans modes: `ciwfoo<Esc>`
/// is three strokes, then three characters that never reach
/// [`BodyEditor::modal_key`] at all.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MacroStep {
    /// A Normal/Visual stroke, replayed through the modal dispatch.
    Key(String),
    /// Text typed in INSERT, replayed verbatim.
    Text(String),
}

/// Lines a `PageUp`/`PageDown` stroke moves when the editor is asked
/// without a viewport. A shell that knows its own height should call
/// [`BodyEditor::page`] with it instead.
const PAGE_LINES: usize = 20;

/// The one-line vocabulary hint a shell paints beside the mode chip.
///
/// Shared so the terminal and the GUI advertise the same editor: a
/// chord named here works in both (I4). Keep it in step with
/// [`BodyEditor::modal_key`] — a hint that outlives its command is the
/// same lie as a dead chord.
///
/// It takes the *input* mode too, because half the vocabulary does not
/// exist in all of them: Notion and Emacs open a buffer straight into
/// INSERT and have no NORMAL to drop into, so "Esc → NORMAL" there is
/// an instruction that does something else entirely ("vim :q! and other
/// bindings are shown in the editor even if I am in Notion or Emacs
/// mode … hide irrelevant ones").
#[must_use]
pub const fn editor_hint(mode: EditorMode, input: closure_config::InputMode) -> &'static str {
    let modal = matches!(
        input,
        closure_config::InputMode::Vim
            | closure_config::InputMode::Doom
            | closure_config::InputMode::Helix
    );
    match mode {
        EditorMode::Insert if modal => {
            "type · TAB tempo (<s…) · C-n complete · C-a/e/k/y readline · Esc → NORMAL"
        }
        // No NORMAL to be sent to, and `Esc` closes a clean buffer here
        // rather than changing mode.
        EditorMode::Insert => {
            "type · TAB tempo (<s…) · C-n complete · C-a/e/k/y readline · Esc closes"
        }
        EditorMode::Normal if modal => {
            "w b e f t % move · diw caw dis dt, gUiw operate · . repeat · dd yy Y p · \
             \"a reg · ma `a mark · qa @a macro · /pat n N * # · C-a/C-x · C-d/C-u/C-f/C-b · \
             A I O R J r gv gi · v V · Esc"
        }
        EditorMode::Visual | EditorMode::VisualLine if modal => {
            "motions + iw aw i( a\" extend · d c y > < operate · o swap ends · Esc → NORMAL"
        }
        // Unreachable in a non-modal mode; naming its vocabulary would
        // advertise chords that mode cannot run.
        _ => "type · C-a/e/k/y readline · C-s saves · Esc closes",
    }
}

/// A modal multi-line text editor with a real cursor — the state
/// behind the org-edit-special surface.
///
/// Pure and unicode-safe (the cursor is a byte offset kept on a `char`
/// boundary); a shell paints `text()` + `cursor_line_col()` and feeds
/// keys through [`ModalApp`].
// The flags are independent latches on the editing session (a count was
// typed, the register is linewise, an Insert burst is armed, a change is
// recording, a replay is running); grouping them would only add
// indirection to what is already one cohesive editor state.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct BodyEditor {
    buf: String,
    cursor: usize,
    /// The column vertical motion is *trying* to be in — vim's
    /// `curswant`. A `j` through a two-character line clamps to it and
    /// keeps wanting the original, so the next `j` comes back out to
    /// where you were reading. Cleared by anything horizontal.
    wanted_col: Option<usize>,
    mode: EditorMode,
    /// Visual-mode selection anchor (byte offset).
    anchor: usize,
    /// The yank/kill register shared by vim (`y`/`d`/`p`) and the
    /// readline chords (`C-k`/`C-u`/`C-w`/`C-y`).
    register: String,
    /// How many times [`Self::register`] has changed — the seam the
    /// system-clipboard mirror watches.
    register_gen: u64,
    /// How many times `gcc` has been pressed. The buffer knows the
    /// chord and cannot know the *comment*: which token to use comes
    /// from the enclosing source block's language, which is org's
    /// business and not the editor's. Same seam as the register.
    comment_asked: u64,
    /// Whether the register holds whole lines (`dd`/`yy` → `p` pastes
    /// below the current line).
    linewise: bool,
    /// What the in-progress chord is still waiting for (`d` of `diw`).
    pending: Pending,
    /// Pending vim count in Normal/Visual modes (0 = none).
    count: usize,
    /// Count typed *before* the operator (the `2` of `2d3w`), so both
    /// halves multiply the way vim multiplies them.
    op_count: usize,
    /// Whether the chord being resolved carried an explicit count —
    /// `G` and `gg` mean "last"/"first" line only without one.
    count_given: bool,
    /// Last `f`/`F`/`t`/`T` as `(kind, target)`, for `;` and `,`.
    last_find: Option<(char, char)>,
    /// Strokes of the command being typed, kept until it is known to be
    /// a change worth remembering for `.`.
    scratch: Vec<String>,
    /// Text typed during the Insert session of the change in progress.
    insert_text: String,
    /// Whether the live Insert session belongs to a recorded change.
    recording_insert: bool,
    /// The last change as `(strokes, inserted text)` — what `.` replays.
    last_change: Option<(Vec<String>, String)>,
    /// True while `.` is replaying, so the replay records nothing.
    replaying: bool,
    /// Bumped by every [`Self::checkpoint`]: the signal that a command
    /// actually changed the buffer, and so is a change `.` can repeat.
    edit_seq: u64,
    /// Editor-local undo snapshots (buffer, cursor), newest last.
    undo_stack: Vec<(String, usize)>,
    /// Redo snapshots cleared by any fresh edit.
    redo_stack: Vec<(String, usize)>,
    /// Armed on every INSERT entry: the first buffer-changing INSERT
    /// edit takes one checkpoint, so the whole burst undoes as a unit
    /// (vim rule, G4).
    insert_armed: bool,
    /// Named registers `a`–`z` with their own linewise flags. The
    /// unnamed register stays [`Self::register`], so every command that
    /// does not say otherwise behaves exactly as it always did.
    registers: std::collections::BTreeMap<char, (String, bool)>,
    /// The register named by a `"x` prefix, consumed by the next
    /// yank/delete/paste. Uppercase means "append".
    target_register: Option<char>,
    /// Marks `a`–`z` as byte offsets. Edits do not move them, so a
    /// stale mark is clamped to the buffer on use rather than trusted.
    marks: std::collections::BTreeMap<char, usize>,
    /// Recorded macros `a`–`z`.
    macros: std::collections::BTreeMap<char, Vec<MacroStep>>,
    /// The register being recorded into and the steps taken so far.
    recording: Option<(char, Vec<MacroStep>)>,
    /// The macro `@@` repeats.
    last_macro: Option<char>,
    /// True while a macro replays, so the replay records nothing.
    running_macro: bool,
    /// The last Visual range as `(anchor, cursor, mode)` — what `gv`
    /// puts back.
    last_visual: Option<(usize, usize, EditorMode)>,
    /// Where INSERT was last left, which is where `gi` resumes.
    last_insert: Option<usize>,
    /// REPLACE mode (`R`): typing overwrites the char under the cursor
    /// instead of pushing it right.
    replacing: bool,
    /// The open search line as `(forward, pattern so far)`.
    search_input: Option<(bool, String)>,
    /// The operator armed when the search line opened (`d/foo`).
    search_op: Option<char>,
    /// The last search as `(pattern, forward)` — what `n`/`N` repeat.
    last_search: Option<(String, bool)>,
}

impl Default for BodyEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl BodyEditor {
    /// Empty editor in Insert mode.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buf: String::new(),
            cursor: 0,
            wanted_col: None,
            mode: EditorMode::Insert,
            anchor: 0,
            register: String::new(),
            register_gen: 0,
            comment_asked: 0,
            linewise: false,
            pending: Pending::None,
            count: 0,
            op_count: 0,
            count_given: false,
            last_find: None,
            scratch: Vec::new(),
            insert_text: String::new(),
            recording_insert: false,
            last_change: None,
            replaying: false,
            edit_seq: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            insert_armed: false,
            registers: std::collections::BTreeMap::new(),
            target_register: None,
            marks: std::collections::BTreeMap::new(),
            macros: std::collections::BTreeMap::new(),
            recording: None,
            last_macro: None,
            running_macro: false,
            last_visual: None,
            last_insert: None,
            replacing: false,
            search_input: None,
            search_op: None,
            last_search: None,
        }
    }

    /// Load `text`, cursor at the end, Insert mode (the edit-body flow).
    pub fn load(&mut self, text: String) {
        self.load_in(text, EditorMode::Insert);
    }

    /// Load `text` and land in `mode`.
    ///
    /// Where the cursor goes follows from the mode rather than being a
    /// second decision: INSERT opens at the end, the way appending to a
    /// note does; NORMAL opens at the top, the way opening a buffer
    /// does. A VISUAL entry mode makes no sense with no selection yet,
    /// so it starts at the top too and collapses the anchor onto the
    /// cursor.
    pub fn load_in(&mut self, text: String, mode: EditorMode) {
        self.cursor = if mode == EditorMode::Insert {
            text.len()
        } else {
            0
        };
        self.buf = text;
        self.mode = mode;
        self.anchor = self.cursor;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.insert_armed = true;
        self.reset_buffer_state();
    }

    /// Replace the whole buffer as one undoable edit, leaving the
    /// cursor at (or just before) byte `at`.
    ///
    /// [`Self::load_in`] is for *arriving* in a buffer and clears the
    /// undo stack with everything else; a transform of the text you are
    /// already editing — realigning a table, moving one of its columns
    /// — is an edit like any other and has to be undoable like one.
    pub fn replace_all(&mut self, text: String, at: usize) {
        self.checkpoint();
        self.buf = text;
        let mut at = at.min(self.buf.len());
        while at > 0 && !self.buf.is_char_boundary(at) {
            at -= 1;
        }
        self.cursor = at;
        self.anchor = at;
    }

    /// Drop everything that describes *this* buffer's positions.
    ///
    /// Marks, the last selection and the last insert point are byte
    /// offsets into the text that was here; carried into the next
    /// headline they would point at unrelated words. Registers, macros
    /// and the last search pattern are deliberately kept — those are
    /// global in vim, and the whole point of yanking in one note is
    /// pasting it into another.
    fn reset_buffer_state(&mut self) {
        self.marks.clear();
        self.last_visual = None;
        self.last_insert = None;
        self.replacing = false;
        self.search_input = None;
        self.search_op = None;
        self.recording = None;
        self.target_register = None;
        self.pending = Pending::None;
        self.count = 0;
        self.op_count = 0;
    }

    /// The buffer contents.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.buf
    }

    /// The vim-style mode.
    #[must_use]
    pub const fn mode(&self) -> EditorMode {
        self.mode
    }

    /// The cursor as a byte offset into the buffer — what
    /// [`Self::replace_to_cursor`] addresses.
    #[must_use]
    pub const fn cursor_byte(&self) -> usize {
        self.cursor
    }

    /// Park the cursor at byte offset `byte`, clamped to the buffer and
    /// to a char boundary.
    ///
    /// Unlike the motions, this crosses lines: an inserted multi-line
    /// template needs the caret placed *inside* it, which `left`/`up`
    /// cannot express because they deliberately stop at newlines.
    pub fn set_cursor_byte(&mut self, byte: usize) {
        let mut at = byte.min(self.buf.len());
        while at > 0 && !self.buf.is_char_boundary(at) {
            at -= 1;
        }
        self.cursor = at;
    }

    /// Cursor as `(line, column)` — both 0-based, column in chars.
    #[must_use]
    pub fn cursor_line_col(&self) -> (usize, usize) {
        let before = &self.buf[..self.cursor];
        let line = before.matches('\n').count();
        let col = before
            .rsplit_once('\n')
            .map_or(before, |(_, tail)| tail)
            .chars()
            .count();
        (line, col)
    }

    /// Clear buffer + cursor (cancelling an edit).
    pub fn clear(&mut self) {
        self.buf.clear();
        self.cursor = 0;
        self.mode = EditorMode::Insert;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.insert_armed = true;
        self.reset_buffer_state();
    }

    /// Switch to Normal (from Insert `Esc`).
    pub fn to_normal(&mut self) {
        // A macro that entered INSERT has to leave it again on replay,
        // and `Esc` never reaches `modal_key` — the shells answer it
        // themselves. So the exit is recorded here.
        if self.mode == EditorMode::Insert {
            self.last_insert = Some(self.cursor);
            self.push_macro_step(MacroStep::Key("escape".to_owned()));
        }
        self.replacing = false;
        // Leaving Insert closes a recorded change: the strokes that
        // opened it plus everything typed inside become one `.` unit,
        // exactly as vim scopes a change.
        if self.recording_insert {
            self.recording_insert = false;
            if self.scratch.is_empty() {
                // An Insert session nobody opened with a command (the
                // editor loads in Insert) is not a repeatable change.
                self.insert_text.clear();
            } else {
                self.last_change = Some((
                    std::mem::take(&mut self.scratch),
                    std::mem::take(&mut self.insert_text),
                ));
            }
        }
        self.mode = EditorMode::Normal;
        // vim rule: leaving Insert steps the cursor back onto the last
        // typed char (clamped at the line start by left()).
        self.left();
    }

    /// Switch to Insert at the cursor (`i`), arming the burst
    /// checkpoint (G4).
    pub const fn to_insert(&mut self) {
        self.mode = EditorMode::Insert;
        self.insert_armed = true;
    }

    /// G4: the first buffer-changing edit of an INSERT burst records
    /// one pre-edit snapshot, so `Esc` + `u` undoes the whole burst.
    /// A no-op outside Insert mode or once the burst has checkpointed.
    fn insert_guard(&mut self) {
        if self.mode == EditorMode::Insert && self.insert_armed {
            self.checkpoint();
            self.insert_armed = false;
        }
    }

    /// Insert `c` at the cursor — or, in REPLACE mode, overwrite the
    /// char under it. A newline is never overwritten: `R` past the line
    /// end appends, it does not weld two lines together.
    pub fn insert_char(&mut self, c: char) {
        self.insert_guard();
        self.record_typed(c.encode_utf8(&mut [0u8; 4]));
        if self.replacing
            && let Some(under) = self.char_at(self.cursor).filter(|u| *u != '\n')
        {
            self.buf
                .replace_range(self.cursor..self.cursor + under.len_utf8(), "");
        }
        self.buf.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Insert `s` at the cursor, cursor after it.
    pub fn insert_str(&mut self, s: &str) {
        self.insert_guard();
        self.record_typed(s);
        self.buf.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    /// Note text typed inside a change, so `.` can retype it.
    fn record_typed(&mut self, s: &str) {
        if self.recording_insert && !self.replaying {
            self.insert_text.push_str(s);
        }
        self.push_macro_step(MacroStep::Text(s.to_owned()));
    }

    /// Delete the char before the cursor (Insert Backspace).
    pub fn backspace(&mut self) {
        if let Some((i, _)) = self.buf[..self.cursor].char_indices().next_back() {
            self.insert_guard();
            if self.recording_insert && !self.replaying {
                self.insert_text.pop();
            }
            self.buf.remove(i);
            self.cursor = i;
        }
    }

    /// Delete the char under the cursor (Normal `x`).
    pub fn delete_at(&mut self) {
        if self.cursor < self.buf.len() {
            self.insert_guard();
            self.buf.remove(self.cursor);
        }
    }

    /// Move one char left (clamped to the line start).
    pub fn left(&mut self) {
        if let Some((i, c)) = self.buf[..self.cursor].char_indices().next_back()
            && c != '\n'
        {
            self.cursor = i;
        }
    }

    /// Move one char right (clamped to the line end).
    pub fn right(&mut self) {
        if let Some(c) = self.buf[self.cursor..].chars().next()
            && c != '\n'
        {
            self.cursor += c.len_utf8();
        }
    }

    /// Move to the start of the current line (`0`).
    pub fn line_home(&mut self) {
        self.cursor = self.line_start(self.cursor);
    }

    /// Move to the end of the current line (`$`).
    pub fn line_end_motion(&mut self) {
        self.cursor = self.line_end(self.cursor);
    }

    /// Move `lines` whole lines up or down — the page motion, sized by
    /// the caller because only the shell knows its viewport height.
    pub fn page(&mut self, down: bool, lines: usize) {
        let line = if down {
            self.cursor_line() + lines
        } else {
            self.cursor_line().saturating_sub(lines)
        };
        self.cursor = self.line_col_offset(line, 0);
    }

    /// Move up one line, column clamped.
    pub fn up(&mut self) {
        let (line, col) = self.cursor_line_col();
        if line > 0 {
            self.goto_line_col(line - 1, col);
        }
    }

    /// Move down one line, column clamped.
    pub fn down(&mut self) {
        let (line, col) = self.cursor_line_col();
        self.goto_line_col(line + 1, col);
    }

    /// Open a new line below the current one and enter Insert (`o`).
    /// The caller (`o`) checkpoints before the open, so the burst
    /// checkpoint stays disarmed — `o` plus typing undoes as one unit.
    pub fn open_below(&mut self) {
        self.cursor = self.line_end(self.cursor);
        self.buf.insert(self.cursor, '\n');
        self.cursor += 1;
        self.mode = EditorMode::Insert;
        self.insert_armed = false;
    }

    /// Byte offset of the start of the line containing `pos`.
    fn line_start(&self, pos: usize) -> usize {
        self.buf[..pos].rfind('\n').map_or(0, |i| i + 1)
    }

    /// Byte offset of the end (before `\n`) of the line containing `pos`.
    fn line_end(&self, pos: usize) -> usize {
        self.buf[pos..]
            .find('\n')
            .map_or(self.buf.len(), |i| pos + i)
    }

    /// Place the cursor at `line`/`col` (both clamped — a line past the
    /// end lands on the last line, the mouse-click rule).
    pub(crate) fn goto_line_col(&mut self, line: usize, col: usize) {
        let starts: Vec<usize> = self
            .buf
            .split_inclusive('\n')
            .scan(0usize, |acc, l| {
                let s = *acc;
                *acc += l.len();
                Some(s)
            })
            .collect();
        let Some(&start) = starts.get(line.min(starts.len().saturating_sub(1))) else {
            return;
        };
        let end = self.line_end(start);
        let mut pos = start;
        for c in self.buf[start..end].chars().take(col) {
            pos += c.len_utf8();
        }
        self.cursor = pos;
    }

    /// Select the whitespace-delimited word containing the cursor
    /// (double-click): anchor at the word start, Visual mode, cursor on
    /// its last char. Whitespace under the cursor selects nothing.
    pub fn select_word_at_cursor(&mut self) {
        let positions: Vec<(usize, char)> = self.buf.char_indices().collect();
        let Some(mut i) = positions.iter().position(|&(off, _)| off == self.cursor) else {
            return;
        };
        if positions[i].1.is_whitespace() {
            return;
        }
        while i > 0 && !positions[i - 1].1.is_whitespace() {
            i -= 1;
        }
        let mut j = i;
        while j + 1 < positions.len() && !positions[j + 1].1.is_whitespace() {
            j += 1;
        }
        self.anchor = positions[i].0;
        self.cursor = positions[j].0;
        self.mode = EditorMode::Visual;
    }

    /// Mouse drag extends a charwise Visual selection: on the first drag
    /// event that actually moves the cursor, the pre-drag cursor becomes
    /// the anchor and the mode switches to Visual; every drag event then
    /// moves the cursor to the clamped line and col, exactly like a
    /// click does. A drag that stays on the clicked cell (a click's
    /// micro-movement) leaves the mode untouched.
    pub fn drag_to(&mut self, line: usize, col: usize) {
        let from = self.cursor;
        self.goto_line_col(line, col);
        if !matches!(self.mode, EditorMode::Visual | EditorMode::VisualLine) && self.cursor != from
        {
            self.anchor = from;
            self.mode = EditorMode::Visual;
        }
    }

    /// Move to the start of the previous word (simple rule).
    fn word_backward(&mut self) {
        let positions: Vec<(usize, char)> = self.buf.char_indices().collect();
        let mut pos = if self.cursor == self.buf.len() {
            positions.len()
        } else {
            match positions.iter().position(|&(i, _)| i == self.cursor) {
                Some(p) => p,
                None => return,
            }
        };
        // Skip whitespace immediately before the cursor, then walk back
        // to the start of the word (char before is whitespace or start).
        while pos > 0 && positions[pos - 1].1.is_whitespace() {
            pos -= 1;
        }
        while pos > 0 && !positions[pos - 1].1.is_whitespace() {
            pos -= 1;
        }
        self.cursor = if pos < positions.len() {
            positions[pos].0
        } else {
            0
        };
    }

    /// The text of the line the cursor is on.
    #[must_use]
    pub fn current_line(&self) -> &str {
        &self.buf[self.line_start(self.cursor)..self.line_end(self.cursor)]
    }

    /// Replace the line the cursor is on, keeping the cursor's column
    /// where the new line is long enough to hold it (Q3-V5).
    pub fn replace_current_line(&mut self, text: &str) {
        let start = self.line_start(self.cursor);
        let end = self.line_end(self.cursor);
        let column = self.cursor - start;
        self.buf.replace_range(start..end, text);
        self.cursor = start + column.min(text.len());
    }

    /// Byte offset where the word containing the cursor starts (the
    /// dabbrev prefix start; word chars are alphanumeric plus
    /// `_`/`#`/`+`/`:` so org keywords like `#+BEGIN_SRC` complete).
    #[must_use]
    pub fn word_start(&self) -> usize {
        let mut start = self.cursor;
        for (i, c) in self.buf[..self.cursor].char_indices().rev() {
            if c.is_alphanumeric() || matches!(c, '_' | '#' | '+' | ':') {
                start = i;
            } else {
                break;
            }
        }
        start
    }

    /// The word fragment between [`Self::word_start`] and the cursor —
    /// the dabbrev completion prefix.
    #[must_use]
    pub fn word_prefix(&self) -> &str {
        &self.buf[self.word_start()..self.cursor]
    }

    /// Replace `start..cursor` with `text`, cursor after it (the
    /// completion-accept edit). `start` must be a char boundary at or
    /// before the cursor.
    pub fn replace_to_cursor(&mut self, start: usize, text: &str) {
        if start <= self.cursor && self.buf.is_char_boundary(start) {
            self.insert_guard();
            self.buf.replace_range(start..self.cursor, text);
            self.cursor = start + text.len();
        }
    }

    /// Undo the last editor-local edit (Normal u).
    pub fn undo_local(&mut self) {
        if let Some((buf, cur)) = self.undo_stack.pop() {
            self.redo_stack.push((self.buf.clone(), self.cursor));
            self.buf = buf;
            self.cursor = cur;
        }
    }

    /// Replace a byte range of the buffer with `text`, parking the
    /// cursor after it.
    ///
    /// The seam an input method needs: a composed character arrives as
    /// a replacement over a range, not as a keypress. Offsets are
    /// clamped to the buffer and snapped down to char boundaries, so a
    /// bad range from the platform cannot panic (I5).
    pub fn replace_range(&mut self, range: std::ops::Range<usize>, text: &str) {
        let lo = self.snap_boundary(range.start.min(self.buf.len()));
        let hi = self.snap_boundary(range.end.clamp(lo, self.buf.len()));
        if lo == hi && text.is_empty() {
            return;
        }
        self.checkpoint();
        self.buf.replace_range(lo..hi, text);
        self.cursor = lo + text.len();
        self.last_insert = Some(self.cursor);
    }

    /// Round `at` down to the nearest char boundary.
    fn snap_boundary(&self, at: usize) -> usize {
        let mut at = at.min(self.buf.len());
        while at > 0 && !self.buf.is_char_boundary(at) {
            at -= 1;
        }
        at
    }

    /// Redo the last editor-local undo (Normal C-r).
    pub fn redo_local(&mut self) {
        if let Some((buf, cur)) = self.redo_stack.pop() {
            self.undo_stack.push((self.buf.clone(), self.cursor));
            self.buf = buf;
            self.cursor = cur;
        }
    }

    /// One Normal/Visual-mode key.
    ///
    /// The vocabulary is vim's real grammar rather than a handful of
    /// hard-coded pairs: `[count]operator[count]{motion|text-object}`,
    /// so `diw`, `2d3w`, `ca"`, `dt,`, `yG` and `>>` all compose from
    /// the same three tables ([`Self::motion`], [`Self::text_object`],
    /// [`Self::apply_operator`]).
    ///
    /// `Esc` leaves Visual or clears an in-progress chord; the *caller*
    /// cancels the edit on `Esc` when [`Self::pending_stroke`] is clear
    /// and the mode is Normal.
    pub fn modal_key(&mut self, key: &str) {
        // `.` is not itself a change and must never be recorded, or it
        // would repeat itself.
        if key == "." && self.pending == Pending::None && self.search_input.is_none() {
            return self.repeat_change();
        }
        // The strokes that drive the recorder are not part of what it
        // records: `qa` … `q` would otherwise replay as "stop, start".
        let recorder_stroke = self.pending == Pending::RecordMacro
            || (key == "q" && self.pending == Pending::None && self.recording.is_some());
        if !recorder_stroke {
            self.push_macro_step(MacroStep::Key(key.to_owned()));
        }
        if !self.replaying {
            self.scratch.push(key.to_owned());
        }
        let before = self.edit_seq;
        self.dispatch_modal_key(key);
        if !self.replaying {
            self.record_step(before);
        }
    }

    /// Add one step to the macro being recorded, if one is.
    fn push_macro_step(&mut self, step: MacroStep) {
        if self.running_macro || self.replaying {
            return;
        }
        let Some((_, steps)) = self.recording.as_mut() else {
            return;
        };
        // Consecutive typed characters coalesce into one run, so a
        // replay inserts them as a single edit.
        if let (MacroStep::Text(add), Some(MacroStep::Text(tail))) = (&step, steps.last_mut()) {
            tail.push_str(add);
            return;
        }
        steps.push(step);
    }

    /// Decide what the stroke just dispatched means for `.`.
    ///
    /// A command that changed the buffer is remembered; one that only
    /// moved the cursor is forgotten; a chord still mid-flight is kept
    /// so `d` + `i` + `w` commits as the single change `diw`.
    fn record_step(&mut self, before: u64) {
        if self.mode == EditorMode::Insert {
            // The change continues into the typing; `to_normal` commits.
            self.recording_insert = true;
            self.insert_text.clear();
        } else if self.pending == Pending::None && self.count == 0 && self.op_count == 0 {
            // A bare count is still mid-command — `3` of `3x` must stay
            // in the scratch or `.` would repeat a countless `x`.
            if self.edit_seq == before {
                self.scratch.clear();
            } else {
                self.last_change = Some((std::mem::take(&mut self.scratch), String::new()));
            }
        }
    }

    /// `.`: replay the last change at the cursor.
    fn repeat_change(&mut self) {
        let Some((keys, text)) = self.last_change.clone() else {
            return;
        };
        self.replaying = true;
        for k in &keys {
            self.dispatch_modal_key(k);
        }
        if !text.is_empty() {
            self.insert_str(&text);
        }
        if self.mode == EditorMode::Insert {
            self.to_normal();
        }
        self.replaying = false;
    }

    /// The modal vocabulary proper, with no `.`-recording around it.
    fn dispatch_modal_key(&mut self, key: &str) {
        // The search line owns every key while it is open — a pattern
        // is text, not a chord.
        if self.search_input.is_some() {
            return self.search_key(key);
        }
        let was_visual = matches!(self.mode, EditorMode::Visual | EditorMode::VisualLine)
            .then_some((self.anchor, self.cursor, self.mode));
        // A chord waiting on a literal char argument consumes any key,
        // digits included — `r5` and `df3` are not counts.
        match self.pending {
            Pending::Replace => self.finish_replace(key),
            Pending::Find { op, kind } => self.finish_find(op, kind, key),
            Pending::Obj { op, around } => self.finish_object(op, around, key),
            Pending::G(op) => self.finish_g(op, key),
            Pending::Op(op) => self.after_operator(op, key),
            Pending::Register => self.finish_register(key),
            Pending::Mark => self.finish_mark(key),
            Pending::JumpMark { op, linewise } => self.finish_mark_jump(op, linewise, key),
            Pending::SurroundWith { lo, hi } => self.finish_surround(lo, hi, key),
            Pending::Comment => {
                // `gcc`, and nothing else: `gc` is an operator in
                // evil-nerd-commenter, but every motion it could take
                // resolves to a line range here, and a half-supported
                // operator is worse than a chord that says no.
                if key == "c" {
                    self.comment_asked = self.comment_asked.wrapping_add(1);
                }
                self.pending = Pending::None;
            }
            Pending::SurroundDelete => self.finish_surround_delete(key),
            Pending::SurroundChange(old) => self.finish_surround_change(old, key),
            Pending::RecordMacro => self.finish_record(key),
            Pending::RunMacro => self.finish_run_macro(key),
            Pending::None => {
                if !self.take_digit(key) {
                    self.normal_key(key);
                }
            }
        }
        // Whatever ended a Visual selection — `Esc`, an operator, a
        // mode switch — is what `gv` puts back.
        if let Some(last) = was_visual
            && !matches!(self.mode, EditorMode::Visual | EditorMode::VisualLine)
        {
            self.last_visual = Some(last);
        }
    }

    /// The inclusive line range an operator would act on: the visual
    /// selection when there is one, the caret's line when there is not.
    ///
    /// This is what makes `gcc` and `gc` over a selection one command
    /// rather than two — vim's own rule, where an operator in Visual
    /// takes the selection and in Normal takes what the motion gives
    /// it, and a bare `cc`-shaped double takes the line.
    #[must_use]
    pub fn selected_lines(&self) -> (usize, usize) {
        let line_of = |byte: usize| self.buf[..byte.min(self.buf.len())].matches('\n').count();
        let here = line_of(self.cursor);
        if !matches!(self.mode, EditorMode::Visual | EditorMode::VisualLine) {
            return (here, here);
        }
        let there = line_of(self.anchor);
        (here.min(there), here.max(there))
    }

    /// Accumulate a count digit; `true` when the key was one. A leading
    /// `0` is the line-start motion, not a count.
    fn take_digit(&mut self, key: &str) -> bool {
        let Some(d) = key.chars().next().filter(|_| key.len() == 1) else {
            return false;
        };
        let Some(d) = d.to_digit(10).and_then(|d| usize::try_from(d).ok()) else {
            return false;
        };
        if d == 0 && self.count == 0 {
            return false;
        }
        self.count = self.count * 10 + d;
        true
    }

    /// A whole command with no chord outstanding.
    // One arm per vim key: splitting the vocabulary would hide the
    // mode dispatch (same precedent as `run_command`).
    #[allow(clippy::too_many_lines)]
    fn normal_key(&mut self, key: &str) {
        let visual = matches!(self.mode, EditorMode::Visual | EditorMode::VisualLine);
        match key {
            // --- Operators. In Visual they act on the selection now;
            // in Normal they arm a pending chord.
            "d" | "x" if visual => self.visual_operator('d'),
            "c" | "s" if visual => self.visual_operator('c'),
            "y" if visual => self.visual_operator('y'),
            ">" if visual => self.visual_operator('>'),
            "<" if visual => self.visual_operator('<'),
            // In Visual these are case changes, not undo — the `u` of
            // `viwu`. Normal-mode `u` stays undo, further down.
            "u" | "U" | "~" if visual => {
                self.visual_operator(key.chars().next().unwrap_or('u'));
            }
            "D" | "X" if visual => self.visual_linewise_operator('d'),
            // evil-surround takes VISUAL `S`; `C` keeps vim's linewise
            // change, so the muscle memory for that is untouched.
            "S" if visual => {
                let (lo, hi) = self.selection();
                self.mode = EditorMode::Normal;
                self.pending = Pending::SurroundWith { lo, hi };
            }
            "C" if visual => self.visual_linewise_operator('c'),
            "Y" if visual => self.visual_linewise_operator('y'),
            "p" | "P" if visual => self.visual_paste(),
            "d" | "c" | "y" | ">" | "<" => {
                self.op_count = self.count;
                self.count = 0;
                self.pending = Pending::Op(key.chars().next().unwrap_or('d'));
            }
            // --- Text objects and find-char open a chord in Visual too.
            "i" | "a" if visual => {
                self.pending = Pending::Obj {
                    op: None,
                    around: key == "a",
                };
            }
            "f" | "F" | "t" | "T" => {
                self.pending = Pending::Find {
                    op: None,
                    kind: key.chars().next().unwrap_or('f'),
                };
            }
            "g" => self.pending = Pending::G(None),
            "r" => self.pending = Pending::Replace,
            // --- The stateful prefixes: register, mark, macro.
            "\"" => self.pending = Pending::Register,
            "m" => self.pending = Pending::Mark,
            "`" | "'" => {
                self.pending = Pending::JumpMark {
                    op: None,
                    linewise: key == "'",
                };
            }
            "q" => {
                // A second `q` closes the recording rather than opening
                // another one.
                if let Some((reg, steps)) = self.recording.take() {
                    self.macros.insert(reg, steps);
                } else {
                    self.pending = Pending::RecordMacro;
                }
            }
            "@" => self.pending = Pending::RunMacro,
            // --- Search.
            "/" | "?" => self.open_search(None, key == "/"),
            "n" | "N" => {
                let n = self.take_count();
                self.repeat_search(None, key == "n", n);
            }
            "*" | "#" => {
                let n = self.take_count();
                self.search_word_under_cursor(key == "*", n);
            }
            // --- Motions.
            "h" | "left" | "l" | "right" | "j" | "down" | "k" | "up" | "w" | "W" | "b" | "B"
            | "e" | "E" | "0" | "home" | "^" | "$" | "end" | "G" | "{" | "}" | ";" | ","
            | "pageup" | "pagedown" | "_" | "+" | "-" | "enter" | "|" | "%" | "C-f" | "C-b"
            | "C-d" | "C-u" => {
                let n = self.take_count();
                // A vertical motion keeps aiming at the column it was
                // aiming at; everything else re-aims at where it lands.
                let vertical = matches!(key, "j" | "down" | "k" | "up");
                let wanted = vertical.then(|| self.wanted_col());
                if let Some((target, _)) = self.motion(key, n, false) {
                    self.cursor = target;
                    self.set_wanted_col(wanted);
                    // `$` as an operator target is the line end; as a
                    // cursor move it lands *on* the last char, because
                    // a Normal cursor never sits past one (vim).
                    if matches!(key, "$" | "end") {
                        self.cursor = self.offset_left(self.cursor);
                    }
                }
            }
            // --- Mode switches.
            "i" => {
                self.count = 0;
                self.to_insert();
            }
            "a" => {
                self.count = 0;
                self.right();
                self.to_insert();
            }
            "I" => {
                self.count = 0;
                self.cursor = self.first_non_blank(self.cursor);
                self.to_insert();
            }
            "A" => {
                self.count = 0;
                self.line_end_motion();
                self.to_insert();
            }
            "o" if visual => {
                std::mem::swap(&mut self.anchor, &mut self.cursor);
            }
            "o" => {
                self.count = 0;
                self.checkpoint();
                self.open_below();
            }
            "O" => {
                self.count = 0;
                self.checkpoint();
                self.open_above();
            }
            "v" => {
                self.count = 0;
                if self.mode == EditorMode::Visual {
                    self.mode = EditorMode::Normal;
                } else {
                    if self.mode == EditorMode::Normal {
                        self.anchor = self.cursor;
                    }
                    self.mode = EditorMode::Visual;
                }
            }
            "V" => {
                self.count = 0;
                if self.mode == EditorMode::VisualLine {
                    self.mode = EditorMode::Normal;
                } else {
                    if self.mode == EditorMode::Normal {
                        self.anchor = self.cursor;
                    }
                    self.mode = EditorMode::VisualLine;
                }
            }
            "escape" => {
                self.count = 0;
                self.op_count = 0;
                self.mode = EditorMode::Normal;
            }
            // --- Whole-line and single-char edits.
            "D" => self.operate_to_line_end('d'),
            "C" => self.operate_to_line_end('c'),
            "S" => {
                let n = self.take_count();
                self.apply_linewise_operator('c', n);
            }
            // `Y` is `yy`, not `y$` — vim's own inconsistency, and the
            // one every muscle memory has.
            "Y" => {
                let n = self.take_count();
                self.apply_linewise_operator('y', n);
            }
            "s" => {
                let n = self.take_count();
                self.checkpoint();
                self.delete_chars_forward(n);
                self.mode = EditorMode::Insert;
                self.insert_armed = false;
            }
            "x" | "delete" => {
                let n = self.take_count();
                self.checkpoint();
                self.delete_chars_forward(n);
            }
            "X" => {
                let n = self.take_count();
                self.checkpoint();
                for _ in 0..n {
                    if let Some((i, c)) = self.buf[..self.cursor].char_indices().next_back()
                        && c != '\n'
                    {
                        self.buf.remove(i);
                        self.cursor = i;
                    }
                }
            }
            "~" => {
                let n = self.take_count();
                self.checkpoint();
                for _ in 0..n {
                    self.toggle_case_at_cursor();
                }
            }
            "J" => {
                let n = self.take_count();
                self.checkpoint();
                for _ in 0..n {
                    self.join_line();
                }
            }
            "p" | "P" => {
                let n = self.take_count();
                self.checkpoint();
                for _ in 0..n {
                    if key == "p" {
                        self.paste();
                    } else {
                        self.paste_before();
                    }
                }
            }
            "u" => {
                self.count = 0;
                self.undo_local();
            }
            "R" => {
                self.count = 0;
                self.to_insert();
                self.replacing = true;
            }
            "C-a" | "C-x" => {
                let n = self.take_count();
                let delta = i64::try_from(n).unwrap_or(1);
                self.change_number(if key == "C-a" { delta } else { -delta });
            }
            _ => self.count = 0,
        }
    }

    // ---- Chord continuations -------------------------------------

    /// The stroke after an operator: its own key doubles it (`dd`), a
    /// text object or find opens a further chord, anything else must
    /// resolve as a motion or the operator is abandoned.
    fn after_operator(&mut self, op: char, key: &str) {
        if self.take_digit(key) {
            return;
        }
        self.pending = Pending::None;
        match key {
            "escape" => {
                self.count = 0;
                self.op_count = 0;
            }
            // evil-surround takes the three `s` chords off the
            // operators: `ys{motion}{char}` wraps, `ds{char}` unwraps,
            // `cs{old}{new}` swaps. None of the three collides with a
            // motion, because `s` is not one.
            "s" if op == 's' => {
                // `yss` — the line, without its newline.
                let lo = self.line_start(self.cursor);
                let hi = self.line_end(self.cursor);
                self.count = 0;
                self.op_count = 0;
                self.pending = Pending::SurroundWith { lo, hi };
            }
            "s" if op == 'y' => self.pending = Pending::Op('s'),
            "s" if op == 'd' => self.pending = Pending::SurroundDelete,
            "s" if op == 'c' => self.pending = Pending::SurroundChange(None),
            // `dd`, `yy`, `cc`, `>>`, `<<` — linewise over count lines.
            // `S` doubles for `cc` the way vim lets it.
            k if k.starts_with(op) => {
                let n = self.take_count_for_op();
                self.apply_linewise_operator(op, n);
            }
            "i" | "a" => {
                self.pending = Pending::Obj {
                    op: Some(op),
                    around: key == "a",
                };
            }
            "f" | "F" | "t" | "T" => {
                self.pending = Pending::Find {
                    op: Some(op),
                    kind: key.chars().next().unwrap_or('f'),
                };
            }
            "`" | "'" => {
                self.pending = Pending::JumpMark {
                    op: Some(op),
                    linewise: key == "'",
                };
            }
            "/" | "?" => self.open_search(Some(op), key == "/"),
            "n" | "N" => {
                let n = self.take_count_for_op();
                self.repeat_search(Some(op), key == "n", n);
            }
            "g" => self.pending = Pending::G(Some(op)),
            _ => {
                // Vim's oldest wart: on a non-blank, `cw` changes the
                // word without its trailing space, i.e. it is `ce`.
                let key = match key {
                    "w" | "W"
                        if op == 'c'
                            && self
                                .char_at(self.cursor)
                                .is_some_and(|c| !c.is_whitespace()) =>
                    {
                        if key == "w" {
                            "e"
                        } else {
                            "E"
                        }
                    }
                    k => k,
                };
                let n = self.take_count_for_op();
                if let Some((target, kind)) = self.motion(key, n, true) {
                    self.apply_operator(op, self.cursor, target, kind);
                } else {
                    self.count = 0;
                    self.op_count = 0;
                }
            }
        }
    }

    /// `g`-prefixed chords: `gg` (and `dgg`), `ge`, `g_`.
    fn finish_g(&mut self, op: Option<char>, key: &str) {
        self.pending = Pending::None;
        let n = if op.is_some() {
            self.take_count_for_op()
        } else {
            self.take_count()
        };
        // `gu`, `gU`, `g~` are operators in their own right: they arm a
        // pending chord exactly as `d` does, and then take a motion or
        // a text object (`guiw`) or double (`guu`).
        if op.is_none() && matches!(key, "u" | "U" | "~") {
            self.op_count = n;
            self.count = 0;
            self.pending = Pending::Op(key.chars().next().unwrap_or('u'));
            return;
        }
        // `gJ` joins with no separator at all — the one thing plain `J`
        // will not do. Not a motion, so it is answered here.
        if op.is_none() && key == "J" {
            self.checkpoint();
            for _ in 0..n {
                self.join_line_raw();
            }
            return;
        }
        // `gv` puts the last selection back; `gi` resumes typing where
        // INSERT was last left. Neither is a range, so neither can be
        // an operator target.
        // `gc`: comment. In Visual the selection is the range and it
        // fires now; in Normal it waits for the second `c`.
        if op.is_none() && key == "c" {
            if matches!(self.mode, EditorMode::Visual | EditorMode::VisualLine) {
                self.comment_asked = self.comment_asked.wrapping_add(1);
            } else {
                self.pending = Pending::Comment;
            }
            return;
        }
        if op.is_none() && key == "v" {
            if let Some((anchor, cursor, mode)) = self.last_visual {
                self.anchor = anchor.min(self.buf.len());
                self.set_cursor_byte(cursor);
                self.mode = mode;
            }
            return;
        }
        if op.is_none() && key == "i" {
            if let Some(at) = self.last_insert {
                self.set_cursor_byte(at);
            }
            self.to_insert();
            return;
        }
        let target = match key {
            "g" => Some(self.line_col_offset(if self.count_given { n - 1 } else { 0 }, 0)),
            "e" | "E" => Some(self.word_end_back(self.cursor, key == "E", n)),
            "_" => Some(self.first_non_blank_backwards_from_line_end()),
            // The display-line motions. Nothing soft-wraps here, so a
            // display line is a real line and these are their plain
            // counterparts — but they must exist, or `gj` would eat the
            // `g` and leave the cursor where it was.
            "0" | "home" => Some(self.line_start(self.cursor)),
            "^" => Some(self.first_non_blank(self.cursor)),
            "$" | "end" => Some(self.line_end(self.cursor)),
            "j" | "down" => Some(self.line_col_offset(self.cursor_line() + n, 0)),
            "k" | "up" => Some(self.line_col_offset(self.cursor_line().saturating_sub(n), 0)),
            _ => None,
        };
        let Some(target) = target else {
            self.count = 0;
            self.op_count = 0;
            return;
        };
        let kind = match key {
            "g" | "j" | "down" | "k" | "up" => MotionKind::Linewise,
            "0" | "home" | "^" | "$" | "end" => MotionKind::Exclusive,
            _ => MotionKind::Inclusive,
        };
        if let Some(op) = op {
            self.apply_operator(op, self.cursor, target, kind);
        } else {
            self.cursor = target;
            // As with `$`, a Normal cursor never sits past the last
            // char of a line.
            if matches!(key, "$" | "end") {
                self.cursor = self.offset_left(self.cursor);
            }
        }
    }

    /// The target char of an `f`/`F`/`t`/`T` chord.
    fn finish_find(&mut self, op: Option<char>, kind: char, key: &str) {
        self.pending = Pending::None;
        let Some(target_char) = key.chars().next().filter(|_| key.chars().count() == 1) else {
            self.count = 0;
            self.op_count = 0;
            return;
        };
        self.last_find = Some((kind, target_char));
        self.run_find(op, kind, target_char);
    }

    /// Execute a find, shared by `f`-chords and the `;`/`,` repeats.
    fn run_find(&mut self, op: Option<char>, kind: char, target_char: char) {
        let n = if op.is_some() {
            self.take_count_for_op()
        } else {
            self.take_count()
        };
        let Some(target) = self.find_char(kind, target_char, n) else {
            self.count = 0;
            self.op_count = 0;
            return;
        };
        let motion_kind = if matches!(kind, 'f' | 't') {
            MotionKind::Inclusive
        } else {
            MotionKind::Exclusive
        };
        match op {
            Some(op) => self.apply_operator(op, self.cursor, target, motion_kind),
            None => self.cursor = target,
        }
    }

    /// The object char of an `i`/`a` chord (`iw`, `a"`, `ip`, …).
    fn finish_object(&mut self, op: Option<char>, around: bool, key: &str) {
        self.pending = Pending::None;
        let n = if op.is_some() {
            self.take_count_for_op()
        } else {
            self.take_count()
        };
        let Some(obj) = key.chars().next().filter(|_| key.chars().count() == 1) else {
            self.count = 0;
            self.op_count = 0;
            return;
        };
        let Some((lo, hi, kind)) = self.text_object(obj, around, n) else {
            return;
        };
        // An operator consumes the object range directly; in Visual the
        // object *becomes* the selection.
        if let Some(op) = op {
            self.apply_range_operator(op, lo, hi, kind);
        } else {
            self.mode = if kind == MotionKind::Linewise {
                EditorMode::VisualLine
            } else {
                EditorMode::Visual
            };
            self.anchor = lo;
            self.cursor = self.prev_offset(hi).max(lo);
        }
    }

    /// The register named by a `"x` prefix.
    fn finish_register(&mut self, key: &str) {
        self.pending = Pending::None;
        self.target_register = key
            .chars()
            .next()
            .filter(|c| key.chars().count() == 1 && c.is_ascii_alphabetic());
    }

    /// The mark named by an `m` chord.
    fn finish_mark(&mut self, key: &str) {
        self.pending = Pending::None;
        if let Some(c) = key
            .chars()
            .next()
            .filter(|c| key.chars().count() == 1 && c.is_ascii_alphabetic())
        {
            self.marks.insert(c, self.cursor);
        }
    }

    /// The mark named by a `` ` `` / `'` chord, jumped to or operated on.
    ///
    /// An unset mark is not an error, it is nothing: vim beeps, and the
    /// buffer must be left exactly as it was.
    fn finish_mark_jump(&mut self, op: Option<char>, linewise: bool, key: &str) {
        self.pending = Pending::None;
        let n = if op.is_some() {
            self.take_count_for_op()
        } else {
            self.take_count()
        };
        let _ = n;
        let Some(&raw) = key
            .chars()
            .next()
            .filter(|_| key.chars().count() == 1)
            .and_then(|c| self.marks.get(&c))
        else {
            return;
        };
        let mut target = raw.min(self.buf.len());
        while target > 0 && !self.buf.is_char_boundary(target) {
            target -= 1;
        }
        if linewise {
            target = self.first_non_blank(target);
        }
        let kind = if linewise {
            MotionKind::Linewise
        } else {
            MotionKind::Exclusive
        };
        match op {
            Some(op) => self.apply_operator(op, self.cursor, target, kind),
            None => self.cursor = target,
        }
    }

    /// The register a `q` chord starts recording into.
    fn finish_record(&mut self, key: &str) {
        self.pending = Pending::None;
        if let Some(c) = key
            .chars()
            .next()
            .filter(|c| key.chars().count() == 1 && c.is_ascii_alphabetic())
        {
            self.recording = Some((c, Vec::new()));
        }
    }

    /// The macro an `@` chord replays; `@@` repeats the last one.
    fn finish_run_macro(&mut self, key: &str) {
        self.pending = Pending::None;
        let n = self.take_count();
        let reg = if key == "@" {
            self.last_macro
        } else {
            key.chars()
                .next()
                .filter(|c| key.chars().count() == 1 && c.is_ascii_alphabetic())
        };
        if let Some(reg) = reg {
            self.run_macro(reg, n);
        }
    }

    /// Replay macro `reg` `n` times.
    ///
    /// The replay records nothing (a macro that recorded itself would
    /// grow without bound) and leaves INSERT behind it, so a macro that
    /// ends mid-typing is repeatable.
    fn run_macro(&mut self, reg: char, n: usize) {
        let Some(steps) = self.macros.get(&reg).cloned() else {
            return;
        };
        self.last_macro = Some(reg);
        let outer = self.running_macro;
        self.running_macro = true;
        for _ in 0..n {
            for step in &steps {
                match step {
                    MacroStep::Key(k) if k == "escape" && self.mode == EditorMode::Insert => {
                        self.to_normal();
                    }
                    MacroStep::Key(k) => self.dispatch_modal_key(k),
                    MacroStep::Text(t) => {
                        let t = t.clone();
                        self.insert_str(&t);
                    }
                }
            }
            if self.mode == EditorMode::Insert {
                self.to_normal();
            }
        }
        self.running_macro = outer;
    }

    /// The register being recorded into, for a shell's `recording @q`
    /// indicator. `None` when nothing is.
    #[must_use]
    pub const fn recording_register(&self) -> Option<char> {
        match self.recording {
            Some((reg, _)) => Some(reg),
            None => None,
        }
    }

    /// Whether typing overwrites rather than inserts (`R`).
    #[must_use]
    pub const fn replacing(&self) -> bool {
        self.replacing
    }

    /// The open search line as the user sees it (`/beta`), or `None`.
    #[must_use]
    pub fn search_prompt(&self) -> Option<String> {
        self.search_input
            .as_ref()
            .map(|(fwd, pat)| format!("{}{pat}", if *fwd { '/' } else { '?' }))
    }

    /// The last search pattern, for a shell that highlights matches.
    #[must_use]
    pub fn search_pattern(&self) -> Option<&str> {
        self.last_search.as_ref().map(|(p, _)| p.as_str())
    }

    /// Open the `/` (or `?`) line, carrying any armed operator with it
    /// so `d/foo` deletes up to the match.
    fn open_search(&mut self, op: Option<char>, forward: bool) {
        self.search_input = Some((forward, String::new()));
        self.search_op = op;
    }

    /// One key of the open search line.
    fn search_key(&mut self, key: &str) {
        let Some((forward, pattern)) = self.search_input.as_mut() else {
            return;
        };
        match key {
            "escape" => {
                self.search_input = None;
                self.search_op = None;
                self.count = 0;
                self.op_count = 0;
            }
            "enter" => {
                let forward = *forward;
                let pattern = std::mem::take(pattern);
                self.search_input = None;
                let op = self.search_op.take();
                if pattern.is_empty() {
                    return;
                }
                self.last_search = Some((pattern, forward));
                let n = if op.is_some() {
                    self.take_count_for_op()
                } else {
                    self.take_count()
                };
                self.repeat_search(op, true, n);
            }
            // Backspacing past the `/` closes the line, the way it
            // closes vim's — the pattern and its prompt are one thing.
            "backspace" => {
                if pattern.pop().is_none() {
                    self.search_input = None;
                    self.search_op = None;
                }
            }
            k => {
                if let Some(c) = k.chars().next().filter(|_| k.chars().count() == 1) {
                    pattern.push(c);
                }
            }
        }
    }

    /// Run the last search `n` times, `same` direction or reversed
    /// (`N`), optionally as an operator target.
    fn repeat_search(&mut self, op: Option<char>, same: bool, n: usize) {
        let Some((pattern, forward)) = self.last_search.clone() else {
            self.count = 0;
            self.op_count = 0;
            return;
        };
        let forward = forward == same;
        let mut at = self.cursor;
        for _ in 0..n {
            let Some(next) = self.find_from(&pattern, at, forward) else {
                self.count = 0;
                self.op_count = 0;
                return;
            };
            at = next;
        }
        match op {
            Some(op) => self.apply_operator(op, self.cursor, at, MotionKind::Exclusive),
            None => self.cursor = at,
        }
    }

    /// `*`/`#`: search for the word under the cursor.
    fn search_word_under_cursor(&mut self, forward: bool, n: usize) {
        let Some((lo, hi, _)) = self.word_object(false, false, 1) else {
            return;
        };
        let word = self.buf[lo..hi].to_owned();
        if word.trim().is_empty() {
            return;
        }
        self.last_search = Some((word, forward));
        self.repeat_search(None, true, n);
    }

    /// The next occurrence of `pattern` from `at`, wrapping around the
    /// buffer exactly once. `None` when the pattern is nowhere.
    fn find_from(&self, pattern: &str, at: usize, forward: bool) -> Option<usize> {
        if pattern.is_empty() {
            return None;
        }
        if forward {
            let from = self.next_offset(at);
            self.buf
                .get(from..)
                .and_then(|s| s.find(pattern))
                .map(|i| from + i)
                .or_else(|| self.buf.find(pattern))
        } else {
            self.buf
                .get(..at)
                .and_then(|s| s.rfind(pattern))
                .or_else(|| self.buf.rfind(pattern))
        }
    }

    /// `C-a`/`C-x`: add `delta` to the first number at or right of the
    /// cursor on its line, leaving the cursor on the number's last
    /// digit (vim's rule).
    fn change_number(&mut self, delta: i64) {
        let line_end = self.line_end(self.cursor);
        let Some(digit) = self.buf[self.cursor..line_end]
            .char_indices()
            .find(|&(_, c)| c.is_ascii_digit())
            .map(|(i, _)| self.cursor + i)
        else {
            return;
        };
        let line_start = self.line_start(self.cursor);
        let mut lo = digit;
        while lo > line_start && self.buf.as_bytes()[lo - 1].is_ascii_digit() {
            lo -= 1;
        }
        // A `-` immediately before the digits is part of the number.
        if lo > line_start && self.buf.as_bytes()[lo - 1] == b'-' {
            lo -= 1;
        }
        let mut hi = digit;
        while hi < line_end && self.buf.as_bytes()[hi].is_ascii_digit() {
            hi += 1;
        }
        let Ok(value) = self.buf[lo..hi].parse::<i64>() else {
            return;
        };
        self.checkpoint();
        let replacement = (value.saturating_add(delta)).to_string();
        self.buf.replace_range(lo..hi, &replacement);
        self.cursor = lo + replacement.len() - 1;
    }

    /// The replacement char of an `r` chord.
    fn finish_replace(&mut self, key: &str) {
        self.pending = Pending::None;
        let n = self.take_count();
        let Some(c) = key.chars().next().filter(|_| key.chars().count() == 1) else {
            return;
        };
        if key == "escape" {
            return;
        }
        // Vim refuses `r` when fewer than `count` chars remain on the line.
        let mut end = self.cursor;
        for _ in 0..n {
            let next = self.next_offset(end);
            if next == end || self.buf[end..].starts_with('\n') {
                return;
            }
            end = next;
        }
        self.checkpoint();
        let replacement: String = std::iter::repeat_n(c, n).collect();
        self.buf.replace_range(self.cursor..end, &replacement);
        self.cursor = self.prev_offset(end.min(self.buf.len())).max(self.cursor);
    }

    // ---- Motions -------------------------------------------------

    /// Resolve `key` to a target offset and how it combines into a
    /// range. `for_op` applies vim's rule that `dw` never joins lines.
    /// The column a vertical motion should aim for: the one it has been
    /// aiming for since the last horizontal move, else where the cursor
    /// is now.
    fn wanted_col(&self) -> usize {
        self.wanted_col.unwrap_or_else(|| self.cursor_line_col().1)
    }

    /// Remember the column for the vertical motions, or forget it.
    ///
    /// Everything horizontal — a motion, an edit, a click — decides a
    /// new column; only `j`/`k` and the arrows inherit one.
    const fn set_wanted_col(&mut self, col: Option<usize>) {
        self.wanted_col = col;
    }

    fn motion(&self, key: &str, n: usize, for_op: bool) -> Option<(usize, MotionKind)> {
        use MotionKind::{Exclusive, Inclusive, Linewise};
        let (target, kind) = match key {
            "h" | "left" => (self.repeat(n, self.cursor, Self::offset_left), Exclusive),
            "l" | "right" => (self.repeat(n, self.cursor, Self::offset_right), Exclusive),
            // Vertical motion keeps the column, and keeps *wanting* the
            // column it started in: vim's `curswant`, so passing through
            // a short line on the way down does not cost you the place
            // you were reading. It used to be a flat column zero, so
            // every `j` jumped to the start of the next line.
            "j" | "down" => (
                self.line_col_offset(self.cursor_line() + n, self.wanted_col()),
                Linewise,
            ),
            "k" | "up" => (
                self.line_col_offset(self.cursor_line().saturating_sub(n), self.wanted_col()),
                Linewise,
            ),
            "w" | "W" => {
                let big = key == "W";
                let mut at = self.cursor;
                for _ in 0..n {
                    at = self.word_forward_from(at, big);
                }
                // `dw` on the last word of a line stops at the line end
                // instead of dragging the next line up.
                if for_op && self.line_end(self.cursor) < at {
                    (self.line_end(self.cursor), Exclusive)
                } else {
                    (at, Exclusive)
                }
            }
            "b" | "B" => {
                let big = key == "B";
                let mut at = self.cursor;
                for _ in 0..n {
                    at = self.word_backward_from(at, big);
                }
                (at, Exclusive)
            }
            "e" | "E" => {
                let big = key == "E";
                let mut at = self.cursor;
                for _ in 0..n {
                    at = self.word_end_from(at, big);
                }
                (at, Inclusive)
            }
            "0" | "home" => (self.line_start(self.cursor), Exclusive),
            "^" => (self.first_non_blank(self.cursor), Exclusive),
            "pagedown" => (
                self.line_col_offset(self.cursor_line() + PAGE_LINES * n, 0),
                Linewise,
            ),
            "pageup" => (
                self.line_col_offset(self.cursor_line().saturating_sub(PAGE_LINES * n), 0),
                Linewise,
            ),
            "$" | "end" => {
                let line = self.cursor_line() + n - 1;
                (self.line_end(self.line_col_offset(line, 0)), Exclusive)
            }
            "G" => {
                let last = self.line_count().saturating_sub(1);
                let line = if self.count_given { n - 1 } else { last };
                (self.line_col_offset(line.min(last), 0), Linewise)
            }
            "}" => (self.paragraph_forward(n), Exclusive),
            "{" => (self.paragraph_backward(n), Exclusive),
            // The first-non-blank line motions. `_` counts the current
            // line, `+`/`-` count from it — vim's off-by-one, kept.
            "_" => (
                self.first_non_blank(self.line_col_offset(self.cursor_line() + n - 1, 0)),
                Linewise,
            ),
            "+" | "enter" => (
                self.first_non_blank(self.line_col_offset(self.cursor_line() + n, 0)),
                Linewise,
            ),
            "-" => (
                self.first_non_blank(self.line_col_offset(self.cursor_line().saturating_sub(n), 0)),
                Linewise,
            ),
            // `|` is 1-based: `1|` is the line start.
            "|" => (self.line_col_offset(self.cursor_line(), n - 1), Exclusive),
            "%" => (self.matching_bracket()?, Inclusive),
            "C-f" | "C-b" | "C-d" | "C-u" => (self.scroll_motion(key, n), Linewise),
            ";" | "," => {
                let (kind, ch) = self.last_find?;
                let kind = if key == ";" {
                    kind
                } else {
                    match kind {
                        'f' => 'F',
                        'F' => 'f',
                        't' => 'T',
                        _ => 't',
                    }
                };
                let target = self.find_char(kind, ch, n)?;
                let k = if matches!(kind, 'f' | 't') {
                    Inclusive
                } else {
                    Exclusive
                };
                (target, k)
            }
            _ => return None,
        };
        Some((target, kind))
    }

    /// `C-f`/`C-b` (a page) and `C-d`/`C-u` (half of one).
    ///
    /// The editor owns no viewport — the shells derive their scroll
    /// from the cursor — so moving the cursor *is* the scroll, and both
    /// pairs are line motions rather than viewport commands.
    fn scroll_motion(&self, key: &str, n: usize) -> usize {
        let step = if matches!(key, "C-f" | "C-b") {
            PAGE_LINES
        } else {
            PAGE_LINES / 2
        };
        let line = self.cursor_line();
        let target = if matches!(key, "C-f" | "C-d") {
            line + step * n
        } else {
            line.saturating_sub(step * n)
        };
        self.line_col_offset(target, 0)
    }

    /// `%`: the bracket matching the first one at or right of the
    /// cursor on its line.
    ///
    /// Vim scans forward for a bracket before matching, so `%` works
    /// from anywhere on a line that has one; the match itself nests and
    /// crosses lines. `None` when the line holds no bracket, or when
    /// the one it holds is unbalanced.
    fn matching_bracket(&self) -> Option<usize> {
        const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];
        let line_end = self.line_end(self.cursor);
        let (at, here) = self.buf[self.cursor..line_end]
            .char_indices()
            .find(|&(_, c)| PAIRS.iter().any(|&(o, cl)| c == o || c == cl))
            .map(|(i, c)| (self.cursor + i, c))?;
        let forward = PAIRS.iter().find(|&&(o, _)| o == here);
        let mut depth = 0usize;
        if let Some(&(open, close)) = forward {
            for (i, c) in self.buf[self.next_offset(at)..].char_indices() {
                if c == open {
                    depth += 1;
                } else if c == close {
                    if depth == 0 {
                        return Some(self.next_offset(at) + i);
                    }
                    depth -= 1;
                }
            }
            return None;
        }
        let &(open, close) = PAIRS.iter().find(|&&(_, cl)| cl == here)?;
        for (i, c) in self.buf[..at].char_indices().rev() {
            if c == close {
                depth += 1;
            } else if c == open {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
        }
        None
    }

    /// Apply `f`/`F`/`t`/`T` `n` times within the cursor's line.
    fn find_char(&self, kind: char, target: char, n: usize) -> Option<usize> {
        let line_start = self.line_start(self.cursor);
        let line_end = self.line_end(self.cursor);
        let mut at = self.cursor;
        for _ in 0..n {
            at = match kind {
                'f' | 't' => {
                    // `t` starts one char further so `;` makes progress.
                    let from = self.next_offset(if kind == 't' {
                        self.next_offset(at)
                    } else {
                        at
                    });
                    let rel = self.buf[from.min(line_end)..line_end].find(target)?;
                    from + rel
                }
                _ => {
                    let to = if kind == 'T' {
                        self.prev_offset(at)
                    } else {
                        at
                    };
                    let rel = self.buf[line_start..to.max(line_start)].rfind(target)?;
                    line_start + rel
                }
            };
        }
        Some(match kind {
            't' => self.prev_offset(at),
            'T' => self.next_offset(at),
            _ => at,
        })
    }

    // ---- Text objects --------------------------------------------

    /// Resolve `iw`/`aw`, `i(`/`a(` …, `i"`/`a"`, `ip`/`ap` to a byte
    /// range plus its kind. `None` when the cursor is not inside one.
    /// Wrap `lo..hi` in the pair `key` names — the second half of
    /// `ys{motion}`, of `yss`, and of VISUAL `S`.
    fn finish_surround(&mut self, lo: usize, hi: usize, key: &str) {
        self.pending = Pending::None;
        let Some((open, close)) = key
            .chars()
            .next()
            .filter(|_| key.chars().count() == 1)
            .and_then(surround_pair)
        else {
            // `escape`, or a char that names no pair (`t` — HTML tags
            // are not implemented). Nothing typed, nothing changed.
            return;
        };
        let hi = hi.min(self.buf.len());
        let lo = lo.min(hi);
        self.checkpoint();
        self.buf.insert_str(hi, close);
        self.buf.insert_str(lo, open);
        self.cursor = lo;
        self.mode = EditorMode::Normal;
        self.edit_seq += 1;
    }

    /// `ds{char}`: take the pair away, keep what was inside it.
    fn finish_surround_delete(&mut self, key: &str) {
        self.pending = Pending::None;
        let Some((outer, inner)) = self.surround_spans(key) else {
            return;
        };
        let kept = self.buf[inner.0..inner.1].to_owned();
        self.checkpoint();
        self.buf.replace_range(outer.0..outer.1, &kept);
        self.cursor = outer.0;
        self.edit_seq += 1;
    }

    /// `cs{old}{new}`: the first stroke names the pair to find, the
    /// second what to put in its place.
    fn finish_surround_change(&mut self, old: Option<char>, key: &str) {
        let Some(c) = key.chars().next().filter(|_| key.chars().count() == 1) else {
            self.pending = Pending::None;
            return;
        };
        let Some(old) = old else {
            self.pending = Pending::SurroundChange(Some(c));
            return;
        };
        self.pending = Pending::None;
        let Some((open, close)) = surround_pair(c) else {
            return;
        };
        let Some((outer, inner)) = self.surround_spans(&old.to_string()) else {
            return;
        };
        let replaced = format!("{open}{}{close}", &self.buf[inner.0..inner.1]);
        self.checkpoint();
        self.buf.replace_range(outer.0..outer.1, &replaced);
        self.cursor = outer.0;
        self.edit_seq += 1;
    }

    /// The pair `key` names around the cursor, as `(outer, inner)` byte
    /// ranges — the delimiters included and excluded.
    ///
    /// It is the `a…`/`i…` text objects doing the finding, which is why
    /// `ds)` understands nesting and `ds*` understands org emphasis:
    /// they are the same two spans `di)` and `da)` already resolve.
    fn surround_spans(&self, key: &str) -> Option<((usize, usize), (usize, usize))> {
        let c = key.chars().next().filter(|_| key.chars().count() == 1)?;
        surround_pair(c)?;
        let (alo, ahi, _) = self.text_object(c, true, 1)?;
        let (ilo, ihi, _) = self.text_object(c, false, 1)?;
        (alo < ilo && ihi < ahi).then_some(((alo, ahi), (ilo, ihi)))
    }

    fn text_object(&self, obj: char, around: bool, n: usize) -> Option<(usize, usize, MotionKind)> {
        match obj {
            'w' | 'W' => self.word_object(obj == 'W', around, n),
            's' => self.sentence_object(around),
            'p' => self.paragraph_object(around),
            // Org's emphasis markers are same-char pairs exactly like
            // the quotes, so `ci*` changes the bold run and `ds/` takes
            // the italics off — the constructs this app is made of.
            '"' | '\'' | '`' | '*' | '/' | '_' | '=' | '~' | '+' => self.quote_object(obj, around),
            '(' | ')' | 'b' => self.bracket_object('(', ')', around),
            '[' | ']' => self.bracket_object('[', ']', around),
            '{' | '}' | 'B' => self.bracket_object('{', '}', around),
            '<' | '>' => self.bracket_object('<', '>', around),
            _ => None,
        }
    }

    /// `iw`/`aw`: the run of same-class chars under the cursor, plus
    /// (for `aw`) the blanks after it — or before it at a line end.
    fn word_object(&self, big: bool, around: bool, n: usize) -> Option<(usize, usize, MotionKind)> {
        let c = self.char_at(self.cursor)?;
        let class = char_class(c, big);
        let mut lo = self.cursor;
        while let Some((i, prev)) = self.buf[..lo].char_indices().next_back() {
            if prev != '\n' && char_class(prev, big) == class {
                lo = i;
            } else {
                break;
            }
        }
        let mut hi = self.cursor;
        while let Some(next) = self.char_at(hi) {
            if next != '\n' && char_class(next, big) == class {
                hi += next.len_utf8();
            } else {
                break;
            }
        }
        // `2iw` spans the following runs too (blanks count as a run).
        for _ in 1..n {
            let Some(next) = self.char_at(hi) else { break };
            if next == '\n' {
                break;
            }
            let next_class = char_class(next, big);
            while let Some(c) = self.char_at(hi) {
                if c != '\n' && char_class(c, big) == next_class {
                    hi += c.len_utf8();
                } else {
                    break;
                }
            }
        }
        if around {
            let trailing_start = hi;
            while let Some(next) = self.char_at(hi) {
                if next != '\n' && next.is_whitespace() {
                    hi += next.len_utf8();
                } else {
                    break;
                }
            }
            // No trailing blanks (last word on the line): take the
            // leading ones instead, the way vim does.
            if hi == trailing_start && class != CharClass::Blank {
                while let Some((i, prev)) = self.buf[..lo].char_indices().next_back() {
                    if prev != '\n' && prev.is_whitespace() {
                        lo = i;
                    } else {
                        break;
                    }
                }
            }
        }
        Some((lo, hi, MotionKind::Exclusive))
    }

    /// `is`/`as`: the sentence around the cursor.
    ///
    /// A sentence ends at `.`, `!` or `?` (with any closing bracket or
    /// quote after it) and runs to the start of the next one. `as`
    /// takes the blanks that separate it from its successor, so
    /// `das` leaves the surrounding prose spaced as it was.
    ///
    /// Always resolves — every cursor sits in some sentence — but it
    /// returns the shape of its sibling objects so the caller can treat
    /// all of them alike.
    #[allow(clippy::unnecessary_wraps)]
    fn sentence_object(&self, around: bool) -> Option<(usize, usize, MotionKind)> {
        const CLOSERS: [char; 4] = [')', ']', '"', '\''];
        let para_start = self.line_start(self.cursor);
        let para_end = self.line_end(self.cursor);
        let text = &self.buf[para_start..para_end];
        // Every sentence end within the line, as an offset one past the
        // terminator and its closers.
        let mut ends = Vec::new();
        let bytes: Vec<(usize, char)> = text.char_indices().collect();
        for (i, (off, c)) in bytes.iter().enumerate() {
            if !matches!(c, '.' | '!' | '?') {
                continue;
            }
            let mut end = off + c.len_utf8();
            let mut j = i + 1;
            while let Some((o, c)) = bytes.get(j).filter(|(_, c)| CLOSERS.contains(c)) {
                end = o + c.len_utf8();
                j += 1;
            }
            // A terminator mid-word (`3.5`) does not end a sentence.
            if bytes.get(j).is_none_or(|(_, c)| c.is_whitespace()) {
                ends.push(end);
            }
        }
        let here = self.cursor - para_start;
        let lo = ends.iter().copied().rfind(|&e| e <= here).map_or(0, |e| {
            e + text[e..]
                .find(|c: char| !c.is_whitespace())
                .unwrap_or(text.len() - e)
        });
        let mut hi = ends
            .iter()
            .copied()
            .find(|&e| e > here)
            .unwrap_or(text.len());
        if around {
            hi += text[hi..]
                .find(|c: char| !c.is_whitespace())
                .unwrap_or(text.len() - hi);
        }
        Some((para_start + lo, para_start + hi, MotionKind::Exclusive))
    }

    /// `ip`/`ap`: the run of blank or non-blank lines around the cursor.
    /// Always resolves — every cursor sits in some paragraph — but it
    /// returns the same shape as its sibling objects so the caller can
    /// treat all of them alike.
    #[allow(clippy::unnecessary_wraps)]
    fn paragraph_object(&self, around: bool) -> Option<(usize, usize, MotionKind)> {
        let blank = |line: usize, ed: &Self| -> bool {
            let s = ed.line_col_offset(line, 0);
            ed.buf[s..ed.line_end(s)].trim().is_empty()
        };
        let cur = self.cursor_line();
        let last = self.line_count().saturating_sub(1);
        let want = blank(cur, self);
        let mut first = cur;
        while first > 0 && blank(first - 1, self) == want {
            first -= 1;
        }
        let mut end = cur;
        while end < last && blank(end + 1, self) == want {
            end += 1;
        }
        if around {
            let tail = end;
            while end < last && blank(end + 1, self) != want {
                end += 1;
            }
            if end == tail {
                while first > 0 && blank(first - 1, self) != want {
                    first -= 1;
                }
            }
        }
        let lo = self.line_col_offset(first, 0);
        let hi = self.line_end(self.line_col_offset(end, 0));
        Some((lo, hi, MotionKind::Linewise))
    }

    /// `i"`/`a"`: the quoted run on the cursor's line. Quotes pair up
    /// left to right, so the cursor picks the first pair it is not
    /// already past.
    fn quote_object(&self, q: char, around: bool) -> Option<(usize, usize, MotionKind)> {
        let start = self.line_start(self.cursor);
        let end = self.line_end(self.cursor);
        let positions: Vec<usize> = self.buf[start..end]
            .char_indices()
            .filter(|&(_, c)| c == q)
            .map(|(i, _)| start + i)
            .collect();
        let (open, close) = positions
            .chunks_exact(2)
            .map(|p| (p[0], p[1]))
            .find(|&(_, close)| self.cursor <= close)?;
        Some(if around {
            (open, self.next_offset(close), MotionKind::Exclusive)
        } else {
            (self.next_offset(open), close, MotionKind::Exclusive)
        })
    }

    /// `i(`/`a(` and friends: the innermost balanced pair containing
    /// the cursor, brackets included when `around`.
    fn bracket_object(
        &self,
        open_c: char,
        close_c: char,
        around: bool,
    ) -> Option<(usize, usize, MotionKind)> {
        let here = self.char_at(self.cursor);
        // Sitting on a bracket picks that pair rather than its parent.
        let open = if here == Some(open_c) {
            self.cursor
        } else {
            let mut depth = 0usize;
            let mut found = None;
            let search_end = if here == Some(close_c) {
                self.cursor
            } else {
                self.next_offset(self.cursor)
            };
            for (i, c) in self.buf[..search_end].char_indices().rev() {
                if c == close_c {
                    depth += 1;
                } else if c == open_c {
                    if depth == 0 {
                        found = Some(i);
                        break;
                    }
                    depth -= 1;
                }
            }
            found?
        };
        let mut depth = 0usize;
        let mut close = None;
        for (i, c) in self.buf[self.next_offset(open)..].char_indices() {
            if c == open_c {
                depth += 1;
            } else if c == close_c {
                if depth == 0 {
                    close = Some(self.next_offset(open) + i);
                    break;
                }
                depth -= 1;
            }
        }
        let close = close?;
        Some(if around {
            (open, self.next_offset(close), MotionKind::Exclusive)
        } else {
            (self.next_offset(open), close, MotionKind::Exclusive)
        })
    }

    // ---- Operators -----------------------------------------------

    /// Combine cursor and motion target into a range, then run the
    /// operator over it.
    fn apply_operator(&mut self, op: char, from: usize, target: usize, kind: MotionKind) {
        let (lo, hi) = match kind {
            // Linewise ranges are widened to whole lines downstream, so
            // the raw endpoints are all `run_linewise` needs.
            MotionKind::Exclusive | MotionKind::Linewise => (from.min(target), from.max(target)),
            MotionKind::Inclusive => {
                let hi = from.max(target);
                (from.min(target), self.next_offset(hi))
            }
        };
        self.apply_range_operator(op, lo, hi, kind);
    }

    /// Run `op` over an already-resolved range.
    fn apply_range_operator(&mut self, op: char, lo: usize, hi: usize, kind: MotionKind) {
        self.count = 0;
        self.op_count = 0;
        if kind == MotionKind::Linewise {
            let first = self.line_start(lo);
            let last_end = self.line_end(hi);
            return self.run_linewise(op, first, last_end);
        }
        if hi <= lo {
            return;
        }
        // The surround "operator" resolves a range like any other and
        // then waits: the pair it wraps in is the *next* stroke.
        if op == 's' {
            self.pending = Pending::SurroundWith { lo, hi };
            return;
        }
        match op {
            'y' => {
                self.set_register(self.buf[lo..hi].to_owned(), false);
                self.cursor = lo;
            }
            'd' | 'c' => {
                self.checkpoint();
                self.set_register(self.buf[lo..hi].to_owned(), false);
                self.buf.replace_range(lo..hi, "");
                self.cursor = lo;
                if op == 'c' {
                    self.mode = EditorMode::Insert;
                    self.insert_armed = false;
                } else if self.mode != EditorMode::Normal {
                    self.mode = EditorMode::Normal;
                }
            }
            '>' | '<' => self.run_linewise(op, self.line_start(lo), self.line_end(hi)),
            'u' | 'U' | '~' => {
                self.checkpoint();
                self.recase(lo, hi, op);
                self.cursor = lo;
                if self.mode != EditorMode::Normal {
                    self.mode = EditorMode::Normal;
                }
            }
            _ => {}
        }
    }

    /// Rewrite `lo..hi` to lower (`u`), upper (`U`) or flipped (`~`).
    fn recase(&mut self, lo: usize, hi: usize, op: char) {
        let mut recased = String::with_capacity(hi - lo);
        for c in self.buf[lo..hi].chars() {
            let to_upper = match op {
                'u' => false,
                'U' => true,
                _ => !c.is_uppercase(),
            };
            if to_upper {
                recased.extend(c.to_uppercase());
            } else {
                recased.extend(c.to_lowercase());
            }
        }
        self.buf.replace_range(lo..hi, &recased);
    }

    /// `dd`/`yy`/`cc`/`>>`: `n` whole lines from the cursor's.
    fn apply_linewise_operator(&mut self, op: char, n: usize) {
        self.count = 0;
        self.op_count = 0;
        let first = self.line_start(self.cursor);
        let mut last_end = self.line_end(first);
        for _ in 1..n {
            if last_end >= self.buf.len() {
                break;
            }
            last_end = self.line_end(last_end + 1);
        }
        self.run_linewise(op, first, last_end);
    }

    /// The linewise core: `first` is a line start, `last_end` the end
    /// (before the newline) of the last line in the range.
    fn run_linewise(&mut self, op: char, first: usize, last_end: usize) {
        let with_newline = if last_end < self.buf.len() {
            last_end + 1
        } else {
            last_end
        };
        match op {
            'y' => {
                self.set_register(Self::as_lines(&self.buf[first..with_newline]), true);
                self.cursor = first;
            }
            'd' => {
                self.checkpoint();
                self.set_register(Self::as_lines(&self.buf[first..with_newline]), true);
                // Deleting through the last (newline-less) line eats the
                // preceding newline so no dangling terminator remains.
                let cut_from = if with_newline >= self.buf.len() && first > 0 {
                    first - 1
                } else {
                    first
                };
                self.buf.replace_range(cut_from..with_newline, "");
                // Vim parks the cursor on the line that moved up into
                // the deleted one, clamped at the last line.
                self.cursor = self.line_start(cut_from.min(self.buf.len()));
            }
            'c' => {
                self.checkpoint();
                self.set_register(Self::as_lines(&self.buf[first..with_newline]), true);
                // The line itself survives, emptied — that is what makes
                // `cc` different from `dd` followed by `O`.
                self.buf.replace_range(first..last_end, "");
                self.cursor = first;
                self.mode = EditorMode::Insert;
                self.insert_armed = false;
            }
            '>' | '<' => {
                self.checkpoint();
                self.shift_lines(first, last_end, op == '>');
            }
            'u' | 'U' | '~' => {
                self.checkpoint();
                self.recase(first, last_end, op);
                self.cursor = first;
            }
            _ => {}
        }
        if self.mode != EditorMode::Insert {
            self.mode = EditorMode::Normal;
        }
    }

    /// Indent (`>`) or dedent (`<`) every line in the range by two
    /// spaces — org bodies are space-indented, never tabbed.
    fn shift_lines(&mut self, first: usize, last_end: usize, indent: bool) {
        let mut starts = vec![first];
        let mut at = first;
        while at < last_end {
            let end = self.line_end(at);
            if end >= last_end {
                break;
            }
            at = end + 1;
            starts.push(at);
        }
        // Back to front so earlier edits do not shift later offsets.
        for &start in starts.iter().rev() {
            if indent {
                self.buf.insert_str(start, "  ");
            } else {
                let end = self.line_end(start);
                let drop = self.buf[start..end]
                    .chars()
                    .take(2)
                    .take_while(|c| *c == ' ')
                    .count();
                self.buf.replace_range(start..start + drop, "");
            }
        }
        self.cursor = self.first_non_blank(self.line_start(first.min(self.buf.len())));
    }

    /// `D`/`C`: the rest of the line, `n - 1` further lines included.
    fn operate_to_line_end(&mut self, op: char) {
        let n = self.take_count();
        let mut end = self.line_end(self.cursor);
        for _ in 1..n {
            if end >= self.buf.len() {
                break;
            }
            end = self.line_end(end + 1);
        }
        self.apply_range_operator(op, self.cursor, end, MotionKind::Exclusive);
        if op == 'c' {
            self.mode = EditorMode::Insert;
            self.insert_armed = false;
        }
    }

    /// A Visual-mode operator over the charwise selection.
    /// Insert text from outside the editor at the cursor, as one
    /// undoable edit, replacing a VISUAL selection if there is one.
    ///
    /// Separate from [`Self::insert_str`] because that one is *typing*:
    /// it feeds the dot-register so `.` retypes it. A clipboard paste
    /// is not something the user typed, and `.` repeating it would be
    /// a surprise with somebody else's text in it.
    pub fn paste_external(&mut self, text: &str) {
        self.checkpoint();
        if matches!(self.mode, EditorMode::Visual | EditorMode::VisualLine) {
            let (lo, hi) = if self.mode == EditorMode::VisualLine {
                (
                    self.line_start(self.anchor.min(self.cursor)),
                    self.line_end(self.anchor.max(self.cursor)),
                )
            } else {
                let (lo, hi) = self.selection();
                (lo, hi)
            };
            self.buf.replace_range(lo..hi, "");
            self.cursor = lo;
            self.mode = EditorMode::Normal;
        }
        self.buf.insert_str(self.cursor, text);
        self.cursor += text.len();
        // A Normal cursor never sits past the last character of the
        // line it is on.
        if self.mode == EditorMode::Normal {
            self.cursor = self
                .prev_offset(self.cursor)
                .max(self.line_start(self.cursor));
        }
    }

    fn visual_operator(&mut self, op: char) {
        if self.mode == EditorMode::VisualLine {
            return self.visual_linewise_operator(op);
        }
        let (lo, hi) = self.selection();
        self.apply_range_operator(op, lo, hi, MotionKind::Exclusive);
        if self.mode != EditorMode::Insert {
            self.mode = EditorMode::Normal;
        }
    }

    /// A Visual-mode operator forced linewise (`V`-mode, or `D`/`C`).
    fn visual_linewise_operator(&mut self, op: char) {
        let first = self.line_start(self.anchor.min(self.cursor));
        let last_end = self.line_end(self.anchor.max(self.cursor));
        self.count = 0;
        self.op_count = 0;
        self.run_linewise(op, first, last_end);
    }

    /// Visual `p`: the selection is replaced by the register, and the
    /// replaced text becomes the new register (vim's swap).
    fn visual_paste(&mut self) {
        let (lo, hi) = if self.mode == EditorMode::VisualLine {
            let first = self.line_start(self.anchor.min(self.cursor));
            let end = self.line_end(self.anchor.max(self.cursor));
            (first, end)
        } else {
            self.selection()
        };
        self.checkpoint();
        let (text, linewise) = self.take_register();
        let replaced = self.buf[lo..hi].to_owned();
        let insert = if linewise {
            text.trim_end_matches('\n').to_owned()
        } else {
            text
        };
        self.buf.replace_range(lo..hi, &insert);
        self.register = replaced;
        self.linewise = false;
        self.cursor = lo;
        self.mode = EditorMode::Normal;
    }

    /// `x`/`s`: delete up to `n` chars forward, never past the line end.
    fn delete_chars_forward(&mut self, n: usize) {
        let start = self.cursor;
        let line_end = self.line_end(start);
        let mut end = start;
        for _ in 0..n {
            let next = self.next_offset(end);
            if next == end || next > line_end {
                break;
            }
            end = next;
        }
        if end > start {
            self.set_register(self.buf[start..end].to_owned(), false);
            self.buf.replace_range(start..end, "");
        }
    }

    /// `~`: flip the case under the cursor and step right.
    fn toggle_case_at_cursor(&mut self) {
        let Some(c) = self.char_at(self.cursor).filter(|c| *c != '\n') else {
            return;
        };
        let flipped: String = if c.is_uppercase() {
            c.to_lowercase().collect()
        } else {
            c.to_uppercase().collect()
        };
        let end = self.cursor + c.len_utf8();
        self.buf.replace_range(self.cursor..end, &flipped);
        self.cursor = (self.cursor + flipped.len()).min(self.buf.len());
    }

    /// `J`: pull the next line up, separated by exactly one space.
    fn join_line(&mut self) {
        let end = self.line_end(self.cursor);
        if end >= self.buf.len() {
            return;
        }
        let next_start = end + 1;
        let next_end = self.line_end(next_start);
        let tail = self.buf[next_start..next_end].trim_start().to_owned();
        let needs_space =
            !tail.is_empty() && !self.buf[self.line_start(self.cursor)..end].ends_with(' ');
        let joiner = if needs_space { " " } else { "" };
        self.buf
            .replace_range(end..next_end, &format!("{joiner}{tail}"));
        self.cursor = end;
    }

    /// `gJ`: pull the next line up verbatim — no separator inserted and
    /// the next line's own indent kept, which is the whole difference
    /// from [`Self::join_line`].
    fn join_line_raw(&mut self) {
        let end = self.line_end(self.cursor);
        if end >= self.buf.len() {
            return;
        }
        self.buf.replace_range(end..=end, "");
        self.cursor = end;
    }

    /// `O`: open a line above the current one and enter Insert.
    pub fn open_above(&mut self) {
        let start = self.line_start(self.cursor);
        self.buf.insert(start, '\n');
        self.cursor = start;
        self.mode = EditorMode::Insert;
        self.insert_armed = false;
    }

    /// `P`: paste linewise above the current line, charwise at the
    /// cursor (as opposed to [`Self::paste`], which goes after).
    pub fn paste_before(&mut self) {
        let (text, linewise) = self.take_register();
        if text.is_empty() {
            return;
        }
        if linewise {
            let start = self.line_start(self.cursor);
            let text = format!("{}\n", text.trim_end_matches('\n'));
            self.buf.insert_str(start, &text);
            self.cursor = start;
        } else {
            self.buf.insert_str(self.cursor, &text);
        }
    }

    // ---- Offsets and lines ---------------------------------------

    /// The char starting at `pos`, if any.
    fn char_at(&self, pos: usize) -> Option<char> {
        self.buf.get(pos..).and_then(|s| s.chars().next())
    }

    /// The offset after the char at `pos` (`pos` itself at the end).
    fn next_offset(&self, pos: usize) -> usize {
        self.char_at(pos).map_or(pos, |c| pos + c.len_utf8())
    }

    /// The offset of the char before `pos` (`0` at the start).
    fn prev_offset(&self, pos: usize) -> usize {
        self.buf
            .get(..pos)
            .and_then(|s| s.char_indices().next_back())
            .map_or(0, |(i, _)| i)
    }

    /// One char left, stopping at the line start (the `h` motion).
    fn offset_left(&self, pos: usize) -> usize {
        match self.buf[..pos].char_indices().next_back() {
            Some((i, c)) if c != '\n' => i,
            _ => pos,
        }
    }

    /// One char right, stopping at the line end (the `l` motion).
    fn offset_right(&self, pos: usize) -> usize {
        match self.char_at(pos) {
            Some(c) if c != '\n' => pos + c.len_utf8(),
            _ => pos,
        }
    }

    /// Apply an offset step `n` times.
    fn repeat(&self, n: usize, from: usize, step: fn(&Self, usize) -> usize) -> usize {
        let mut at = from;
        for _ in 0..n {
            at = step(self, at);
        }
        at
    }

    /// The 0-based line the cursor is on.
    fn cursor_line(&self) -> usize {
        self.buf[..self.cursor].matches('\n').count()
    }

    /// Total line count (a trailing newline does not open a new line).
    fn line_count(&self) -> usize {
        self.buf.matches('\n').count() + 1
    }

    /// Byte offset of `line`/`col`, both clamped.
    fn line_col_offset(&self, line: usize, col: usize) -> usize {
        let last = self.line_count().saturating_sub(1);
        let line = line.min(last);
        let mut start = 0usize;
        for _ in 0..line {
            start = self.line_end(start);
            if start < self.buf.len() {
                start += 1;
            }
        }
        let end = self.line_end(start);
        let mut pos = start;
        for c in self.buf[start..end].chars().take(col) {
            pos += c.len_utf8();
        }
        pos
    }

    /// The first non-blank char of the line containing `pos` (`^`).
    fn first_non_blank(&self, pos: usize) -> usize {
        let start = self.line_start(pos);
        let end = self.line_end(pos);
        let indent = self.buf[start..end]
            .char_indices()
            .find(|&(_, c)| !c.is_whitespace())
            .map_or(0, |(i, _)| i);
        start + indent
    }

    /// `g_`: the last non-blank char of the line.
    fn first_non_blank_backwards_from_line_end(&self) -> usize {
        let start = self.line_start(self.cursor);
        let end = self.line_end(self.cursor);
        self.buf[start..end]
            .char_indices()
            .rfind(|&(_, c)| !c.is_whitespace())
            .map_or(start, |(i, _)| start + i)
    }

    /// `w`/`W` from `pos`: past the current run, then past the blanks.
    fn word_forward_from(&self, pos: usize, big: bool) -> usize {
        let mut at = pos;
        if let Some(c) = self.char_at(at) {
            let class = char_class(c, big);
            if class != CharClass::Blank {
                while let Some(c) = self.char_at(at) {
                    if char_class(c, big) == class {
                        at += c.len_utf8();
                    } else {
                        break;
                    }
                }
            }
        }
        while let Some(c) = self.char_at(at) {
            if char_class(c, big) == CharClass::Blank {
                at += c.len_utf8();
            } else {
                break;
            }
        }
        at
    }

    /// `b`/`B` from `pos`: back over blanks, then to the run's start.
    fn word_backward_from(&self, pos: usize, big: bool) -> usize {
        let mut at = self.prev_offset(pos);
        if at == pos {
            return 0;
        }
        while at > 0 {
            match self.char_at(at) {
                Some(c) if char_class(c, big) == CharClass::Blank => at = self.prev_offset(at),
                _ => break,
            }
        }
        let Some(class) = self
            .char_at(at)
            .map(|c| char_class(c, big))
            .filter(|c| *c != CharClass::Blank)
        else {
            return at;
        };
        while let Some((i, c)) = self.buf[..at].char_indices().next_back() {
            if char_class(c, big) == class {
                at = i;
            } else {
                break;
            }
        }
        at
    }

    /// `e`/`E` from `pos`: forward to the end of the next run.
    fn word_end_from(&self, pos: usize, big: bool) -> usize {
        let mut at = self.next_offset(pos);
        while let Some(c) = self.char_at(at) {
            if char_class(c, big) == CharClass::Blank {
                at = self.next_offset(at);
            } else {
                break;
            }
        }
        let Some(class) = self.char_at(at).map(|c| char_class(c, big)) else {
            return self.prev_offset(self.buf.len());
        };
        while let Some(c) = self.char_at(self.next_offset(at)) {
            if char_class(c, big) == class && self.next_offset(at) != at {
                at = self.next_offset(at);
            } else {
                break;
            }
        }
        at
    }

    /// `ge`/`gE`: backwards to the previous run's end.
    fn word_end_back(&self, pos: usize, big: bool, n: usize) -> usize {
        let mut at = pos;
        for _ in 0..n {
            at = self.prev_offset(at);
            while at > 0 {
                match self.char_at(at) {
                    Some(c) if char_class(c, big) == CharClass::Blank => at = self.prev_offset(at),
                    _ => break,
                }
            }
        }
        at
    }

    /// `}`: the next blank line, or the last line.
    fn paragraph_forward(&self, n: usize) -> usize {
        let last = self.line_count().saturating_sub(1);
        let mut line = self.cursor_line();
        for _ in 0..n {
            let mut next = line + 1;
            while next <= last && !self.line_is_blank(next) {
                next += 1;
            }
            line = next.min(last);
        }
        self.line_col_offset(line, 0)
    }

    /// `{`: the previous blank line, or line 0.
    fn paragraph_backward(&self, n: usize) -> usize {
        let mut line = self.cursor_line();
        for _ in 0..n {
            let mut prev = line;
            loop {
                if prev == 0 {
                    break;
                }
                prev -= 1;
                if self.line_is_blank(prev) {
                    break;
                }
            }
            line = prev;
        }
        self.line_col_offset(line, 0)
    }

    /// True when `line` holds nothing but whitespace.
    fn line_is_blank(&self, line: usize) -> bool {
        let start = self.line_col_offset(line, 0);
        self.buf[start..self.line_end(start)].trim().is_empty()
    }

    /// Store yanked or deleted text, honouring a `"x` prefix.
    ///
    /// The unnamed register is always written — vim's rule, and what
    /// keeps a bare `p` working after `"ayy`. An uppercase name appends
    /// to the lowercase register instead of replacing it.
    fn set_register(&mut self, text: String, linewise: bool) {
        if let Some(name) = self.target_register.take() {
            let lower = name.to_ascii_lowercase();
            if name.is_uppercase() {
                let entry = self
                    .registers
                    .entry(lower)
                    .or_insert_with(|| (String::new(), linewise));
                entry.0.push_str(&text);
                entry.1 = linewise;
            } else {
                self.registers.insert(lower, (text.clone(), linewise));
            }
        }
        // The seam the clipboard mirror watches. A shell cannot ask
        // "did the register move?" without it, so it would have to
        // write the system clipboard on every keystroke and fight
        // whatever else owns it.
        if self.register != text {
            self.register_gen = self.register_gen.wrapping_add(1);
        }
        self.register = text;
        self.linewise = linewise;
    }

    /// How many times the unnamed register has changed.
    #[must_use]
    pub const fn register_generation(&self) -> u64 {
        self.register_gen
    }

    /// How many times `gcc` (or `gc` over a selection) has been asked
    /// for — the shell answers it, because only the shell knows which
    /// comment the enclosing source block wants.
    #[must_use]
    pub const fn comment_asked(&self) -> u64 {
        self.comment_asked
    }

    /// What a bare `p` would paste.
    #[must_use]
    pub fn register_text(&self) -> &str {
        &self.register
    }

    /// Put the system clipboard into the unnamed register, so `p`
    /// pastes it.
    ///
    /// The other half of "sync with system clipboard (two way)": `y`
    /// and `p` used an internal register, so a URL copied in a browser
    /// could not be pasted with the key a vim user's hands know.
    ///
    /// Text the register already holds is not a change — both
    /// directions of the mirror run on every key, and without that
    /// rule they would take turns clobbering each other. Nor is an
    /// empty clipboard: losing a yank to it would be the worst kind of
    /// surprise.
    pub fn set_register_from_clipboard(&mut self, text: &str) {
        if text.is_empty() || self.register == text {
            return;
        }
        text.clone_into(&mut self.register);
        // A clipboard has no notion of lines, so it pastes as
        // characters — which is what every other application means by
        // a paste.
        self.linewise = false;
        self.register_gen = self.register_gen.wrapping_add(1);
    }

    /// The text a paste takes, honouring a `"x` prefix.
    fn take_register(&mut self) -> (String, bool) {
        match self.target_register.take() {
            Some(name) => self
                .registers
                .get(&name.to_ascii_lowercase())
                .cloned()
                .unwrap_or_default(),
            None => (self.register.clone(), self.linewise),
        }
    }

    /// Normalise a linewise register to always end in a newline.
    fn as_lines(text: &str) -> String {
        let mut out = text.to_owned();
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out
    }

    /// The stroke of the chord in progress, if any.
    #[must_use]
    pub const fn pending_stroke(&self) -> Option<char> {
        // An open search line is mid-chord too: without this the shells
        // would read its `Esc` as "cancel the whole edit".
        match self.search_input {
            Some((true, _)) => Some('/'),
            Some((false, _)) => Some('?'),
            None => self.pending.stroke(),
        }
    }

    /// The whole chord in progress as the user typed it (`2d3i`) —
    /// what a shell echoes in its status line. Empty when idle.
    ///
    /// The editor renders this itself rather than handing shells the
    /// pieces, so the terminal and the GUI show the same thing (I4).
    #[must_use]
    pub fn pending_chord(&self) -> String {
        if let Some(prompt) = self.search_prompt() {
            return prompt;
        }
        let mut out = String::new();
        if self.op_count > 0 {
            out.push_str(&self.op_count.to_string());
        }
        if let Some(op) = self.pending_op() {
            out.push(op);
        }
        if self.count > 0 {
            out.push_str(&self.count.to_string());
        }
        match self.pending {
            Pending::G(_) => out.push('g'),
            Pending::Comment => out.push_str("gc"),
            Pending::Obj { around, .. } => out.push(if around { 'a' } else { 'i' }),
            Pending::Find { kind, .. } => out.push(kind),
            Pending::Replace => out.push('r'),
            Pending::Register => out.push('"'),
            Pending::Mark => out.push('m'),
            Pending::JumpMark { linewise, .. } => out.push(if linewise { '\'' } else { '`' }),
            Pending::RecordMacro => out.push('q'),
            Pending::RunMacro => out.push('@'),
            // Mid-surround: the range is settled and the pair is what
            // the next stroke says.
            Pending::SurroundWith { .. } | Pending::SurroundDelete => out.push('s'),
            Pending::SurroundChange(old) => {
                out.push('s');
                if let Some(c) = old {
                    out.push(c);
                }
            }
            Pending::Op(_) | Pending::None => {}
        }
        out
    }

    /// The operator armed by the chord in progress, if any.
    const fn pending_op(&self) -> Option<char> {
        match self.pending {
            Pending::Op(c) => Some(c),
            Pending::G(op)
            | Pending::Obj { op, .. }
            | Pending::Find { op, .. }
            | Pending::JumpMark { op, .. } => op,
            Pending::Replace
            | Pending::Register
            | Pending::Mark
            | Pending::Comment
            | Pending::RecordMacro
            | Pending::RunMacro
            | Pending::SurroundWith { .. }
            | Pending::SurroundDelete
            | Pending::SurroundChange(_)
            | Pending::None => None,
        }
    }

    /// The pending vim count (0 = none) - the caller's cancel guard.
    #[must_use]
    pub const fn pending_count(&self) -> usize {
        self.count
    }

    /// Consume the count typed for a plain motion or edit.
    fn take_count(&mut self) -> usize {
        self.count_given = self.count > 0;
        let n = self.count.max(1);
        self.count = 0;
        n
    }

    /// Consume both halves of `[count]op[count]motion` — vim multiplies
    /// them, so `2d3w` deletes six words.
    fn take_count_for_op(&mut self) -> usize {
        self.count_given = self.count > 0 || self.op_count > 0;
        let n = self.count.max(1) * self.op_count.max(1);
        self.count = 0;
        self.op_count = 0;
        n
    }

    /// Record the current state before a mutating edit (bounded at 50).
    fn checkpoint(&mut self) {
        // Every buffer change funnels through here, which makes this the
        // one honest signal that a command was a *change* (what `.`
        // repeats) rather than a motion.
        self.edit_seq = self.edit_seq.wrapping_add(1);
        if self.undo_stack.len() >= 50 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push((self.buf.clone(), self.cursor));
        self.redo_stack.clear();
    }

    /// Selection range for the renderer, or None outside visual modes.
    ///
    /// Returns the charwise selection range in Visual and the line range
    /// from `line_start` of min to `line_end` of max in `VisualLine`.
    #[must_use]
    pub fn visual_selection(&self) -> Option<(usize, usize)> {
        if self.mode == EditorMode::Visual {
            Some(self.selection())
        } else if self.mode == EditorMode::VisualLine {
            let lo = self.line_start(self.anchor.min(self.cursor));
            let hi = self.line_end(self.anchor.max(self.cursor));
            Some((lo, hi))
        } else {
            None
        }
    }

    /// The inclusive Visual selection as an exclusive byte range.
    fn selection(&self) -> (usize, usize) {
        let (lo, hi) = if self.anchor <= self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        };
        let end = self.buf[hi..]
            .chars()
            .next()
            .filter(|c| *c != '\n')
            .map_or(hi, |c| hi + c.len_utf8());
        (lo, end)
    }

    /// `yy`: copy the current line (linewise register).
    pub fn yank_line(&mut self, n: usize) {
        let n = n.max(1);
        let lo = self.line_start(self.cursor);
        let mut hi = lo;
        for _ in 0..n {
            let e = self.line_end(hi);
            hi = e;
            if hi < self.buf.len() {
                hi += 1;
            } else {
                break;
            }
        }
        let mut text = self.buf[lo..hi].to_owned();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        self.register = text;
        self.linewise = true;
    }

    /// `dd`: cut the current line (linewise register).
    pub fn delete_line(&mut self, n: usize) {
        let n = n.max(1);
        let lo = self.line_start(self.cursor);
        let mut hi = lo;
        for _ in 0..n {
            let e = self.line_end(hi);
            hi = e;
            if hi < self.buf.len() {
                hi += 1;
            } else {
                break;
            }
        }
        let mut text = self.buf[lo..hi].to_owned();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        self.register = text;
        self.linewise = true;
        // Deleting through the last (newline-less) line eats the
        // preceding newline so no dangling terminator remains.
        let lo_cut = if hi >= self.buf.len() && !self.buf[lo..hi].ends_with('\n') && lo > 0 {
            lo - 1
        } else {
            lo
        };
        self.buf.replace_range(lo_cut..hi, "");
        self.cursor = if lo_cut > 0 {
            self.line_start(lo_cut - 1)
        } else {
            0
        };
    }

    /// `p`: paste the register — linewise below the current line,
    /// charwise after the cursor.
    pub fn paste(&mut self) {
        let (text, linewise) = self.take_register();
        if text.is_empty() {
            return;
        }
        if linewise {
            let end = self.line_end(self.cursor);
            let text = format!("\n{}", text.trim_end_matches('\n'));
            self.buf.insert_str(end, &text);
            self.cursor = end + 1;
        } else {
            let pos = self.buf[self.cursor..]
                .chars()
                .next()
                .filter(|c| *c != '\n')
                .map_or(self.cursor, |c| self.cursor + c.len_utf8());
            self.buf.insert_str(pos, &text);
            self.cursor = pos;
        }
    }

    /// Readline `C-k`: kill from the cursor to the end of the line
    /// into the register.
    pub fn kill_rest_of_line(&mut self) {
        let end = self.line_end(self.cursor);
        if end > self.cursor {
            self.insert_guard();
            self.register = self.buf[self.cursor..end].to_owned();
            self.linewise = false;
            self.buf.replace_range(self.cursor..end, "");
        }
    }

    /// Readline `C-u`: kill from the line start to the cursor into the
    /// register.
    pub fn kill_to_line_start(&mut self) {
        let start = self.line_start(self.cursor);
        if start < self.cursor {
            self.insert_guard();
            self.register = self.buf[start..self.cursor].to_owned();
            self.linewise = false;
            self.buf.replace_range(start..self.cursor, "");
            self.cursor = start;
        }
    }

    /// Readline `C-w`: delete the word (plus trailing spaces) before
    /// the cursor into the register.
    /// Move to the end of the word after the cursor — the desktop's
    /// ctrl/alt+right, and Emacs' `forward-word`.
    ///
    /// Not vim's `w`, which the same buffer also has: `w` lands on the
    /// start of the *next* word, and the two disagreeing is exactly the
    /// discrepancy between the editor and the prompts that this pair of
    /// chords was reported for.
    pub fn word_end_forward(&mut self) {
        self.cursor = self.word_end_offset();
    }

    /// Byte offset [`Self::word_end_forward`] moves to.
    fn word_end_offset(&self) -> usize {
        let rest = &self.buf[self.cursor..];
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let mut at = 0usize;
        for c in rest.chars() {
            if c == '\n' || word(c) {
                break;
            }
            at += c.len_utf8();
        }
        for c in rest[at..].chars() {
            if !word(c) {
                break;
            }
            at += c.len_utf8();
        }
        self.cursor + at
    }

    /// Delete from the cursor to the end of the word after it
    /// (`M-d` — readline's and Emacs' `kill-word`).
    ///
    /// Deliberately not vim's `w` motion, which lands on the *start of
    /// the next* word and so would swallow the space between them.
    /// Emacs stops at the end of the word it killed, which is what
    /// leaves you able to type a replacement in place.
    pub fn delete_word_forward(&mut self) {
        let rest: Vec<(usize, char)> = self.buf[self.cursor..].char_indices().collect();
        let mut at = 0;
        // Whitespace before the word is part of the reach, not the kill
        // target: point between two words kills the next one.
        while let Some(&(_, c)) = rest.get(at)
            && c.is_whitespace()
            && c != '\n'
        {
            at += 1;
        }
        let word = |c: char| c.is_alphanumeric() || c == '_';
        // A run of word characters, or — when point is on punctuation —
        // a run of that instead, so the chord is never a no-op with
        // something in front of it.
        let on_word = rest.get(at).is_some_and(|&(_, c)| word(c));
        while let Some(&(_, c)) = rest.get(at) {
            if c == '\n' || c.is_whitespace() || word(c) != on_word {
                break;
            }
            at += 1;
        }
        let end = rest
            .get(at)
            .map_or(self.buf.len(), |&(i, _)| self.cursor + i);
        if end > self.cursor {
            self.insert_guard();
            self.register = self.buf[self.cursor..end].to_owned();
            self.linewise = false;
            self.buf.replace_range(self.cursor..end, "");
        }
    }

    /// Delete from the start of the run before the cursor to the cursor
    /// (`C-w`, ctrl+backspace, Alt+Backspace).
    ///
    /// Spaces first, then one run of *the same kind* — letters, or
    /// punctuation. Trimming only the letters left a path's separators
    /// undeletable by this chord: from `~/dev/` there was no run of
    /// alphanumerics to take, so the press did nothing at all, which is
    /// "impossible to delete/kill a . or /".
    pub fn delete_word_back(&mut self) {
        let line_start = self.line_start(self.cursor);
        let s = &self.buf[line_start..self.cursor];
        let word_char = |c: char| c.is_alphanumeric() || c == '_';
        let trimmed = s.trim_end_matches(' ');
        let word = match trimmed.chars().next_back() {
            Some(c) if word_char(c) => trimmed.trim_end_matches(word_char),
            Some(_) => trimmed.trim_end_matches(|c: char| !word_char(c) && c != ' '),
            None => trimmed,
        };
        let start = line_start + word.len();
        if start < self.cursor {
            self.insert_guard();
            self.register = self.buf[start..self.cursor].to_owned();
            self.linewise = false;
            self.buf.replace_range(start..self.cursor, "");
            self.cursor = start;
        }
    }

    /// Readline `C-y`: insert the register at the cursor.
    pub fn yank_insert(&mut self) {
        let text = self.register.clone();
        self.insert_str(&text);
    }

    /// TAB in Insert mode: org-tempo expansion when the current line is
    /// a template trigger (`<s` → `#+BEGIN_SRC …`, like Emacs
    /// org-tempo), otherwise a two-space soft indent. After `<s` the
    /// cursor sits at the end of the `#+BEGIN_SRC ` line so the
    /// language can be typed immediately.
    pub fn tempo_expand_or_indent(&mut self) {
        let template: Option<(&str, &str)> = match self.current_line().trim() {
            "<s" => Some(("#+BEGIN_SRC ", "#+END_SRC")),
            "<e" => Some(("#+BEGIN_EXAMPLE", "#+END_EXAMPLE")),
            "<q" => Some(("#+BEGIN_QUOTE", "#+END_QUOTE")),
            "<c" => Some(("#+BEGIN_CENTER", "#+END_CENTER")),
            "<C" => Some(("#+BEGIN_COMMENT", "#+END_COMMENT")),
            "<v" => Some(("#+BEGIN_VERSE", "#+END_VERSE")),
            _ => None,
        };
        if let Some((begin, end)) = template {
            let start = self.line_start(self.cursor);
            let stop = self.line_end(self.cursor);
            self.buf
                .replace_range(start..stop, &format!("{begin}\n\n{end}"));
            self.cursor = start + begin.len();
        } else {
            self.insert_str("  ");
        }
    }
}

/// Org keywords always offered by the body-editor completion, beside
/// the dabbrev words mined from the vault.
const ORG_COMPLETION_KEYWORDS: &[&str] = &[
    "TODO",
    "DONE",
    "NEXT",
    "WAIT",
    "CANCELLED",
    "SCHEDULED:",
    "DEADLINE:",
    ":PROPERTIES:",
    ":END:",
    "#+BEGIN_SRC",
    "#+END_SRC",
    "#+BEGIN_QUOTE",
    "#+END_QUOTE",
    "#+TITLE:",
    "#+FILETAGS:",
];

/// Fuzzy-ranked completion candidates for `prefix`.
///
/// Org keywords and vault words (≥ 3 chars) that fuzzy-match by
/// subsequence ([`closure_query::fuzzy_score`]) and differ from the
/// prefix; ranked score-descending, keywords first on ties, then by
/// name — deduped after sorting.
#[must_use]
pub fn body_completions(prefix: &str, vault: &closure_store::Vault) -> Vec<String> {
    let sources: Vec<String> = vault.iter().map(|(_p, doc)| doc.source()).collect();
    body_completions_from(prefix, sources.iter().map(String::as_str))
}

/// The keywords a one-line prompt offers: the TODO axis and nothing
/// structural.
///
/// A title can start with `TODO`. It cannot usefully start with
/// `:PROPERTIES:` — that is offered in a body because a body is where
/// drawers live.
const PROMPT_COMPLETION_KEYWORDS: &[&str] = &["TODO", "DONE", "NEXT", "WAIT", "CANCELLED"];

/// Completion candidates for a one-line prompt: the same vault dabbrev
/// the body editor mines, over a shorter keyword list.
#[must_use]
pub fn prompt_completions(prefix: &str, vault: &closure_store::Vault) -> Vec<String> {
    let sources: Vec<String> = vault.iter().map(|(_p, doc)| doc.source()).collect();
    prompt_completions_from(prefix, sources.iter().map(String::as_str))
}

/// [`prompt_completions`] over raw sources, for a shell that holds text
/// rather than a vault.
#[must_use]
pub fn prompt_completions_from<'a>(
    prefix: &str,
    sources: impl Iterator<Item = &'a str>,
) -> Vec<String> {
    completions_from(prefix, PROMPT_COMPLETION_KEYWORDS, sources)
}

/// [`body_completions`] over raw document sources, for a shell that
/// holds text rather than a [`closure_store::Vault`] — the terminal
/// shell keeps the vault in its driver, not in its app state.
#[must_use]
pub fn body_completions_from<'a>(
    prefix: &str,
    sources: impl Iterator<Item = &'a str>,
) -> Vec<String> {
    completions_from(prefix, ORG_COMPLETION_KEYWORDS, sources)
}

/// The ranking both completion sets share; only the keyword list
/// differs between a body and a one-line prompt.
fn completions_from<'a>(
    prefix: &str,
    keywords: &[&str],
    sources: impl Iterator<Item = &'a str>,
) -> Vec<String> {
    if prefix.is_empty() {
        return Vec::new();
    }
    let mut entries: Vec<(u32, bool, String)> = Vec::new();
    for &k in keywords {
        if k != prefix
            && let Some(score) = closure_query::fuzzy_score(prefix, k)
        {
            entries.push((score, false, k.to_owned()));
        }
    }
    for source in sources {
        for word in source.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            if word.len() >= 3
                && word != prefix
                && let Some(score) = closure_query::fuzzy_score(prefix, word)
            {
                entries.push((score, true, word.to_owned()));
            }
        }
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut result: Vec<String> = Vec::new();
    for (_score, _is_word, name) in entries {
        if seen.insert(name.clone()) {
            result.push(name);
        }
    }
    result
}

/// A live completion cycle in the body editor: the prefix start, the
/// candidate list, and the currently-applied index.
#[derive(Debug, Clone)]
struct CompletionSession {
    start: usize,
    items: Vec<String>,
    /// Applied candidate index; `None` while the popup only shows
    /// candidates (auto-popup) and nothing replaced the prefix yet.
    ix: Option<usize>,
}

/// Modal command-surface launcher (the "modal GUI" experiment).
///
/// Unlike [`App`] (a Notion-style type-to-filter launcher), `ModalApp`
/// treats Browse as a command surface: every key resolves against
/// [`closure_input::mode_keymap`] for the active [`InputMode`], so the
/// five editing modes (vim `j`/`k`, `g g`; emacs `C-x C-c`; …) drive a
/// GUI exactly as in the TUI. Typing happens only in the Search/Capture
/// overlays. Pure + headless-testable; mutations via [`Shell`] (I8).
// Four independent facts about the session — quitting, whether a row is
// selected, whether an answer is in flight, whether to re-render it.
// They are orthogonal and each is genuinely two-valued; an enum over
// their sixteen combinations would describe the same thing worse.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct ModalApp {
    mode: InputMode,
    /// What `config.org` said about the keymap, in file order.
    key_overrides: Vec<(String, String)>,
    /// [`Self::mode`]'s keymap with [`Self::key_overrides`] applied —
    /// the one every lookup in the app reads.
    ///
    /// Resolved once per change rather than per lookup: the palette and
    /// the which-key panel both walk the whole map, and both are on the
    /// render path.
    keys: Vec<(String, String)>,
    surface: ModalSurface,
    selected: usize,
    /// The filter every list surface types into — search, body search,
    /// and the buffer/file/tag/refile pickers. One field rather than
    /// one per surface, so the readline chords cannot be right in some
    /// of them and missing in the rest.
    query: LineInput,
    /// Folded body ranges as `(first visible line, last hidden line)`.
    body_folds: Vec<(usize, usize)>,
    /// Whether the headline tree is pinned beside the full-window
    /// editor. The shells paint it; the flag lives here so every shell
    /// answers `toggle-tree` the same way (I7).
    tree_open: bool,
    /// Capture lines typed before, newest last — the arrows and the
    /// chords walk it, including ones that were cancelled.
    capture_history: Vec<String>,
    /// Where in [`Self::capture_history`] the field currently is;
    /// `None` is the fresh line at the end.
    capture_hist_at: Option<usize>,
    /// The capture overlay's one-line field (text + cursor).
    capture_buf: LineInput,
    /// Which step of [`Self::capture_crumbs`] this capture files into,
    /// when the user has picked one; `None` is the selection itself.
    ///
    /// A pick belongs to the capture being typed, not to the app, so
    /// it is cleared whenever the overlay opens.
    capture_crumb_pick: Option<usize>,
    /// The headline the open capture's path was drawn from.
    ///
    /// Pinned when the overlay opens, because picking a crumb moves
    /// the outline selection onto it — and a path re-derived from the
    /// selection would truncate itself at every click, taking the way
    /// back down with it.
    capture_path_root: Option<String>,
    body: BodyEditor,
    completion: Option<CompletionSession>,
    /// The same cycle over whichever one-line prompt is open. Separate
    /// from [`Self::completion`] because both can be alive at once —
    /// the capture overlay opens over a buffer that may itself be
    /// mid-completion, and neither should inherit the other's
    /// candidates.
    prompt_completion: Option<CompletionSession>,
    edit_target: Option<String>,
    /// Path of the file open in the editor view, when one is.
    file_target: Option<std::path::PathBuf>,
    /// Which shape of the shell we are in ([`ViewMode`]).
    view: ViewMode,
    /// The body as it was when the editor opened it, so
    /// [`Self::body_dirty`] can answer by comparison rather than with
    /// a "was touched" bit — a buffer put back the way it was found
    /// has nothing to save.
    body_baseline: String,
    /// Headline id whose backlinks the Backlinks surface is showing.
    link_target: Option<String>,
    /// Target id + single-line buffer for the TagsEdit/PropertyEdit
    /// surfaces (tags: space-separated; property: `key value`).
    field_target: Option<String>,
    field_buf: LineInput,
    /// The link type picked in `insert-link`, while the destination
    /// and the description are still being typed.
    link_kind: Option<String>,
    /// The destination of the pending link, once it has been typed —
    /// which is also what says the description is the step we are on.
    link_dest: Option<String>,
    /// The buffer a prompt was opened over, when it was opened over
    /// one — where it goes back to, and what stays painted behind it
    /// while it is up.
    ///
    /// `None` means the outline: a prompt reached from there has no
    /// buffer to claim. One field for both prompts that need it, since
    /// "which buffer am I on top of" is one fact.
    prompt_from: Option<ModalSurface>,
    /// The last `gcc` the buffer reported having been asked for.
    comment_seen: u64,
    /// The wall-clock minute each job last fired in, as
    /// `(day, hour, minute)`, keyed by the command it runs.
    ///
    /// A scheduler needs memory, not only a predicate: the frame timer
    /// asks many times a second and a minute is sixty seconds of
    /// asking, so "does this job match now" fires it thousands of
    /// times. The day is in the key so tomorrow's 09:00 is a different
    /// minute from today's.
    cron_fired: std::collections::HashMap<String, (u8, u8, u8)>,
    /// The left rail collapsed to its icons, once something has said.
    ///
    /// `Option`, like [`Self::outline_width`], so that "never toggled"
    /// and "toggled back on" are different things: only the second is
    /// worth writing into `config.org`.
    rail_docked: Option<bool>,
    /// Which settings row the assistant screen has selected.
    settings_cursor: usize,
    /// Which peer the next tick dials, so one tick dials one peer.
    dial_next: usize,
    /// How wide the outline pane is, once something has said.
    outline_width: Option<u32>,
    /// Where the read-only panes (Jobs, Journal, Agenda…) are looking.
    ///
    /// Their own, not the outline's. They used `selected`, so walking a
    /// pane moved the outline underneath it and leaving reset the
    /// outline to the top — "selection at top of outline headings list
    /// when switching from any element to the Jobs panel and back".
    pane_cursor: usize,
    /// The setting whose value prompt is open, if one is.
    editing_setting: Option<&'static str>,
    /// Which of the four new-headline chords opened the title prompt.
    new_heading: NewHeading,
    /// Whether the which-key panel is pinned open. A pending chord
    /// shows it too, but that one closes itself the moment the chord
    /// resolves; this is the one a person asked for.
    which_key_open: bool,
    /// Every status line this session has shown, newest first.
    ///
    /// The bottom line was one `String`, overwritten by whatever
    /// happened next — "saved", "3 file(s) changed on disk", "peer
    /// added" all landed in the same slot and the previous one was gone
    /// before you had finished reading it. Emacs keeps a `*Messages*`
    /// buffer for exactly this reason.
    messages: Vec<String>,
    /// Whether every keypress is being timed (`toggle-trace`).
    ///
    /// Off by default and free when off: a measurement that costs
    /// something to collect changes the thing it is measuring.
    tracing: bool,
    /// The file the last vault-changing command touched.
    ///
    /// What `u` and `C-r` speak to. Undo follows the edit rather
    /// than the cursor, because the commands most worth undoing are
    /// the ones that move the cursor off what they changed.
    last_edited_file: Option<std::path::PathBuf>,
    /// The vault's git state, memoised against the revision it was
    /// read at.
    ///
    /// Shelling out to git costs tens of milliseconds on a large
    /// repository. Asked once per change rather than once per frame,
    /// for the reason the detail is memoised: the same mistake in a
    /// new place would be the level-1 microfreeze again.
    git_memo: std::cell::RefCell<Option<GitMemo>>,
    /// How many times git has actually been run. What a test asserts
    /// on instead of timing it.
    git_reads: std::cell::Cell<u64>,
    /// Per-line git marks for the open buffer, memoised against the
    /// revision and the path they were read for.
    fringe_memo: std::cell::RefCell<Option<FringeMemo>>,
    /// Where TAB has walked to in the `:` line's candidates.
    ex_cycle: usize,
    /// What was typed when TAB was first pressed. Cycling walks the
    /// candidates of *this*, not of the line it keeps rewriting.
    ex_stem: Option<String>,
    /// The notification log. It lived in the gpui window, which is
    /// why it had no command and no chord — there was nothing for the
    /// keymap to point at. Here it has both, and the terminal shell
    /// reads the same log rather than growing a second one.
    notifications: Feedback,
    /// The kill every one-line prompt shares, so `C-k` in one field and
    /// `C-y` in another mean what they do in a terminal.
    ///
    /// Deliberately *not* the vault's kill ring: that one holds org
    /// subtrees and is what `p` splices back into the outline, so a
    /// fragment of a title on it would paste prose where a headline
    /// belongs.
    /// Commands run *from the palette*, most recent first, deduped and
    /// capped. Chords are deliberately not in here: `j` and `k` are
    /// pressed hundreds of times a session and are never what you open
    /// the palette to find.
    palette_history: Vec<String>,
    /// What each prompt has been given before, newest first, keyed by
    /// the kind of prompt rather than by the surface — the four
    /// new-headline chords share one prompt and so share one history.
    ///
    /// Recorded on the way *out* whichever door was used. A history
    /// that only kept what you accepted would forget exactly the case
    /// the report is about: three sentences into a capture, `Esc`.
    prompt_history: std::collections::BTreeMap<&'static str, Vec<String>>,
    /// The argument the running command was given, if any.
    ///
    /// "We may have to reinvent the command/function system, because
    /// currently there are [no] arguments/parameter." This is the
    /// smallest honest version of that: the `:` line splits a name
    /// from the rest, and a command that wants an argument reads it.
    command_arg: Option<String>,
    /// Where the full-size view was opened from.
    image_return: Option<ModalSurface>,
    /// The picture the full-size view is showing.
    ///
    /// An inline preview is deliberately small — it sits under the
    /// line that links it — and a picture worth opening is worth the
    /// window.
    image_view: Option<std::path::PathBuf>,
    /// A directory named on the `:` line, when one was.
    vault_switch_path: Option<String>,
    /// Bumped when something asks to change vaults; the window
    /// watches it and raises the directory dialog.
    vault_switch_asked: u64,
    /// Headlines marked for a bulk action, by id.
    ///
    /// dired's marks. Held by id rather than by row index because the
    /// rows are derived and renumber themselves whenever the vault
    /// changes — a mark that meant "row 3" would point at a different
    /// headline the moment one above it was deleted, which is the one
    /// thing a bulk delete must never do.
    marks: std::collections::BTreeSet<String>,
    /// Which directory `find-file` is looking at, vault-relative.
    ///
    /// Relative rather than absolute so that it cannot express a path
    /// outside the vault: a picker that walks above the root is a file
    /// manager with somebody's home directory in reach.
    find_dir: std::path::PathBuf,
    /// The buffer a pane was opened over, to come back to.
    ///
    /// "This is like the =n=th time. Do I have to experience this for
    /// every new command?" — no: every command that opens a pane over
    /// an editor records the way back here, so a command written
    /// tomorrow is right without anybody having to remember.
    pane_return: Option<ModalSurface>,
    /// Where a history walk has got to, and the line it interrupted.
    ///
    /// The draft is held so that walking past the newest entry gives it
    /// back — looking through history must not cost you what you had
    /// already typed.
    history_walk: Option<(usize, String)>,
    /// Bumped whenever [`Self::palette_history`] changes, so the
    /// palette memo notices — its key is the query and the mode, and
    /// neither of those moves when the history does.
    history_gen: u64,
    /// Cursor into [`Self::palette_entries`] while the Palette is open.
    palette_cursor: usize,
    pending: Vec<String>,
    /// The rest of the command line being run — set by
    /// [`split_command`] at the one door every command comes through,
    /// and taken by the arm that wants it. A field rather than a
    /// parameter because the dispatch is one flat match of ~200 arms
    /// and two of them care.
    command_args: String,
    status: String,
    quit: bool,
    /// Explicit wheel-scroll viewport offset; None = follow selection.
    scroll_override: Option<usize>,
    /// How many body lines the shell last said it can paint. The
    /// kernel decides *where* the viewport sits and the shell knows how
    /// big it is, so the shell reports it ([`Self::set_body_viewport`])
    /// and the framing chords read it back.
    body_viewport: usize,
    /// How many outline rows the shell last said it can paint — what
    /// `C-d` / `C-u` take half of. Same split as the body's: the core
    /// knows where the cursor is, the window knows how tall it is.
    outline_viewport: usize,
    /// The first visible line as last resolved by [`Self::body_scroll_follow`],
    /// which is what "scroll by the minimum" is measured from.
    body_anchor: Option<usize>,
    /// `C-l`'s place in the centre → top → bottom cycle, with the
    /// framing it produced — a press that finds the viewport somewhere
    /// else is a first press, not the next one.
    recenter: Option<(u8, usize, usize)>,
    /// A body-editor prefix key waiting for the rest of its chord.
    pending_body: Option<BodyPrefix>,
    /// Output of the last `:!` shell escape, kept so a shell can show
    /// more than the one line a status bar holds.
    shell_out: Option<String>,
    /// Where the cursor was left in each body, by block id, so opening
    /// a note again resumes rather than restarting at byte zero.
    body_cursors: std::collections::HashMap<String, usize>,
    /// Whether long body lines wrap instead of scrolling sideways.
    ///
    /// Kernel state rather than a field in the window: it is a view
    /// toggle like the tree and the images, the terminal shell wants
    /// the same switch, and read from `config.org` into the window it
    /// had no command and so no chord — there was no way to change your
    /// mind about the paragraph in front of you.
    wrap: bool,
    /// Whether image links are painted as pictures (org's
    /// `org-toggle-inline-images`). Shown to begin with: a note with a
    /// screenshot in it is a note you want to look at.
    images_shown: bool,
    /// The colour diagrams are drawn in: the shell's foreground,
    /// reported the way the viewport is. Defaults to the dark
    /// theme's, so a shell that never says still gets readable ink.
    ink: u32,
    /// The headline this session last opened a body on — what
    /// [`Self::save_last_place`] remembers in preference to whatever
    /// the cursor happens to be resting on.
    last_edited: Option<String>,
    /// Modified buffers for headlines that are not the one on screen,
    /// by block id, with the text they were loaded from.
    ///
    /// The buffer used to be a single slot: opening another note
    /// overwrote it and the paragraph in the old one was gone. Clicking
    /// a row in the outline beside the buffer is the most ordinary
    /// thing there is to do while editing, so it cannot be the gesture
    /// that loses text. Nothing here is on disk — the vault is written
    /// by `:w`, `C-Enter`, or the window closing.
    body_stash: std::collections::HashMap<String, (String, String)>,
    /// The outline row the search overlay was opened from, so Esc can
    /// put the cursor back rather than leaving it on a result index.
    search_return: Option<usize>,
    /// Buffer text scale, in [`ZOOM_STEP`] powers.
    zoom_steps: i8,
    /// Whether a row is *actually* selected, as opposed to the cursor
    /// merely sitting somewhere. Escape in the outline clears it, which
    /// is how a capture is told "file this at the top level, not under
    /// whatever I happened to be looking at"; any motion selects again.
    selection_active: bool,
    /// Body-editor wheel viewport `(start, cursor_line_when_set)`; the
    /// override self-clears when the cursor line changes (G5).
    body_scroll: Option<(usize, usize)>,
    /// Cursor row inside the `UndoHistory` pane (Q2-U3).
    hist_cursor: usize,
    /// The Notion "/" block menu's query while it is open, and its
    /// cursor row. `None` when closed.
    slash: Option<(String, usize)>,
    /// The `:` command line's buffer while it is open.
    ex_buf: LineInput,
    /// Surface the `:` line was opened from, so Escape returns there.
    ex_return: Option<ModalSurface>,
    /// The live org-edit-special session: where it came from and the
    /// language it is editing.
    special: Option<(SpecialOrigin, String)>,
    /// Surface the edit-special session was opened from.
    special_return: Option<ModalSurface>,
    /// Output of the last source block run from the Blocks surface.
    /// Cleared whenever the cursor moves or the pane closes, because
    /// output shown beside a block that did not produce it is a lie.
    block_out: Option<String>,
    /// Collaboration state, created on first use so a shell that never
    /// pairs never generates a keypair.
    sync: Option<SyncApp>,
    /// Where pairing binds and what it advertises, from `config.org`.
    /// Held here rather than in [`SyncApp`] because the shell reads the
    /// config long before anything pairs, and creating the state early
    /// would generate a keypair for a session that never asked to.
    sync_bind: std::net::SocketAddr,
    sync_advertise: Option<std::net::IpAddr>,
    /// The ticket-entry field on the Sync surface.
    sync_buf: LineInput,
    /// The assistant transcript, oldest first.
    chat: Vec<ChatTurn>,
    /// The question field on the assistant surface.
    chat_buf: LineInput,
    /// Whether a question is in flight, so the pane can say so rather
    /// than looking asleep.
    chat_busy: bool,
    /// The sniffer surface's captured flows and rules (X3).
    sniffer: SnifferApp,
    /// Whether the sniffer pane is showing the raw record behind the
    /// selected flow ("debug"), rather than only what it made of it.
    sniffer_debug: bool,
    /// The conflict surface's pending CRDT decisions.
    conflicts: ConflictApp,
    /// Whether the LLM may read the *rendered* view (V3b).
    ///
    /// Off until explicitly granted, and revocable at any time —
    /// `toggle-llm-render` is the live toggle, bound in every keymap so
    /// it shows up in which-key. Mirrors
    /// `closure_llm::LlmPermissions::toggle_render`; a session created
    /// from this app takes its render grant from here.
    llm_render: bool,
    /// Memoised outline rows — see [`ModalApp::rows`]. Interior
    /// mutability because `rows` is a `&self` query on the render
    /// path; the memo is exact, guarded by the vault revision and the
    /// active filter, so it can never serve a stale list.
    row_memo: std::cell::RefCell<Option<RowMemo>>,
    /// How many full vault walks the memo has paid for. Observability
    /// for the render budget (`rows_recomputes`), asserted in tests.
    row_recomputes: std::cell::Cell<u64>,
    /// Memoised detail for the selected row — see [`ModalApp::detail`].
    detail_memo: std::cell::RefCell<Option<DetailMemo>>,
    /// Derivations paid for; the render budget's second number.
    detail_recomputes: std::cell::Cell<u64>,
    /// Memoised palette entries — see [`ModalApp::palette_entries`].
    palette_memo: std::cell::RefCell<Option<PaletteMemo>>,
    /// Derivations paid for; the render budget's third number.
    palette_recomputes: std::cell::Cell<u64>,
    /// Every buffer this session has open, in the order they were
    /// opened (what `buffer-next` walks); the MRU order the picker
    /// shows is [`OpenBuffer::seq`].
    buffers: Vec<OpenBuffer>,
    /// The visit counter handed to the buffer opened next.
    buf_seq: u64,
    /// The jumplist: every place a non-local move left, oldest first.
    jumps: Vec<JumpPoint>,
    /// Where in [`Self::jumps`] we are. Equal to the length means "at
    /// the present" — nothing ahead to go forward to.
    jump_at: usize,
    /// Files recent sessions opened, most recent first. Persisted to
    /// `config.org` with the rest of the durable view state.
    recent_files: Vec<std::path::PathBuf>,
    /// Today, as the shell last told us (`YYYY-MM-DD`). The core reads
    /// no clock: a date picker whose "today" came from `SystemTime`
    /// would be a different picker in every test (Q3-V4).
    today: String,
    /// The live date-picker session, when one is open.
    date_pick: Option<DatePickSession>,
    /// The subtree waiting for a refile target (Q3-V1).
    refile_source: Option<String>,
    /// Now, as the shell last said (`YYYY-MM-DD HH:MM`) — what a
    /// clock entry is stamped with (Q3-V3).
    now: String,
    /// What the palette was opened over, when that was a buffer.
    ///
    /// The palette floats rather than replacing a pane, and closing it
    /// has to give back exactly what it covered — a buffer with its
    /// text and cursor, not the outline.
    palette_return: Option<ModalSurface>,
    /// The headline the tag picker is editing (Q3-V6).
    tag_target: Option<String>,
    /// The tags ticked so far, before they are written.
    tag_draft: Vec<String>,
    /// How many times this session has started over
    /// ([`ModalApp::reload_session`]). A window holds parts of a launch
    /// the kernel does not — the theme, the shape it opens in — so it
    /// needs to notice a reload; watching a counter covers whatever ran
    /// it, chord or palette or `:` line, the way the clipboard mirror
    /// watches the kill ring rather than a list of commands.
    reloads: u64,
    /// Memoised source-block list — see [`ModalApp::block_rows`].
    block_memo: std::cell::RefCell<Option<(u64, std::sync::Arc<Vec<BlockRow>>)>>,
    /// Derivations paid for; the render budget's fourth number.
    block_recomputes: std::cell::Cell<u64>,
}

/// What an open buffer is: a headline's body, or a whole file.
///
/// Both are things the editor surfaces already open ([`ModalSurface::EditBody`]
/// and [`ModalSurface::EditFile`]); until Q1 neither was *listed*, so
/// opening a second one made the first unreachable except by finding
/// its headline again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BufferRef {
    /// A headline body, by stable block id (I2).
    Body(String),
    /// A whole file, by vault-relative path.
    File(std::path::PathBuf),
}

/// One row of the open-buffer list ([`ModalApp::buffer_rows`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferRow {
    /// What to show: the headline title, or the file's name.
    pub name: String,
    /// Which buffer this row is, for opening it.
    pub target: BufferRef,
    /// Whether it holds text the vault does not.
    pub dirty: bool,
    /// Whether it is the buffer on screen.
    pub current: bool,
    /// Whether it survives the picker's live filter.
    pub matches_filter: bool,
}

/// One tag in the picker ([`ModalApp::tag_rows`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagRow {
    /// The tag, without its colons.
    pub name: String,
    /// Whether the headline will carry it when the picker commits.
    pub on: bool,
    /// Whether it survives the live filter.
    pub matches_filter: bool,
}

/// Every kind of link `insert-link` can make.
///
/// Org's own `C-c C-l` menu is longer, but most of what pads it out is
/// Emacs' furniture: `elisp:`, `help:`, `var:`, `face:` and `doom:`
/// name things that exist inside a running Emacs and nowhere else.
/// What is left is what a link can mean in a vault of plain files —
/// another note, a file, something on the web, someone's mailbox — and
/// offering only those keeps the list short enough to read at a glance.
///
/// Both shells offer the same list, from here, because a chord that
/// means different things in the terminal and the window is worse than
/// one that is missing from one of them.
pub const LINK_TYPES: &[&str] = &[
    "id:",
    "file:",
    "https:",
    "http:",
    "mailto:",
    "attachment:",
    "ftp:",
    "news:",
];

/// One org link, written the way org writes it.
///
/// `[[dest][]]` is a link with an *empty* description, and org renders
/// that as nothing at all — a link you cannot see and cannot click. So
/// no description means the bare one-part form, not the two-part one
/// with a hole in it.
#[must_use]
pub fn org_link(kind: &str, dest: &str, description: &str) -> String {
    let description = description.trim();
    if description.is_empty() {
        format!("[[{kind}{dest}]]")
    } else {
        format!("[[{kind}{dest}][{description}]]")
    }
}

/// The invariants the whole system is built to hold.
///
/// One list. `closure spec` prints it and the manual carries it, and
/// before this they were the same ten sentences typed twice — which is
/// exactly the drift the manual exists to prevent.
pub const INVARIANTS: &[&str] = &[
    "I1  byte-exact roundtrip on the golden corpus",
    "I2  stable BlockId (ULID) survives parse/print/CRDT merges",
    "I3  every mutation undoable via Edit + branching UndoTree",
    "I4  every command carries a keybinding (whichkey reads registry)",
    "I5  no panics in kernel crates (forbid unsafe, deny unwrap/expect, fuzz)",
    "I6  determinism for parse/print/queries",
    "I7  shells address content by id, never by byte offset; spans pub(crate) firewall",
    "I8  command-registry is the only side-effect surface",
    "I9  config validation at load, not at use (typed schema)",
    "I10 deterministic / hermetic / reproducible builds (nix flake check)",
];

/// closure's manual, generated from the running program.
///
/// "Emacs like manual directly within closure (self documented) …
/// Generate on the fly (or JIT) via LLM from the source repository?"
///
/// The doubts in that question are the argument against it: an LLM
/// reading the source is a second thing that can be wrong, it costs
/// money, and it is not there on a train. What makes Emacs' manual
/// trustworthy is not the prose — it is that `C-h k` answers from the
/// running program, so the documentation cannot drift from the binary.
///
/// So this is built from the same registry the palette and which-key
/// read, and from the keymap of the mode in force. It cannot go stale,
/// it costs nothing, and it works offline, which is the point of a
/// local-first tool.
///
/// `tutorial.org` is a different document and stays: a tutorial teaches
/// one path through, a manual is complete.
#[must_use]
pub fn manual_org(mode: InputMode) -> String {
    use std::fmt::Write as _;
    let keys = closure_input::mode_keymap(mode);
    let chords_for = |command: &str| -> Vec<&str> {
        keys.iter()
            .filter(|(_, c)| *c == command)
            .map(|(chord, _)| *chord)
            .collect()
    };
    let mut out = String::new();
    out.push_str("#+TITLE: closure manual\n");
    let _ = writeln!(out, "#+SUBTITLE: {mode:?} keys");
    out.push_str(
        // One paragraph per line. A hard wrap here is a hard wrap in
        // the pane too, where the width is not ours to guess — the
        // shell wraps what it is given, and given pre-broken text it
        // wraps twice.
        "\nGenerated from the command registry and the keymap in force, every time \
         it is asked for. Not hand-written and not worth hand-editing: an edit here \
         is gone the next time you open it, and whatever you were correcting is in \
         the code that generates it.\n\n\
         This is the reference. =tutorial.org= is the other half — it teaches one \
         path through; this lists everything.\n",
    );

    out.push_str(
        "\n* Invariants\n\nWhat the system is built to hold. =closure spec= prints \
         the same list.\n\n",
    );
    for line in INVARIANTS {
        let _ = writeln!(out, "- {line}");
    }

    out.push_str(
        "\n* Keys\n\nEvery command, by what it is for. A command with no chord in \
         this mode is reached from the palette (=M-x=).\n",
    );
    for section in PALETTE_SECTIONS {
        let _ = writeln!(out, "\n** {section}\n");
        for (label, command, sect, desc) in PALETTE_COMMANDS {
            if sect != section {
                continue;
            }
            let chords = chords_for(command);
            let keys = if chords.is_empty() {
                "M-x".to_owned()
            } else {
                chords
                    .iter()
                    .map(|c| format!("={c}="))
                    .collect::<Vec<_>>()
                    .join(" or ")
            };
            // Both names. The label is what the palette shows; the
            // canonical one is what `M-x`, `where-is` and an LLM
            // calling a command all use, and they are not always the
            // same word.
            if label == command {
                let _ = writeln!(out, "- ={label}= — {desc}. {keys}");
            } else {
                let _ = writeln!(out, "- ={label}= (={command}=) — {desc}. {keys}");
            }
        }
    }

    out.push_str(
        "\n* Asking closure itself\n\n\
         - =describe-key= takes one chord and says what it runs.\n\
         - =M-x= lists every command by name, with its keys.\n\
         - which-key opens mid-chord and shows what the next key would do.\n\
         - =closure spec= prints the invariants above.\n\
         - =closure where-is <command>= prints its keys.\n",
    );
    out
}

/// What a key does: Emacs' `C-h k`, answered from the running program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyDescription {
    /// The chord asked about, as the keymap spells it.
    pub chord: String,
    /// The command it runs.
    pub command: String,
    /// What that command does, in the registry's own words.
    pub description: String,
    /// Which part of the palette it lives in.
    pub section: String,
}

/// What a command is and how to reach it: `C-h f` and `where-is` in one
/// answer, because those are two halves of the same question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandDescription {
    /// Its canonical name — what `M-x` and an LLM both call it.
    pub command: String,
    /// What it does.
    pub description: String,
    /// Which part of the palette it lives in.
    pub section: String,
    /// Every chord that reaches it in the mode in force. Empty is an
    /// answer: some commands are palette-only.
    pub chords: Vec<String>,
}

/// One thing the destination field can be completed to.
///
/// The two halves are genuinely different facts and neither can be
/// derived from the other: an `id:` link carries a ULID, and the only
/// way to find the right one is by the title it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkCompletion {
    /// What goes into the link after the type.
    pub value: String,
    /// What to show while choosing it.
    pub label: String,
}

/// Every scheduled job declared in the vault's `#+BEGIN_SRC cron`
/// blocks.
///
/// A free function because both shells need it and both had their own
/// copy — each reading the *whole document* as one cron listing, which
/// an org file never is: it begins `* Something`, two fields is not a
/// spec, the parse failed, and the `.ok()` on the end dropped every job
/// in the file. The Jobs pane was empty in any vault with a headline in
/// it, which is all of them.
#[must_use]
pub fn job_rows(vault: &closure_store::Vault) -> Vec<JobRow> {
    // The `#+BEGIN_SRC cron` blocks, not the whole file. This read
    // every document as one cron listing, and an org document
    // begins `* Something` — two fields, not a spec — so the parse
    // failed and this `.ok()` dropped every job in the file. The
    // pane was empty in any vault with a headline in it.
    vault
        .iter()
        .flat_map(|(_, doc)| {
            segment_body(&doc.source())
                .into_iter()
                .filter_map(|seg| match seg {
                    // Both spellings: the vaults in this repo write
                    // `closure-cron`, and `cron` is what anyone typing
                    // it fresh would reach for.
                    BodySegment::Code { lang, text }
                        if lang.eq_ignore_ascii_case("cron")
                            || lang.eq_ignore_ascii_case("closure-cron") =>
                    {
                        closure_cron::parse_jobs(&text).ok()
                    }
                    _ => None,
                })
                .flatten()
                .collect::<Vec<_>>()
        })
        .map(|job| JobRow {
            schedule: closure_cron::expression(&job.spec),
            when: closure_cron::describe(&job.spec),
            command: job.command,
        })
        .collect()
}

/// One scheduled job, as the Jobs pane shows it.
///
/// The pane paired `format!("{:?}", job.spec)` with the command in a
/// bare tuple: the parser's idea of the schedule rather than the
/// user's, no word about when it next runs, and nothing in the type
/// saying which string was which.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRow {
    /// The cron expression, spelled as it was written.
    pub schedule: String,
    /// The same thing in words, where there are words for it.
    pub when: String,
    /// The registry command it runs.
    pub command: String,
}

/// One candidate refile target ([`ModalApp::refile_rows`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefileRow {
    /// Headline title, as shown.
    pub title: String,
    /// Stable block id — what the move addresses.
    pub id: String,
    /// Vault-relative file, so two notes with one title are telling
    /// apart.
    pub path: String,
    /// Outline level, for the indent in the list.
    pub level: u8,
    /// Whether it survives the picker's live filter.
    pub matches_filter: bool,
}

/// One row of the file picker ([`ModalApp::file_rows`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRow {
    /// Vault-relative path, as shown.
    pub name: String,
    /// The path to open.
    pub path: std::path::PathBuf,
    /// Whether a recent session was in it.
    pub recent: bool,
    /// Whether it survives the picker's live filter.
    pub matches_filter: bool,
}

/// A place the jumplist can return to.
///
/// Deliberately not a byte offset into a file: a jump point names the
/// *buffer* and the outline row, so a vault edited elsewhere between
/// two jumps still lands somewhere true (I7 — no shell addresses
/// content by offset). The cursor is a hint, clamped on arrival.
#[derive(Debug, Clone, PartialEq, Eq)]
struct JumpPoint {
    /// The buffer that was open, if one was.
    buffer: Option<BufferRef>,
    /// The outline row's block id, if a row was selected.
    row: Option<String>,
    /// Where the cursor was inside the buffer.
    cursor: usize,
}

/// An open buffer with the visit that put it at the top of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenBuffer {
    /// What it is.
    target: BufferRef,
    /// Monotonic visit counter — the MRU order without a clock.
    seq: u64,
}

/// A filled detail memo with the key it is valid under.
#[derive(Debug, Clone)]
struct DetailMemo {
    /// Vault revision the detail was read at.
    revision: u64,
    /// Block id it describes (`None` when there is no selection).
    id: Option<String>,
    /// The derived detail, shared so a repaint costs a refcount bump
    /// rather than cloning a whole headline body.
    detail: Option<std::sync::Arc<Detail>>,
}

/// A filled palette memo with the key it is valid under.
#[derive(Debug, Clone)]
struct PaletteMemo {
    /// The filter the entries were scored against.
    query: String,
    /// The mode whose chords they carry.
    mode: InputMode,
    /// The palette history generation they were built against.
    history_gen: u64,
    /// The derived entries.
    entries: std::sync::Arc<Vec<PaletteEntry>>,
}

/// One listed source block: `(file, language, first line)`.
/// One source block in the vault, as the Blocks picker shows it.
///
/// A named struct rather than `(String, String, String)`: three
/// positional strings say nothing about which is the file and which
/// the language, every caller destructures by position, and getting
/// two of them the wrong way round compiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRow {
    /// Vault-relative file the block is in.
    pub file: String,
    /// Its language, canonicalised — `sh` is listed as `shell`.
    pub lang: String,
    /// Which line of the file it starts on, as shown.
    pub line: String,
}

/// One headline, as the headline picker and the jump list show it.
///
/// Was `(String, String)`, which is a title and an id in an order you
/// have to go and look up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadlineRow {
    /// What it is called.
    pub title: String,
    /// Its block id — what a jump addresses.
    pub id: String,
}

/// One row of a floating picker, whatever the picker is over.
///
/// Three columns because that is what a picker row is everywhere it is
/// done well: the thing, a word about the thing, and how to reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickRow {
    /// What this row is — the command, the title, the file.
    pub label: String,
    /// A word about it: the description, the body preview, the path.
    pub detail: String,
    /// The right-hand column: a chord, a language, a marker.
    pub trailing: String,
    /// Byte ranges of [`Self::label`] the filter matched, for the
    /// shell to paint — vertico's highlighting, which is what tells you
    /// why a row is in a list of near-identical ones.
    pub matches: Vec<(usize, usize)>,
    /// Whether the cursor is on it.
    pub current: bool,
}

/// A floating picker: a filter and a list of things to pick.
///
/// "M-x list commands don't seem to use the 'new' command palette" —
/// `buffer-list`, `block-list`, `undo-history`, `headline-list` each
/// opened a pane with a bare `j`/`k` list in it, so there were five
/// presentations of one idea and only one of them was the good one.
/// The shells paint this instead, and the surface only has to say what
/// its rows are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerView {
    /// What this picker is picking, shown beside the filter.
    pub title: String,
    /// What Enter will do, shown along the bottom.
    pub hint: String,
    /// The rows surviving the filter.
    pub rows: Vec<PickRow>,
    /// Which of them the cursor is on.
    pub cursor: usize,
}

/// A filled outline-row memo together with the key it is valid under.
#[derive(Debug, Clone)]
struct RowMemo {
    /// Vault revision the rows were derived from.
    revision: u64,
    /// Active fuzzy filter (empty outside the Search surface).
    filter: String,
    /// The derived rows, shared so handing them out costs a refcount
    /// bump rather than a deep clone of every headline string.
    rows: std::sync::Arc<Vec<Row>>,
}

impl ModalApp {
    /// New modal app in the given editing mode, Browse surface.
    #[must_use]
    // One struct literal naming every field once. Splitting it to
    // satisfy a line count would put half the app's initial state in a
    // helper and hide which fields exist.
    #[allow(clippy::too_many_lines)]
    pub fn new(mode: InputMode) -> Self {
        Self {
            key_overrides: Vec::new(),
            keys: closure_input::keymap_with(mode, &[]),
            slash: None,
            ex_buf: LineInput::default(),
            ex_return: None,
            special: None,
            special_return: None,
            block_out: None,
            sync: None,
            sync_bind: DEFAULT_SYNC_BIND,
            sync_advertise: None,
            sync_buf: LineInput::default(),
            chat: Vec::new(),
            chat_buf: LineInput::default(),
            chat_busy: false,
            sniffer: SnifferApp::new(),
            sniffer_debug: false,
            conflicts: ConflictApp::new(Vec::new(), mode),
            llm_render: false,
            mode,
            surface: ModalSurface::Browse,
            selected: 0,
            query: LineInput::default(),
            tree_open: false,
            body_folds: Vec::new(),
            capture_history: Vec::new(),
            capture_hist_at: None,
            capture_buf: LineInput::default(),
            capture_crumb_pick: None,
            capture_path_root: None,
            body: BodyEditor::new(),
            body_baseline: String::new(),
            completion: None,
            prompt_completion: None,
            edit_target: None,
            file_target: None,
            // Every window opens on the outline: it is where the rail
            // and every affordance live, and a shell that opened
            // straight into a raw file buffer would hide the app from
            // anyone who had not asked for that. `set_view` is how a
            // vault config (`view = editor`) asks for it, and
            // `toggle-view` is how a keyboard does.
            view: ViewMode::Clickable,
            link_target: None,
            field_target: None,
            field_buf: LineInput::default(),
            link_kind: None,
            link_dest: None,
            prompt_from: None,
            comment_seen: 0,
            cron_fired: std::collections::HashMap::new(),
            rail_docked: None,
            settings_cursor: 0,
            dial_next: 0,
            outline_width: None,
            pane_cursor: 0,
            editing_setting: None,
            new_heading: NewHeading {
                child: false,
                todo: false,
                above: false,
            },
            palette_history: Vec::new(),
            prompt_history: std::collections::BTreeMap::new(),
            vault_switch_asked: 0,
            vault_switch_path: None,
            image_view: None,
            image_return: None,
            command_arg: None,
            marks: std::collections::BTreeSet::new(),
            find_dir: std::path::PathBuf::new(),
            pane_return: None,
            history_walk: None,
            history_gen: 0,
            which_key_open: false,
            messages: Vec::new(),
            tracing: false,
            last_edited_file: None,
            git_memo: std::cell::RefCell::new(None),
            git_reads: std::cell::Cell::new(0),
            fringe_memo: std::cell::RefCell::new(None),
            ex_cycle: 0,
            ex_stem: None,
            notifications: Feedback::default(),
            palette_cursor: 0,
            pending: Vec::new(),
            command_args: String::new(),
            status: String::new(),
            quit: false,
            scroll_override: None,
            body_viewport: BODY_VIEWPORT_DEFAULT,
            outline_viewport: BODY_VIEWPORT_DEFAULT,
            body_anchor: None,
            recenter: None,
            pending_body: None,
            selection_active: true,
            zoom_steps: 0,
            search_return: None,
            body_cursors: std::collections::HashMap::new(),
            wrap: false,
            images_shown: true,
            ink: 0x00cd_d6f4,
            last_edited: None,
            body_stash: std::collections::HashMap::new(),
            buffers: Vec::new(),
            buf_seq: 0,
            jumps: Vec::new(),
            jump_at: 0,
            recent_files: Vec::new(),
            // A vault opened before the shell says what day it is still
            // has to draw a calendar; this is the epoch, and every
            // shell overwrites it on the first frame.
            today: "1970-01-01".to_owned(),
            date_pick: None,
            refile_source: None,
            now: "1970-01-01 00:00".to_owned(),
            palette_return: None,
            tag_target: None,
            tag_draft: Vec::new(),
            shell_out: None,
            body_scroll: None,
            hist_cursor: 0,
            row_memo: std::cell::RefCell::new(None),
            row_recomputes: std::cell::Cell::new(0),
            detail_memo: std::cell::RefCell::new(None),
            detail_recomputes: std::cell::Cell::new(0),
            palette_memo: std::cell::RefCell::new(None),
            palette_recomputes: std::cell::Cell::new(0),
            reloads: 0,
            block_memo: std::cell::RefCell::new(None),
            block_recomputes: std::cell::Cell::new(0),
        }
    }

    /// Run `command` directly — the mouse path: a clicked which-key
    /// chip or palette row dispatches the SAME command a chord would
    /// (I8; no shell-private verbs). Key handling resolves chords to
    /// exactly this entry point.
    pub fn run(&mut self, shell: &mut Shell, command: &str) {
        self.run_command(shell, canonical_command(command));
    }

    /// Paste whatever is on the clipboard into the outline as a
    /// headline after `after`.
    ///
    /// The fallback when closure's own ring is empty. Text that is
    /// already org lands as it is; text that is not gets a headline of
    /// its first line, because "nothing happened" is the outcome the
    /// report is about and a browser selection is rarely a `*`.
    fn paste_clipboard_subtree(
        &mut self,
        shell: &mut Shell,
        after: &closure_core::BlockId,
        title: &str,
    ) {
        let text = self.register_text().trim_end().to_owned();
        if text.is_empty() {
            self.say("nothing to paste — the ring and the clipboard are both empty");
            return;
        }
        let org = if text.trim_start().starts_with('*') {
            // A trailing newline, always. Without it the splice runs
            // the pasted body straight into the headline that follows
            // — `with a body line* Beta` — and that headline stops
            // being one. Caught on screen and in the file on disk; the
            // outline showed two rows where there had been three.
            format!("{text}\n")
        } else {
            // The first line names it; the rest becomes its body.
            let mut lines = text.lines();
            let head = lines.next().unwrap_or_default();
            let rest: Vec<&str> = lines.collect();
            if rest.is_empty() {
                format!("* {head}\n")
            } else {
                format!("* {head}\n{}\n", rest.join("\n"))
            }
        };
        match shell.paste_org_after(after, &org) {
            Ok(()) => self.status = format!("pasted the clipboard after {title}"),
            Err(e) => self.status = format!("paste failed: {e}"),
        }
    }

    /// The file `u` and `C-r` speak to.
    ///
    /// The last file a command actually changed, and only the selected
    /// row's file when nothing has been changed yet. Asking the
    /// selection was the whole bug: `d` moves the cursor off the thing
    /// it just cut, so `u` undid an edit in whatever file the cursor
    /// landed in — and once the last headline was gone there was no
    /// row to ask and `u` did nothing at all.
    fn undo_target(&self, shell: &Shell) -> Option<std::path::PathBuf> {
        self.last_edited_file.clone().or_else(|| {
            self.rows_shared(shell)
                .get(self.selected)
                .map(|r| std::path::PathBuf::from(&r.path))
        })
    }

    /// Record the buffer a command just left, so the pane it opened
    /// knows the way back.
    ///
    /// One place rather than one per command: the three reports were
    /// three commands with the same omission, and a fourth was only a
    /// matter of time.
    fn note_pane_return(&mut self, from: ModalSurface) {
        // Browse is *home*, not a pane: `reload-shell` and the other
        // commands that deliberately return to the outline are asking
        // to leave the buffer, and offering to put it back would undo
        // what was asked for.
        let opened_a_pane = !self.surface.is_editor() && self.surface != ModalSurface::Browse;
        if from.is_editor() && opened_a_pane {
            // Only the first: panes stack, and the buffer under the
            // bottom one is the buffer to come back to.
            if self.pane_return.is_none() {
                self.pane_return = Some(from);
            }
        } else if !opened_a_pane {
            // Back in a buffer, or back at the outline: whatever was
            // remembered has been used or is no longer wanted.
            self.pane_return = None;
        }
    }

    /// Which-key items for the active mode, as structured
    /// `(chord, command)` pairs a GUI renders as clickable chips —
    /// sourced from [`closure_input::mode_keymap`] (I4), never a
    /// hand-maintained list.
    #[must_use]
    pub fn hint_items(&self) -> Vec<(String, String)> {
        self.keys.clone()
    }

    /// What which-key should be scoped to right now.
    ///
    /// The panel filters its rows by this. It used to read the
    /// outline's pending strokes, which are empty in a buffer however
    /// many prefix keys the editor is holding — so pressing `g` in a
    /// note showed the whole map instead of the `g` map.
    #[must_use]
    pub fn which_key_pending(&self) -> String {
        if self.surface.is_editor() {
            // The alt leader is the app's chord, not the buffer's, and
            // it is the one moment a buffer is open and the *app* is
            // mid-chord. Asking the buffer alone is how `C-SPC` opened a
            // silent prefix — which is a worse leader than no leader.
            let app_pending = self.pending_chord();
            if !app_pending.is_empty() {
                return app_pending;
            }
            let pending = self.body_pending_chord();
            if !pending.is_empty() {
                return pending;
            }
            // The `z` viewport prefix is the editor's other pending
            // state, and it is held outside the vim engine.
            if self.pending_body == Some(BodyPrefix::Viewport) {
                return "z".to_owned();
            }
            if self.pending_body == Some(BodyPrefix::OrgAccept) {
                return "C-c".to_owned();
            }
            return String::new();
        }
        self.pending_chord()
    }

    /// The three things a person wants from an open buffer, as
    /// `(label, command, chord)` — the chord being `None` where this
    /// mode has none and the button is the only way.
    ///
    /// The discard button said `:q!` in every mode, including the two
    /// where `:` types a colon and the ex line cannot be opened from
    /// inside a buffer at all: "only show the keybindings that are
    /// relevant to the corresponding mode".
    #[must_use]
    pub fn buffer_actions(&self) -> Vec<(&'static str, &'static str, Option<String>)> {
        vec![
            (
                "\u{2713} save",
                "save-buffer",
                self.chord_for("save-buffer").map(ToOwned::to_owned),
            ),
            // org-edit-special's own pair. Not in the outline keymap:
            // `C-c C-c` there is org's "do the thing at point", which
            // for a source block is running it. One chord, two
            // meanings by surface — which is org's own rule, and only
            // the surface can tell them apart.
            (
                "\u{2713} save & close",
                "commit-edit",
                Some("C-c C-c".to_owned()),
            ),
            (
                "\u{2715} discard",
                "discard-edit",
                Some("C-c C-k".to_owned()),
            ),
        ]
    }

    /// Which-key data grouped for the Doom-style popup: every keymap
    /// pair once, grouped by its palette section ("Command" when
    /// uncurated), groups in section order, entries chord-sorted (I4).
    ///
    /// Scoped to the surface. Built from `mode_keymap` alone it listed
    /// the *outline's* chords wherever you were, so a buffer was
    /// offered `j:next-file` and `c:capture` — neither of which runs
    /// there, one of which is a letter you were about to type.
    #[must_use]
    pub fn which_key_groups(&self) -> Vec<(String, Vec<(String, String)>)> {
        // Whose chord is pending decides whose keys these are. With the
        // alt leader open inside a buffer, the panel that listed the
        // buffer's keys filtered them by `SPC` and found none — the
        // pending chord and the list it filters have to come from the
        // same keymap or the panel goes blank exactly when it is needed.
        if self.surface.is_editor() && self.pending_chord().is_empty() {
            return editor_which_key(self.mode);
        }
        // …and the same courtesy for every other surface that is not
        // the outline. The editor got an arm of its own and nothing
        // else did, so a prompt, a floating picker and the full-size
        // image viewer all answered with the outline's hundred and
        // forty chords — plainest in a screenshot of the viewer, where
        // `m:toggle-mark` and `D:delete-marked` sat under a picture
        // that answers to exactly one key.
        //
        // Classified from what the surface *has* rather than from a
        // new list beside the other lists: a picker is one because
        // `is_floating_picker` says so, and a prompt is one because it
        // has a line to type into.
        match self.surface {
            ModalSurface::ImageView => image_which_key(),
            ModalSurface::DatePick => date_which_key(),
            s if Self::is_floating_picker(s) => picker_which_key(),
            _ if self.prompt().is_some() => prompt_which_key(),
            // Browse and the pane lists — Agenda, Backlinks, Journal,
            // Conflicts — are the outline's own keymap: `j`/`k` move,
            // `RET` opens. They were right all along.
            _ => self.outline_which_key(),
        }
    }

    /// The outline's own keymap, grouped.
    fn outline_which_key(&self) -> Vec<(String, Vec<(String, String)>)> {
        let section_of = |cmd: &str| -> &str {
            PALETTE_COMMANDS
                .iter()
                .find(|(_, canonical, ..)| *canonical == cmd)
                .map_or("Command", |(.., sec, _)| sec)
        };
        let mut groups: Vec<(String, Vec<(String, String)>)> = PALETTE_SECTIONS
            .iter()
            .chain(std::iter::once(&"Command"))
            .map(|s| ((*s).to_owned(), Vec::new()))
            .collect();
        for (chord, cmd) in &self.keys {
            let sec = section_of(cmd);
            if let Some((_, v)) = groups.iter_mut().find(|(t, _)| t == sec) {
                v.push((chord.clone(), cmd.clone()));
            }
        }
        groups.retain(|(_, v)| !v.is_empty());
        for (_, v) in &mut groups {
            v.sort();
        }
        groups
    }

    /// Palette rows for the current filter: the shared
    /// [`command_palette`] source flattened in section order, so the
    /// modal palette shows the same grouped, described, chord-carrying
    /// entries as every other shell (G6).
    #[must_use]
    pub fn palette_entries(&self) -> Vec<PaletteEntry> {
        self.palette_shared().as_ref().clone()
    }

    /// The same entries, shared rather than cloned — the render path's
    /// entry point.
    ///
    /// Building them scores every command against the query and walks
    /// the keymap once per entry to find its chord, allocating a
    /// handful of `String`s each time. The palette pane asked for that
    /// on every frame *and* twice per keystroke, which is why it was
    /// the slowest surface to scroll. Memoised against
    /// `(query, mode)` — the mode matters because every entry carries
    /// the chord for the *active* mode.
    #[must_use]
    pub fn palette_shared(&self) -> std::sync::Arc<Vec<PaletteEntry>> {
        {
            let memo = self.palette_memo.borrow();
            if let Some(m) = memo.as_ref()
                && m.query == self.field_buf.text()
                && m.mode == self.mode
                && m.history_gen == self.history_gen
            {
                return std::sync::Arc::clone(&m.entries);
            }
        }
        let entries = std::sync::Arc::new(self.palette_entries_uncached());
        self.palette_recomputes
            .set(self.palette_recomputes.get() + 1);
        *self.palette_memo.borrow_mut() = Some(PaletteMemo {
            query: self.field_buf.text().to_owned(),
            mode: self.mode,
            history_gen: self.history_gen,
            entries: std::sync::Arc::clone(&entries),
        });
        entries
    }

    /// Ground truth: build the palette without consulting or filling
    /// the memo. What [`Self::palette_shared`] must always agree with.
    #[must_use]
    pub fn palette_entries_uncached(&self) -> Vec<PaletteEntry> {
        palette_in_keymap(self.field_buf.text(), &self.keys, &self.palette_history)
            .into_iter()
            .flat_map(|s| s.items)
            .collect()
    }

    /// How many times the palette has actually been rebuilt. The
    /// render budget is one per query or mode change.
    #[must_use]
    pub const fn palette_recomputes(&self) -> u64 {
        self.palette_recomputes.get()
    }

    /// Cursor into [`Self::palette_entries`].
    #[must_use]
    pub const fn palette_cursor(&self) -> usize {
        self.palette_cursor
    }

    /// Palette keys: typing filters, Up/Down move, Enter runs the
    /// highlighted command through [`Self::run`], Esc cancels.
    fn on_palette_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        // The palette is a minibuffer, so it walks the way every other
        // popup does ([`list_step`]) — arrows, `C-n`/`C-p`, `C-j`/`C-k`.
        // It had only the arrows, which is a mouse-hand gesture on a
        // surface you reached from the home row.
        if let Some(step) = list_step(key, ctrl) {
            let len = self.palette_entries().len();
            self.palette_cursor = step_wrapping(self.palette_cursor, len, step);
            return;
        }
        match key {
            "escape" => {
                self.field_buf.clear();
                self.palette_cursor = 0;
                self.close_palette();
            }
            "enter" => self.commit_palette(shell),
            // Everything the list did not claim is the field's — the
            // same field, and so the same chords, as every other
            // prompt. A key that changed the text puts the cursor back
            // on the first match, because the old index belonged to the
            // old list.
            _ => {
                let before = self.field_buf.text().to_owned();
                let mut kill = self.shared_kill();
                line_key(&mut self.field_buf, &mut kill, key, ctrl, alt, text);
                self.keep_shared_kill(&kill);
                if self.field_buf.text() != before {
                    self.palette_cursor = 0;
                }
            }
        }
    }

    /// Run the palette entry under the cursor and close the palette.
    fn commit_palette(&mut self, shell: &mut Shell) {
        // What was typed wins when it is a command line rather than a
        // filter: `capture Weekly review` fuzzy-matched the `capture`
        // entry and ran the bare form, so the argument vanished
        // between the typing and the running. The list is still the
        // list — this only fires when the first word is a command that
        // takes an argument and there is one.
        let typed = self.field_buf.text().to_owned();
        let (name, args) = split_command(&typed);
        if !args.is_empty() && command_argument(canonical_command(name)).is_some() {
            self.field_buf.clear();
            self.palette_cursor = 0;
            self.close_palette();
            self.remember_palette_command(name);
            self.run_command(shell, &typed);
            return;
        }
        let pick = self
            .palette_entries()
            .get(self.palette_cursor)
            .map(|e| e.action.command().to_owned());
        self.field_buf.clear();
        self.palette_cursor = 0;
        self.close_palette();
        if let Some(cmd) = pick {
            self.remember_palette_command(&cmd);
            self.run_command(shell, &cmd);
        }
    }

    /// Record a command the palette just ran, newest first.
    ///
    /// Re-running something moves it back to the front rather than
    /// adding a second copy, so the list stays a set of distinct
    /// commands in the order you last wanted them. Capped because a
    /// suggestion list longer than the eye reads is not a suggestion.
    fn remember_palette_command(&mut self, cmd: &str) {
        const KEEP: usize = 5;
        self.palette_history.retain(|c| c != cmd);
        self.palette_history.insert(0, cmd.to_owned());
        self.palette_history.truncate(KEEP);
        self.history_gen += 1;
    }

    /// Open the command palette over whatever is on screen.
    fn open_palette(&mut self) {
        self.field_buf.clear();
        self.palette_cursor = 0;
        // Opened over a buffer it is a floating bar, not a replacement:
        // closing it gives that buffer back ([`Self::close_palette`]).
        self.palette_return = self.surface.is_editor().then_some(self.surface);
        self.surface = ModalSurface::Palette;
        self.say("palette — type to filter, Enter to run");
    }

    /// Put the palette away, giving back whatever it floated over.
    const fn close_palette(&mut self) {
        match self.palette_return.take() {
            Some(surface) => self.surface = surface,
            None => self.go_home(),
        }
    }

    /// The surface the palette is floating over — what a shell paints
    /// underneath it.
    ///
    /// The palette is a bar over your work (Raycast, Zed, the VS Code
    /// command bar), not a pane that replaces it, so the window needs
    /// to know what was there. Everywhere else this is just the active
    /// surface.
    #[must_use]
    pub fn surface_beneath(&self) -> ModalSurface {
        match self.surface {
            ModalSurface::Palette => self.palette_return.unwrap_or_else(|| self.home_surface()),
            // `C-c C-l` floats over the buffer, because the whole
            // question it asks is "where in this text", and a menu that
            // replaced the text would take the answer away with it.
            // Both prompts a buffer can open: the outline behind a
            // title field is a different screen, and "everything is
            // shifting and I always get confused" is what that costs.
            ModalSurface::InsertLink | ModalSurface::AddSibling => {
                self.prompt_from.unwrap_or_else(|| self.home_surface())
            }
            // The `:` line is a bar at the bottom of whatever you are
            // in — vim's is, Emacs' minibuffer is, and the palette
            // above already is. It used to have nothing underneath it,
            // so opening it in a buffer made the window fall back to
            // the outline and the whole layout jumped ("everything is
            // shifting and I always get confused").
            ModalSurface::Ex => self.ex_return.unwrap_or_else(|| self.home_surface()),
            // A picture is a light box over your work, and the work is
            // usually the note that links it. Without this it fell
            // through to `other`, the window had no pane to paint for a
            // picture, and the outline appeared behind the image — so
            // opening a photo from a buffer looked like the buffer had
            // closed.
            ModalSurface::ImageView => self.image_return.unwrap_or_else(|| self.home_surface()),
            // Every floating picker, not the two that were spelled out
            // here. The other five told the pane to paint their own
            // list, and then floated over it — so the same rows were
            // drawn twice, the lower copy clipped by the panel edge.
            // That is the "weird selection shadow": a row of the
            // undo-history pane showing below the undo-history picker.
            s if Self::is_floating_picker(s) => self.home_surface(),
            other => other,
        }
    }

    /// Is this surface drawn as a floating picker over whatever was
    /// already there?
    ///
    /// The list that [`Self::picker_rows`] answers for. Kept beside
    /// nothing else, because two places deciding which surfaces float
    /// is how five of them came to paint themselves twice.
    const fn is_floating_picker(surface: ModalSurface) -> bool {
        matches!(
            surface,
            ModalSurface::Palette
                | ModalSurface::Buffers
                | ModalSurface::Files
                | ModalSurface::Headlines
                | ModalSurface::Blocks
                | ModalSurface::Messages
                | ModalSurface::UndoHistory
                | ModalSurface::DbView
                | ModalSurface::Graph
                | ModalSurface::InsertLink
        )
    }

    /// Mouse path for the palette: clicking row `i` runs that entry —
    /// the same commit Enter performs on a moved cursor.
    pub fn palette_click(&mut self, shell: &mut Shell, i: usize) {
        if self.surface != ModalSurface::Palette {
            return;
        }
        self.palette_cursor = i.min(self.palette_entries().len().saturating_sub(1));
        self.commit_palette(shell);
    }

    /// Active editing mode.
    #[must_use]
    pub const fn input_mode(&self) -> InputMode {
        self.mode
    }
    /// Active surface.
    #[must_use]
    pub const fn surface(&self) -> ModalSurface {
        self.surface
    }
    /// Highlighted row index.
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }
    /// Search filter (only meaningful on the Search surface).
    #[must_use]
    pub fn query(&self) -> &str {
        self.query.text()
    }
    /// The Search overlay context line: search glyph, live query, caret
    /// bar, and the live match count, pluralized.
    #[must_use]
    pub fn search_context(&self, shell: &Shell) -> String {
        let n = self.rows_shared(shell).len();
        let m = if n == 1 { "match" } else { "matches" };
        format!("\u{2315} {}\u{258f} \u{b7} {} {}", self.query(), n, m)
    }
    /// In-progress capture title.
    #[must_use]
    pub fn capture_buffer(&self) -> &str {
        self.capture_buf.text()
    }
    /// Byte offset of the cursor in the capture field, so a shell can
    /// draw the caret where the next character will actually go.
    #[must_use]
    pub const fn capture_cursor(&self) -> usize {
        self.capture_buf.cursor()
    }
    /// One-line status.
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }
    /// Whether the user asked to quit.
    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.quit
    }

    /// The notification log — what a shell paints as toasts.
    #[must_use]
    pub const fn notifications(&self) -> &Feedback {
        &self.notifications
    }

    /// Add a notification to the log.
    pub fn notify(&mut self, level: ToastLevel, text: impl Into<String>) {
        self.notifications.notify(level, text);
    }

    /// Whether the which-key panel is pinned open.
    ///
    /// It used to be a bool inside the gpui window with a button as its
    /// only door, so there was no command to bind and nothing for the
    /// palette to list — for a panel whose whole job is telling you
    /// what the keys are, there was no key.
    #[must_use]
    pub const fn which_key_open(&self) -> bool {
        self.which_key_open
    }

    /// The chords worth a line along the bottom, for the surface you
    /// are on.
    ///
    /// It listed the outline's keymap wherever you were, so a buffer's
    /// footer offered `j:next-file` and `q:quit` — one of which is a
    /// letter you were about to type and neither of which runs there.
    /// In a buffer it names the editor's own vocabulary instead, from
    /// the same table which-key reads.
    #[must_use]
    pub fn key_hints(&self) -> String {
        // The panel's answer, flattened — not a second copy of the
        // question. This held its own `is_editor` branch and its own
        // fallback to the outline's map, which is why fixing the panel
        // for a surface left the strip along the bottom of the same
        // window still advertising `m:toggle-mark` under a picture.
        // One place decides what a surface answers to.
        self.which_key_groups()
            .into_iter()
            .flat_map(|(_, rows)| rows)
            .map(|(chord, what)| format!("{chord}:{what}"))
            .collect::<Vec<_>>()
            .join("  ")
    }

    /// The chord strokes entered so far but not yet resolved, joined
    /// (e.g. `"g"` after pressing `g`, `"C-x"` mid emacs chord). Empty
    /// when nothing is pending. Backs the which-key popup.
    #[must_use]
    pub fn pending_chord(&self) -> String {
        self.pending.join(" ")
    }

    /// While a multi-stroke chord is pending, the `(remaining, command)`
    /// completions from the active mode's keymap — every binding whose
    /// chord extends the pending prefix, with the prefix stripped.
    /// Empty when nothing is pending. Sorted by remaining chord.
    #[must_use]
    pub fn completions(&self) -> Vec<(String, String)> {
        if self.pending.is_empty() {
            return Vec::new();
        }
        let prefix = format!("{} ", self.pending.join(" "));
        let mut out: Vec<(String, String)> = self
            .keys
            .iter()
            .filter_map(|(chord, cmd)| {
                chord
                    .strip_prefix(&prefix)
                    .map(|rest| (rest.to_owned(), cmd.clone()))
            })
            .collect();
        out.sort();
        out
    }

    /// Rows: all headlines on Browse, fuzzy-filtered while searching.
    #[must_use]
    pub fn rows(&self, shell: &Shell) -> Vec<Row> {
        self.rows_shared(shell).as_ref().clone()
    }

    /// The same outline rows as [`Self::rows`], shared rather than
    /// cloned — the render path's entry point.
    ///
    /// A frame asks for the row list from the context line, the
    /// outline pane, the detail pane and from inside every mouse
    /// listener. Deriving it walks every document and allocates five
    /// strings per headline, so the result is memoised against
    /// `(vault revision, active filter)`: an unchanged vault and an
    /// unchanged query hand back the previous `Arc` for the cost of a
    /// refcount bump. Both key components are exactly what the
    /// derivation reads, so the memo cannot go stale — a mutation
    /// bumps [`closure_store::Vault::revision`] and a keystroke in the
    /// search overlay changes the filter.
    #[must_use]
    pub fn rows_shared(&self, shell: &Shell) -> std::sync::Arc<Vec<Row>> {
        let filter = if self.surface == ModalSurface::Search {
            self.query.text()
        } else {
            ""
        };
        let revision = shell.vault.revision();
        {
            let memo = self.row_memo.borrow();
            if let Some(m) = memo.as_ref()
                && m.revision == revision
                && m.filter == filter
            {
                return std::sync::Arc::clone(&m.rows);
            }
        }
        let rows = std::sync::Arc::new(Self::derive_rows(shell, filter));
        self.row_recomputes.set(self.row_recomputes.get() + 1);
        *self.row_memo.borrow_mut() = Some(RowMemo {
            revision,
            filter: filter.to_owned(),
            rows: std::sync::Arc::clone(&rows),
        });
        rows
    }

    /// Drop the memo so the next [`Self::rows_shared`] pays for a full
    /// walk. Nothing in normal operation needs this — the revision key
    /// handles invalidation — but it lets a caller (or a test) demand
    /// ground truth.
    pub fn invalidate_rows(&mut self) {
        *self.row_memo.borrow_mut() = None;
    }

    /// Ground truth: derive the outline rows without consulting or
    /// filling the memo. What [`Self::rows_shared`] must always agree
    /// with, and the escape hatch for a caller that would rather pay
    /// the walk than trust a cache.
    #[must_use]
    pub fn rows_uncached(&self, shell: &Shell) -> Vec<Row> {
        let filter = if self.surface == ModalSurface::Search {
            self.query.text()
        } else {
            ""
        };
        Self::derive_rows(shell, filter)
    }

    /// How many full vault walks [`Self::rows_shared`] has paid for
    /// since this app was created. The render budget is "at most one
    /// per actual change", and the tests assert it.
    #[must_use]
    pub const fn rows_recomputes(&self) -> u64 {
        self.row_recomputes.get()
    }

    /// The uncached derivation behind [`Self::rows_shared`].
    fn derive_rows(shell: &Shell, filter: &str) -> Vec<Row> {
        outline_rows(shell, filter)
    }

    /// Move the selection to row `i`, clamped to the current result
    /// set. Used by mouse clicks on a row (draw parity with [`App`]).
    pub fn select(&mut self, i: usize, shell: &Shell) {
        self.scroll_override = None;
        let last = self.rows_shared(shell).len().saturating_sub(1);
        self.selected = i.min(last);
        // Clicking a row is the least ambiguous way there is of saying
        // "this one".
        self.selection_active = true;
    }

    /// Wheel scrolling: move the viewport by `delta` rows (negative =
    /// up), clamped to the row range for a page-sized window. Does not
    /// move the selection; any selection movement clears the override
    /// and [`Self::view_window`] returns to its keep-selection-visible
    /// rule.
    pub fn scroll_by(&mut self, delta: i32, shell: &Shell, page: usize) {
        let rows = self.rows_shared(shell).len();
        let page = page.max(1);
        let max_off = rows.saturating_sub(page);
        let base = self
            .scroll_override
            .unwrap_or_else(|| self.selected.saturating_sub(page - 1).min(max_off));
        let step = usize::try_from(delta.unsigned_abs()).unwrap_or(usize::MAX);
        let new = if delta < 0 {
            base.saturating_sub(step)
        } else {
            base.saturating_add(step).min(max_off)
        };
        self.scroll_override = Some(new);
    }

    /// The visible slice of rows for a viewport of `page` rows, plus its
    /// start offset, chosen so the selection stays on screen. Stateless
    /// (offset derived from the selection each call); mirrors
    /// [`App::view_window`].
    #[must_use]
    pub fn view_window(&self, shell: &Shell, page: usize) -> (usize, Vec<Row>) {
        if let Some(o) = self.scroll_override {
            let rows = self.rows_shared(shell);
            let page = page.max(1);
            if rows.len() > page {
                let off = o.min(rows.len() - page);
                return (off, rows[off..off + page].to_vec());
            }
            return (0, rows.as_ref().clone());
        }
        let rows = self.rows_shared(shell);
        if page == 0 || rows.len() <= page {
            return (0, rows.as_ref().clone());
        }
        let max_offset = rows.len() - page;
        let offset = self.selected.saturating_sub(page - 1).min(max_offset);
        let slice = rows[offset..offset + page].to_vec();
        (offset, slice)
    }

    /// Full preview of the currently-selected headline (resolved by its
    /// stable id through the vault index), for the detail pane. Mirrors
    /// [`App::detail`].
    #[must_use]
    pub fn detail(&self, shell: &Shell) -> Option<Detail> {
        self.detail_shared(shell).map(|d| d.as_ref().clone())
    }

    /// The detail of the *selected* headline — what a shell paints in
    /// the side pane, and nothing when Escape has said there is no
    /// selection.
    ///
    /// [`Self::detail`] answers a different question: what is under the
    /// cursor, which is what the commands that act on a row read. The
    /// two came apart when Escape dropped the selection and the side
    /// pane went on showing the headline it had just stopped pointing
    /// at — the screen said one thing while the next capture did
    /// another.
    #[must_use]
    pub fn selected_detail(&self, shell: &Shell) -> Option<std::sync::Arc<Detail>> {
        self.selection_active
            .then(|| self.detail_shared(shell))
            .flatten()
    }

    /// The same detail, shared rather than cloned — the render path's
    /// entry point.
    ///
    /// Deriving it copies the whole headline: body text, tags,
    /// properties. Scrolling the outline repaints the detail pane
    /// without changing the selection, so it is memoised against
    /// `(vault revision, selected id)` — the two things it reads.
    #[must_use]
    pub fn detail_shared(&self, shell: &Shell) -> Option<std::sync::Arc<Detail>> {
        let revision = shell.vault.revision();
        let id = self
            .rows_shared(shell)
            .get(self.selected)
            .map(|r| r.id.clone());
        {
            let memo = self.detail_memo.borrow();
            if let Some(m) = memo.as_ref()
                && m.revision == revision
                && m.id == id
            {
                return m.detail.clone();
            }
        }
        let detail = Self::derive_detail(shell, id.as_deref()).map(std::sync::Arc::new);
        self.detail_recomputes.set(self.detail_recomputes.get() + 1);
        *self.detail_memo.borrow_mut() = Some(DetailMemo {
            revision,
            id,
            detail: detail.clone(),
        });
        detail
    }

    /// How many times the detail has actually been read out of the
    /// vault. The render budget is one per selection change.
    #[must_use]
    pub const fn detail_recomputes(&self) -> u64 {
        self.detail_recomputes.get()
    }

    /// The uncached derivation behind [`Self::detail_shared`].
    fn derive_detail(shell: &Shell, id: Option<&str>) -> Option<Detail> {
        let bid = closure_core::BlockId::from_existing(id?);
        let (h, path) = shell.vault.find_by_id(&bid)?;
        let children = shell
            .vault
            .children_source(&bid)
            .map(|src| closure_org::strip_property_drawers(&src))
            .unwrap_or_default();
        Some(Detail::of(h, path, children))
    }

    /// Feed one key. `key` is the gpui/egui-style name; `ctrl`/`alt`
    /// are modifiers; `text` is the printable char when any.
    pub fn on_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        // What the config wrote down beats what the surface would have
        // done. The readline chords inside a buffer and inside every
        // prompt are resolved by those handlers rather than through the
        // keymap, so without this a `bind C-b = …` landed everywhere
        // except the twenty keys a writer spends the day pressing.
        //
        // Modified chords only: `bind j = next-file` is an outline
        // binding, and a pre-empt that took bare letters too would move
        // the selection every time you typed a `j` into a note.
        if (ctrl || alt)
            && let Some(stroke) = modal_stroke(key, ctrl, alt, text)
            && let Some(cmd) = self
                .key_overrides
                .iter()
                .find(|(chord, _)| *chord == stroke)
                .map(|(_, cmd)| cmd.clone())
        {
            // An unbind is a chord that does nothing, everywhere —
            // which is what taking a key away has to mean.
            if !cmd.is_empty() {
                self.run_command(shell, &cmd);
            }
            return;
        }
        // Every motion in an editor ends here, including the ones the
        // vim engine resolves inside itself (`j`, `k`, `G`, a search)
        // and the file buffer's own handler — none of which know about
        // folds, because folds live on the app. So the rescue is one
        // wrapper rather than a hook per motion: whatever the key did,
        // the caret must not end on a line no shell will paint.
        // History, before the surface sees the key. `M-p`/`M-n` walk
        // it; `escape` and `enter` are the two doors out of a prompt
        // and both record what was in it — a history that kept only
        // what you accepted would forget the case the report is about.
        if (key == "p" || key == "n") && alt && !ctrl && self.walk_history(key == "p") {
            return;
        }
        if matches!(key, "escape" | "enter") {
            self.remember_prompt();
        }
        let before = self.body.cursor_line_col().0;
        let editing = self.surface.is_editor();
        match self.surface {
            ModalSurface::Search => self.on_search_key(shell, key, ctrl, alt, text),
            ModalSurface::Capture => self.on_capture_key(shell, key, ctrl, alt, text),
            ModalSurface::EditBody => self.on_editbody_key(shell, key, ctrl, alt, text),
            ModalSurface::Backlinks => self.on_backlinks_key(shell, key),
            ModalSurface::Agenda => self.on_list_key(shell, key, ListKind::Agenda),
            ModalSurface::TagsEdit => {
                self.on_field_key(shell, key, ctrl, alt, text, FieldKind::Tags);
            }
            ModalSurface::PropertyEdit => {
                self.on_field_key(shell, key, ctrl, alt, text, FieldKind::Property);
            }
            ModalSurface::Rename => {
                self.on_field_key(shell, key, ctrl, alt, text, FieldKind::Rename);
            }
            ModalSurface::AddSibling => {
                self.on_field_key(shell, key, ctrl, alt, text, FieldKind::AddSibling);
            }
            ModalSurface::Palette => self.on_palette_key(shell, key, ctrl, alt, text),
            ModalSurface::UndoHistory
            | ModalSurface::Headlines
            | ModalSurface::Blocks
            | ModalSurface::DbView
            | ModalSurface::Graph
            | ModalSurface::Messages => {
                self.on_picker_list_key(shell, key, ctrl, alt, text);
            }
            // The same picker, with its own Enter: a directory is
            // walked into, a name that is not there yet is made.
            ModalSurface::FindFile => self.on_find_file_key(shell, key, ctrl, alt, text),
            // One picture and one way out: anything that is not a way
            // out leaves it open, because a viewer that closes on a
            // stray key is a viewer you cannot read from.
            ModalSurface::ImageView => {
                if matches!(key, "escape" | "enter" | "q") {
                    self.image_view = None;
                    self.surface = self.image_return.take().unwrap_or(ModalSurface::Browse);
                }
            }
            ModalSurface::BodySearch => self.on_body_search_key(shell, key, ctrl, alt, text),
            ModalSurface::Sniffer => self.on_sniffer_key(shell, key),
            ModalSurface::Conflicts => self.on_conflicts_key(shell, key),
            ModalSurface::Ex => self.on_ex_key(shell, key, ctrl, alt, text),
            ModalSurface::Sync => self.on_sync_key(shell, key, ctrl, alt, text),
            ModalSurface::Llm => self.on_llm_key(key, ctrl, alt, text),
            // The read-only lists: nothing to type into, so they all
            // walk the same way and differ only in how long they are.
            ModalSurface::Journal | ModalSurface::Cron => self.on_list_pane_key(shell, key),
            ModalSurface::Settings => self.on_settings_key(shell, key),
            ModalSurface::Setting => match key {
                "escape" => {
                    self.editing_setting = None;
                    self.surface = ModalSurface::Settings;
                }
                "enter" => self.commit_setting(shell),
                _ => {
                    self.field_buf.key(key, ctrl, alt, text);
                }
            },
            ModalSurface::Manual => self.on_manual_key(key),
            ModalSurface::DescribeKey => self.on_describe_key(key, ctrl, alt, text),
            ModalSurface::InsertLink => self.on_insert_link_key(shell, key, ctrl, alt, text),
            ModalSurface::DatePick => self.on_datepick_key(shell, key, text),
            ModalSurface::Refile => self.on_refile_key(shell, key, ctrl, alt, text),
            ModalSurface::TagPick => self.on_tagpick_key(shell, key, ctrl, alt, text),
            ModalSurface::Buffers => {
                self.on_picker_key(shell, key, ctrl, alt, text, PickerKind::Buffers);
            }
            ModalSurface::Files => {
                self.on_picker_key(shell, key, ctrl, alt, text, PickerKind::Files);
            }
            ModalSurface::EditBlock => self.on_editblock_key(shell, key, ctrl, alt, text),
            ModalSurface::EditFile => self.on_editfile_key(shell, key, ctrl, alt, text),
            ModalSurface::Browse => self.on_browse_key(shell, key, ctrl, alt, text),
        }
        self.answer_comment_ask();
        // Only while a buffer is still open, and only if it was open
        // before: a key that *closed* one has no caret left to rescue.
        if editing && self.surface.is_editor() {
            let after = self.body.cursor_line_col().0;
            self.leave_hidden_line(after > before);
        }
    }

    /// The `:` command line's buffer while it is open.
    #[must_use]
    pub fn ex_buffer(&self) -> &str {
        self.ex_buf.text()
    }

    /// Keys for the org-edit-special session.
    ///
    /// The editing vocabulary is the body editor's, verbatim — same
    /// modes, same motions — because it *is* the body editor with a
    /// different buffer in it. Only the two exits differ: `C-Enter`
    /// writes the block back, and a quiet `Esc` in NORMAL discards the
    /// session rather than cancelling a body edit.
    fn on_editblock_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        if self.leader_key(shell, key, ctrl, alt, text) {
            return;
        }
        if self.org_accept_chord(shell, key, ctrl, alt, text) {
            return;
        }
        if key == "enter" && ctrl {
            self.commit_edit_special(shell);
            return;
        }
        if key == "escape"
            && self.body.mode() == EditorMode::Normal
            && self.body.pending_stroke().is_none()
            && self.body.pending_count() == 0
        {
            self.cancel_edit_special();
            return;
        }
        self.edit_body_key(shell, key, ctrl, alt, text);
    }

    /// Whether this keystroke belongs to the `SPC` leader rather than
    /// to the buffer, and route it if so.
    ///
    /// Doom's leader is the thing a Doom user's hands know, and it has
    /// to work where they spend the session — inside the buffer. In
    /// NORMAL that costs nothing: `SPC` is evil's forward-char, which
    /// is exactly the binding Doom takes away from it. In INSERT it is
    /// a space, because there it is prose. Only the Doom keymap has a
    /// leader, so no other mode loses a keystroke to this.
    fn leader_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) -> bool {
        if self.mode != InputMode::Doom {
            return false;
        }
        if self.pending.is_empty() {
            // A `SPC` mid-chord belongs to the chord: `d` then `SPC` is
            // vim's "delete the next character", not a leader.
            let editor_busy = self.body.pending_stroke().is_some() || self.body.pending_count() > 0;
            if key != "space" || editor_busy {
                return false;
            }
            // In INSERT a bare `SPC` is a space — that is what INSERT is
            // for — so the forty-two leader chords went away for as long
            // as you were typing. `C-SPC` and `M-SPC` (Doom's own
            // `doom-leader-alt-key`) open the leader instead, and open
            // the *same* one: a second door is worth having only if the
            // room behind it is the same room.
            if self.body.mode() == EditorMode::Insert && !(ctrl || alt) {
                return false;
            }
            // Whichever key opened it, what the dispatcher sees is `SPC`.
            self.on_browse_key(shell, "space", false, false, Some(' '));
            return true;
        }
        // Opening a chord, or continuing one: the keymap dispatcher owns
        // the strokes until the chord resolves or dies.
        self.on_browse_key(shell, key, ctrl, alt, text);
        true
    }

    /// The file buffer's keys: the same editor, with `:w` semantics.
    ///
    /// `C-Enter` writes and *stays* — a file you are editing is a file
    /// you keep editing, unlike a body, where the commit is the end of
    /// the errand. `Esc` out of NORMAL leaves without writing.
    /// The chords a buffer must not swallow.
    ///
    /// A buffer takes every modified chord for itself, which is right
    /// for the readline set and wrong for the ones that are about the
    /// *window*: saving means most where a buffer is open, and the
    /// which-key panel is the one thing you press when you cannot
    /// remember what to press. Reports whether it claimed the key.
    fn window_chord(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) -> bool {
        if !(ctrl || alt) {
            return false;
        }
        let Some(stroke) = modal_stroke(key, ctrl, alt, text) else {
            return false;
        };
        let Some(cmd) = self.command_for(&stroke).map(ToOwned::to_owned) else {
            return false;
        };
        let cmd = cmd.as_str();
        if !matches!(
            cmd,
            "save-buffer"
                | "toggle-which-key"
                | "reload-shell"
                | "toggle-wrap"
                | "toggle-fold"
                // Which keymap is in force is a property of the window,
                // not of the text: "it isn't possible at all to
                // cycle-mode via hotkey in the editor view".
                | "next-input-mode"
                // Doom binds the new-headline chords `:ni` in
                // `evil-org-mode-map` — they are buffer chords first,
                // and the buffer is where an org user is when they
                // want another headline. `C-RET` used to commit the
                // buffer here while the keymap said it made a
                // headline; the keymap wins (I4), and the two chords
                // the header actually advertises for saving — `C-s`
                // and `C-c C-c` — are untouched.
                | "add-heading"
                | "add-heading-above"
                | "add-todo-heading"
                | "add-child-heading"
                | "add-todo-child-heading"
        ) {
            return false;
        }
        self.pending_body = None;
        self.run_command(shell, cmd);
        true
    }

    fn on_editfile_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        if self.leader_key(shell, key, ctrl, alt, text) {
            return;
        }
        if self.window_chord(shell, key, ctrl, alt, text) {
            return;
        }
        if self.org_accept_chord(shell, key, ctrl, alt, text) {
            return;
        }
        if key == "enter" && ctrl {
            self.commit_file_buffer(shell);
            return;
        }
        // The full-window editor answers `:` the way the body editor
        // does. It did not, so in the one view that is nothing but a
        // buffer, the chord every vim user reaches for typed a colon.
        if text == Some(':')
            && self.body.mode() != EditorMode::Insert
            && self.body.search_prompt().is_none()
        {
            self.begin_ex();
            return;
        }
        if key == "escape"
            && self.body.mode() == EditorMode::Normal
            && self.body.pending_stroke().is_none()
            && self.body.pending_count() == 0
        {
            self.view = ViewMode::Clickable;
            self.close_file_buffer();
            return;
        }
        self.edit_body_key(shell, key, ctrl, alt, text);
    }

    /// Headlines ranked by how many links point at them, most first,
    /// as `(id, title, count)`.
    ///
    /// The hubs of the vault: what everything else refers back to. A
    /// title comes along because an id alone is not a thing anyone can
    /// read.
    #[must_use]
    pub fn hub_rows(&self, shell: &Shell) -> Vec<(String, String, usize)> {
        let mut counts: std::collections::HashMap<closure_core::BlockId, usize> =
            std::collections::HashMap::new();
        for targets in shell.vault.link_graph().values() {
            for t in targets {
                *counts.entry(t.clone()).or_insert(0) += 1;
            }
        }
        let mut ranked: Vec<_> = counts.into_iter().collect();
        // Count descending, then id, so equal counts do not shuffle
        // between frames.
        ranked.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.0.to_string().cmp(&b.0.to_string()))
        });
        ranked
            .into_iter()
            .map(|(id, n)| {
                let title = shell
                    .vault
                    .find_by_id(&id)
                    .map_or_else(|| "?".to_owned(), |(h, _)| h.title().to_owned());
                (id.to_string(), title, n)
            })
            .collect()
    }

    /// Headlines nothing links to, as `(id, title)` — the far end of
    /// the same graph, and usually the interesting one.
    #[must_use]
    pub fn orphan_rows(&self, shell: &Shell) -> Vec<(String, String)> {
        let mut targeted: std::collections::HashSet<closure_core::BlockId> =
            std::collections::HashSet::new();
        for targets in shell.vault.link_graph().values() {
            for t in targets {
                targeted.insert(t.clone());
            }
        }
        shell
            .vault
            .iter()
            .flat_map(|(_, doc)| doc.all_headlines())
            .filter(|h| !targeted.contains(h.id()))
            .map(|h| (h.id().to_string(), h.title().to_owned()))
            .collect()
    }

    /// Link targets that resolve to nothing — typos and things that
    /// were deleted out from under a reference.
    #[must_use]
    pub fn dead_link_rows(&self, shell: &Shell) -> Vec<String> {
        let mut out = Vec::new();
        for (path, doc) in shell.vault.iter() {
            for h in doc.all_headlines() {
                for raw in h.link_targets() {
                    let Some(stripped) = raw.strip_prefix("id:") else {
                        continue;
                    };
                    if !shell
                        .vault
                        .has_id(&closure_core::BlockId::from_existing(stripped))
                    {
                        // The source matters as much as the target: a
                        // dead link is only fixable where it is
                        // written.
                        out.push(format!("{raw}  ← {} in {}", h.title(), path.display()));
                    }
                }
            }
        }
        out
    }

    /// The recorded command journal, newest last.
    #[must_use]
    pub fn journal_rows(&self, shell: &Shell) -> Vec<String> {
        closure_record::Journal::new(shell.vault.root(), true)
            .entries()
            .unwrap_or_default()
    }

    /// Scheduled jobs declared anywhere in the vault, as
    /// `(spec, command)`.
    ///
    /// A malformed block is skipped rather than fatal: a typo in one
    /// job must not make the pane unopenable.
    #[must_use]
    pub fn cron_rows(&self, shell: &Shell) -> Vec<JobRow> {
        job_rows(&shell.vault)
    }

    /// Collaboration state, created on first use.
    ///
    /// Creating it generates a keypair, so a session that never pairs
    /// never generates one. It binds [`DEFAULT_SYNC_BIND`] unless
    /// [`Self::configure_sync`] has said otherwise.
    pub fn sync_mut(&mut self) -> &mut SyncApp {
        let (bind, advertise) = (self.sync_bind, self.sync_advertise);
        self.sync
            .get_or_insert_with(|| SyncApp::with_bind("local", bind, advertise))
    }

    /// Point pairing at a socket, from the vault's `config.org`.
    ///
    /// Called before the user pairs, and safe afterwards: an identity
    /// that already exists is moved rather than replaced, so a ticket
    /// handed out earlier keeps its key (see [`SyncApp::rebind`]).
    pub fn configure_sync(
        &mut self,
        bind: std::net::SocketAddr,
        advertise: Option<std::net::IpAddr>,
    ) {
        self.sync_bind = bind;
        self.sync_advertise = advertise;
        if let Some(sync) = self.sync.as_mut() {
            sync.rebind(bind, advertise);
        }
    }

    /// Collaboration state, or `None` until something has asked for it
    /// — merely *reading* must not be what generates a keypair.
    #[must_use]
    pub const fn sync(&self) -> Option<&SyncApp> {
        self.sync.as_ref()
    }

    /// The ticket-entry field on the Sync surface.
    #[must_use]
    pub fn sync_buffer(&self) -> &str {
        self.sync_buf.text()
    }

    /// The assistant transcript, oldest first.
    #[must_use]
    pub fn chat_turns(&self) -> &[ChatTurn] {
        &self.chat
    }

    /// The question field on the assistant surface.
    #[must_use]
    pub fn chat_buffer(&self) -> &str {
        self.chat_buf.text()
    }

    /// Whether a question is waiting on the provider.
    #[must_use]
    pub const fn chat_busy(&self) -> bool {
        self.chat_busy
    }

    /// Record a question and mark the conversation as waiting.
    pub fn chat_ask(&mut self, text: String) {
        self.chat.push(ChatTurn {
            from_user: true,
            text,
        });
        self.chat_busy = true;
    }

    /// Record an answer (or a failure) and stop waiting.
    pub fn chat_answer(&mut self, text: String) {
        self.chat.push(ChatTurn {
            from_user: false,
            text,
        });
        self.chat_busy = false;
    }

    /// Keys for the assistant: typing edits the question, Enter sends
    /// it, Escape leaves.
    fn on_llm_key(&mut self, key: &str, ctrl: bool, alt: bool, text: Option<char>) {
        match key {
            "escape" => {
                self.go_home();
            }
            "enter" => {
                let question = self.chat_buf.take();
                let question = question.trim();
                if !question.is_empty() {
                    self.chat_ask(question.to_owned());
                }
            }
            _ => {
                let mut kill = self.shared_kill();
                line_key(&mut self.chat_buf, &mut kill, key, ctrl, alt, text);
                self.keep_shared_kill(&kill);
            }
        }
    }

    /// What the vault's `config.org` says about the assistant, and
    /// whether it can be used right now.
    ///
    /// A chat box that silently does nothing because no provider is
    /// configured is the worst version of this feature, so the pane
    /// gets a sentence naming the keys to add. When a provider *is*
    /// set, the key's environment variable is checked too — configured
    /// and usable are different questions — and only ever named, never
    /// printed.
    #[must_use]
    pub fn llm_config_status(&self, shell: &Shell) -> LlmStatus {
        let cfg = closure_config::Config::from_path(
            &shell.vault.root().join(closure_config::CONFIG_FILE),
        )
        .ok();
        let provider = cfg.as_ref().and_then(|c| c.llm_provider.clone());
        let model = cfg.as_ref().and_then(|c| c.llm_model.clone());
        let endpoint = cfg.as_ref().and_then(|c| c.llm_endpoint.clone());
        let key_env = cfg.as_ref().and_then(|c| c.llm_key_env.clone());
        let Some(provider_name) = provider.clone() else {
            return LlmStatus {
                ready: false,
                provider,
                model,
                endpoint,
                detail: "no assistant configured — add `llm_provider` (and `llm_model`, \
                         `llm_key_env`) to the closure-config block in config.org"
                    .to_owned(),
            };
        };
        let model_name = model
            .clone()
            .unwrap_or_else(|| "(default model)".to_owned());
        if let Some(var) = key_env {
            if closure_llm::resolve_key(&var).is_none() {
                return LlmStatus {
                    ready: false,
                    provider,
                    model,
                    endpoint,
                    detail: format!(
                        "{provider_name} / {model_name} — ${var} is not set in this environment"
                    ),
                };
            }
            return LlmStatus {
                ready: true,
                provider,
                model,
                endpoint,
                detail: format!("{provider_name} / {model_name} — key from ${var}"),
            };
        }
        LlmStatus {
            ready: true,
            provider,
            model,
            endpoint: endpoint.clone(),
            detail: endpoint.map_or_else(
                || format!("{provider_name} / {model_name} — no key required"),
                |url| format!("{provider_name} / {model_name} — {url}"),
            ),
        }
    }

    /// Run one assistant tool line against the vault.
    ///
    /// The gate is the live one: `view-render` answers with the
    /// *current* view only while render access is granted, and starts
    /// refusing the moment it is revoked. A model that could still read
    /// the screen after the user said no would make the toggle a lie.
    /// Takes `&mut Shell` because some of these tools write — capture,
    /// rename, set-property — and all of them go through the same
    /// registry surface the chords do (I8).
    pub fn llm_tool(&self, shell: &mut Shell, line: &str) -> String {
        if line.trim() == closure_llm::RENDER_TOOL {
            if !self.llm_render {
                return format!(
                    "error: '{}' not allowed — render access is revoked (toggle-llm-render)",
                    closure_llm::RENDER_TOOL
                );
            }
            return serialize_view(&browse_view(&shell.vault));
        }
        shell.vault.run_tool(line)
    }

    /// Keys for the Sync surface: typing edits the ticket field, Enter
    /// adds the peer, Escape leaves.
    fn on_sync_key(&mut self, shell: &Shell, key: &str, ctrl: bool, alt: bool, text: Option<char>) {
        match key {
            "escape" => {
                self.sync_buf.clear();
                self.go_home();
            }
            "enter" => {
                let ticket = self.sync_buf.take();
                match self.sync_mut().add_peer(ticket.trim()) {
                    Ok(()) => {
                        let n = self.sync_mut().peers().len();
                        // A peer pasted once is a peer tomorrow.
                        self.save_peers(shell);
                        self.say(format!("peer added — {n} peer(s)"));
                    }
                    Err(e) => {
                        // Keep the text so it can be corrected rather
                        // than retyped.
                        self.sync_buf.set_text(&ticket);
                        self.say(format!("bad ticket: {e}"));
                    }
                }
            }
            _ => {
                let mut kill = self.shared_kill();
                line_key(&mut self.sync_buf, &mut kill, key, ctrl, alt, text);
                self.keep_shared_kill(&kill);
            }
        }
    }

    /// Replace the status line — for a shell reporting something the
    /// core did not produce, such as what the pointer is hovering.
    pub fn set_status(&mut self, text: impl Into<String>) {
        self.say(text);
    }

    /// Show `text` on the bottom line and keep it.
    ///
    /// The one place a message reaches that line, so it is also the one
    /// place that can remember it. A repeat of the newest is not new
    /// information — a log full of "saved" is a log you stop reading —
    /// and the depth is capped, because a history that never forgets is
    /// a leak with a scrollbar.
    fn say(&mut self, text: impl Into<String>) {
        /// How far back the log goes.
        const KEEP: usize = 200;
        let text = text.into();
        if self.messages.first() != Some(&text) {
            self.messages.insert(0, text.clone());
            self.messages.truncate(KEEP);
        }
        self.status = text;
    }

    /// Every status line this session has shown, newest first.
    #[must_use]
    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    /// Whether every keypress is being timed.
    #[must_use]
    pub const fn tracing(&self) -> bool {
        self.tracing
    }

    /// Record a keypress that took longer than a frame.
    ///
    /// The seam a shell calls with a real measurement, and the answer
    /// to "how can I get logs in order to help with debugging?". The
    /// microfreeze on a level-1 headline does not reproduce here — on
    /// a vault shaped like the report every step of the selection
    /// costs the same few milliseconds — so this exists to be read on
    /// the machine where it does.
    ///
    /// The reading names what the step was doing, not just how long it
    /// took: a number with nowhere to go is not evidence. In
    /// particular it says whether the detail had to be derived again,
    /// because a memo that never hits would make every frame pay for
    /// the whole subtree of whatever is selected.
    pub fn note_slow_key(&mut self, key: &str, took: std::time::Duration, shell: &Shell) {
        self.note_slow_step(&format!("`{key}`"), took, shell);
    }

    /// Time any named step, not only a keystroke.
    ///
    /// "SPC t T doesn't do anything other than its activation message."
    /// It was armed and honest: it timed *keystrokes*, and the stall
    /// being chased happens on the reload timer, where the live session
    /// dials its peers. So the instrument was watching the one place
    /// the problem was not, and correctly reported nothing.
    ///
    /// A tracer that only sees the work you already suspect is not an
    /// instrument, it is a confirmation.
    pub fn note_slow_step(&mut self, step: &str, took: std::time::Duration, shell: &Shell) {
        /// Anything under a frame is not a freeze, and logging it
        /// would bury the one step that is.
        const FRAME: std::time::Duration = std::time::Duration::from_millis(16);
        if !self.tracing || took < FRAME {
            return;
        }
        let before = self.detail_recomputes();
        let level = self
            .detail(shell)
            .map_or_else(|| "-".to_owned(), |d| d.level.to_string());
        let derived = self.detail_recomputes() - before;
        self.say(format!(
            "trace: {step} took {}ms · {:?} · level {level} · detail derived {derived}x",
            took.as_millis(),
            self.surface,
        ));
    }

    /// One turn of the live session, called from the shell's frame
    /// loop: tell peers where we are, serve whoever is calling, and
    /// dial each paired peer in turn.
    ///
    /// This is what makes the stack a session rather than a command.
    /// It was manual before — and more manual than it looked: the
    /// running shell never opened a connection at all, it wrote bundle
    /// files into a shared folder when you pressed a key.
    ///
    /// Returns how many rounds actually completed, so a caller can
    /// tell "nobody is there" from "we did nothing".
    pub fn session_tick(&mut self, shell: &Shell) -> usize {
        if self.sync.is_none() {
            return 0;
        }
        if let Some(here) = self.local_presence(shell) {
            self.sync_mut().set_local_presence(&here.block, here.line);
        }
        // Open the socket ourselves when it is allowed. A session that
        // only accepts after someone clicks "listen" is not continuous
        // — it is a button you have to find again every launch, and
        // the peer that dialled while you were finding it got
        // "connection refused". `inbound_ready` is the consent rule and
        // it is unchanged: loopback, or at least one trusted peer.
        if self.sync_mut().listener().is_none()
            && self.sync_mut().inbound_ready().is_ok()
            && let Err(e) = self.sync_mut().listen()
        {
            // Reported once rather than every 1.5 seconds: a port in
            // use is a thing to fix, not a thing to shout about.
            self.say(format!("could not open the sync socket: {e}"));
        }
        let mut rounds = self.sync_mut().serve_pending();
        // One peer per tick, round-robin. Accepting is free; *dialling*
        // costs its timeout whenever nobody answers, and doing every
        // peer on every tick makes the worst case scale with how many
        // people you have paired with. A peer waits a few seconds
        // longer for its turn, which is nothing against a session that
        // reconnects on its own.
        let addrs: Vec<std::net::SocketAddr> =
            self.sync_mut().peers().iter().map(|p| p.addr).collect();
        if !addrs.is_empty() {
            let at = self.dial_next % addrs.len();
            self.dial_next = self.dial_next.wrapping_add(1);
            let addr = addrs[at];
            // A peer that is not up is the ordinary case, not an
            // error: this runs on a timer and must stay quiet.
            let outcome = self.sync_mut().sync_with(addr);
            let ok = outcome.is_ok();
            self.sync_mut().record_outcome(addr, outcome.map(|()| 0));
            if ok {
                rounds += 1;
            }
        }
        rounds
    }

    /// The message for a save: which file was rewritten, and its size.
    fn saved_message(shell: &Shell, id: &closure_core::BlockId) -> String {
        shell.vault.find_by_id(id).map_or_else(
            || "body saved".to_owned(),
            |(_, path)| {
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or_default();
                save_report(&name, bytes)
            },
        )
    }

    /// The kill a prompt starts from — the editor's register.
    ///
    /// "Input fields should work with the system clipboard just as the
    /// editor does. C-k should add to the system clipboard." They had
    /// their own `line_kill` beside the editor's register: two
    /// clipboards in one program, and only one of them mirrored. Text
    /// killed in the capture prompt went somewhere nothing else could
    /// reach, including the buffer two inches below.
    fn shared_kill(&self) -> String {
        self.body.register_text().to_owned()
    }

    /// Put a prompt's kill back into the shared register.
    ///
    /// Empty is not a kill: `C-k` at the end of a line kills nothing,
    /// and letting that overwrite the register would lose what you had
    /// copied to one stray keypress.
    fn keep_shared_kill(&mut self, kill: &str) {
        if !kill.is_empty() && kill != self.body.register_text() {
            self.set_register_from_clipboard(kill);
        }
    }

    /// Remember how wide the outline pane has been dragged.
    ///
    /// "I do have to resize the outline headlines tree view EVERY time
    /// I open closure." The pane was always resizable and the width
    /// was never written down, so every session started at the default
    /// and every session began with the same drag.
    pub const fn set_outline_width(&mut self, width: Option<u32>) {
        self.outline_width = width;
    }

    /// The remembered outline width, if a session has set one.
    #[must_use]
    pub const fn outline_width(&self) -> Option<u32> {
        self.outline_width
    }

    /// Whether the left rail is collapsed to its icons.
    ///
    /// "I would like to make it dockable, in order that just the icons
    /// are visible and none of the text." Labels on until told
    /// otherwise: the rail is how you learn where the panes are.
    #[must_use]
    pub const fn rail_docked(&self) -> bool {
        matches!(self.rail_docked, Some(true))
    }

    /// The setting as it will be written — `None` while no session has
    /// touched it, so an untouched config keeps no line about it.
    #[must_use]
    pub const fn rail_docked_setting(&self) -> Option<bool> {
        self.rail_docked
    }

    /// What `config.org` said, at open.
    pub const fn set_rail_docked(&mut self, docked: Option<bool>) {
        self.rail_docked = docked;
    }

    /// Record where a peer is (what a live round hands us).
    pub fn note_peer_presence(&mut self, peer: &str, block: &str, line: u32) {
        self.sync_mut().note_peer(peer, block, line);
    }

    /// Where every peer is, as of the last round.
    #[must_use]
    pub fn peer_presence(&self) -> &[PeerAt] {
        self.sync.as_ref().map_or(&[], SyncApp::peer_presence)
    }

    /// The peers sitting on one block.
    ///
    /// Asked per row rather than folded into the row list: that list
    /// is memoised against the vault revision, and presence changes
    /// many times a second without the vault changing at all. Baking
    /// it in would either defeat the memo or make every twitch of
    /// somebody else's cursor rebuild every row in the vault.
    #[must_use]
    pub fn peers_on(&self, block: &str) -> Vec<&PeerAt> {
        self.peer_presence()
            .iter()
            .filter(|p| p.block == block)
            .collect()
    }

    /// Where *we* are, for broadcast: the selected row and the line
    /// the caret is on inside it.
    ///
    /// Position only. Presence is session chatter and must never carry
    /// document text — that is what keeps it out of the replica and
    /// out of the undo tree.
    #[must_use]
    pub fn local_presence(&self, shell: &Shell) -> Option<PeerAt> {
        let rows = self.rows_shared(shell);
        let row = rows.get(self.selected)?;
        Some(PeerAt {
            peer: self.sync.as_ref().map_or("local", SyncApp::name).to_owned(),
            block: row.id.clone(),
            line: u32::try_from(self.body.cursor_line_col().0).unwrap_or(0),
        })
    }

    /// The assistant's settings, read fresh from `config.org`.
    ///
    /// Fresh rather than cached because this screen is the one place
    /// whose whole job is to agree with that file — a stale copy here
    /// would show the user their own edit not taking effect.
    #[must_use]
    pub fn settings_rows(&self, shell: &Shell) -> Vec<SettingField> {
        assistant_settings(&Self::vault_config(shell))
    }

    /// The key whose value prompt is open, if one is.
    #[must_use]
    pub const fn editing_setting(&self) -> Option<&'static str> {
        self.editing_setting
    }

    /// What is currently typed into the settings value prompt.
    #[must_use]
    pub fn field_text(&self) -> &str {
        self.field_buf.text()
    }

    /// Which settings row is selected.
    #[must_use]
    pub const fn settings_cursor(&self) -> usize {
        self.settings_cursor
    }

    /// Keys on the settings screen: move, edit, leave.
    fn on_settings_key(&mut self, shell: &Shell, key: &str) {
        let len = self.settings_rows(shell).len();
        match key {
            // `go_home` rather than Browse: a pane opened from inside
            // a buffer has to give the buffer back, which is an
            // invariant the pane-return test holds every pane to.
            "escape" | "q" => self.go_home(),
            "j" | "down" => {
                self.settings_cursor = (self.settings_cursor + 1).min(len.saturating_sub(1));
            }
            "k" | "up" => self.settings_cursor = self.settings_cursor.saturating_sub(1),
            "g" => self.settings_cursor = 0,
            "G" => self.settings_cursor = len.saturating_sub(1),
            "enter" | "i" => self.begin_setting_edit(shell),
            _ => {}
        }
    }

    /// Open the value prompt for the selected setting, prefilled with
    /// what it is set to now.
    fn begin_setting_edit(&mut self, shell: &Shell) {
        let rows = self.settings_rows(shell);
        let Some(field) = rows.get(self.settings_cursor) else {
            return;
        };
        self.editing_setting = Some(field.key);
        self.field_buf.set_text(&field.value);
        self.surface = ModalSurface::Setting;
    }

    /// Write the edited setting into `config.org` and go back to the
    /// list.
    ///
    /// The write is [`closure_config::set_key`], so everything else in
    /// the file — comments, ordering, keys this build does not know —
    /// comes back untouched.
    fn commit_setting(&mut self, shell: &mut Shell) {
        let Some(key) = self.editing_setting.take() else {
            return;
        };
        let value = self.field_buf.text().trim().to_owned();
        let relative = std::path::Path::new(closure_config::CONFIG_FILE);
        let path = shell.vault.root().join(relative);
        let before = std::fs::read_to_string(&path).unwrap_or_default();
        let Ok(after) = closure_config::set_config_key(&before, key, &value) else {
            self.surface = ModalSurface::Settings;
            self.say("config.org could not be rewritten — it is not org this parser reads");
            return;
        };
        let wrote = if path.is_file() {
            let _ = shell.vault.reload_incremental();
            shell
                .vault
                .set_source(&path, &after)
                .map_err(|e| e.to_string())
        } else {
            shell
                .vault
                .create_file(relative, &after)
                .map(|_| ())
                .map_err(|e| e.to_string())
        };
        self.surface = ModalSurface::Settings;
        match wrote {
            Ok(()) => {
                self.invalidate_rows();
                // Read it back rather than trusting the write: the
                // loader refuses some combinations (a provider with no
                // key variable), and a screen that said "saved" over a
                // config.org that will not load is worse than an error.
                match closure_config::Config::from_path(&path) {
                    Ok(_) => self.say(if value.is_empty() {
                        format!("{key} cleared")
                    } else {
                        format!("{key} = {value}")
                    }),
                    Err(e) => self.say(format!("saved, but config.org will not load: {e}")),
                }
            }
            Err(e) => self.say(format!("could not write config.org: {e}")),
        }
    }

    /// The activity rail: every pane of the shell as a clickable
    /// destination, in a fixed order, each with its chord and its live
    /// badge.
    ///
    /// The status bar ([`Self::indicators`]) reports *state*; this
    /// reports *where you can go*, which is not the same list — the
    /// sniffer appears in both, pairing appeared in neither until this
    /// existed, and the outline needs a way home that Esc alone gave
    /// only the keyboard.
    #[must_use]
    pub fn destinations(&self, shell: &Shell) -> Vec<Destination> {
        RAIL.iter()
            .map(|&(id, icon, label, command, surface)| Destination {
                id,
                icon,
                label,
                command,
                chord: self.chord_for(command).map(ToOwned::to_owned),
                surface,
                badge: self.rail_badge(shell, id).map(|n| n.to_string()),
                urgent: self.rail_urgent(id),
                active: self.surface == surface,
            })
            .collect()
    }

    /// What the rail button `id` counts, when a count is worth showing.
    /// `None` — and a zero — render no badge at all rather than a `0`.
    fn rail_badge(&self, shell: &Shell, id: &str) -> Option<usize> {
        let n = match id {
            "backlinks" => self.backlink_rows(shell).len(),
            "peers" => self.sync.as_ref().map_or(0, |s| s.peers().len()),
            "sniffer" => self.sniffer.events().len(),
            "conflicts" => self.conflicts.conflicts().len(),
            _ => 0,
        };
        (n > 0).then_some(n)
    }

    /// Whether the rail button `id` is counting work that waits on the
    /// user rather than mere activity.
    fn rail_urgent(&self, id: &str) -> bool {
        match id {
            "conflicts" => !self.conflicts.conflicts().is_empty(),
            "sniffer" => self
                .sniffer
                .events()
                .iter()
                .any(|e| e.action == Some(FlowAction::Block)),
            _ => false,
        }
    }

    /// The bottom-right status bar: one item per subsystem, each
    /// reporting its live state and carrying the chord that opens it.
    ///
    /// Derived here rather than assembled in a render function, so
    /// every shell shows the same set — and so "is the LLM allowed to
    /// read my screen?" has an answer you can see rather than one
    /// buried in a config file.
    /// The vault's git state, read at most once per vault revision.
    ///
    /// `None` when the vault is not in a repository, which is the
    /// ordinary case and not an error.
    #[must_use]
    pub fn git_state(&self, shell: &Shell) -> Option<closure_store::GitStatus> {
        /// The shortest gap between two `git` runs for the widget.
        ///
        /// The revision key alone is right for correctness and wrong
        /// for cost: every mutation moves the revision, so every
        /// keystroke that changed anything spawned `git rev-parse`,
        /// `git status --porcelain` and friends on the UI thread —
        /// 6.3ms measured on a real vault, against 0.35ms for the edit
        /// itself.
        ///
        /// This widget is ambient: it says roughly where the working
        /// tree stands, nobody acts on it within a second of typing,
        /// and nothing else in the shell reads it. So it is allowed to
        /// be a little behind, and the last answer stands until the
        /// next run is due.
        const MIN_GAP: std::time::Duration = std::time::Duration::from_secs(2);
        let revision = shell.vault.revision();
        {
            let memo = self.git_memo.borrow();
            if let Some(m) = memo.as_ref()
                && (m.revision == revision || m.taken.elapsed() < MIN_GAP)
            {
                return m.state.clone();
            }
        }
        let state = closure_store::git_status(shell.vault.root());
        self.git_reads.set(self.git_reads.get() + 1);
        *self.git_memo.borrow_mut() = Some(GitMemo {
            revision,
            taken: std::time::Instant::now(),
            state: state.clone(),
        });
        state
    }

    /// How many times git has actually been run for the widget.
    #[must_use]
    pub const fn git_reads(&self) -> u64 {
        self.git_reads.get()
    }

    /// Per-line git marks for the file the editor is showing.
    ///
    /// "git (diff) fringes in the editor". Empty when nothing is open,
    /// the vault is not a repository, or the file is unchanged — all
    /// ordinary. Memoised against the vault revision like the vault
    /// widget, and for the same reason: a painter asks this every
    /// frame, and `git diff` per frame would be the microfreeze again.
    #[must_use]
    pub fn body_fringes(&self, shell: &Shell) -> Vec<(usize, closure_store::LineChange)> {
        let Some(path) = self.editing_file(shell) else {
            return Vec::new();
        };
        let revision = shell.vault.revision();
        {
            let memo = self.fringe_memo.borrow();
            if let Some((at, for_path, marks)) = memo.as_ref()
                && *at == revision
                && *for_path == path
            {
                return marks.clone();
            }
        }
        let marks = closure_store::file_diff(shell.vault.root(), &path);
        *self.fringe_memo.borrow_mut() = Some((revision, path, marks.clone()));
        marks
    }

    /// The vault-relative path of whatever the editor has open.
    fn editing_file(&self, shell: &Shell) -> Option<std::path::PathBuf> {
        if !self.surface.is_editor() {
            return None;
        }
        // The whole-file editor knows its path outright; the body
        // editor knows the headline, whose detail carries one.
        let absolute = if let Some(path) = self.file_target.clone() {
            path
        } else {
            std::path::PathBuf::from(&self.detail(shell)?.path)
        };
        // Relative to the vault, because that is how git names a file.
        Some(
            absolute
                .strip_prefix(shell.vault.root())
                .map_or_else(|_| absolute.clone(), std::path::Path::to_path_buf),
        )
    }

    /// The status bar's own row of state.
    ///
    /// Derived here rather than assembled in a render function, so
    /// every shell shows the same set — and so "is the LLM allowed to
    /// read my screen?" has an answer you can see rather than one
    /// buried in a config file.
    #[must_use]
    pub fn indicators(&self, shell: &Shell) -> Vec<Indicator> {
        let item =
            |id, label: String, tooltip: String, level, command: Option<&'static str>| Indicator {
                id,
                label,
                tooltip,
                level,
                command,
                chord: command.and_then(|c| self.chord_for(c).map(ToOwned::to_owned)),
            };
        let headlines = self.rows_shared(shell).len();
        let files = shell.vault.iter().count();
        let flows = self.sniffer.events().len();
        let blocked = self
            .sniffer
            .events()
            .iter()
            .filter(|e| e.action == Some(FlowAction::Block))
            .count();
        let conflicts = self.conflicts.conflicts().len();
        let blocks = self.block_rows(shell).len();
        let mut out = vec![
            item(
                "vault",
                format!("⌂ {headlines}"),
                format!("{headlines} headline(s) across {files} file(s)"),
                IndicatorLevel::Idle,
                Some("list-headlines"),
            ),
            item(
                "blocks",
                format!("⌗ {blocks}"),
                format!("{blocks} source block(s) — run one with eval-block"),
                IndicatorLevel::Idle,
                Some("list-blocks"),
            ),
            item(
                "sniffer",
                format!("⇅ {flows}"),
                if flows == 0 {
                    "network sniffer: no flows captured".to_owned()
                } else {
                    format!("network sniffer: {flows} flow(s), {blocked} blocked")
                },
                if blocked > 0 {
                    IndicatorLevel::Warn
                } else if flows > 0 {
                    IndicatorLevel::Active
                } else {
                    IndicatorLevel::Idle
                },
                Some("sniffer"),
            ),
            item(
                "llm",
                if self.llm_render {
                    "◉ llm".to_owned()
                } else {
                    "○ llm".to_owned()
                },
                if self.llm_render {
                    "LLM render access GRANTED — a model may read the rendered view".to_owned()
                } else {
                    "LLM render access revoked — a model cannot read the rendered view".to_owned()
                },
                if self.llm_render {
                    IndicatorLevel::Active
                } else {
                    IndicatorLevel::Idle
                },
                Some("toggle-llm-render"),
            ),
            item(
                "sync",
                format!("⇄ {conflicts}"),
                if conflicts == 0 {
                    "sync: every field converged".to_owned()
                } else {
                    format!("sync: {conflicts} conflict(s) awaiting a decision")
                },
                if conflicts > 0 {
                    IndicatorLevel::Warn
                } else {
                    IndicatorLevel::Idle
                },
                Some("conflicts"),
            ),
        ];
        out.extend(self.git_indicator(shell));
        out
    }

    /// The vault's git state as one status-bar item, or nothing when
    /// the vault is not in a repository.
    ///
    /// Most vaults are a directory of org files and nothing more; a
    /// widget reading "not a repository" in every one of them would be
    /// noise where the item asked for information.
    fn git_indicator(&self, shell: &Shell) -> Option<Indicator> {
        let git = self.git_state(shell)?;
        Some(Indicator {
            id: "git",
            label: format!("\u{2387} {}", git.summary()),
            tooltip: if git.is_clean() {
                format!("git: {} — nothing to commit", git.summary())
            } else {
                format!(
                    "git: {} staged, {} changed, {} untracked",
                    git.staged, git.modified, git.untracked
                )
            },
            level: if git.is_clean() {
                IndicatorLevel::Idle
            } else {
                IndicatorLevel::Active
            },
            // Read-only "for now", as the item says: there is nothing
            // to run yet, and a widget that looked clickable and did
            // nothing would be worse than one that does not.
            command: None,
            chord: None,
        })
    }

    /// Language of the live org-edit-special session (empty when the
    /// block declared none, or when no session is open).
    #[must_use]
    pub fn special_language(&self) -> &str {
        self.special.as_ref().map_or("", |(_, lang)| lang.as_str())
    }

    /// Open the source block under the cursor on its own
    /// (org-edit-special).
    ///
    /// From the body editor the block is the one containing the
    /// cursor; from the Blocks list it is the row under it. Anywhere
    /// else there is no block to edit, and saying so beats opening an
    /// empty editor.
    fn begin_edit_special(&mut self, shell: &Shell) {
        match self.surface {
            ModalSurface::EditBody => self.begin_special_from_body(),
            ModalSurface::Blocks => self.begin_special_from_list(shell),
            _ => self.say("edit-special: open a source block first (g e lists them)"),
        }
    }

    /// org-edit-special from the body editor: the block is inside the
    /// buffer, so the session remembers the buffer and the range to
    /// splice back into.
    fn begin_special_from_body(&mut self) {
        let buffer = self.body.text().to_owned();
        let cursor = self.body.cursor_byte();
        let Some((range, lang)) = enclosing_src_block(&buffer, cursor) else {
            self.say("edit-special: the cursor is not inside a source block");
            return;
        };
        let content = buffer[range.clone()].to_owned();
        // Both entry points report the same language name: the Blocks
        // list already shows the normalised one, and a session that
        // called it `sh` from one door and `shell` from the other
        // would be reporting the door, not the block.
        self.special = Some((
            SpecialOrigin::Body {
                range,
                buffer,
                cursor,
            },
            normalise_lang(&lang),
        ));
        self.open_special(content);
    }

    /// org-edit-special from the Blocks list: the block belongs to a
    /// file, so the session remembers which one.
    fn begin_special_from_list(&mut self, shell: &Shell) {
        let rows = self.block_rows(shell);
        // The block list's own cursor — see [`Self::picker_cursor`].
        let Some(BlockRow {
            file: path, lang, ..
        }) = rows.get(self.pane_cursor).cloned()
        else {
            self.say("edit-special: no source blocks in this vault");
            return;
        };
        let index = rows[..self.pane_cursor]
            .iter()
            .filter(|b| b.file == path)
            .count();
        let path = std::path::PathBuf::from(&path);
        let Some(content) = shell
            .vault
            .document(&path)
            .and_then(|doc| doc.org().code_blocks().get(index).copied())
            .and_then(|n| n.as_code_block().map(|cb| cb.content.to_owned()))
        else {
            self.say("edit-special: could not read that block");
            return;
        };
        self.special = Some((SpecialOrigin::File { path, index }, lang));
        self.open_special(content);
    }

    /// Which shape of the shell is showing ([`ViewMode`]).
    #[must_use]
    pub const fn view_mode(&self) -> ViewMode {
        self.view
    }

    /// Put the shell in `view`, opening or closing the file buffer to
    /// match.
    ///
    /// [`ModalApp::new`] has no vault, so a config that asks to start in
    /// the editor view cannot be honoured at construction; the shells
    /// call this once they have one.
    pub fn set_view(&mut self, view: ViewMode, shell: &Shell) {
        self.view = view;
        match (view, self.surface) {
            (ViewMode::Editor, ModalSurface::Browse) => self.open_file_buffer(shell),
            (ViewMode::Clickable, ModalSurface::EditFile) => self.close_file_buffer(),
            _ => {}
        }
    }

    /// Open the selected row's file as one full-window buffer.
    ///
    /// The editor view *is* this: no rows, no detail pane — the file,
    /// the way `find-file` gives it to you. An empty vault has no file
    /// to open, so it stays where it is and says so.
    fn open_file_buffer(&mut self, shell: &Shell) {
        let Some(row) = self.rows_shared(shell).get(self.selected).cloned() else {
            self.say("no file to open — the vault is empty");
            self.view = ViewMode::Clickable;
            return;
        };
        let id = closure_core::BlockId::from_existing(&row.id);
        let Some((_, path)) = shell.vault.find_by_id(&id) else {
            self.say("that headline has no file on disk");
            self.view = ViewMode::Clickable;
            return;
        };
        let path = path.to_path_buf();
        self.open_file_path(shell, &path);
    }

    /// Leave the file buffer without writing it.
    fn close_file_buffer(&mut self) {
        // The buffer is gone; a pane must not offer to put it back.
        self.pane_return = None;
        self.file_target = None;
        self.body.clear();
        self.body_baseline.clear();
        self.surface = ModalSurface::Browse;
    }

    /// Write the file buffer back to its file, staying in it — `:w`,
    /// not `:wq`.
    fn commit_file_buffer(&mut self, shell: &mut Shell) {
        let Some(path) = self.file_target.clone() else {
            return;
        };
        let source = self.body.text().to_owned();
        match shell.vault.set_source(&path, &source) {
            Ok(()) => {
                self.body_baseline = source;
                self.invalidate_rows();
                self.selected = self
                    .selected
                    .min(self.rows_shared(shell).len().saturating_sub(1));
                self.say(format!("wrote {}", path.display()));
            }
            Err(e) => self.status = format!("save failed: {e}"),
        }
    }

    // ---- Q1: buffers, jumps, recents ---------------------------------

    /// Every buffer this session has open, most recently visited first.
    ///
    /// The shell held exactly one buffer before this: opening a second
    /// note put the first in a stash with no way back to it except
    /// finding its headline again. The list is derived, never stored
    /// twice — the names come from the vault, the dirty bit from the
    /// same comparison [`Self::body_dirty`] makes.
    #[must_use]
    pub fn buffer_rows(&self, shell: &Shell) -> Vec<BufferRow> {
        let mut open: Vec<&OpenBuffer> = self.buffers.iter().collect();
        // Most recent first, and a stable tiebreak so two buffers that
        // somehow share a visit never swap between frames.
        open.sort_by_key(|b| std::cmp::Reverse(b.seq));
        let filter = self.picker_filter().to_owned();
        open.into_iter()
            .enumerate()
            .map(|(i, b)| {
                let name = Self::buffer_label(shell, &b.target);
                BufferRow {
                    matches_filter: filter.is_empty()
                        || closure_query::fuzzy_score(&filter, &name).is_some(),
                    name,
                    target: b.target.clone(),
                    dirty: self.buffer_is_dirty(&b.target),
                    current: i == 0,
                }
            })
            .collect()
    }

    /// The vault's files, the ones recent sessions were in first.
    ///
    /// There was no way to open a file that had no headline you could
    /// find — a new note, an `#+INCLUDE`d fragment, a file whose
    /// headings you did not remember. This is that way.
    #[must_use]
    pub fn file_rows(&self, shell: &Shell) -> Vec<FileRow> {
        let filter = self.picker_filter();
        // Named the way the user names them — relative to the vault —
        // and opened by the absolute path the vault holds.
        let all: Vec<std::path::PathBuf> = shell
            .vault
            .iter()
            .map(|(p, _)| vault_relative(shell, p))
            .collect();
        let mut ordered: Vec<(std::path::PathBuf, bool)> = Vec::with_capacity(all.len());
        for recent in &self.recent_files {
            // A remembered path the vault no longer has is skipped, not
            // an error: a vault is edited elsewhere too.
            if all.contains(recent) {
                ordered.push((recent.clone(), true));
            }
        }
        for p in all {
            if !ordered.iter().any(|(q, _)| *q == p) {
                ordered.push((p, false));
            }
        }
        ordered
            .into_iter()
            .map(|(path, recent)| {
                let name = path.display().to_string();
                FileRow {
                    matches_filter: filter.is_empty()
                        || closure_query::fuzzy_score(filter, &name).is_some(),
                    name,
                    path,
                    recent,
                }
            })
            .collect()
    }

    /// Open the file on row `i` of [`Self::file_rows`] — the mouse path
    /// into the file picker.
    pub fn file_click(&mut self, shell: &Shell, i: usize) {
        let Some(row) = self.file_rows(shell).into_iter().nth(i) else {
            return;
        };
        self.open_buffer(shell, &BufferRef::File(row.path), true);
    }

    /// Open the buffer on row `i` of [`Self::buffer_rows`] — the mouse
    /// path into the buffer list (and the tab strip).
    pub fn buffer_click(&mut self, shell: &Shell, i: usize) {
        let Some(row) = self.buffer_rows(shell).into_iter().nth(i) else {
            return;
        };
        self.open_buffer(shell, &row.target, true);
    }

    /// The live filter the two pickers share, empty everywhere else so
    /// a stale query can never filter the outline.
    fn picker_filter(&self) -> &str {
        if matches!(self.surface, ModalSurface::Buffers | ModalSurface::Files) {
            self.query.text()
        } else {
            ""
        }
    }

    /// What to call a buffer: the headline's title, or the file's path.
    fn buffer_label(shell: &Shell, target: &BufferRef) -> String {
        match target {
            BufferRef::Body(id) => {
                let bid = closure_core::BlockId::from_existing(id);
                shell
                    .vault
                    .find_by_id(&bid)
                    .map_or_else(|| id.clone(), |(h, _)| h.title().to_owned())
            }
            BufferRef::File(path) => vault_relative(shell, path).display().to_string(),
        }
    }

    /// Whether a buffer holds text the vault does not — the on-screen
    /// one by comparison, the others by what they left in the stash.
    fn buffer_is_dirty(&self, target: &BufferRef) -> bool {
        match target {
            BufferRef::Body(id) => {
                if self.edit_target.as_ref() == Some(id) && self.surface == ModalSurface::EditBody {
                    self.body.text() != self.body_baseline
                } else {
                    self.body_stash.contains_key(id)
                }
            }
            BufferRef::File(path) => {
                self.file_target.as_ref() == Some(path)
                    && self.surface == ModalSurface::EditFile
                    && self.body.text() != self.body_baseline
            }
        }
    }

    /// Put `target` at the top of the buffer list, adding it if this is
    /// the first visit.
    fn touch_buffer(&mut self, target: BufferRef) {
        self.buf_seq += 1;
        let seq = self.buf_seq;
        if let Some(existing) = self.buffers.iter_mut().find(|b| b.target == target) {
            existing.seq = seq;
        } else {
            self.buffers.push(OpenBuffer { target, seq });
        }
    }

    /// The buffer on screen, as the list understands it: the most
    /// recently visited one.
    fn current_buffer(&self) -> Option<BufferRef> {
        self.buffers
            .iter()
            .max_by_key(|b| b.seq)
            .map(|b| b.target.clone())
    }

    /// The one before it — what `C-^` toggles with.
    fn alternate_buffer(&self) -> Option<BufferRef> {
        let mut by_recency: Vec<&OpenBuffer> = self.buffers.iter().collect();
        by_recency.sort_by_key(|b| std::cmp::Reverse(b.seq));
        by_recency.get(1).map(|b| b.target.clone())
    }

    /// Where we are now, in the terms the jumplist stores.
    fn current_place(&self, shell: &Shell) -> JumpPoint {
        JumpPoint {
            buffer: self.current_buffer(),
            row: self.selected_row_id(shell),
            cursor: self.body.cursor_byte(),
        }
    }

    /// Record the place a non-local move is leaving.
    ///
    /// Vim's rule: a fresh jump drops whatever was ahead of the cursor
    /// in the list, because you cannot go forward to a future you just
    /// replaced. A repeat of the same place is not a jump.
    fn push_jump(&mut self, place: JumpPoint) {
        self.jumps.truncate(self.jump_at);
        if self.jumps.last() == Some(&place) {
            self.jump_at = self.jumps.len();
            return;
        }
        self.jumps.push(place);
        self.jump_at = self.jumps.len();
    }

    /// Go to a recorded place without recording one.
    fn goto_place(&mut self, shell: &Shell, place: &JumpPoint) {
        if let Some(target) = place.buffer.clone() {
            self.open_buffer(shell, &target, false);
            let len = self.body.text().len();
            self.body.set_cursor_byte(place.cursor.min(len));
        } else {
            if self.surface.is_editor() {
                self.leave_buffer();
            }
            self.surface = ModalSurface::Browse;
        }
        if let Some(id) = &place.row {
            let id = id.clone();
            self.select_by_id(shell, &id);
        }
    }

    /// Open a buffer, recording the place being left when `record` is
    /// set (every user-facing path does; the jumplist's own navigation
    /// does not, or going back would be a move you could go back from).
    fn open_buffer(&mut self, shell: &Shell, target: &BufferRef, record: bool) {
        if record {
            let place = self.current_place(shell);
            self.push_jump(place);
        }
        match target {
            BufferRef::Body(id) => {
                let id = id.clone();
                self.open_body_by_id(shell, &id);
            }
            BufferRef::File(path) => {
                let path = path.clone();
                self.open_file_path(shell, &path);
            }
        }
    }

    /// Leave whatever buffer is open: remember its cursor, put any
    /// unsaved text aside against its own name.
    fn leave_buffer(&mut self) {
        self.remember_body_cursor();
        self.stash_body();
    }

    /// Walk the open-buffer list by `delta`, wrapping — `:bnext` and
    /// `:bprev`, which walk the order buffers were *opened* in, not the
    /// order they were last looked at.
    fn cycle_buffer(&mut self, shell: &Shell, delta: isize) {
        let len = self.buffers.len();
        if len == 0 {
            self.say("no buffers open — open a note first");
            return;
        }
        let current = self.current_buffer();
        let at = current
            .as_ref()
            .and_then(|c| self.buffers.iter().position(|b| b.target == *c))
            .unwrap_or(0);
        let len_i = isize::try_from(len).unwrap_or(1);
        let at_i = isize::try_from(at).unwrap_or(0);
        let next = usize::try_from((at_i + delta).rem_euclid(len_i)).unwrap_or(0);
        let target = self.buffers[next].target.clone();
        self.open_buffer(shell, &target, true);
    }

    /// Close the buffer on screen. Unsaved text takes a second, louder
    /// chord (`buffer-close-force`) — one keystroke may not be able to
    /// throw away something you typed.
    fn close_current_buffer(&mut self, shell: &Shell, force: bool) {
        let Some(target) = self.current_buffer() else {
            self.say("no buffer to close");
            return;
        };
        if !force && self.buffer_is_dirty(&target) {
            self.say("unsaved edits — :w saves, buffer-close-force discards");
            return;
        }
        if force {
            if let BufferRef::Body(id) = &target {
                self.body_stash.remove(id);
            }
        } else {
            self.leave_buffer();
        }
        self.buffers.retain(|b| b.target != target);
        // Jumping back into a buffer that is gone is worse than
        // forgetting it was ever there.
        self.jumps.retain(|j| j.buffer.as_ref() != Some(&target));
        self.jump_at = self.jumps.len();
        if let Some(next) = self.current_buffer() {
            self.open_buffer(shell, &next, false);
        } else {
            self.edit_target = None;
            self.file_target = None;
            self.body.clear();
            self.body_baseline.clear();
            self.surface = ModalSurface::Browse;
            self.say("no buffers left");
        }
    }

    /// Open a headline's body as the buffer, by id.
    ///
    /// Addressed by id rather than by "whatever row is selected", so
    /// the buffer list, the jumplist and the outline can all open the
    /// same note the same way. Whatever was open first has its cursor
    /// remembered and its unsaved text put aside against its own name —
    /// neither is going in the vault unasked.
    fn open_body_by_id(&mut self, shell: &Shell, id: &str) {
        let bid = closure_core::BlockId::from_existing(id);
        let Some(vault_body) = shell
            .vault
            .find_by_id(&bid)
            .map(|(h, _)| closure_org::unescape_body(h.body_text()))
        else {
            self.say(format!("that note is no longer in the vault: {id}"));
            return;
        };
        // The buffer is the whole subtree: the headline's own prose,
        // then its children verbatim. Showing only the prose is what
        // made a headline typed into a note disappear the moment it was
        // saved — it had become a child, and children were not shown.
        let vault_body = match shell.children_source(&bid) {
            Ok(kids) if !kids.is_empty() => {
                let mut whole = vault_body;
                if !whole.is_empty() && !whole.ends_with('\n') {
                    whole.push('\n');
                }
                whole.push_str(&kids);
                whole
            }
            _ => vault_body,
        };
        let file = shell.vault.find_by_id(&bid).map(|(_, p)| p.to_path_buf());
        self.leave_buffer();
        let resume = self.body_cursors.get(id).copied();
        let restored = self.body_stash.remove(id);
        let came_back = restored.is_some();
        self.last_edited = Some(id.to_owned());
        self.edit_target = Some(id.to_owned());
        self.file_target = None;
        let (body, baseline) = restored.unwrap_or_else(|| (vault_body.clone(), vault_body));
        self.body_baseline = baseline;
        let len = body.len();
        self.load_body(body);
        // Opening a note you were just in used to start at byte zero, so
        // any edit deeper in it meant navigating back down every time. A
        // body can shrink between visits, so the remembered offset is
        // clamped rather than trusted (I5).
        if let Some(at) = resume {
            self.body.set_cursor_byte(at.min(len));
        }
        self.surface = ModalSurface::EditBody;
        self.select_by_id(shell, id);
        self.touch_buffer(BufferRef::Body(id.to_owned()));
        if let Some(path) = file {
            self.remember_recent_file(&path);
        }
        self.say(if came_back {
            // Said out loud, because the buffer disagrees with the file
            // and the user did not do it just now.
            "unsaved edits restored — :w saves, :q! discards".to_owned()
        } else if self.modal_editing() {
            "edit body — NORMAL, i to insert, C-s saves, :q closes".to_owned()
        } else {
            "edit body — C-c C-c saves & closes, C-s saves".to_owned()
        });
    }

    /// Open a file as one full-window buffer, by path — what the file
    /// picker and the buffer list open a [`BufferRef::File`] with.
    fn open_file_path(&mut self, shell: &Shell, path: &std::path::Path) {
        // A path from the picker is vault-relative (that is how it is
        // shown and how `config.org` keeps it); a path from the vault
        // is absolute. Both name the same file, so both open it.
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            shell.vault.root().join(path)
        };
        let path = &absolute;
        let Some(source) = shell
            .vault
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, doc)| doc.source())
        else {
            self.say(format!("no such file in this vault: {}", path.display()));
            return;
        };
        self.leave_buffer();
        self.edit_target = None;
        self.body_baseline.clone_from(&source);
        self.load_body(source);
        self.file_target = Some(path.clone());
        self.surface = ModalSurface::EditFile;
        self.view = ViewMode::Editor;
        self.touch_buffer(BufferRef::File(path.clone()));
        let shown = vault_relative(shell, path);
        self.remember_recent_file(&shown);
        self.say(if self.modal_editing() {
            format!("{} — NORMAL, i to insert, C-s saves", shown.display())
        } else {
            format!("{} — C-s saves, C-c C-c saves & closes", shown.display())
        });
    }

    /// The two pickers' keys: type to filter, arrows (and `C-n`/`C-p`'s
    /// list equivalents) to walk, Enter to open, Esc to go back.
    ///
    /// One handler for both because they are the same interaction over
    /// different rows — a second copy would be a second set of bugs.
    fn on_picker_key(
        &mut self,
        shell: &Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
        kind: PickerKind,
    ) {
        let matches: Vec<usize> = match kind {
            PickerKind::Buffers => self
                .buffer_rows(shell)
                .iter()
                .enumerate()
                .filter(|(_, r)| r.matches_filter)
                .map(|(i, _)| i)
                .collect(),
            PickerKind::Files => self
                .file_rows(shell)
                .iter()
                .enumerate()
                .filter(|(_, r)| r.matches_filter)
                .map(|(i, _)| i)
                .collect(),
        };
        match key {
            "escape" => {
                self.query.clear();
                self.selected = 0;
                self.go_home();
            }
            "enter" => {
                let Some(&row) = matches.get(self.selected) else {
                    self.say("nothing matches");
                    return;
                };
                self.query.clear();
                self.selected = 0;
                match kind {
                    PickerKind::Buffers => self.buffer_click(shell, row),
                    PickerKind::Files => self.file_click(shell, row),
                }
            }
            _ => {
                self.filter_key(key, ctrl, alt, text, matches.len().saturating_sub(1));
            }
        }
    }

    /// Keys for the list pickers — headlines, source blocks, the undo
    /// tree.
    ///
    /// They had a pane each with a bare `j`/`k` in it and no way to
    /// narrow; they are pickers now, so they answer exactly what the
    /// other pickers answer. The cursor a picker moves is its own
    /// ([`Self::picker_cursor`]), which for the undo tree is not the
    /// outline selection — that one still points at the file whose
    /// history is being walked.
    /// The rows of the directory `find-file` is looking at.
    ///
    /// Directories first: you are usually narrowing *towards* one, and
    /// a list that mixes them is a list you have to read rather than
    /// skim. `..` is a row rather than a chord, because a picker whose
    /// only way back is a key you have to know is a picker you get
    /// stuck in.
    fn find_file_rows(&self, shell: &Shell) -> Vec<PickRow> {
        let here = shell.vault.root().join(&self.find_dir);
        let mut dirs: Vec<PickRow> = Vec::new();
        let mut files: Vec<PickRow> = Vec::new();
        if self.find_dir.components().next().is_some() {
            dirs.push(PickRow {
                label: "../".to_owned(),
                detail: self.find_dir.display().to_string(),
                trailing: "up".to_owned(),
                matches: Vec::new(),
                current: false,
            });
        }
        if let Ok(entries) = std::fs::read_dir(&here) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                // A dotfile in a vault is somebody's `.git`, not a note.
                if name.starts_with('.') {
                    continue;
                }
                let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
                if is_dir {
                    dirs.push(PickRow {
                        label: format!("{name}/"),
                        detail: String::new(),
                        trailing: "dir".to_owned(),
                        matches: Vec::new(),
                        current: false,
                    });
                } else if std::path::Path::new(&name)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("org"))
                {
                    files.push(PickRow {
                        label: name,
                        detail: String::new(),
                        trailing: "org".to_owned(),
                        matches: Vec::new(),
                        current: false,
                    });
                }
            }
        }
        dirs.sort_by(|a, b| a.label.cmp(&b.label));
        files.sort_by(|a, b| a.label.cmp(&b.label));
        dirs.extend(files);
        Self::narrow(self.prompt_text().unwrap_or_default(), dirs)
    }

    /// `find-file`'s Enter: walk in, open, or make.
    fn on_find_file_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        if key != "enter" {
            self.on_picker_list_key(shell, key, ctrl, alt, text);
            return;
        }
        let typed = self.query.text().trim().to_owned();
        let rows = self.find_file_rows(shell);
        let picked = rows.get(self.selected.min(rows.len().saturating_sub(1)));
        // A row under the cursor wins over the text: the text is how
        // you narrowed to it. Only when nothing matches is the text a
        // name for something new.
        match picked {
            Some(row) if row.trailing == "up" => {
                self.find_dir.pop();
                self.query.clear();
                self.selected = 0;
            }
            Some(row) if row.trailing == "dir" => {
                self.find_dir.push(row.label.trim_end_matches('/'));
                self.query.clear();
                self.selected = 0;
            }
            Some(row) => {
                let path = self.find_dir.join(&row.label);
                self.query.clear();
                self.open_file_path(shell, &path);
            }
            None if !typed.is_empty() => self.create_and_open(shell, &typed),
            None => self.say("nothing here — type a name to make one"),
        }
    }

    /// Make the file `typed` names, with the directories it needs, and
    /// open it.
    ///
    /// One gesture, the way Doom's `find-file` is: typing
    /// `notes/2026/q3.org` should not take three steps. A new file is
    /// given a headline and an id, because an empty file is not a note
    /// — the outline would have nothing to select and the editor
    /// nothing to open.
    /// `open-config`: put config.org on screen, writing it first if
    /// the vault has not got one.
    ///
    /// "command/function for jump to or generate config.org (if not
    /// already existent)". Both verbs, because they are one intention:
    /// you want to be looking at your configuration, and whether the
    /// file exists yet is closure's problem rather than yours.
    ///
    /// The generated file is [`closure_config::Config::default_org`] —
    /// every key rendered from the defaults, the ones without a default
    /// commented out — so it cannot drift from the schema the way a
    /// hand-written sample would.
    fn open_config(&mut self, shell: &mut Shell) {
        let relative = std::path::Path::new(closure_config::CONFIG_FILE);
        let mut created = false;
        if shell.vault.root().join(relative).is_file() {
            // On disk but possibly not in the index — a config written
            // by hand, or by another closure, while this one was
            // running. Opening it has to work either way.
            let _ = shell.vault.reload_incremental();
        } else {
            match shell
                .vault
                .create_file(relative, &closure_config::Config::default_org())
            {
                Ok(_) => created = true,
                Err(e) => {
                    self.say(format!("could not create {}: {e}", relative.display()));
                    return;
                }
            }
            self.invalidate_rows();
        }
        self.open_file_path(shell, relative);
        if created {
            // *After* opening: the editor sets its own status as it
            // comes up, so saying this first says it to nobody. A file
            // appearing is worth a word; opening one that was already
            // there is not a surprise.
            self.say(format!("created {}", relative.display()));
        }
    }

    fn create_and_open(&mut self, shell: &mut Shell, typed: &str) {
        // A vault is a directory, and a picker that can be talked into
        // writing above its root is a file manager with somebody's home
        // in reach. `..` and an absolute path are the two ways to ask.
        let asked = std::path::Path::new(typed);
        let escapes = asked.is_absolute()
            || asked.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::RootDir
                )
            });
        if escapes {
            self.say(format!("{typed}: a note lives inside the vault"));
            return;
        }
        let mut relative = self.find_dir.join(typed);
        if relative.extension().is_none() {
            relative.set_extension("org");
        }
        let title = relative.file_stem().map_or_else(
            || "Untitled".to_owned(),
            |s| s.to_string_lossy().into_owned(),
        );
        let source = format!(
            "* {title}\n:PROPERTIES:\n:ID: {}\n:END:\n",
            closure_core::BlockId::fresh()
        );
        match shell.vault.create_file(&relative, &source) {
            Ok(_) => {
                self.query.clear();
                self.invalidate_rows();
                self.say(format!("created {}", relative.display()));
                self.open_file_path(shell, &relative);
            }
            Err(e) => self.say(format!("could not create {}: {e}", relative.display())),
        }
    }

    fn on_picker_list_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        match key {
            "escape" => {
                self.query.clear();
                self.block_out = None;
                if Self::picker_has_own_cursor(self.surface) {
                    self.pane_cursor = 0;
                } else if self.surface != ModalSurface::UndoHistory {
                    self.selected = 0;
                }
                self.go_home();
            }
            "enter" => self.pick_current(shell),
            _ => {
                // Output shown beside a block that did not produce it is
                // a lie, and both moving the cursor and narrowing the
                // list change which block that is.
                self.block_out = None;
                let len = self.picker_len(shell);
                if self.surface == ModalSurface::UndoHistory {
                    if let Some(step) = list_step(key, ctrl) {
                        self.hist_cursor = step_wrapping(self.hist_cursor, len, step);
                        return;
                    }
                    let before = self.query.text().to_owned();
                    let mut kill = self.shared_kill();
                    line_key(&mut self.query, &mut kill, key, ctrl, alt, text);
                    self.keep_shared_kill(&kill);
                    if self.query.text() != before {
                        self.hist_cursor = 0;
                    }
                    return;
                }
                self.filter_key(key, ctrl, alt, text, len.saturating_sub(1));
            }
        }
    }

    /// The keys a list filter and its field share.
    ///
    /// The arrows and `C-n`/`C-p`/`C-j`/`C-k` walk the results — in a
    /// filter those have meant "next match" for as long as filters have
    /// existed — and everything the list does not claim goes to the
    /// field, which is the same [`LineInput`] every other prompt uses.
    /// A key that changed the text puts the cursor back on the first
    /// result, because the old index belonged to the old list.
    fn filter_key(&mut self, key: &str, ctrl: bool, alt: bool, text: Option<char>, last: usize) {
        // Whose cursor this list is walking — see [`Self::picker_cursor`].
        let own = Self::picker_has_own_cursor(self.surface);
        if let Some(step) = list_step(key, ctrl) {
            if own {
                self.pane_cursor = step_wrapping(self.pane_cursor, last + 1, step);
                return;
            }
            self.selected = step_wrapping(self.selected, last + 1, step);
            return;
        }
        let before = self.query.text().to_owned();
        let mut kill = self.shared_kill();
        line_key(&mut self.query, &mut kill, key, ctrl, alt, text);
        self.keep_shared_kill(&kill);
        if self.query.text() != before {
            // Narrowing restarts the list at the top — the list this
            // surface is actually walking.
            if own {
                self.pane_cursor = 0;
            } else {
                self.selected = 0;
            }
        }
    }

    // ---- Q3-V6: the tag picker -------------------------------------

    /// Every tag in the vault, ticked where the selected headline
    /// already carries it, filtered as you type.
    ///
    /// A typed tag that matches nothing is still offered as itself:
    /// the picker is for spelling tags you already use consistently,
    /// not for refusing new ones.
    #[must_use]
    pub fn tag_rows(&self, shell: &Shell) -> Vec<TagRow> {
        let filter = if self.surface == ModalSurface::TagPick {
            self.query.text()
        } else {
            ""
        };
        let mut rows: Vec<TagRow> = shell
            .vault
            .all_tags()
            .into_iter()
            .map(|name| TagRow {
                matches_filter: filter.is_empty()
                    || closure_query::fuzzy_score(filter, &name).is_some(),
                on: self.tag_draft.contains(&name),
                name,
            })
            .collect();
        // The tags the headline has but the vault does not know about
        // — because they were only just ticked — are rows too.
        for name in &self.tag_draft {
            if !rows.iter().any(|r| r.name == *name) {
                rows.push(TagRow {
                    matches_filter: filter.is_empty()
                        || closure_query::fuzzy_score(filter, name).is_some(),
                    on: true,
                    name: name.clone(),
                });
            }
        }
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        rows
    }

    /// Open the tag picker on the selected headline.
    fn open_tag_picker(&mut self, shell: &Shell) {
        let selected = self
            .selection_active
            .then(|| self.selected_row_id(shell))
            .flatten();
        let Some(id) = selected else {
            self.say("nothing selected — put the cursor on a headline first");
            return;
        };
        let bid = closure_core::BlockId::from_existing(&id);
        self.tag_draft = shell
            .vault
            .find_by_id(&bid)
            .map(|(h, _)| h.tags().to_vec())
            .unwrap_or_default();
        self.tag_target = Some(id);
        self.surface = ModalSurface::TagPick;
        self.query.clear();
        self.selected = 0;
        self.say("tags — type to filter · SPC toggles · RET writes · Esc cancels");
    }

    /// Tick or untick `name` in the draft — the click path.
    pub fn tag_toggle(&mut self, name: &str) {
        if let Some(at) = self.tag_draft.iter().position(|t| t == name) {
            self.tag_draft.remove(at);
        } else {
            self.tag_draft.push(name.to_owned());
        }
    }

    /// Write the draft to the headline and close the picker.
    fn commit_tag_picker(&mut self, shell: &mut Shell) {
        let Some(id) = self.tag_target.take() else {
            return;
        };
        let bid = closure_core::BlockId::from_existing(&id);
        let tags = self.tag_draft.clone();
        match shell.set_tags(&bid, &tags) {
            Ok(()) => {
                self.invalidate_rows();
                self.say(if tags.is_empty() {
                    "tags cleared".to_owned()
                } else {
                    format!("tags: {}", tags.join(" "))
                });
            }
            Err(e) => self.status = format!("could not set the tags: {e}"),
        }
        self.tag_draft.clear();
        self.query.clear();
        self.surface = ModalSurface::Browse;
    }

    /// The tag picker's keys.
    fn on_tagpick_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        let matches: Vec<String> = self
            .tag_rows(shell)
            .into_iter()
            .filter(|r| r.matches_filter)
            .map(|r| r.name)
            .collect();
        match key {
            "escape" => {
                self.tag_target = None;
                self.tag_draft.clear();
                self.query.clear();
                self.selected = 0;
                self.go_home();
                self.say("tags left as they were");
            }
            "enter" => self.commit_tag_picker(shell),
            " " | "space" => {
                // Space ticks what the cursor is on; with nothing
                // matching, it ticks what was typed — that is how a new
                // tag gets into a vault at all.
                let target = matches.get(self.selected).cloned().or_else(|| {
                    let typed = self.query.text().trim().to_owned();
                    (!typed.is_empty()).then_some(typed)
                });
                if let Some(name) = target {
                    self.tag_toggle(&name);
                    self.query.clear();
                    self.selected = 0;
                }
            }
            _ => {
                self.filter_key(key, ctrl, alt, text, matches.len().saturating_sub(1));
            }
        }
    }

    // ---- Q3-V3: the clock ------------------------------------------

    /// Tell the core what time it is (`YYYY-MM-DD HH:MM`), which is
    /// also what day it is.
    ///
    /// Same rule as [`Self::set_today`]: the shells own the clock so
    /// the core stays reproducible.
    pub fn set_now(&mut self, stamp: &str) {
        let trimmed = stamp.trim();
        let (date, _) = trimmed.split_once(' ').unwrap_or((trimmed, ""));
        if parse_ymd(date).is_some() {
            trimmed.clone_into(&mut self.now);
            date.clone_into(&mut self.today);
        }
    }

    /// What the core believes the time is.
    #[must_use]
    pub fn now(&self) -> &str {
        &self.now
    }

    /// The running clock as a status line says it: the note and how
    /// long it has been going. `None` when nothing is clocked in.
    #[must_use]
    pub fn running_clock(&self, shell: &Shell) -> Option<String> {
        let (id, started) = shell.vault.running_clock()?;
        let bid = closure_core::BlockId::from_existing(&id);
        let title = shell
            .vault
            .find_by_id(&bid)
            .map_or_else(|| id.clone(), |(h, _)| h.title().to_owned());
        Some(format!("⏱ {title}  (since {started})"))
    }

    /// Start, stop or drop the clock on the selected headline.
    fn clock(&mut self, shell: &mut Shell, verb: &str) {
        // `clock-out` and `clock-cancel` act on whatever is running,
        // wherever it is: you stop the clock you started, not the row
        // the cursor happens to be on.
        let running = shell.vault.running_clock().map(|(id, _)| id);
        let selected = self
            .selection_active
            .then(|| self.selected_row_id(shell))
            .flatten();
        let target = match verb {
            "clock-in" => selected,
            _ => running.or(selected),
        };
        let Some(id) = target else {
            self.say("nothing selected, and no clock running");
            return;
        };
        let bid = closure_core::BlockId::from_existing(&id);
        let now = self.now.clone();
        let result = match verb {
            "clock-in" => shell.vault.clock_in(&bid, &now),
            "clock-out" => shell.vault.clock_out(&bid, &now),
            _ => shell.vault.clock_cancel(&bid),
        };
        match result {
            Ok(()) => {
                self.invalidate_rows();
                self.say(match verb {
                    "clock-in" => "clocked in".to_owned(),
                    "clock-out" => "clocked out".to_owned(),
                    _ => "clock cancelled".to_owned(),
                });
            }
            Err(e) => self.status = format!("{verb}: {e}"),
        }
    }

    /// Jump the outline to the headline whose clock is running.
    fn clock_goto(&mut self, shell: &Shell) {
        let Some((id, _)) = shell.vault.running_clock() else {
            self.say("no clock is running");
            return;
        };
        self.select_by_id(shell, &id);
        self.surface = ModalSurface::Browse;
    }

    /// Clocked time per headline, longest first — `(title, minutes)`.
    #[must_use]
    pub fn clock_report(shell: &Shell) -> Vec<(String, u64)> {
        shell.vault.clock_minutes()
    }

    // ---- Q3-V1/V2: refile and archive ------------------------------

    /// Every headline that could take the selected subtree, filtered as
    /// you type.
    ///
    /// The subtree being filed is not among them, and neither is
    /// anything inside it: those are the two moves that would lose the
    /// tree, and the store refuses them anyway — but a picker that
    /// offers a target it cannot use is a picker that lies.
    #[must_use]
    pub fn refile_rows(&self, shell: &Shell) -> Vec<RefileRow> {
        let moving = self.refile_source.clone();
        let filter = if self.surface == ModalSurface::Refile {
            self.query.text()
        } else {
            ""
        };
        let inside = moving.as_ref().map(|id| Self::subtree_ids(shell, id));
        shell
            .vault
            .iter()
            .flat_map(|(path, doc)| {
                let shown = vault_relative(shell, path).display().to_string();
                doc.all_headlines().map(move |h| (shown.clone(), h))
            })
            .filter(|(_, h)| {
                inside
                    .as_ref()
                    .is_none_or(|ids| !ids.contains(&h.id().to_string()))
            })
            .map(|(path, h)| {
                let title = h.title().to_owned();
                RefileRow {
                    matches_filter: filter.is_empty()
                        || closure_query::fuzzy_score(filter, &format!("{title} {path}")).is_some(),
                    title,
                    id: h.id().to_string(),
                    path,
                    level: h.level(),
                }
            })
            .collect()
    }

    /// The ids of a subtree: the headline itself and everything under
    /// it, by document order and level.
    fn subtree_ids(shell: &Shell, id: &str) -> Vec<String> {
        let bid = closure_core::BlockId::from_existing(id);
        let Some((_, path)) = shell.vault.find_by_id(&bid) else {
            return vec![id.to_owned()];
        };
        let path = path.to_path_buf();
        let Some((_, doc)) = shell.vault.iter().find(|(p, _)| *p == path) else {
            return vec![id.to_owned()];
        };
        let mut out = Vec::new();
        let mut inside: Option<u8> = None;
        for h in doc.all_headlines() {
            let this = h.id().to_string();
            match inside {
                None if this == id => {
                    inside = Some(h.level());
                    out.push(this);
                }
                None => {}
                Some(level) if h.level() > level => out.push(this),
                Some(_) => break,
            }
        }
        out
    }

    /// Open the refile picker on the selected subtree.
    fn open_refile(&mut self, shell: &Shell) {
        let selected = self
            .selection_active
            .then(|| self.selected_row_id(shell))
            .flatten();
        let Some(id) = selected else {
            self.say("nothing selected — put the cursor on a headline first");
            return;
        };
        self.refile_source = Some(id);
        self.surface = ModalSurface::Refile;
        self.query.clear();
        self.selected = 0;
        self.say("refile to — type to filter · RET files it · Esc cancels");
    }

    /// File the pending subtree under the target on row `i` — the click
    /// path, and what Enter runs.
    pub fn refile_click(&mut self, shell: &mut Shell, i: usize) {
        let Some(source) = self.refile_source.clone() else {
            return;
        };
        let Some(row) = self.refile_rows(shell).into_iter().nth(i) else {
            return;
        };
        let from = closure_core::BlockId::from_existing(&source);
        let to = closure_core::BlockId::from_existing(&row.id);
        match shell.vault.refile(&from, &to) {
            Ok(()) => {
                self.say(format!("filed under {}", row.title));
                self.invalidate_rows();
                self.select_by_id(shell, &source);
            }
            Err(e) => self.status = format!("could not file it: {e}"),
        }
        self.refile_source = None;
        self.surface = ModalSurface::Browse;
        self.query.clear();
    }

    /// Move the selected subtree into its file's archive sibling.
    fn archive_selected(&mut self, shell: &mut Shell) {
        let selected = self
            .selection_active
            .then(|| self.selected_row_id(shell))
            .flatten();
        let Some(id) = selected else {
            self.say("nothing selected — put the cursor on a headline first");
            return;
        };
        let bid = closure_core::BlockId::from_existing(&id);
        let today = self.today.clone();
        match shell.vault.archive_subtree(&bid, &today) {
            Ok(path) => {
                self.say(format!(
                    "archived to {}",
                    vault_relative(shell, &path).display()
                ));
                self.invalidate_rows();
                self.selected = self
                    .selected
                    .min(self.rows_shared(shell).len().saturating_sub(1));
            }
            Err(e) => self.status = format!("could not archive it: {e}"),
        }
    }

    /// The refile picker's keys — the same interaction as the other
    /// pickers, over targets.
    fn on_refile_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        let matches: Vec<usize> = self
            .refile_rows(shell)
            .iter()
            .enumerate()
            .filter(|(_, r)| r.matches_filter)
            .map(|(i, _)| i)
            .collect();
        match key {
            "escape" => {
                self.refile_source = None;
                self.query.clear();
                self.selected = 0;
                self.go_home();
                self.say("left where it was");
            }
            "enter" => {
                let Some(&row) = matches.get(self.selected) else {
                    self.say("nothing matches");
                    return;
                };
                self.refile_click(shell, row);
            }
            _ => {
                self.filter_key(key, ctrl, alt, text, matches.len().saturating_sub(1));
            }
        }
    }

    // ---- C-c C-l: a link, the way org offers to make one -----------

    /// The link types the picker is showing — every one of them until
    /// something is typed, then the ones that match.
    #[must_use]
    pub fn link_types(&self) -> Vec<String> {
        let filter = if self.link_kind.is_none() {
            self.field_buf.text()
        } else {
            ""
        };
        LINK_TYPES
            .iter()
            .filter(|t| filter.is_empty() || closure_query::fuzzy_score(filter, t).is_some())
            .map(|t| (*t).to_owned())
            .collect()
    }

    /// What the destination field can be completed to, for the kinds
    /// of link whose destinations live in this vault.
    ///
    /// This is the reason `id:` is worth offering at all: nobody
    /// remembers a ULID, they remember what the note is called. `http:`
    /// and the rest get nothing, because the vault has no opinion
    /// about the web.
    #[must_use]
    pub fn link_completions(&self, shell: &Shell) -> Vec<LinkCompletion> {
        let Some(kind) = self.link_kind.as_deref() else {
            return Vec::new();
        };
        if self.link_dest.is_some() {
            // Past the destination: the description is prose, and
            // completing prose against filenames would be noise.
            return Vec::new();
        }
        let all: Vec<LinkCompletion> = match kind {
            "id:" => shell
                .vault
                .iter()
                .flat_map(|(path, doc)| {
                    let shown = vault_relative(shell, path).display().to_string();
                    doc.all_headlines().map(move |h| LinkCompletion {
                        value: h.id().to_string(),
                        label: format!("{} — {shown}", h.title()),
                    })
                })
                .collect(),
            "file:" | "attachment:" => shell
                .vault
                .iter()
                .map(|(path, _)| {
                    let shown = vault_relative(shell, path).display().to_string();
                    LinkCompletion {
                        value: shown.clone(),
                        label: shown,
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        let typed = self.field_buf.text();
        all.into_iter()
            .filter(|c| typed.is_empty() || closure_query::fuzzy_score(typed, &c.label).is_some())
            .collect()
    }

    /// Which of the two things the link field is asking for.
    #[must_use]
    pub const fn link_asks_description(&self) -> bool {
        self.link_dest.is_some()
    }

    /// The link type already picked, while the rest is being typed —
    /// what a shell puts in front of the field so you can see what you
    /// are completing.
    #[must_use]
    pub fn link_kind(&self) -> &str {
        self.link_kind.as_deref().unwrap_or_default()
    }

    /// Start `C-c C-l`.
    fn open_insert_link(&mut self) {
        if !self.surface.is_editor() {
            self.say("open a body first — a link goes into text");
            return;
        }
        self.prompt_from = Some(self.surface);
        self.link_kind = None;
        self.link_dest = None;
        self.field_buf.clear();
        self.selected = 0;
        self.surface = ModalSurface::InsertLink;
        self.say("link type — type to filter · RET picks · Esc cancels");
    }

    /// Keys for `C-c C-l`, whichever of its three steps is open.
    fn on_insert_link_key(
        &mut self,
        shell: &Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        if key == "escape" {
            self.abandon_link();
            return;
        }
        if self.link_kind.is_none() {
            self.on_link_type_key(key, ctrl, alt, text);
            return;
        }
        match key {
            "tab" => {
                let Some(pick) = self.link_completions(shell).into_iter().nth(self.selected) else {
                    self.say("nothing here to complete");
                    return;
                };
                self.field_buf.set_text(&pick.value);
            }
            "enter" => {
                if self.link_dest.is_some() {
                    self.finish_link();
                    return;
                }
                // The highlighted candidate, where there is one: the
                // list is the answer to the question `id:` asks, and
                // Enter used to read the empty field beside it — so
                // the one link type that needs the picker was the one
                // you could not finish without knowing the ULID.
                let dest = self
                    .link_completions(shell)
                    .into_iter()
                    .nth(self.selected)
                    .map_or_else(
                        || self.field_buf.text().trim().to_owned(),
                        |pick| pick.value,
                    );
                if dest.is_empty() {
                    self.say("a link needs somewhere to go");
                    return;
                }
                self.link_dest = Some(dest);
                self.field_buf.clear();
                self.selected = 0;
                self.say("what to call it — RET, or empty to show the link itself");
            }
            _ => {
                let last = self.link_completions(shell).len();
                if let Some(step) = list_step(key, ctrl) {
                    self.selected = step_wrapping(self.selected, last, step);
                    return;
                }
                let before = self.field_buf.text().to_owned();
                let mut kill = self.shared_kill();
                line_key(&mut self.field_buf, &mut kill, key, ctrl, alt, text);
                self.keep_shared_kill(&kill);
                if self.field_buf.text() != before {
                    self.selected = 0;
                }
            }
        }
    }

    /// The first step: picking the type, which filters as you type.
    fn on_link_type_key(&mut self, key: &str, ctrl: bool, alt: bool, text: Option<char>) {
        let rows = self.link_types();
        // TAB completes the type and RET picks it — org's own "Type TAB
        // to complete link type, then RET to complete destination",
        // where both land in the same place because there is only ever
        // one thing to do here.
        if key == "enter" || key == "tab" {
            let Some(kind) = rows.get(self.selected).cloned() else {
                self.say("no link type matches");
                return;
            };
            self.field_buf.clear();
            self.say(format!("{kind} — where does it go? · RET · Esc cancels"));
            self.link_kind = Some(kind);
            return;
        }
        if let Some(step) = list_step(key, ctrl) {
            self.selected = step_wrapping(self.selected, rows.len(), step);
            return;
        }
        let before = self.field_buf.text().to_owned();
        let mut kill = self.shared_kill();
        line_key(&mut self.field_buf, &mut kill, key, ctrl, alt, text);
        self.keep_shared_kill(&kill);
        if self.field_buf.text() != before {
            self.selected = 0;
        }
    }

    /// Pick the link type on row `i` — what a click on it does.
    pub fn link_type_click(&mut self, i: usize) {
        if self.surface != ModalSurface::InsertLink || self.link_kind.is_some() {
            return;
        }
        self.selected = i;
        self.on_link_type_key("enter", false, false, None);
    }

    /// Write the finished link into the buffer at the caret.
    fn finish_link(&mut self) {
        let (Some(kind), Some(dest)) = (self.link_kind.take(), self.link_dest.take()) else {
            self.abandon_link();
            return;
        };
        let link = org_link(&kind, &dest, self.field_buf.text().trim());
        self.body.insert_str(&link);
        self.field_buf.clear();
        self.surface = self.prompt_from.take().unwrap_or(ModalSurface::EditBody);
        self.say(format!("inserted {link}"));
    }

    /// Leave without writing anything.
    fn abandon_link(&mut self) {
        self.link_kind = None;
        self.link_dest = None;
        self.field_buf.clear();
        self.surface = self.prompt_from.take().unwrap_or(ModalSurface::EditBody);
        self.say("no link");
    }

    // ---- Q3-V5: cycling keywords, priorities and checkboxes --------

    /// The vault's typed config, or the defaults when it has none.
    ///
    /// Read per command rather than cached: these are keystroke-rate
    /// verbs, not frame-rate ones, and a config the user has just
    /// edited in the other pane should take effect on the next press.
    fn vault_config(shell: &Shell) -> closure_config::Config {
        closure_config::Config::from_path(&shell.vault.root().join(closure_config::CONFIG_FILE))
            .unwrap_or_default()
    }

    /// Step the selected headline's TODO keyword through the vault's
    /// own list — forwards (`toggle-todo`) or backwards (`todo-back`).
    ///
    /// The cycle is `none → first → … → last → none`, so "no keyword"
    /// is a position in it, the way org's is. A keyword the vault does
    /// not know (a note from another org setup) is not a position, so
    /// stepping from it starts the list over rather than guessing.
    fn cycle_todo(&mut self, shell: &mut Shell, delta: isize) {
        let Some(row) = self.rows_shared(shell).get(self.selected).cloned() else {
            return;
        };
        let cfg = Self::vault_config(shell);
        let keywords = cfg.todo_keywords.clone();
        if keywords.is_empty() {
            self.say("no TODO keywords configured");
            return;
        }
        let current = self.detail(shell).and_then(|d| d.todo);
        let at = current
            .as_deref()
            .and_then(|k| keywords.iter().position(|w| w == k));
        // `none` sits at index `len`, so the ring is one longer than the
        // keyword list.
        let ring = isize::try_from(keywords.len() + 1).unwrap_or(1);
        let pos = match at {
            Some(i) => isize::try_from(i).unwrap_or(0),
            // An unknown keyword steps to the first one either way.
            None if current.is_some() => -delta,
            None => isize::try_from(keywords.len()).unwrap_or(0),
        };
        let next_index = usize::try_from((pos + delta).rem_euclid(ring)).unwrap_or(0);
        let next = keywords.get(next_index).cloned();
        let bid = closure_core::BlockId::from_existing(&row.id);
        // A keyword only *is* a keyword in a file that declares it, so
        // the file learns the vault's sequence before it is written
        // with one — otherwise `NEXT` goes in and comes back as the
        // first word of the title.
        if let Some(path) = shell.vault.find_by_id(&bid).map(|(_, p)| p.to_path_buf())
            && let Err(e) = shell.vault.ensure_todo_keywords(&path, &keywords)
        {
            self.say(format!("could not declare the keywords: {e}"));
            return;
        }
        if shell.set_todo(&bid, next.as_deref()).is_err() {
            self.say("could not change the keyword");
            return;
        }
        self.log_done_stamp(shell, &row.id, next.as_deref(), &keywords, cfg.log_done);
        self.invalidate_rows();
        self.say(next.map_or_else(
            || format!("cleared the keyword on {}", row.title),
            |k| format!("{k}: {}", row.title),
        ));
    }

    /// Stamp (or unstamp) `CLOSED:` when a headline reaches or leaves
    /// the last keyword — org's `org-log-done 'time`, off unless the
    /// vault asks for it.
    ///
    /// "Done" is the *last* configured keyword, which is what a keyword
    /// list means: the states run left to right and the rightmost is
    /// the finished one.
    fn log_done_stamp(
        &mut self,
        shell: &mut Shell,
        id: &str,
        keyword: Option<&str>,
        keywords: &[String],
        enabled: bool,
    ) {
        if !enabled {
            return;
        }
        let done = keywords.last().map(String::as_str);
        let is_done = keyword.is_some() && keyword == done;
        let bid = closure_core::BlockId::from_existing(id);
        let Some((scheduled, deadline, closed)) = shell.vault.find_by_id(&bid).map(|(h, _)| {
            (
                h.scheduled().map(ToOwned::to_owned),
                h.deadline().map(ToOwned::to_owned),
                h.closed().map(ToOwned::to_owned),
            )
        }) else {
            return;
        };
        if is_done == closed.is_some() {
            return;
        }
        let stamp = is_done.then(|| {
            let (y, m, d) = parse_ymd(&self.today).unwrap_or((1970, 1, 1));
            // A CLOSED stamp is inactive: it is a record of when
            // something happened, not something to put in the agenda.
            format!("[{}]", org_stamp(y, m, d, "").trim_matches(['<', '>']))
        });
        if let Err(e) = shell.vault.set_planning(
            &bid,
            scheduled.as_deref(),
            deadline.as_deref(),
            stamp.as_deref(),
        ) {
            self.say(format!("could not stamp CLOSED: {e}"));
        }
    }

    /// Step the priority cookie through the vault's own list.
    ///
    /// `delta` is a step in the ring (`none → A → B → … → none`), the
    /// cycle key's move.
    fn cycle_priority(&mut self, shell: &mut Shell, delta: isize) {
        let Some(row) = self.rows_shared(shell).get(self.selected).cloned() else {
            return;
        };
        let levels = Self::vault_config(shell).priority_levels;
        if levels.is_empty() {
            self.say("no priorities configured");
            return;
        }
        let current = self.detail(shell).and_then(|d| d.priority);
        let at = current.and_then(|c| levels.iter().position(|l| *l == c));
        let ring = isize::try_from(levels.len() + 1).unwrap_or(1);
        let pos = match at {
            Some(i) => isize::try_from(i).unwrap_or(0),
            None if current.is_some() => -delta,
            None => isize::try_from(levels.len()).unwrap_or(0),
        };
        let next = levels
            .get(usize::try_from((pos + delta).rem_euclid(ring)).unwrap_or(0))
            .copied();
        let bid = closure_core::BlockId::from_existing(&row.id);
        let _ = shell.set_priority(&bid, next);
        self.invalidate_rows();
    }

    /// Move the priority by one step of *severity* and stop at the ends
    /// — `priority-up` / `priority-down`.
    ///
    /// Different from the cycle on purpose: raising the top priority
    /// should not silently clear it, which is what wrapping would do.
    fn step_priority(&mut self, shell: &mut Shell, delta: isize) {
        let Some(row) = self.rows_shared(shell).get(self.selected).cloned() else {
            return;
        };
        let levels = Self::vault_config(shell).priority_levels;
        if levels.is_empty() {
            self.say("no priorities configured");
            return;
        }
        let current = self.detail(shell).and_then(|d| d.priority);
        let next = match current.and_then(|c| levels.iter().position(|l| *l == c)) {
            // Nothing set: a step down starts at the top, a step up at
            // the bottom — either way the first press means something.
            None if delta > 0 => levels.first().copied(),
            None => levels.last().copied(),
            Some(i) => {
                let at = isize::try_from(i).unwrap_or(0) + delta;
                let last = isize::try_from(levels.len().saturating_sub(1)).unwrap_or(0);
                levels
                    .get(usize::try_from(at.clamp(0, last)).unwrap_or(0))
                    .copied()
            }
        };
        let bid = closure_core::BlockId::from_existing(&row.id);
        let _ = shell.set_priority(&bid, next);
        self.invalidate_rows();
    }

    /// Tick or untick the checkbox on the body editor's current line,
    /// and recount every `[/]` / `[%]` cookie the buffer has.
    ///
    /// Buffer-local: the vault sees it when the buffer is written, like
    /// every other edit made in the editor.
    fn toggle_checkbox(&mut self) {
        if !self.surface.is_editor() {
            self.say("no buffer open — a checkbox lives in a body");
            return;
        }
        let line = self.body.current_line().to_owned();
        let Some(toggled) = toggle_checkbox_line(&line) else {
            self.say("no checkbox on this line");
            return;
        };
        self.body.replace_current_line(&toggled);
        let recounted = recount_cookies(self.body.text());
        if recounted != self.body.text() {
            let cursor = self.body.cursor_byte();
            self.body.load_in(recounted, self.body.mode());
            self.body
                .set_cursor_byte(cursor.min(self.body.text().len()));
        }
    }

    // ---- Q3-V4: the date picker ------------------------------------

    /// Tell the core what day it is (`YYYY-MM-DD`).
    ///
    /// The shells own the clock; the core owns the calendar. A date
    /// nobody sets leaves the picker on the epoch rather than guessing.
    pub fn set_today(&mut self, ymd: &str) {
        if parse_ymd(ymd).is_some() {
            ymd.clone_into(&mut self.today);
        }
    }

    /// What the core believes today is.
    #[must_use]
    pub fn today(&self) -> &str {
        &self.today
    }

    /// The month the picker is showing, or an empty grid when it is
    /// closed — the renderers ask unconditionally.
    #[must_use]
    pub fn date_grid(&self) -> DateGrid {
        let Some(session) = &self.date_pick else {
            return DateGrid {
                year: 0,
                month: 0,
                field: String::new(),
                selected: String::new(),
                weeks: Vec::new(),
                typed: String::new(),
            };
        };
        let (y, m, d) = session.date;
        let first_col = weekday_index(y, m, 1);
        let len = days_in_month(y, m);
        let mut cells: Vec<Option<u32>> = vec![None; first_col];
        cells.extend((1..=len).map(Some));
        while !cells.len().is_multiple_of(7) {
            cells.push(None);
        }
        DateGrid {
            year: y,
            month: m,
            field: session.field.keyword().to_owned(),
            selected: format!("{y:04}-{m:02}-{d:02}"),
            weeks: cells.chunks(7).map(<[Option<u32>]>::to_vec).collect(),
            typed: session.typed.clone(),
        }
    }

    /// Put the picker's cursor on `day` of the month it is showing —
    /// the mouse path into the calendar.
    pub fn date_click(&mut self, day: u32) {
        if let Some(session) = self.date_pick.as_mut() {
            let (y, m, _) = session.date;
            session.date = (y, m, day.clamp(1, days_in_month(y, m)));
        }
    }

    /// Open the picker on the selected headline's `field`.
    ///
    /// It opens on the date the headline already has — planning a task
    /// again is nearly always moving it by a few days, not starting
    /// from today — and on today when it has none.
    fn open_date_pick(&mut self, shell: &Shell, field: PlanField) {
        // Escape drops the selection; planning "the headline the cursor
        // happens to rest on" after that would file a date against a
        // note the user has said they are not looking at.
        let selected = self
            .selection_active
            .then(|| self.selected_row_id(shell))
            .flatten();
        let Some(row) = selected else {
            self.say("nothing selected — put the cursor on a headline first");
            return;
        };
        let bid = closure_core::BlockId::from_existing(&row);
        let existing = shell.vault.find_by_id(&bid).and_then(|(h, _)| {
            let stamp = match field {
                PlanField::Scheduled => h.scheduled(),
                PlanField::Deadline => h.deadline(),
            }?;
            parse_ymd(stamp.trim_start_matches(['<', '[']).get(..10)?)
        });
        let date = existing
            .or_else(|| parse_ymd(&self.today))
            .unwrap_or((1970, 1, 1));
        self.date_pick = Some(DatePickSession {
            id: row,
            field,
            date,
            typed: String::new(),
        });
        self.surface = ModalSurface::DatePick;
        self.say(format!(
            "{} — h/l day · j/k week · </> month · . today · RET set · x clear · Esc cancel",
            field.keyword()
        ));
    }

    /// Move the picker's cursor by `days`, keeping it a real date.
    fn date_step_days(&mut self, days: i64) {
        if let Some(session) = self.date_pick.as_mut() {
            let (y, m, d) = session.date;
            session.date = civil_from_days(days_from_civil(y, i64::from(m), i64::from(d)) + days);
        }
    }

    /// Move by whole months, clamping the day — a 31st stepped into a
    /// 30-day month is the 30th, not a date that does not exist.
    fn date_step_months(&mut self, months: i64) {
        if let Some(session) = self.date_pick.as_mut() {
            let (y, m, d) = session.date;
            let total = y * 12 + i64::from(m) - 1 + months;
            let ny = total.div_euclid(12);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let nm = (total.rem_euclid(12) + 1) as u32;
            session.date = (ny, nm, d.min(days_in_month(ny, nm)));
        }
    }

    /// Write what the picker is showing, and close it.
    ///
    /// The typed field wins when it parses: a repeater (`+1w`) is
    /// something no month grid can express, and refusing to accept one
    /// would make the picker the *less* capable way to plan.
    fn commit_date_pick(&mut self, shell: &mut Shell) {
        let Some(session) = self.date_pick.clone() else {
            return;
        };
        let stamp = if session.typed.trim().is_empty() {
            let (y, m, d) = session.date;
            org_stamp(y, m, d, "")
        } else {
            let typed = session.typed.trim();
            let (head, tail) = typed.split_once(' ').unwrap_or((typed, ""));
            let Some((y, m, d)) = parse_ymd(head) else {
                self.status =
                    format!("`{typed}` is not a date — YYYY-MM-DD, with an optional repeater");
                return;
            };
            org_stamp(y, m, d, tail)
        };
        self.write_planning(shell, &session.id, session.field, Some(&stamp));
    }

    /// Clear the field the picker was opened on.
    fn clear_date_pick(&mut self, shell: &mut Shell) {
        let Some(session) = self.date_pick.clone() else {
            return;
        };
        self.write_planning(shell, &session.id, session.field, None);
    }

    /// Set one planning field, leaving the others as they are.
    ///
    /// [`closure_store::Vault::set_planning`] replaces the whole triple,
    /// so the other two are read back first — planning a deadline must
    /// not silently drop the schedule.
    fn write_planning(
        &mut self,
        shell: &mut Shell,
        id: &str,
        field: PlanField,
        stamp: Option<&str>,
    ) {
        let bid = closure_core::BlockId::from_existing(id);
        let Some((scheduled, deadline, closed)) = shell.vault.find_by_id(&bid).map(|(h, _)| {
            (
                h.scheduled().map(ToOwned::to_owned),
                h.deadline().map(ToOwned::to_owned),
                h.closed().map(ToOwned::to_owned),
            )
        }) else {
            self.say("that headline is no longer in the vault");
            return;
        };
        let (scheduled, deadline) = match field {
            PlanField::Scheduled => (stamp.map(ToOwned::to_owned), deadline),
            PlanField::Deadline => (scheduled, stamp.map(ToOwned::to_owned)),
        };
        match shell.vault.set_planning(
            &bid,
            scheduled.as_deref(),
            deadline.as_deref(),
            closed.as_deref(),
        ) {
            Ok(()) => {
                self.invalidate_rows();
                self.say(stamp.map_or_else(
                    || format!("{} cleared", field.keyword()),
                    |s| format!("{}: {s}", field.keyword()),
                ));
            }
            Err(e) => self.status = format!("could not set {}: {e}", field.keyword()),
        }
        self.date_pick = None;
        self.surface = ModalSurface::Browse;
    }

    /// The date picker's keys.
    fn on_datepick_key(&mut self, shell: &mut Shell, key: &str, text: Option<char>) {
        match key {
            "escape" => {
                self.date_pick = None;
                // Back to whatever this was opened over, buffer
                // included — `schedule` from inside a note is the
                // ordinary way to reach it.
                self.go_home();
                self.say("left as it was");
            }
            "enter" => self.commit_date_pick(shell),
            "backspace" => {
                if let Some(session) = self.date_pick.as_mut() {
                    session.typed.pop();
                }
            }
            // The motions are the app's own, so a hand that knows the
            // outline knows the calendar. They apply to the grid, not
            // to a typed date — typing is the other way in.
            "h" | "left" => self.date_step_days(-1),
            "l" | "right" => self.date_step_days(1),
            "k" | "up" => self.date_step_days(-7),
            "j" | "down" => self.date_step_days(7),
            ">" => self.date_step_months(1),
            "<" => self.date_step_months(-1),
            "." => {
                if let Some(today) = parse_ymd(&self.today)
                    && let Some(session) = self.date_pick.as_mut()
                {
                    session.date = today;
                }
            }
            "x" => self.clear_date_pick(shell),
            _ => {
                // Digits and dashes build a typed date; a repeater needs
                // a space and a `+`.
                if let Some(c) = text
                    && (c.is_ascii_digit()
                        || matches!(c, '-' | '+' | ' ' | '.' | 'w' | 'd' | 'm' | 'y'))
                    && let Some(session) = self.date_pick.as_mut()
                {
                    session.typed.push(c);
                }
            }
        }
    }

    /// Remember a file the way `config.org` will keep it: most recent
    /// first, no duplicates, and not so many that the picker becomes a
    /// second file list.
    fn remember_recent_file(&mut self, path: &std::path::Path) {
        const RECENT_MAX: usize = 20;
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_path_buf());
        self.recent_files.truncate(RECENT_MAX);
    }

    /// What the open buffer is, for the shells to put where a modeline
    /// puts a buffer name. `None` when no buffer is open.
    ///
    /// A full-window buffer with no name is a wall of text you cannot
    /// place: which headline is this, and which file did it come from?
    /// Derived here so every shell names it identically.
    #[must_use]
    pub fn buffer_name(&self, shell: &Shell) -> Option<String> {
        if !self.surface.is_editor() {
            return None;
        }
        if let Some(path) = &self.file_target {
            return Some(path.display().to_string());
        }
        let detail = self.detail(shell)?;
        // The id as well as the title: the title is what a person
        // recognises the buffer by, and the id is what everything else
        // addresses the block by — a link, a sync round, an undo entry,
        // a bug report. The preview pane beside it had shown it all
        // along, so the two panes disagreed about whether it mattered.
        let id = self
            .edit_target
            .as_deref()
            .map(|id| format!(" · {id}"))
            .unwrap_or_default();
        // The keyword, when there is one. "header in editor doesn't
        // show if current headline a TODO/DONE item" — the detail pane
        // had shown it all along, so opening the same headline as a
        // buffer dropped the state that makes it a task rather than a
        // note. It belongs to the headline, not to one view of it.
        //
        // Nothing is added for an ordinary headline: most of them have
        // no keyword, and a placeholder on every one of those is noise.
        let state = detail
            .todo
            .as_deref()
            .map_or_else(String::new, |kw| format!("{kw} "));
        Some(if self.surface == ModalSurface::EditBlock {
            let lang = self.special_language();
            let lang = if lang.is_empty() { "src" } else { lang };
            format!(
                "{lang} block — {state}{} · {}{id}",
                detail.title, detail.path
            )
        } else {
            format!("{state}{} · {}{id}", detail.title, detail.path)
        })
    }

    /// Whether the active input mode edits modally — whether there is a
    /// NORMAL for a buffer to open in at all.
    ///
    /// Notion and Emacs have no modal state, so a body they open is a
    /// text field and typing into it types. Vim, Doom and Helix do, and
    /// in those the first thing typed into a fresh buffer is a command.
    #[must_use]
    pub const fn modal_editing(&self) -> bool {
        matches!(
            self.mode,
            InputMode::Vim | InputMode::Doom | InputMode::Helix
        )
    }

    /// Which editor mode a freshly opened buffer lands in.
    const fn entry_mode(&self) -> EditorMode {
        if self.modal_editing() {
            EditorMode::Normal
        } else {
            EditorMode::Insert
        }
    }

    /// Load `text` into the body editor the way *this* input mode opens
    /// a buffer ([`Self::entry_mode`]).
    fn load_body(&mut self, text: String) {
        // Drawers open folded. They have to be *in* the buffer — that
        // is what carries a child's identity through the read/write
        // round trip — so the only honest way to quiet four lines of
        // `:ID:` per child is to stop painting them. Display-only: the
        // text is untouched, so a save writes exactly what was read.
        self.body_folds = drawer_folds(&text);
        self.body.load_in(text, self.entry_mode());
    }

    /// Load `content` into the editor as an edit-special session.
    fn open_special(&mut self, content: String) {
        self.special_return = Some(self.surface);
        self.load_body(content);
        self.surface = ModalSurface::EditBlock;
        self.say(format!(
            "edit-special [{}] — C-c C-c writes back, C-s writes, Esc discards",
            self.special_language()
        ));
    }

    /// Write the edited block back the way its origin requires.
    fn commit_edit_special(&mut self, shell: &mut Shell) {
        let Some((origin, _)) = self.special.take() else {
            return;
        };
        let edited = self.body.text().to_owned();
        match origin {
            SpecialOrigin::File { path, index } => {
                match shell.vault.set_block_content(&path, index, &edited) {
                    Ok(()) => self.say("block written"),
                    Err(e) => self.say(format!("edit-special failed: {e}")),
                }
                self.body.clear();
            }
            SpecialOrigin::Body {
                range,
                mut buffer,
                cursor,
            } => {
                // Splice into the body buffer; the ordinary body commit
                // is what carries it to disk.
                buffer.replace_range(range, &edited);
                self.load_body(buffer);
                self.body.set_cursor_byte(cursor);
                self.say("block spliced — C-c C-c again to save the body");
            }
        }
        self.surface = self.special_return.take().unwrap_or(ModalSurface::Browse);
    }

    /// Write the edited block back and *stay* in it — `C-s`, not
    /// `C-c C-c`.
    ///
    /// A block reached from the Blocks list is its own thing in a file
    /// and writes straight back. A block opened out of a body buffer
    /// lives inside text the vault does not have yet, so writing it
    /// means writing that body — and then finding the block again in
    /// what was actually written, because the body write escapes,
    /// files typed headlines and reads the note back. A range computed
    /// before all that is a guess, and a wrong one splices the next
    /// save into the middle of somebody's prose.
    fn write_edit_special(&mut self, shell: &mut Shell) {
        let edited = self.body.text().to_owned();
        let block_cursor = self.body.cursor_byte();
        let Some((origin, lang)) = self.special.take() else {
            return;
        };
        match origin {
            SpecialOrigin::File { path, index } => {
                match shell.vault.set_block_content(&path, index, &edited) {
                    Ok(()) => self.say("block written"),
                    Err(e) => self.say(format!("edit-special failed: {e}")),
                }
                self.special = Some((SpecialOrigin::File { path, index }, lang));
            }
            SpecialOrigin::Body {
                range,
                buffer,
                cursor,
            } => {
                let start = range.start;
                let mut whole = buffer;
                whole.replace_range(range, &edited);
                self.load_body(whole);
                self.write_body(shell);
                let written = self.body.text().to_owned();
                let range = enclosing_src_block(&written, start)
                    .map_or(start..start + edited.len(), |(r, _)| r);
                self.special = Some((
                    SpecialOrigin::Body {
                        range,
                        buffer: written,
                        cursor,
                    },
                    lang,
                ));
                self.load_body(edited);
                self.body.set_cursor_byte(block_cursor);
            }
        }
        self.body_baseline = self.body.text().to_owned();
    }

    /// Abandon the edit-special session, restoring what it replaced.
    fn cancel_edit_special(&mut self) {
        let origin = self.special.take().map(|(o, _)| o);
        if let Some(SpecialOrigin::Body { buffer, cursor, .. }) = origin {
            self.load_body(buffer);
            self.body.set_cursor_byte(cursor);
        } else {
            self.body.clear();
        }
        self.say("edit-special discarded");
        self.surface = self.special_return.take().unwrap_or(ModalSurface::Browse);
    }

    /// Open the `:` command line.
    fn begin_ex(&mut self) {
        self.ex_buf.clear();
        self.ex_return = Some(self.surface);
        self.surface = ModalSurface::Ex;
        self.say(":");
    }

    /// Keys for the `:` line: typing edits it, Enter runs it, Escape
    /// abandons it, and backspacing past the start closes it (the same
    /// rule as the `/` menu — deleting the trigger dismisses it).
    fn on_ex_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        match key {
            "escape" => self.close_ex(),
            // Backspace on an empty line is the way out; anywhere else
            // it is the field's, like every other editing key here.
            "backspace" if self.ex_buf.is_empty() => self.close_ex(),
            // TAB completes, as in every shell and in vim's own
            // cmdline. It is not Enter: nothing runs.
            "tab" => self.ex_complete(),
            "enter" => {
                let line = self.ex_buf.take();
                self.run_ex(shell, line.trim());
            }
            _ => {
                // Typing is a new question: the cycle and the stem it
                // walked both start over.
                self.ex_cycle = 0;
                self.ex_stem = None;
                let mut kill = self.shared_kill();
                line_key(&mut self.ex_buf, &mut kill, key, ctrl, alt, text);
                self.keep_shared_kill(&kill);
            }
        }
    }

    /// The vim lines the `:` prompt answers to that are not registry
    /// commands.
    ///
    /// They are why people open the line at all, so a completion list
    /// without them would be missing its most-used entries.
    const EX_VIM_LINES: &'static [&'static str] = &[
        "w", "write", "wq", "x", "q", "q!", "quit", "wq!", "x!", "messages",
    ];

    /// What the `:` line would complete the current input to.
    ///
    /// "ex mode autocompletion". The line took typing and Enter and
    /// nothing else, so knowing a command's name exactly was the price
    /// of using the one surface that is a superset of the palette.
    ///
    /// Drawn from the command registry rather than a second list: a
    /// command added anywhere is completable here the same day, which
    /// a hand-kept list stops being true of almost immediately.
    #[must_use]
    pub fn ex_completions(&self) -> Vec<String> {
        Self::completions_for(self.ex_buf.text())
    }

    /// Everything the `:` line knows that starts with `typed`.
    fn completions_for(typed: &str) -> Vec<String> {
        let mut out: Vec<String> = Self::EX_VIM_LINES
            .iter()
            .map(|s| (*s).to_owned())
            .chain(palette_command_names().into_iter().map(ToOwned::to_owned))
            .filter(|name| name.starts_with(typed))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// TAB on the `:` line: fill in what is certain, then cycle.
    ///
    /// The shell behaviour, and vim's own cmdline: complete to the
    /// longest common prefix rather than guessing which candidate was
    /// meant, and once there is nothing certain left to add, walk
    /// them. A prefix that matches nothing leaves the line alone —
    /// clearing what somebody typed would be worse than doing nothing.
    fn ex_complete(&mut self) {
        // Cycling walks the candidates of the *stem* — what was typed
        // when TAB was first pressed — not of whatever the line now
        // holds. Filling in a whole candidate narrows the line to
        // matching only itself, so cycling against the live text gets
        // stuck on the first thing it offered.
        let stem = self
            .ex_stem
            .clone()
            .unwrap_or_else(|| self.ex_buf.text().to_owned());
        let candidates = Self::completions_for(&stem);
        let Some(first) = candidates.first() else {
            return;
        };
        let typed = self.ex_buf.text().to_owned();
        let common = Self::common_prefix(&candidates);
        if common.len() > typed.len() {
            self.ex_buf.set_text(&common);
            self.ex_stem = Some(stem);
            self.ex_cycle = 0;
            return;
        }
        // Nothing certain left to add: walk them. The index advances
        // first, so a second TAB moves off what the first one put
        // there.
        self.ex_cycle = (self.ex_cycle + 1) % candidates.len();
        let pick = candidates.get(self.ex_cycle).unwrap_or(first);
        self.ex_buf.set_text(pick);
        self.ex_stem = Some(stem);
    }

    /// The longest prefix every candidate shares.
    fn common_prefix(candidates: &[String]) -> String {
        let Some(first) = candidates.first() else {
            return String::new();
        };
        let mut end = first.len();
        for other in &candidates[1..] {
            end = end.min(
                first
                    .char_indices()
                    .zip(other.char_indices())
                    .take_while(|((_, a), (_, b))| a == b)
                    .map(|((i, c), _)| i + c.len_utf8())
                    .last()
                    .unwrap_or(0),
            );
        }
        first[..end].to_owned()
    }

    /// Abandon the `:` line and return to where it was opened from.
    fn close_ex(&mut self) {
        self.ex_buf.clear();
        self.surface = self.ex_return.take().unwrap_or(ModalSurface::Browse);
    }

    /// Execute an ex command.
    ///
    /// The vim set first, then a fall-through to the registry, so `:`
    /// is a superset of the palette it replaced rather than a
    /// replacement for it.
    fn run_ex(&mut self, shell: &mut Shell, line: &str) {
        let was = self.ex_return.take();
        let editing = was == Some(ModalSurface::EditBody);
        // The full-window editor is a buffer too, and `:w` in it means
        // the file, not the headline. Without this the one view that is
        // nothing but a buffer answered `:w` with "the vault is written
        // on every edit — nothing to save", which is a lie about a
        // buffer that genuinely was not written yet.
        let editing_file = was == Some(ModalSurface::EditFile);
        self.ex_buf.clear();
        // The `:` line hands back the surface it was opened over, the
        // way the palette does. It used to drop to Browse here and
        // leave every arm to climb back — so each line that did not
        // think to closed the buffer you were typing in: a bare `:`, a
        // typo, and every command that has nothing to do with the
        // buffer at all. Leaving is a decision the individual lines
        // below make; it is not the command line's default.
        self.surface = was.unwrap_or_else(|| self.home_surface());
        // `:!cmd` is vim's shell escape. It stays where it was typed —
        // running a command is not a reason to close the buffer.
        if let Some(cmd) = line.strip_prefix('!') {
            self.run_shell_escape(shell, cmd.trim());
            return;
        }
        match line {
            "" => {}
            // Vim's rule: `:q` closes the window, and quits when that
            // was the last one. A buffer always has the outline behind
            // it, so `:q` in a buffer closes the buffer and `:q` in the
            // outline quits the app. `:qa` is how you leave from
            // anywhere. The bang is the whole point of the bang: the
            // plain form will not take an unfinished paragraph with it.
            "q" | "quit" if editing => {
                if self.refuse_quit_when_dirty() {
                    self.surface = ModalSurface::EditBody;
                } else {
                    self.close_editor();
                }
            }
            "q!" | "quit!" if editing => self.discard_editor(),
            "q" | "quit" if editing_file => {
                if self.body_dirty() {
                    self.say("unsaved edit — :w writes it · :q! discards");
                } else {
                    self.close_file_buffer();
                    self.view = ViewMode::Clickable;
                }
            }
            "q!" | "quit!" if editing_file => {
                self.close_file_buffer();
                self.view = ViewMode::Clickable;
            }
            "q!" | "quit!" | "qa!" | "quitall!" | "qall!" => self.quit = true,
            // `:q` outside a buffer is the last window: it quits. `:qa`
            // says so from anywhere, and both stop for unsaved text.
            "q" | "quit" | "qa" | "quitall" | "qall" => {
                if self.refuse_quit_when_dirty() {
                    self.surface = ModalSurface::EditBody;
                } else {
                    self.quit = true;
                }
            }
            "w" | "write" | "wq" | "x" | "wq!" | "x!" if editing_file => {
                self.commit_file_buffer(shell);
                if line != "w" && line != "write" {
                    self.close_file_buffer();
                    self.view = ViewMode::Clickable;
                }
            }
            "w" | "write" | "wq" | "x" | "wq!" | "x!" => {
                if editing {
                    // `:w` in every vi ever written means "write and
                    // carry on"; only the `q` half leaves. The ex line
                    // returns to Browse before running its command, so
                    // a plain write used to close the buffer it had
                    // just saved.
                    if line == "w" || line == "write" {
                        self.write_body(shell);
                        self.surface = ModalSurface::EditBody;
                    } else {
                        self.commit_edit_body(shell);
                    }
                } else {
                    // And here it does not, and saying "written" would
                    // be a lie about a write that never happened —
                    // every edit goes through the kernel to disk (I8).
                    self.say("the vault is written on every edit — nothing to save");
                }
                // `:wq` from a buffer wrote it and closed it
                // (`commit_edit_body`); quitting the app as well is the
                // outline's meaning of the same line.
                if !editing && (line.starts_with("wq") || line.starts_with('x')) {
                    self.quit = true;
                }
            }
            "wqa" | "wqa!" | "xa" | "xa!" => {
                if editing {
                    self.commit_edit_body(shell);
                }
                self.save_pending_edit(shell);
                self.quit = true;
            }
            other => {
                // `:open-vault <dir>` names its argument, because a
                // native directory dialog needs a desktop portal and
                // plenty of sessions have none — a command that only
                // works on some desktops is a command you cannot rely
                // on.
                // A line with a space in it is a command and its
                // argument. General rather than a special case per
                // command: "currently there are [no] arguments".
                if let Some((name, arg)) = other.split_once(' ') {
                    let name = canonical_command(name.trim());
                    let known = self.keys.iter().any(|(_, cmd)| cmd == name)
                        || palette_in_keymap("", &self.keys, &[])
                            .iter()
                            .flat_map(|s| &s.items)
                            .any(|e| e.action.command() == name);
                    if known {
                        let name = name.to_owned();
                        let arg = arg.trim().to_owned();
                        self.run_with_arg(shell, &name, &arg);
                        return;
                    }
                }
                // Anything else is a command name. Resolve it against
                // the registry the palette and the chords share (I4).
                let known = self.keys.iter().any(|(_, cmd)| cmd == other)
                    || palette_in_keymap("", &self.keys, &[])
                        .iter()
                        .flat_map(|s| &s.items)
                        .any(|e| e.action.command() == other);
                if known {
                    // `:foo` is the same command `M-x foo` runs, and
                    // the palette hands the buffer back afterwards —
                    // two spellings of one command must not disagree
                    // about whether the buffer survives it (I4). So the
                    // buffer is *written* rather than closed, and where
                    // we end up is the command's decision: `:agenda`
                    // opens the agenda, `:zoom-in` changes nothing but
                    // the size of the text you were already typing.
                    if editing {
                        self.write_body(shell);
                    }
                    self.run_command(shell, other);
                } else {
                    self.say(format!("not an editor command: {other}"));
                }
            }
        }
    }

    /// Shared navigation for the read-only panes: j/k walk, Escape
    /// leaves. `len` is the pane's row count, so the cursor clamps to
    /// what is actually painted.
    fn on_pane_key(&mut self, key: &str, len: usize) {
        match key {
            "j" | "down" => {
                self.pane_cursor = (self.pane_cursor + 1).min(len.saturating_sub(1));
            }
            "k" | "up" => self.pane_cursor = self.pane_cursor.saturating_sub(1),
            "escape" | "q" => {
                // The pane's cursor is reset, not the outline's. Where
                // you were in your notes is not the pane's to spend.
                self.pane_cursor = 0;
                self.go_home();
            }
            _ => {}
        }
    }

    /// Where the read-only panes are looking.
    #[must_use]
    pub const fn pane_cursor(&self) -> usize {
        self.pane_cursor
    }

    /// The body-search overlay: typing narrows, Enter jumps to the hit,
    /// Escape leaves and clears.
    fn on_body_search_key(
        &mut self,
        shell: &Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        match key {
            "escape" => {
                self.query.clear();
                self.selected = 0;
                self.go_home();
            }
            "enter" => {
                if let Some((id, _)) = self.body_search_rows(shell).get(self.selected).cloned() {
                    self.query.clear();
                    self.surface = ModalSurface::Browse;
                    self.select_id(shell, &id);
                }
            }
            _ => {
                let last = self.body_search_rows(shell).len().saturating_sub(1);
                self.filter_key(key, ctrl, alt, text, last);
            }
        }
    }

    /// The flow list: a/b write an allow/block rule for the selected
    /// flow through the same commands the chords run (I8).
    fn on_sniffer_key(&mut self, shell: &mut Shell, key: &str) {
        match key {
            "j" | "down" => self.sniffer.select(self.sniffer_cursor() + 1),
            "k" | "up" => self.sniffer.select(self.sniffer_cursor().saturating_sub(1)),
            "a" => self.run_command(shell, "allow-flow"),
            "b" => self.run_command(shell, "block-flow"),
            "r" => self.run_command(shell, "reload-flows"),
            "d" => self.run_command(shell, "debug-flow"),
            "escape" | "q" => self.go_home(),
            _ => {}
        }
    }

    /// The conflict list: o/t take ours/theirs for the selected field.
    fn on_conflicts_key(&mut self, shell: &mut Shell, key: &str) {
        match key {
            "j" | "down" => self.conflicts.select(self.conflicts.selected() + 1),
            "k" | "up" => self
                .conflicts
                .select(self.conflicts.selected().saturating_sub(1)),
            "o" => self.run_command(shell, "resolve-ours"),
            "t" => self.run_command(shell, "resolve-theirs"),
            "escape" | "q" => self.go_home(),
            _ => {}
        }
    }

    /// Cursor row in the sniffer pane.
    #[must_use]
    pub const fn sniffer_cursor(&self) -> usize {
        self.sniffer.selected()
    }

    /// Single-line field editor (tags / property): Enter commits through
    /// the Shell setter (I8), Esc cancels, Backspace deletes, printable
    /// chars append. Tags split on whitespace; property splits on the
    /// first space into `key value`.
    fn on_field_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
        kind: FieldKind,
    ) {
        match key {
            "escape" => {
                self.field_target = None;
                self.field_buf.clear();
                self.go_home();
            }
            "enter" => {
                let follow = !ctrl;
                if let Some(id) = self.field_target.take() {
                    let bid = closure_core::BlockId::from_existing(&id);
                    match kind {
                        FieldKind::Tags => {
                            let tags: Vec<String> = self
                                .field_buf
                                .text()
                                .split_whitespace()
                                .map(ToOwned::to_owned)
                                .collect();
                            let _ = shell.set_tags(&bid, &tags);
                        }
                        FieldKind::Property => {
                            let entry = self.field_buf.text().to_owned();
                            if let Some((k, v)) = entry.split_once(' ') {
                                let _ = shell.set_property(&bid, k.trim(), v.trim());
                            } else if !entry.trim().is_empty() {
                                let _ = shell.set_property(&bid, entry.trim(), "");
                            }
                        }
                        FieldKind::Rename => {
                            let title = self.field_buf.text().trim().to_owned();
                            if !title.is_empty() {
                                let _ = shell.rename_headline(&bid, &title);
                            }
                        }
                        FieldKind::AddSibling => {
                            let title = self.field_buf.text().trim().to_owned();
                            if !title.is_empty() {
                                let new = self.new_heading;
                                // org writes the keyword into the
                                // headline text itself, which is what
                                // the store's capture prefix is.
                                if new.child {
                                    let _ = shell.add_child(&bid, new.prefix(), &title);
                                } else {
                                    let line = format!("{}{title}", new.prefix());
                                    let _ = if new.above {
                                        shell.add_sibling_before(&bid, &line)
                                    } else {
                                        shell.add_sibling(&bid, &line)
                                    };
                                }
                                // "Should after adding a sibling the
                                // selection be on the new element or
                                // the one it was added to?" — both, on
                                // the rule capture already settled:
                                // Enter goes to what you made, C-Enter
                                // stays put so you can add another. A
                                // second prompt with a different answer
                                // would mean remembering which prompt
                                // you are in.
                                if follow {
                                    self.select_by_title(shell, &title);
                                }
                            }
                        }
                    }
                }
                self.field_buf.clear();
                self.go_home();
            }
            // Completion, on the editor's own chords: `C-n`/`C-p`
            // cycle, TAB accepts, anything else ends the session.
            "n" if ctrl => self.cycle_prompt_completion(shell, true),
            "p" if ctrl => self.cycle_prompt_completion(shell, false),
            "tab" => self.accept_prompt_completion(shell),
            // The arrows and the readline chords are the field's, which
            // is why it is a field and not a `String` with `push` on it.
            _ => {
                self.prompt_completion = None;
                let kill = self.shared_kill();
                self.field_buf.set_kill(&kill);
                self.field_buf.key(key, ctrl, alt, text);
                let after = self.field_buf.kill().to_owned();
                self.keep_shared_kill(&after);
            }
        }
    }

    /// Why a language was refused, and what to do about it.
    ///
    /// One builder rather than three copies of the sentence: the two
    /// block paths and `:!` all refuse for the same reason and should
    /// not drift apart. It also carries the upgrade notice — a vault
    /// whose own `config.org` still says `eval_trust = shell` is
    /// somebody looking at a line that used to work, and the worst
    /// answer is to let them edit it again.
    fn trust_refusal(shell: &Shell, lang: &str) -> String {
        let base = format!(
            "`{lang}` is not trusted here — `M-x trust-language {lang}` grants it, \
             in your own config rather than the vault's"
        );
        if closure_store::vault_claims_trust(shell.vault.root()) {
            format!(
                "{base}. This vault's own config.org still has `eval_trust`; \
                 it no longer grants anything, because a vault you were sent \
                 must not authorise its own code"
            )
        } else {
            base
        }
    }

    /// The read-only list panes, which differ only in their length.
    fn on_list_pane_key(&mut self, shell: &Shell, key: &str) {
        let len = match self.surface {
            ModalSurface::Journal => self.journal_rows(shell).len(),
            _ => self.cron_rows(shell).len(),
        };
        self.on_pane_key(key, len);
    }

    /// Run whatever is due at this wall-clock minute, once.
    ///
    /// Jobs were parsed, listed and never run: `closure-cron`'s
    /// `Scheduler` existed and nothing outside that crate referred to
    /// it, so a `#+BEGIN_SRC cron` block was a list of intentions.
    ///
    /// Firing means running the *registry command* (I8) — a job cannot
    /// reach anything a chord could not, which is what keeps a vault
    /// someone sent you from being a way to run code. Returns what it
    /// ran, so a caller can tell "nothing was due" from "nothing
    /// happened".
    pub fn cron_tick_at(
        &mut self,
        shell: &mut Shell,
        minute: u8,
        hour: u8,
        dom: u8,
        month: u8,
        dow: u8,
    ) -> Vec<String> {
        let due: Vec<String> = job_rows(&shell.vault)
            .into_iter()
            .filter_map(|row| {
                let spec =
                    closure_cron::parse(&format!("{} {}", row.schedule, row.command)).ok()?;
                closure_cron::matches_time(&spec, minute, hour, dom, month, dow)
                    .then_some(row.command)
            })
            .collect();
        let now = (dom, hour, minute);
        let mut ran = Vec::new();
        for command in due {
            if self.cron_fired.get(&command) == Some(&now) {
                continue;
            }
            self.cron_fired.insert(command.clone(), now);
            self.run_command(shell, &command);
            ran.push(command);
        }
        if !ran.is_empty() {
            self.say(format!("cron: ran {}", ran.join(", ")));
        }
        ran
    }

    /// The same, at the machine's own clock.
    ///
    /// Split so the deciding is testable without waiting for a
    /// Tuesday.
    pub fn cron_tick(&mut self, shell: &mut Shell) -> Vec<String> {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        let (minute, hour, dom, month, dow) = civil_parts(secs);
        self.cron_tick_at(shell, minute, hour, dom, month, dow)
    }

    /// The manual pane's keys: it is a list, so it walks like one.
    fn on_manual_key(&mut self, key: &str) {
        let len = self.manual_rows().len();
        self.on_pane_key(key, len);
    }

    /// The manual as lines, for a read-only pane.
    ///
    /// Generated each time it is asked for, from the keymap in force —
    /// so it is right for the mode you are in and cannot go stale.
    #[must_use]
    pub fn manual_rows(&self) -> Vec<String> {
        // The same document, laid out for a pane rather than a file.
        // `#+TITLE:` and friends are how org carries a title and are
        // furniture on screen, and a blank line costs a whole row here
        // where in a file it costs nothing.
        let text = manual_org(self.mode);
        let mut out: Vec<String> = Vec::new();
        for line in text.lines() {
            if line.starts_with("#+") {
                continue;
            }
            if line.is_empty() && out.last().is_none_or(String::is_empty) {
                continue;
            }
            out.push(line.to_owned());
        }
        out
    }

    /// One stroke, then the answer.
    ///
    /// Emacs' `C-h k` shape: it waits for exactly one key, says what
    /// that key does, and gets out of the way — the stroke after it is
    /// an ordinary stroke again.
    fn on_describe_key(&mut self, key: &str, ctrl: bool, alt: bool, text: Option<char>) {
        self.surface = self
            .prompt_from
            .take()
            .unwrap_or_else(|| self.home_surface());
        if key == "escape" {
            self.say("no key described");
            return;
        }
        let Some(stroke) = modal_stroke(key, ctrl, alt, text) else {
            self.say("that is not a key I can name");
            return;
        };
        // A prefix is an answer too: `g` alone runs nothing, and saying
        // so beats saying "undefined" about a key that is half a chord.
        if let Some(told) = self.describe_key(&stroke) {
            self.say(format!(
                "{} runs {} — {} ({})",
                told.chord, told.command, told.description, told.section
            ));
            return;
        }
        if self
            .keymap()
            .iter()
            .any(|(c, _)| c.starts_with(&format!("{stroke} ")))
        {
            self.say(format!(
                "{stroke} is a prefix — press the rest of the chord"
            ));
            return;
        }
        self.say(format!("{stroke} is not bound"));
    }

    /// Grant this vault permission to run one language, in the user's
    /// own config.
    ///
    /// The grant cannot be written from inside the vault — that is the
    /// whole point of where the store lives — so this is the door: you
    /// are here, you typed it, and it is your config that changes.
    fn trust_language(&mut self, shell: &Shell) {
        let lang = self.arg().map(str::trim).unwrap_or_default().to_owned();
        if lang.is_empty() {
            self.say("trust-language needs a language — `M-x trust-language shell`");
            return;
        }
        match closure_store::grant_eval_trust(shell.vault.root(), &lang) {
            Ok(()) => self.say(format!(
                "`{lang}` may now run in this vault — written to your own config, \
                 not the vault's"
            )),
            Err(e) => self.say(format!("could not write the trust store: {e}")),
        }
    }

    /// `gcc` is a chord the buffer can recognise and cannot carry out:
    /// which comment to use is the enclosing source block's business.
    /// Same seam as the clipboard mirror — the editor counts the asks,
    /// and whoever knows the answer watches.
    fn answer_comment_ask(&mut self) {
        if self.body.comment_asked() == self.comment_seen {
            return;
        }
        self.comment_seen = self.body.comment_asked();
        self.toggle_line_comment();
    }

    /// `gcc` / `gc`: comment the lines the operator covers, or take
    /// the comment back off when they all carry one already.
    fn toggle_line_comment(&mut self) {
        if !self.surface.is_editor() {
            self.say("nothing to comment — open a buffer first");
            return;
        }
        let (first, last) = self.body.selected_lines();
        let before = self.body.text().to_owned();
        let Some((text, token, off)) = toggle_comment_lines(&before, first, last) else {
            // The one case worth a word: a block whose language has no
            // line comment at all. Silence there reads as a broken
            // chord.
            self.say("no line comment in this language — nothing to toggle");
            return;
        };
        let token = token.to_owned();
        let cursor = self.body.cursor_byte().min(text.len());
        let visual = matches!(
            self.body.mode(),
            EditorMode::Visual | EditorMode::VisualLine
        );
        self.body.replace_all(text, cursor);
        // An operator ends the selection, the way `gc` does in vim.
        if visual {
            self.body.to_normal();
        }
        self.say(if off {
            "comment off".to_owned()
        } else {
            format!("commented with {token}")
        });
    }

    /// Open the title prompt for one of the new-headline commands.
    ///
    /// org's `M-RET` opens an empty heading and leaves you typing in
    /// it; the outline has no line to type on, so the prompt is where
    /// the title is typed. Which flavour of heading it will become is
    /// remembered until it is accepted.
    fn begin_new_heading(&mut self, shell: &Shell, cmd: &str) {
        // In a buffer the caret says which headline you mean; the
        // outline's `selected` is a different pane's idea of it, and
        // using it here made `M-RET` in the editor add a heading
        // somewhere you were not looking.
        let target = if self.surface.is_editor() {
            self.headline_at_caret(shell)
        } else {
            None
        }
        .or_else(|| {
            self.rows_shared(shell)
                .get(self.selected)
                .map(|r| r.id.clone())
        });
        let Some(target) = target else {
            self.say("nothing selected — put the cursor on a headline first");
            return;
        };
        self.new_heading = NewHeading::for_command(cmd);
        self.prompt_from = self.surface.is_editor().then_some(self.surface);
        self.field_target = Some(target);
        self.field_buf.clear();
        self.surface = ModalSurface::AddSibling;
        self.say(format!(
            "new {} — Enter save, Esc cancel",
            self.new_heading_label()
        ));
    }

    /// What the open title prompt is about to make: `sibling`,
    /// `sibling TODO`, `child` or `child TODO`.
    ///
    /// One prompt serves all four new-headline chords plus `a`, and it
    /// said "add" for every one — so having pressed `A` rather than
    /// `a`, there was nothing on screen to confirm which you got.
    #[must_use]
    pub const fn new_heading_label(&self) -> &'static str {
        match (
            self.new_heading.child,
            self.new_heading.todo,
            self.new_heading.above,
        ) {
            (false, false, false) => "sibling",
            (false, false, true) => "sibling above",
            (false, true, _) => "sibling TODO",
            (true, false, _) => "child",
            (true, true, _) => "child TODO",
        }
    }

    /// Just the kind — sibling or child — with the keyword left out.
    ///
    /// The prompt carries the keyword in its own field now, so a
    /// shell can paint it in that keyword's colour; spelling it into
    /// the label as well would draw it twice.
    pub const fn new_heading_kind(&self) -> &'static str {
        if self.new_heading.child {
            "child"
        } else if self.new_heading.above {
            "sibling above"
        } else {
            "sibling"
        }
    }

    /// The single-line field-edit buffer (tags/property).
    #[must_use]
    pub fn field_buffer(&self) -> &str {
        self.field_buf.text()
    }

    /// Byte offset of the cursor in that field — the twin of
    /// [`Self::capture_cursor`], and for the same reason: a shell that
    /// cannot ask this paints the caret after the last character, and
    /// then Left, `C-a` and Alt+Backspace all look like they did
    /// nothing.
    #[must_use]
    pub const fn field_cursor(&self) -> usize {
        self.field_buf.cursor()
    }

    /// Generic up/down/Esc navigation for the read-only list surfaces
    /// (agenda, blocks) whose rows don't drive a jump.
    fn on_list_key(&mut self, shell: &Shell, key: &str, kind: ListKind) {
        let len = match kind {
            ListKind::Agenda => self.agenda_rows(shell).len(),
        };
        match key {
            "escape" => {
                // Not `self.selected = 0`: that is the outline's
                // cursor, and leaving a pane is not a reason to lose
                // your place in the vault.
                self.pane_cursor = 0;
                // Block output belongs to the pane that produced it.
                self.block_out = None;
                self.go_home();
            }
            "down" | "j" => {
                self.block_out = None;
                self.pane_cursor = (self.pane_cursor + 1).min(len.saturating_sub(1));
            }
            "up" | "k" => {
                self.block_out = None;
                self.pane_cursor = self.pane_cursor.saturating_sub(1);
            }
            "enter" => self.jump_list_row(shell, self.selected),
            _ => {}
        }
    }

    /// Activate row `i` on the active list surface: navigate Browse to
    /// the target headline (agenda: by id; blocks: first headline of the
    /// block's file) and return to Browse. Out-of-range just returns to
    /// Browse. No-op shape on non-list surfaces.
    pub fn jump_list_row(&mut self, shell: &Shell, i: usize) {
        let target_path = match self.surface {
            ModalSurface::Agenda => {
                let id = shell.vault.agenda().into_iter().nth(i).map(|e| e.id);
                self.surface = ModalSurface::Browse;
                self.selected = 0;
                if let Some(id) = id
                    && let Some(idx) = self.rows_shared(shell).iter().position(|r| r.id == id)
                {
                    self.selected = idx;
                }
                return;
            }
            ModalSurface::Blocks => self.block_rows(shell).into_iter().nth(i).map(|b| b.file),
            _ => None,
        };
        self.surface = ModalSurface::Browse;
        self.selected = 0;
        if let Some(path) = target_path
            && let Some(idx) = self.rows_shared(shell).iter().position(|r| r.path == path)
        {
            self.selected = idx;
        }
    }

    /// Agenda rows `(date, title, path)` across the vault, sorted by
    /// date then title (as [`closure_store::Vault::agenda`] returns).
    #[must_use]
    pub fn agenda_rows(&self, shell: &Shell) -> Vec<(String, String, String)> {
        shell
            .vault
            .agenda()
            .into_iter()
            .map(|e| (e.date, e.title, e.path.display().to_string()))
            .collect()
    }

    /// Mouse click into the body editor: place the cursor at
    /// `line`/`col` (clamped), keep the mode, end any completion
    /// session.
    pub fn body_click(&mut self, line: usize, col: usize) {
        self.completion = None;
        self.body.goto_line_col(line, col);
    }

    /// Double-click into the body editor: select the word under the
    /// position ([`BodyEditor::select_word_at_cursor`], Visual mode).
    pub fn body_double_click(&mut self, line: usize, col: usize) {
        self.body_click(line, col);
        self.body.select_word_at_cursor();
    }

    /// Mouse drag into the body editor: extend a charwise Visual
    /// selection from the click anchor to line/col
    /// ([`BodyEditor::drag_to`]); ends any completion session like
    /// [`Self::body_click`] does.
    pub fn body_drag(&mut self, line: usize, col: usize) {
        self.completion = None;
        self.body.drag_to(line, col);
    }

    /// First visible line of the body-editor pane (G5): an explicit
    /// wheel override while the cursor stays put, else follow the
    /// cursor (0 when it fits, otherwise the cursor on the last
    /// visible line) — the outline `view_window` rule for the body.
    #[must_use]
    pub fn body_scroll_start(&self, viewport: usize) -> usize {
        let (cl, _) = self.body.cursor_line_col();
        if let Some((start, at)) = self.body_scroll
            && at == cl
        {
            return start;
        }
        if cl < viewport { 0 } else { cl + 1 - viewport }
    }

    /// Report how many body lines the pane can paint.
    ///
    /// The framing chords (`C-l`, `zz`/`zt`/`zb`) have to answer "where
    /// is the middle of the screen", and only the shell knows how big
    /// the screen is. Called once per frame by the painter; a shell
    /// that never calls it gets [`BODY_VIEWPORT_DEFAULT`].
    /// Tell the core how many outline rows fit on screen, so `C-d` and
    /// `C-u` can move half of them. A shell that never calls it gets
    /// [`BODY_VIEWPORT_DEFAULT`], because a motion that does nothing is
    /// worse than one that moves by a guess.
    pub const fn set_outline_viewport(&mut self, rows: usize) {
        if rows > 0 {
            self.outline_viewport = rows;
        }
    }

    /// Tell the core how many body lines the shell can paint, so the
    /// framing chords know what a screen is. A shell that never calls
    /// it gets [`BODY_VIEWPORT_DEFAULT`].
    pub const fn set_body_viewport(&mut self, lines: usize) {
        if lines > 0 {
            self.body_viewport = lines;
        }
    }

    /// The viewport height the shell last reported.
    #[must_use]
    pub const fn body_viewport(&self) -> usize {
        self.body_viewport
    }

    /// Put the body cursor at the start of `line`, clamped to the
    /// buffer — what a jump (a search hit, `G`, a followed link) does
    /// before the viewport is asked to follow it.
    pub fn body_goto_line(&mut self, line: usize) {
        let text = self.body.text();
        let mut at = 0usize;
        for (n, l) in text.split('\n').enumerate() {
            if n == line {
                break;
            }
            at += l.len() + 1;
        }
        self.body.set_cursor_byte(at.min(text.len()));
        self.completion = None;
    }

    /// Resolve the first visible body line for this frame, moving the
    /// viewport the way Doom's settings say to.
    ///
    /// `lisp/doom-emacs.el` sets `scroll-margin 0` and
    /// `scroll-conservatively 10`: an ordinary move gets no forced
    /// context and scrolls by the minimum, and a *jump* — further than
    /// ten lines off the edge — recentres instead, because a line
    /// pinned to the bottom edge of the pane has nothing under it to
    /// read. The old rule always parked the cursor on the last visible
    /// line, which made every search hit land at the very bottom.
    ///
    /// Stateful by necessity ("scroll by the minimum" is measured from
    /// where the viewport already was), so this is the painter's entry
    /// point; [`Self::body_scroll_start`] stays the pure reader.
    pub fn body_scroll_follow(&mut self, viewport: usize) -> usize {
        /// `scroll-conservatively`: further than this is a jump.
        const CONSERVATIVELY: usize = 10;
        let (cursor, _) = self.body.cursor_line_col();
        let lines = self.body.text().split('\n').count();
        let max = lines.saturating_sub(viewport);
        if let Some((start, at)) = self.body_scroll
            && at == cursor
        {
            self.body_anchor = Some(start);
            return start;
        }
        let previous = self
            .body_anchor
            .unwrap_or_else(|| self.body_scroll_start(viewport));
        let start = if (previous..previous + viewport).contains(&cursor) {
            // Already on screen: Emacs does not move the window, and
            // neither does anything else worth using.
            previous
        } else {
            let distance = if cursor < previous {
                previous - cursor
            } else {
                cursor + 1 - (previous + viewport)
            };
            if distance <= CONSERVATIVELY {
                if cursor < previous {
                    cursor
                } else {
                    cursor + 1 - viewport
                }
            } else {
                cursor.saturating_sub(viewport / 2)
            }
        }
        .min(max);
        self.body_anchor = Some(start);
        self.body_scroll = Some((start, cursor));
        start
    }

    /// `C-l`: cycle the cursor line through centre, top and bottom.
    ///
    /// Emacs' `recenter-top-bottom`, which Doom keeps. A press that
    /// finds the viewport somewhere other than where the last press
    /// left it starts the cycle over — the cycle is about *this* line
    /// in *this* framing, and a motion invalidates both.
    pub fn body_recenter_cycle(&mut self) {
        let viewport = self.body_viewport;
        let (cursor, _) = self.body.cursor_line_col();
        let current = self.body_scroll_start(viewport);
        let step = match self.recenter {
            Some((step, start, at)) if start == current && at == cursor => (step + 1) % 3,
            _ => 0,
        };
        let framing = match step {
            0 => BodyFraming::Centre,
            1 => BodyFraming::Top,
            _ => BodyFraming::Bottom,
        };
        let start = self.frame_body(framing);
        self.recenter = Some((step, start, cursor));
    }

    /// `zz` / `zt` / `zb`: put the cursor line in the middle, at the
    /// top or at the bottom, saying which rather than cycling.
    pub fn body_frame(&mut self, framing: BodyFraming) {
        let start = self.frame_body(framing);
        let (cursor, _) = self.body.cursor_line_col();
        // A framing chord is also a fresh starting point for `C-l`.
        self.recenter = Some((0, start, cursor));
    }

    /// Park the viewport so the cursor line sits where `framing` says,
    /// returning the resolved first visible line.
    fn frame_body(&mut self, framing: BodyFraming) -> usize {
        let viewport = self.body_viewport;
        let (cursor, _) = self.body.cursor_line_col();
        let lines = self.body.text().split('\n').count();
        let max = lines.saturating_sub(viewport);
        let start = match framing {
            BodyFraming::Centre => cursor.saturating_sub(viewport / 2),
            BodyFraming::Top => cursor,
            BodyFraming::Bottom => (cursor + 1).saturating_sub(viewport),
        }
        .min(max);
        self.body_scroll = Some((start, cursor));
        self.body_anchor = Some(start);
        start
    }

    /// Wheel-scroll the body-editor viewport by `delta` lines (G5),
    /// clamped to `0..=lines - viewport`; any cursor-line change
    /// silently drops the override (the sibling of the outline's
    /// `scroll_by`).
    pub fn body_scroll_by(&mut self, delta: i32, viewport: usize) {
        let lines = self.body.text().split('\n').count();
        let max = lines.saturating_sub(viewport);
        let cur = self.body_scroll_start(viewport);
        let new = cur
            .saturating_add_signed(isize::try_from(delta).unwrap_or(0))
            .min(max);
        let (cl, _) = self.body.cursor_line_col();
        self.body_scroll = Some((new, cl));
    }

    /// Complete a drag-and-drop row reorder (G3): move the row at
    /// outline index `from` among its siblings until it sits at `to`,
    /// stepping the registry move commands through [`Self::run`] (I8,
    /// each step undoable, I3) — the selection follows the moved row.
    /// An out-of-range `from` or `from == to` changes nothing; the
    /// walk stops at sibling/parent/file boundaries, so an oversized
    /// `to` clamps to the last reachable slot. Ids are never
    /// regenerated by a move (I2).
    pub fn drag_drop_rows(&mut self, shell: &mut Shell, from: usize, to: usize) {
        let rows = self.rows_shared(shell);
        let Some(row) = rows.get(from) else { return };
        if from == to {
            return;
        }
        let id = row.id.clone();
        self.selected = from;
        let cmd = if to < from {
            "move-subtree-up"
        } else {
            "move-subtree-down"
        };
        for _ in 0..rows.len() {
            let Some(cur) = self.rows_shared(shell).iter().position(|r| r.id == id) else {
                break;
            };
            if (to < from && cur <= to) || (to > from && cur >= to) {
                break;
            }
            self.run(shell, cmd);
            if self.rows_shared(shell).iter().position(|r| r.id == id) == Some(cur) {
                break; // a boundary refused the move
            }
        }
    }

    /// The selected row's document undo-tree, flattened for the
    /// `UndoHistory` pane ([`Document::history_view`], I3). Empty when
    /// nothing is selected or no edit is recorded yet.
    #[must_use]
    pub fn undo_history_rows(&self, shell: &Shell) -> Vec<closure_core::HistoryRow> {
        self.rows_shared(shell)
            .get(self.selected)
            .and_then(|row| {
                let path = std::path::PathBuf::from(&row.path);
                shell
                    .vault
                    .document(&path)
                    .map(closure_core::Document::history_view)
            })
            .unwrap_or_default()
    }

    /// Cursor row inside the `UndoHistory` pane (Q2-U3).
    #[must_use]
    pub const fn undo_history_cursor(&self) -> usize {
        self.hist_cursor
    }

    /// Jump the selected row's document to history node `index`
    /// ([`closure_store::Vault::jump_history_in`] — composed undo/redo
    /// primitives, persisted) and return to Browse.
    fn jump_undo_history(&mut self, shell: &mut Shell, row_at: usize) {
        // The pane lists the tree in walk order; the vault addresses
        // history nodes by insertion order. Once a history has forked
        // those differ, and the row carries the one to send.
        let Some(index) = self.undo_history_rows(shell).get(row_at).map(|r| r.index) else {
            return;
        };
        if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
            let path = std::path::PathBuf::from(&row.path);
            match shell.vault.jump_history_in(&path, index) {
                Ok(()) => self.say("jumped"),
                Err(e) => self.status = format!("jump failed: {e}"),
            }
            self.selected = self
                .selected
                .min(self.rows_shared(shell).len().saturating_sub(1));
        }
        self.surface = ModalSurface::Browse;
    }

    /// Mouse path into the `UndoHistory` pane: clicking row `i` jumps
    /// there, exactly like Enter (Q2-U3).
    pub fn undo_history_click(&mut self, shell: &mut Shell, i: usize) {
        self.hist_cursor = i;
        self.jump_undo_history(shell, i);
    }

    /// Re-point the selection at the row with `id` (the selection
    /// follows a moved subtree); no-op when the id is gone.
    fn select_id(&mut self, shell: &Shell, id: &str) {
        self.select_by_id(shell, id);
    }

    /// Move the outline selection to the row carrying `id`, reporting
    /// whether it was found.
    ///
    /// The list surfaces — headlines, body search, backlinks, the
    /// database table — answer in block ids, so this is how a clicked
    /// row (or a moved subtree) translates back into a selection. An
    /// unknown id leaves the cursor alone rather than resetting it.
    pub fn select_by_id(&mut self, shell: &Shell, id: &str) -> bool {
        if let Some(i) = self.rows_shared(shell).iter().position(|r| r.id == id) {
            self.selected = i;
            self.selection_active = true;
            return true;
        }
        false
    }

    /// Move the outline selection to the first row titled `title`,
    /// reporting whether one was found.
    ///
    /// Org's fuzzy link — `[[Some Heading]]` — points at a title, not
    /// an id, and it is the spelling a person writes by hand. Matching
    /// is exact after trimming: a fuzzy match here would follow a link
    /// to the wrong note, which is worse than not following it.
    pub fn select_by_title(&mut self, shell: &Shell, title: &str) -> bool {
        let want = title.trim();
        if let Some(i) = self
            .rows_shared(shell)
            .iter()
            .position(|r| r.title.trim() == want)
        {
            self.selected = i;
            return true;
        }
        false
    }

    /// Move the outline selection into the file `path` names — to
    /// `title` within it, or to its first headline.
    ///
    /// `file:` links are written relative to wherever the link lives,
    /// so `./b.org`, `b.org` and `notes/b.org` can all mean the same
    /// file; the match is on the trailing path components, which is
    /// the most that can be resolved without knowing the linking
    /// file's own directory.
    pub fn select_in_file(&mut self, shell: &Shell, path: &str, title: Option<&str>) -> bool {
        let want = path.trim().trim_start_matches("./");
        let rows = self.rows_shared(shell);
        let found = rows.iter().position(|r| {
            let same_file = r.path == want
                || r.path.ends_with(&format!("/{want}"))
                || want.ends_with(&format!("/{}", r.path));
            same_file && title.is_none_or(|t| r.title.trim() == t.trim())
        });
        if let Some(i) = found {
            self.selected = i;
            return true;
        }
        false
    }

    /// Index of the selected row's nearest sibling (same file, same
    /// level, no lower-level row between), forward or backward. `None`
    /// at the ends or when a parent boundary intervenes.
    /// At the end of a sibling run, walk the subtree *out* of its
    /// parent instead of refusing to move.
    ///
    /// The move chords used to return in silence here: no move, no
    /// message, no hint that `M-h` then `M-k` was the way to lift a
    /// first child to the top ("cannot move children of headline to the
    /// top and promote to *"). A motion that does nothing and says
    /// nothing reads as a broken feature, and needing two chords to
    /// express one intention is a worse answer than the outliner one —
    /// keep pressing and the item rises through its parents.
    ///
    /// Promoting is the whole move going down: the subtree already sits
    /// after everything of its parent's, so at the parent's level that
    /// *is* "after the parent". Going up it also has to step over the
    /// parent it just left.
    ///
    /// `M-h` / `M-l` stay pure level changes for when that is all you
    /// meant.
    fn escape_parent(&mut self, shell: &mut Shell, forward: bool) {
        let Some(row) = self.rows_shared(shell).get(self.selected).cloned() else {
            return;
        };
        if row.level <= 1 {
            self.say("already at the top level — nothing left to move out of");
            return;
        }
        let bid = closure_core::BlockId::from_existing(&row.id);
        let parent = self
            .parent_index(shell)
            .map(|i| self.rows_shared(shell)[i].id.clone());
        // Out of the parent's subtree *before* changing level. Promoting
        // in place would adopt whichever siblings still followed us —
        // they are deeper than we now are, so the text makes them ours.
        // Landing past the parent's whole subtree first leaves them
        // exactly where they were.
        if let Some(parent) = &parent {
            let _ = shell.move_after(&bid, &closure_core::BlockId::from_existing(parent));
        }
        self.select_id(shell, &row.id);
        if let Err(e) = shell.promote(&bid) {
            self.say(format!("move failed: {e}"));
            return;
        }
        self.select_id(shell, &row.id);
        // Going up it also has to step over the parent it just left,
        // which is now an ordinary previous sibling.
        if !forward && parent.is_some() {
            self.swap_with_sibling(shell, false);
        }
        self.say(format!("{} moved out one level", row.title));
    }

    /// Swap the selected subtree with its neighbouring sibling,
    /// reporting whether there was one. The selection rides along.
    fn swap_with_sibling(&mut self, shell: &mut Shell, forward: bool) -> bool {
        let Some(other) = self.sibling_index(shell, forward) else {
            return false;
        };
        let rows = self.rows_shared(shell);
        let (mine, theirs) = (rows[self.selected].id.clone(), rows[other].id.clone());
        let (mine, theirs) = (
            closure_core::BlockId::from_existing(&mine),
            closure_core::BlockId::from_existing(&theirs),
        );
        // `move_after(id, after)` puts `id` below `after`: going down
        // that is us below them, going up it is them below us.
        let _ = if forward {
            shell.move_after(&mine, &theirs)
        } else {
            shell.move_after(&theirs, &mine)
        };
        self.select_id(shell, mine.as_str());
        true
    }

    /// Index of the row that owns the selection — the nearest one above
    /// it at a shallower level, in the same file.
    fn parent_index(&self, shell: &Shell) -> Option<usize> {
        let rows = self.rows_shared(shell);
        let cur = rows.get(self.selected)?;
        let (path, level) = (cur.path.clone(), cur.level);
        (0..self.selected)
            .rev()
            .find(|&j| rows[j].path == path && rows[j].level < level)
    }

    fn sibling_index(&self, shell: &Shell, forward: bool) -> Option<usize> {
        let rows = self.rows_shared(shell);
        let cur = rows.get(self.selected)?;
        let (path, level) = (cur.path.clone(), cur.level);
        let scan: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new(self.selected + 1..rows.len())
        } else {
            Box::new((0..self.selected).rev())
        };
        for j in scan {
            if rows[j].path != path || rows[j].level < level {
                return None;
            }
            if rows[j].level == level {
                return Some(j);
            }
        }
        None
    }

    /// Read-only agenda rows with today/overdue flags; today is injected
    /// (`YYYY-MM-DD`) so tests stay hermetic. Order = [`Vault::agenda`]
    /// (date ascending, title tie-break).
    #[must_use]
    pub fn agenda_context(&self, shell: &Shell, today: &str) -> Vec<AgendaRow> {
        shell
            .vault
            .agenda()
            .into_iter()
            .map(|e| AgendaRow {
                is_today: e.date == today,
                is_overdue: e.date.as_str() < today,
                kind: match e.kind {
                    closure_store::AgendaKind::Scheduled => "SCHEDULED".to_owned(),
                    closure_store::AgendaKind::Deadline => "DEADLINE".to_owned(),
                },
                date: e.date,
                title: e.title,
            })
            .collect()
    }

    /// Every `#+BEGIN_SRC` block across the vault as `(path, lang,
    /// first-line)` rows, in per-file document order.
    #[must_use]
    pub fn block_rows(&self, shell: &Shell) -> Vec<BlockRow> {
        self.block_rows_shared(shell).as_ref().clone()
    }

    /// The same source-block list, shared rather than cloned.
    ///
    /// Deriving it is expensive in a way that is easy to miss:
    /// `Document::source()` does not hand back a cached string, it
    /// *re-serialises the whole document*. Doing that for every file
    /// in the vault is fine once; doing it from the status bar on
    /// every frame meant re-printing the entire vault on every
    /// keystroke, which is precisely what typing lag feels like.
    /// Memoised on the vault revision — the only thing it reads.
    #[must_use]
    pub fn block_rows_shared(&self, shell: &Shell) -> std::sync::Arc<Vec<BlockRow>> {
        let revision = shell.vault.revision();
        {
            let memo = self.block_memo.borrow();
            if let Some((rev, rows)) = memo.as_ref()
                && *rev == revision
            {
                return std::sync::Arc::clone(rows);
            }
        }
        let mut out = Vec::new();
        for (path, doc) in shell.vault.iter() {
            // Reuse the tested prose/code segmenter over the whole file
            // source (catches preamble + headline blocks uniformly).
            for seg in segment_body(&doc.source()) {
                if let BodySegment::Code { lang, text } = seg {
                    let first = text.lines().next().unwrap_or("").trim().to_owned();
                    out.push(BlockRow {
                        file: path.display().to_string(),
                        lang,
                        line: first,
                    });
                }
            }
        }
        let rows = std::sync::Arc::new(out);
        self.block_recomputes.set(self.block_recomputes.get() + 1);
        *self.block_memo.borrow_mut() = Some((revision, std::sync::Arc::clone(&rows)));
        rows
    }

    /// How many times the source-block list has actually been
    /// re-serialised out of the vault. The render budget is one per
    /// change, and the reason the status bar is affordable.
    #[must_use]
    pub const fn block_recomputes(&self) -> u64 {
        self.block_recomputes.get()
    }

    /// Backlinks list keys: up/down move, Enter jumps to the selected
    /// backlink (navigates Browse to it), Esc returns to Browse.
    fn on_backlinks_key(&mut self, shell: &Shell, key: &str) {
        match key {
            "escape" => {
                self.link_target = None;
                self.pane_cursor = 0;
                // `go_home`, not `Browse`: a pane opened over a buffer
                // goes back to that buffer, and this one was written
                // before that rule existed. The registry-wide test
                // caught it the moment `backlinks` gained a palette
                // entry and came into the property's reach.
                self.go_home();
            }
            "down" | "j" => {
                let last = self.backlink_rows(shell).len().saturating_sub(1);
                self.pane_cursor = (self.pane_cursor + 1).min(last);
            }
            "up" | "k" => self.selected = self.selected.saturating_sub(1),
            "enter" => self.jump_to_selected_backlink(shell),
            _ => {}
        }
    }

    fn jump_to_selected_backlink(&mut self, shell: &Shell) {
        // Jump: make the chosen backlink the Browse selection.
        if let Some((_, title)) = self.backlink_rows(shell).get(self.selected).cloned() {
            self.link_target = None;
            self.surface = ModalSurface::Browse;
            if let Some(idx) = self
                .rows_shared(shell)
                .iter()
                .position(|r| r.title == title)
            {
                self.selected = idx;
            }
        }
    }

    /// Mouse path for the Backlinks surface: clicking row i jumps to
    /// that linking headline - the same jump Enter performs. Out-of-range
    /// clicks and an empty list are safe no-ops.
    pub fn backlink_click(&mut self, shell: &Shell, i: usize) {
        if self.surface != ModalSurface::Backlinks {
            return;
        }
        if self.backlink_rows(shell).get(i).is_none() {
            return;
        }
        self.selected = i;
        self.jump_to_selected_backlink(shell);
    }

    /// Headlines that link to the headline the Backlinks surface was
    /// opened on: `(path, title)` rows from the vault backlink index.
    #[must_use]
    pub fn backlink_rows(&self, shell: &Shell) -> Vec<(String, String)> {
        let Some(target) = &self.link_target else {
            return Vec::new();
        };
        shell
            .vault
            .backlinks_of(target)
            .iter()
            .map(|(path, src_id)| {
                let title = shell
                    .vault
                    .find_by_id(src_id)
                    .map_or_else(|| src_id.to_string(), |(h, _)| h.title().to_owned());
                (path.display().to_string(), title)
            })
            .collect()
    }

    /// Body editor keys (org-edit-special), vim-modal (contract revised
    /// 2026-07-04): `C-<enter>` commits from either mode. INSERT types
    /// at the cursor, `Esc` drops to NORMAL. NORMAL navigates
    /// (`h`/`j`/`k`/`l`/arrows/`0`/`$`), edits (`i`/`a`/`o`/`x`), and
    /// `Esc` cancels the edit.
    fn on_editbody_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        if self.window_chord(shell, key, ctrl, alt, text) {
            return;
        }
        if self.org_accept_chord(shell, key, ctrl, alt, text) {
            return;
        }
        // `RET` on a line that links a picture opens it as large as the
        // window will make it. Not in INSERT — there you are writing,
        // and Enter is a newline — and only where the file is really
        // there, so a broken link still behaves like text.
        if key == "enter"
            && !ctrl
            && !alt
            && self.body.mode() != EditorMode::Insert
            && let Some(path) = self.image_on_caret_line(shell)
        {
            self.show_image(path);
            return;
        }
        if self.leader_key(shell, key, ctrl, alt, text) {
            return;
        }
        if self.window_chord(shell, key, ctrl, alt, text) {
            return;
        }
        // `:` outside INSERT is the ex line, the way vim's is. Inside
        // INSERT it is text — `:PROPERTIES:` and `12:30` are prose —
        // and inside an open `/` search it is part of the pattern.
        if text == Some(':')
            && self.body.mode() != EditorMode::Insert
            && self.body.search_prompt().is_none()
        {
            self.begin_ex();
            return;
        }
        // While the "/" menu is open it owns navigation and accept;
        // everything else falls through, so typing keeps editing the
        // buffer and the query follows along in `sync_slash`.
        if self.slash.is_some() {
            match key {
                "escape" => {
                    self.slash = None;
                    return;
                }
                "enter" => {
                    self.accept_slash();
                    return;
                }
                // The arrows moved the menu and the chords did not, so
                // in a modal mode the "/" menu was mouse-and-arrows
                // only — which is the one thing it must not be.
                "down" | "up" | "j" | "k" | "n" | "p" if key == "down" || key == "up" || ctrl => {
                    if let Some((query, cursor)) = self.slash.as_mut() {
                        let last = block_templates(query).len().saturating_sub(1);
                        *cursor = if matches!(key, "down" | "j" | "n") {
                            (*cursor + 1).min(last)
                        } else {
                            cursor.saturating_sub(1)
                        };
                    }
                    return;
                }
                _ => {}
            }
        }
        // A fold is a range of lines; once the lines move, the range is
        // a guess, and a fold that hides the wrong text is worse than
        // no fold. Dropping them on any edit is honest and cheap.
        let before = self.body.text().len();
        self.edit_body_key(shell, key, ctrl, alt, text);
        if self.body.text().len() != before {
            self.body_folds.clear();
        }
        if self.body.mode() == EditorMode::Insert {
            self.sync_slash();
        } else {
            self.slash = None;
        }
    }

    /// The Notion "/" menu's query while it is open.
    #[must_use]
    pub fn slash_query(&self) -> Option<&str> {
        self.slash.as_ref().map(|(q, _)| q.as_str())
    }

    /// The templates the open menu is offering, best match first.
    #[must_use]
    pub fn slash_items(&self) -> Vec<BlockTemplate> {
        self.slash
            .as_ref()
            .map(|(q, _)| block_templates(q))
            .unwrap_or_default()
    }

    /// Cursor row in the open menu.
    #[must_use]
    pub fn slash_cursor(&self) -> usize {
        self.slash.as_ref().map_or(0, |(_, c)| *c)
    }

    /// Org's table editing chords, when the cursor is in a table.
    ///
    /// `M-<left>`/`M-<right>` move the column, `M-<up>`/`M-<down>` the
    /// row; with shift they delete and insert instead; `M--` rules a
    /// line (`C-c -` in Emacs, but `C-c` is the desktop copy chord
    /// here); `S-TAB` steps back a cell. Each is the key org uses, and
    /// each does nothing outside a table so the outline command it
    /// shadows keeps the key — which is what `org-metaleft` does.
    ///
    /// `true` when the chord was a table command and was handled.
    fn table_chord(&mut self, key: &str, ctrl: bool, alt: bool) -> bool {
        if ctrl {
            return false;
        }
        let text = self.body.text().to_owned();
        let (line, col) = self.body.cursor_line_col();
        let Some(row) = text.lines().nth(line) else {
            return false;
        };
        let at = row
            .char_indices()
            .nth(col)
            .map_or(row.len(), |(byte, _)| byte);
        if key == "shift-tab" && !alt {
            let Some(prev) = table_previous_cell(row, at) else {
                return false;
            };
            let offset: usize = text.lines().take(line).map(|l| l.len() + 1).sum();
            self.body.set_cursor_byte(offset + prev);
            return true;
        }
        if !alt {
            return false;
        }
        let Some(column) = table_column_at(row, at) else {
            return false;
        };
        // Point follows the cell it was in: a column moved right is
        // still the column you are editing, which is what makes
        // `M-<right> M-<right>` walk it across.
        let (edited, target) = match key {
            "left" => (
                table_move_column(&text, line, column, false),
                column.saturating_sub(1),
            ),
            "right" => (table_move_column(&text, line, column, true), column + 1),
            "up" => (table_move_row(&text, line, false), column),
            "down" => (table_move_row(&text, line, true), column),
            "shift-left" => (table_delete_column(&text, line, column), column),
            "shift-right" => (table_insert_column(&text, line, column), column),
            "shift-up" => (table_kill_row(&text, line), column),
            "shift-down" => (table_insert_row(&text, line), column),
            "-" => (table_insert_hline(&text, line), column),
            _ => return false,
        };
        // A chord that *is* a table command has been handled whether or
        // not it could move anything — `M-<left>` in the first column
        // must not fall through and become a word motion.
        if let Some(new_text) = edited {
            let offset: usize = new_text.lines().take(line).map(|l| l.len() + 1).sum();
            let landing = new_text
                .lines()
                .nth(line)
                .map_or(0, |row| cell_start(row, target));
            self.body.replace_all(new_text, offset + landing);
        }
        true
    }

    /// TAB inside an org table: realign the whole table, then move to
    /// the next cell, wrapping to the row below at the end of a row.
    ///
    /// Returns whether the cursor was in a table at all — `false`
    /// leaves TAB to its other jobs.
    fn table_tab(&mut self) -> bool {
        let text = self.body.text().to_owned();
        let (line, col) = self.body.cursor_line_col();
        let Some(rows) = table_bounds(&text, line) else {
            return false;
        };
        // Realign first, so the offsets below describe the table the
        // user is about to land in rather than the one they left.
        let lines: Vec<&str> = text.lines().collect();
        let join = |slice: &[&str]| {
            slice.iter().fold(String::new(), |mut acc, l| {
                acc.push_str(l);
                acc.push('\n');
                acc
            })
        };
        let aligned = align_table(&join(&lines[rows.clone()]));
        let rebuilt = format!(
            "{}{aligned}{}",
            join(&lines[..rows.start]),
            join(&lines[rows.end..])
        );
        let aligned_lines: Vec<&str> = rebuilt.lines().collect();

        // Where the cursor should land: the next cell on this row, or
        // the first cell of the row below.
        let row_text = aligned_lines.get(line).copied().unwrap_or("");
        let at = row_text
            .char_indices()
            .nth(col)
            .map_or(row_text.len(), |(b, _)| b);
        let (target_line, target_byte) = match next_table_cell(row_text, at) {
            Some(next) => (line, next),
            None if line + 1 < rows.end => {
                let below = aligned_lines.get(line + 1).copied().unwrap_or("");
                (line + 1, next_table_cell(below, 0).unwrap_or(0))
            }
            None => (line, at),
        };
        // Byte offset of the target within the whole buffer.
        let mut offset = 0usize;
        for l in aligned_lines.iter().take(target_line) {
            offset += l.len() + 1;
        }
        // A realign happens mid-edit, so it keeps the mode the user is
        // in: reloading into the *entry* mode would throw them out of
        // INSERT halfway through typing a cell.
        let mode = self.body.mode();
        self.body.load_in(rebuilt, mode);
        self.body.set_cursor_byte(offset + target_byte);
        true
    }

    /// Accept menu entry `i` from a click — the same accept Enter
    /// performs (I8). An index past the end leaves the menu alone.
    pub fn slash_click(&mut self, i: usize) {
        if i >= self.slash_items().len() {
            return;
        }
        if let Some((_, cursor)) = self.slash.as_mut() {
            *cursor = i;
        }
        self.accept_slash();
    }

    /// Open the "/" menu, or close it, from what is actually in the
    /// buffer — so backspacing past the slash closes it and no
    /// separate bookkeeping can drift out of sync with the text.
    ///
    /// The trigger is a `/` that *starts a word*: at the beginning of a
    /// line or after whitespace. `and/or`, a URL and a date are text,
    /// not commands. A space after the slash ends it, the same way it
    /// ends a Notion slash command.
    fn sync_slash(&mut self) {
        let (_, col) = self.body.cursor_line_col();
        let before: String = self.body.current_line().chars().take(col).collect();
        let Some(pos) = before.rfind('/') else {
            self.slash = None;
            return;
        };
        let starts_word = before[..pos]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace);
        let query = &before[pos + 1..];
        if !starts_word || query.contains(char::is_whitespace) {
            self.slash = None;
            return;
        }
        let cursor = self.slash_cursor();
        let last = block_templates(query).len().saturating_sub(1);
        self.slash = Some((query.to_owned(), cursor.min(last)));
    }

    /// Replace the `/query` trigger with the selected template and put
    /// the caret where that template asks for it.
    fn accept_slash(&mut self) {
        let Some((query, cursor)) = self.slash.take() else {
            return;
        };
        let Some(tpl) = block_templates(&query).into_iter().nth(cursor) else {
            return;
        };
        // The trigger is the slash plus the query, both immediately
        // behind the cursor.
        let start = self.body.cursor_byte().saturating_sub(1 + query.len());
        self.body.replace_to_cursor(start, tpl.text);
        // The caret goes where the template asks — often inside a
        // multi-line block, which the newline-stopping motions cannot
        // reach, so address it directly.
        let offset = tpl
            .text
            .char_indices()
            .nth(tpl.cursor)
            .map_or(tpl.text.len(), |(b, _)| b);
        self.body.set_cursor_byte(start + offset);
    }

    /// The body editor's own key handling, unaware of the "/" menu.
    /// The chords that move the *viewport* rather than the text, taken
    /// before the mode split because `C-l` is bound globally in Emacs
    /// (Doom keeps it) and so recentres while typing as well.
    ///
    /// Returns whether the key was consumed.
    /// org's `C-c` prefix inside a buffer: `C-c C-c` accepts the edit,
    /// `C-c C-k` abandons it, the way `org-edit-special` does.
    ///
    /// A prefix is two keys and `window_chord` resolves one, so this is
    /// held like the `z` viewport prefix rather than looked up as a
    /// single stroke. `C-c` followed by anything nobody bound does
    /// nothing at all — swallowing the second key too would eat a
    /// keystroke for a chord that does not exist.
    fn org_accept_chord(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) -> bool {
        if self.pending_body == Some(BodyPrefix::OrgAccept) {
            self.pending_body = None;
            match (key, ctrl) {
                ("c", true) => self.run_command(shell, "commit-edit"),
                ("k", true) => self.run_command(shell, "discard-edit"),
                // Anything else after `C-c` goes to the mode's keymap.
                // `C-c` used to be a two-chord dead end that knew only
                // its own two endings, so every other `C-c` chord the
                // keymap advertises — `C-c C-l` first among them — was
                // swallowed in the one place org users type it. What
                // which-key shows is what the buffer answers to (I4).
                _ => {
                    if let Some(stroke) = modal_stroke(key, ctrl, alt, text)
                        && let Some(cmd) = self
                            .command_for(&format!("C-c {stroke}"))
                            .map(ToOwned::to_owned)
                    {
                        self.run_command(shell, &cmd);
                    }
                }
            }
            return true;
        }
        if key == "c" && ctrl && !alt {
            self.pending_body = Some(BodyPrefix::OrgAccept);
            return true;
        }
        false
    }

    fn viewport_chord(
        &mut self,
        shell: &Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) -> bool {
        if ctrl && key == "l" {
            self.pending_body = None;
            self.body_recenter_cycle();
            return true;
        }
        // The chords that belong to the *window* rather than to the
        // text: zoom (Doom's `text-scale`) and the command palette.
        // Every letter in a buffer belongs to the buffer, which is
        // right for letters and wrong for these — `M-x` types nothing,
        // and a writer who cannot reach M-x from the one place they
        // spend the session has no launcher at all. Resolved through
        // the mode's keymap, never spelled out here, so which-key and
        // the palette show the same chord this path answers to (I4).
        if (ctrl || alt)
            && let Some(stroke) = modal_stroke(key, ctrl, alt, text)
            && let Some(cmd) = self.command_for(&stroke).map(ToOwned::to_owned)
        {
            let cmd = cmd.as_str();
            if self.zoom_command(cmd) {
                return true;
            }
            // Only the meta layer, for the palette. `C-p` is bound to
            // it as the desktop prefix, but inside a buffer `C-p`/`C-n`
            // are readline's — they walk the completion — and a chord
            // the buffer already answers is not the window's to take.
            // `M-x` is bound to the palette in all five modes and means
            // nothing to the editor, which is why it is the one that
            // gets through.
            if alt && cmd == "palette" {
                self.pending_body = None;
                self.open_palette();
                return true;
            }
        }
        if self.pending_body == Some(BodyPrefix::Viewport) {
            self.pending_body = None;
            match key {
                "z" => self.body_frame(BodyFraming::Centre),
                "t" => self.body_frame(BodyFraming::Top),
                "b" => self.body_frame(BodyFraming::Bottom),
                // vim's own fold toggle, in the prefix that was already
                // here. `toggle-fold` is bound to `z` and `TAB` in the
                // outline and a buffer owns both — `TAB` expands a
                // tempo snippet, `z` opens this prefix — so the folds
                // existed with no key that reached them from inside the
                // text they fold.
                "a" => self.toggle_body_fold(),
                // `z` followed by anything else is a chord nobody
                // bound; swallowing the second key too would eat an
                // edit, so it falls through as itself.
                _ => self.edit_body_key(shell, key, ctrl, alt, text),
            }
            return true;
        }
        // `z` is evil's viewport prefix, in the modal modes only — in
        // INSERT, and in the mouse-first modes, it is the letter z.
        if key == "z"
            && self.body.mode() != EditorMode::Insert
            && matches!(
                self.mode,
                InputMode::Vim | InputMode::Doom | InputMode::Helix
            )
        {
            self.pending_body = Some(BodyPrefix::Viewport);
            return true;
        }
        false
    }

    /// Kill the word before (or after) the cursor, ending any
    /// completion session — the candidate list was computed for a
    /// prefix that no longer exists.
    fn kill_word(&mut self, forward: bool) {
        self.completion = None;
        if forward {
            self.body.delete_word_forward();
        } else {
            self.body.delete_word_back();
        }
    }

    /// `?` in a NORMAL buffer opens the which-key panel.
    ///
    /// In NORMAL a bare key is a command, not a character, so the
    /// outline's `?` reaches the panel from here too — it printed a
    /// question mark instead, because a buffer resolves bare keys as
    /// text and only consults the keymap for modified chords. In INSERT
    /// it stays a question mark: prose has questions in it, which is
    /// why that rule exists at all. `/`'s open search prompt owns its
    /// own text.
    fn which_key_key(&mut self, text: Option<char>) -> bool {
        if text != Some('?') || self.body.search_prompt().is_some() {
            return false;
        }
        self.which_key_open = !self.which_key_open;
        true
    }

    /// INSERT in the body editor: the readline set, the desktop word
    /// ops, the completion cycle, and the keys every other text field
    /// answers to.
    fn insert_key(&mut self, shell: &Shell, key: &str, ctrl: bool, alt: bool, text: Option<char>) {
        match key {
            // G4: the first buffer-changing edit checkpoints the
            // burst (BodyEditor::insert_guard), so Esc+u undoes it.
            // `C-n`/`C-p` walk the completion *while there is one*,
            // and are `next-line`/`previous-line` the rest of the time
            // — the same trade made for `C-j`/`C-k` just below, for
            // the same reason. They used to be the popup's keys
            // unconditionally, so with no popup up they did nothing at
            // all: "ctlr+p isn't working in editor view". Two of the
            // most worn keys in Emacs, silent, next to a footer
            // advertising the readline set they belong to.
            // `C-n` summons the completion and walks it — the footer
            // says so ("C-n complete") and a suite of tests pins it.
            "n" if ctrl => self.cycle_completion(shell, true),
            // `C-p` walks it *while it is up*, and is `previous-line`
            // the rest of the time. It used to be the popup's key
            // unconditionally, which meant it did nothing whenever
            // there was no popup — "ctlr+p isn't working in editor
            // view", one of the most worn keys in Emacs, silent.
            //
            // Only `C-p` moves, deliberately. `C-n` has a job of its
            // own to do first, and there is no version of "C-p goes
            // back" that summons anything: a popup that is not there
            // cannot be stepped backwards through. Same shape as the
            // `C-j`/`C-k` gate below.
            //
            // `C-k` rides along here: Doom's company map walks the
            // popup with `C-j`/`C-k` too, and only while it is up —
            // with no popup, `C-k` below is still readline's
            // kill-to-end-of-line, and taking that away to gain a
            // second spelling of `C-n` would be a bad trade. The two
            // back-steps are one arm because they are one behaviour.
            "p" | "k" if ctrl && self.completion.is_some() => self.cycle_completion(shell, false),
            "j" if ctrl && self.completion.is_some() => self.cycle_completion(shell, true),
            "p" if ctrl => self.body.up(),
            // Readline chords (the "normal input field" set).
            "a" if ctrl => self.body.line_home(),
            "e" if ctrl => self.body.line_end_motion(),
            "b" if ctrl => self.body.left(),
            "f" if ctrl => self.body.right(),
            "d" if ctrl => self.body.delete_at(),
            // Desktop-standard word ops (Q5): ctrl/alt+arrows jump
            // words, ctrl+backspace kills the word (same as C-w).
            "left" if ctrl || alt => self.body.word_backward(),
            "right" if ctrl || alt => self.body.word_end_forward(),
            // The arrows themselves. Only the modified spellings
            // were bound, so a bare arrow fell through to the
            // branch that inserts characters and did nothing at
            // all. Doom, Vim and Helix hid that behind Esc and
            // NORMAL's motions; Notion and Emacs have no NORMAL to
            // escape to, which left the mouse as the only way to
            // reach the line above the one you were typing on.
            "left" => self.body.left(),
            "right" => self.body.right(),
            "up" => self.body.up(),
            "down" => self.body.down(),
            // The named keys every other text field answers to.
            "home" => self.body.line_home(),
            "end" => self.body.line_end_motion(),
            "delete" => self.body.delete_at(),
            "pageup" => self.body.page(false, 20),
            "pagedown" => self.body.page(true, 20),
            "k" if ctrl => {
                self.completion = None;
                self.body.kill_rest_of_line();
            }
            "u" if ctrl => {
                self.completion = None;
                self.body.kill_to_line_start();
            }
            // `C-w`, the desktop's ctrl+backspace and readline's
            // Alt+Backspace are one kill. The body editor took
            // Ctrl alone, so Alt+Backspace fell through to plain
            // backspace and ate exactly one character — which
            // reads as a broken chord rather than an unbound one,
            // because something did happen.
            "w" if ctrl => self.kill_word(false),
            "backspace" if ctrl || alt => self.kill_word(false),
            // `M-d` is `kill-word` in readline and in Emacs: the
            // twin of the kill above, forwards.
            "d" if alt => self.kill_word(true),
            "y" if ctrl => {
                self.completion = None;
                self.body.yank_insert();
            }
            "escape" => {
                self.completion = None;
                if self.modal_editing() {
                    self.body.to_normal();
                } else {
                    // Notion and Emacs have no NORMAL to drop into
                    // — a buffer left in one is a text field that
                    // will not take text — so there Esc is what
                    // closes, and it still will not take a modified
                    // buffer with it.
                    self.escape_closes_buffer();
                }
            }
            "enter" => {
                self.completion = None;
                self.newline_continuing_list();
            }
            "backspace" => {
                self.completion = None;
                self.body.backspace();
            }
            "tab" => {
                // An active completion session wins over org-tempo:
                // TAB accepts — the applied candidate stays; an
                // unapplied popup applies its first candidate.
                match self.completion.take() {
                    Some(s) if s.ix.is_none() => {
                        if let Some(first) = s.items.first() {
                            self.body.replace_to_cursor(s.start, first);
                        }
                    }
                    Some(_) => {}
                    // Inside a table TAB does the org thing —
                    // realign, then step to the next cell — and
                    // everywhere else it keeps its old job.
                    None if self.table_tab() => {}
                    None => self.body.tempo_expand_or_indent(),
                }
            }
            _ => {
                if let Some(c) = text.filter(|_| !ctrl) {
                    self.completion = None;
                    self.body.insert_char(c);
                }
            }
        }
    }

    fn edit_body_key(
        &mut self,
        shell: &Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        if self.viewport_chord(shell, key, ctrl, alt, text) {
            return;
        }
        // Org's table chords, before the modes: `M-<arrow>` is a table
        // command *in* a table and nothing anywhere else, which is
        // `org-metaleft`'s own dispatch.
        if self.table_chord(key, ctrl, alt) {
            return;
        }
        match self.body.mode() {
            EditorMode::Insert => self.insert_key(shell, key, ctrl, alt, text),
            EditorMode::Normal | EditorMode::Visual | EditorMode::VisualLine => {
                // org's `org-cycle`. Three reports were somebody
                // pressing it: the drawers and the headings would not
                // toggle, and the only keys that did were `z a` and
                // `M-TAB`, neither of which is the one anybody tries.
                // In INSERT it stays org-tempo and completion, which is
                // what TAB is for while you are typing.
                if key == "tab" {
                    self.toggle_body_fold();
                    return;
                }
                if self.which_key_key(text) {
                    return;
                }
                if ctrl && key == "r" {
                    self.body.redo_local();
                    return;
                }
                // Esc on a quiet Normal surface used to leave the
                // editor — first always, then (once a typed paragraph
                // had been lost that way) only when the buffer was
                // clean. Neither is a rule you can build a habit
                // around: switching between INSERT and NORMAL is Esc,
                // the second press after a chord that "did nothing" is
                // a reflex, and a key that closes the buffer on some
                // presses and not others gets pressed twice anyway.
                //
                // In a modal mode Esc means NORMAL and nothing else.
                // `:q` closes. The friendly modes have no NORMAL for it
                // to mean, so there it still leaves a clean buffer.
                if key == "escape"
                    && self.body.mode() == EditorMode::Normal
                    && self.body.pending_stroke().is_none()
                    && self.body.pending_count() == 0
                {
                    if self.modal_editing() {
                        self.say("NORMAL — :q closes, :w saves, C-c C-c saves and closes");
                    } else {
                        self.escape_closes_buffer();
                    }
                } else if ctrl {
                    // `C-d`, `C-f`, `C-a` … are chords in their own
                    // right; dropping the modifier turned them into the
                    // plain letters and silently deleted a char.
                    self.body.modal_key(&format!("C-{key}"));
                } else {
                    self.body.modal_key(key);
                }
            }
        }
    }

    /// The body editor buffer (read).
    #[must_use]
    pub fn body_buffer(&self) -> &str {
        self.body.text()
    }

    /// The body editor's vim mode (for the mode indicator).
    #[must_use]
    pub const fn body_mode(&self) -> EditorMode {
        self.body.mode()
    }

    /// The body editor's chord in progress (`2d3i`), for the shell's
    /// mode chip. Empty when nothing is outstanding.
    #[must_use]
    pub fn body_pending_chord(&self) -> String {
        self.body.pending_chord()
    }

    /// Park the body editor's cursor at a byte offset (the mouse seam,
    /// and what a test needs to address a position directly).
    pub fn body_set_cursor(&mut self, byte: usize) {
        self.body.set_cursor_byte(byte);
    }

    /// The buffer's text scale — Doom's `text-scale`, which is a
    /// property of what you are reading rather than of the chrome
    /// around it.
    #[must_use]
    pub fn zoom(&self) -> f32 {
        ZOOM_STEP.powi(i32::from(self.zoom_steps))
    }

    /// One step larger (`C-+` / `C-=`).
    pub const fn zoom_in(&mut self) {
        if self.zoom_steps < ZOOM_MAX_STEPS {
            self.zoom_steps += 1;
        }
    }

    /// One step smaller (`C--`).
    pub const fn zoom_out(&mut self) {
        if self.zoom_steps > ZOOM_MIN_STEPS {
            self.zoom_steps -= 1;
        }
    }

    /// Back to unscaled (`C-0`).
    pub const fn zoom_reset(&mut self) {
        self.zoom_steps = 0;
    }

    /// Run `cmd` if it is one of the three zoom commands, reporting the
    /// new scale; `false` if it is some other command.
    ///
    /// One implementation for both routes: the outline runs it through
    /// [`Self::run_command`], the buffer through its own key path,
    /// and neither may drift from the other.
    fn zoom_command(&mut self, cmd: &str) -> bool {
        match cmd {
            "zoom-in" => self.zoom_in(),
            "zoom-out" => self.zoom_out(),
            "zoom-reset" => self.zoom_reset(),
            _ => return false,
        }
        self.say(format!("zoom {:.0}%", self.zoom() * 100.0));
        true
    }

    /// What the window should call itself right now.
    ///
    /// The title was set once, at window creation, and never moved: a
    /// closure window said the same thing whether it was showing an
    /// outline, editing a body, or holding an unsaved paragraph. It is
    /// the one piece of the app visible in a task switcher, so it says
    /// which buffer and whether that buffer is saved — the convention
    /// every other editor shares.
    #[must_use]
    pub fn window_title(&self, shell: &Shell, vault: &str) -> String {
        let dirty = if self.body_dirty() { "● " } else { "" };
        self.buffer_name(shell).map_or_else(
            || format!("{dirty}closure — {vault}"),
            |buffer| format!("{dirty}{buffer} — closure"),
        )
    }

    /// Insert `text` from *outside* the editor — the system clipboard —
    /// at the cursor, as one undoable edit.
    ///
    /// The window's paste types its characters as keystrokes, which is
    /// what keeps the slash menu, completion and table alignment in
    /// charge of it — and is exactly why it must not happen outside
    /// INSERT, where a pasted URL would be read as a dozen commands.
    /// This is the other path: text goes in as text, whatever mode the
    /// editor is in, and a VISUAL selection is replaced by it the way
    /// `p` replaces one.
    pub fn body_paste_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.completion = None;
        self.body.paste_external(text);
    }

    /// The body editor's open `/` search line, for the shell to paint.
    #[must_use]
    pub fn body_search_prompt(&self) -> Option<String> {
        self.body.search_prompt()
    }

    /// The pattern the last `/` or `?` searched for, so a shell can
    /// mark every hit rather than only moving the cursor to one.
    #[must_use]
    pub fn body_search_pattern(&self) -> Option<String> {
        self.body.search_pattern().map(ToOwned::to_owned)
    }

    /// Whether the body editor is in REPLACE (`R`) rather than INSERT —
    /// the same mode chip, a different word on it.
    #[must_use]
    pub const fn body_replacing(&self) -> bool {
        self.body.replacing()
    }

    /// The macro register the body editor is recording into, if any.
    #[must_use]
    pub const fn body_recording(&self) -> Option<char> {
        self.body.recording_register()
    }

    /// Candidates of the live completion cycle (empty when none) — the
    /// popup a GUI renders beside the caret.
    #[must_use]
    pub fn body_completion_items(&self) -> &[String] {
        self.completion.as_ref().map_or(&[], |s| &s.items)
    }

    /// Index of the currently-applied completion candidate.
    #[must_use]
    pub fn body_completion_ix(&self) -> Option<usize> {
        self.completion.as_ref().and_then(|s| s.ix)
    }

    /// `C-n`/`C-p`: start or continue a completion cycle over the word
    /// before the cursor — org keywords + vault dabbrev words
    /// ([`body_completions`]). Each step replaces the prefix/previous
    /// candidate in place; any other key ends the session.
    fn cycle_completion(&mut self, shell: &Shell, forward: bool) {
        if let Some(s) = &mut self.completion {
            let n = s.items.len();
            // From an unapplied popup the first step lands on the
            // first (or last) candidate instead of skipping it.
            let ix = match (s.ix, forward) {
                (Some(i), true) => (i + 1) % n,
                (Some(i), false) => (i + n - 1) % n,
                (None, true) => 0,
                (None, false) => n - 1,
            };
            s.ix = Some(ix);
            let (start, text) = (s.start, s.items[ix].clone());
            self.body.replace_to_cursor(start, &text);
            return;
        }
        let start = self.body.word_start();
        let prefix = self.body.word_prefix().to_owned();
        let mut items = body_completions(&prefix, &shell.vault);
        // The popup (and the cycle) shows the top 8 ranked candidates.
        items.truncate(8);
        if items.is_empty() {
            self.say("no completions");
            return;
        }
        let text = items[0].clone();
        self.body.replace_to_cursor(start, &text);
        self.completion = Some(CompletionSession {
            start,
            items,
            ix: Some(0),
        });
    }

    /// Candidates of the live *prompt* completion cycle — the popup a
    /// shell paints beside the prompt caret, empty when none.
    #[must_use]
    pub fn prompt_completion_items(&self) -> &[String] {
        self.prompt_completion.as_ref().map_or(&[], |s| &s.items)
    }

    /// Index of the applied prompt candidate.
    #[must_use]
    pub fn prompt_completion_ix(&self) -> Option<usize> {
        self.prompt_completion.as_ref().and_then(|s| s.ix)
    }

    /// The one-line field the open surface is typing into, if any.
    ///
    /// Five buffers back fourteen surfaces — capture, the shared prompt
    /// field, the list filter, the `:` line, the ticket box and the
    /// assistant's question — and nothing that reads a field should
    /// have to know which. This is the one place that mapping lives.
    const fn active_prompt(&mut self) -> Option<&mut LineInput> {
        match self.surface {
            ModalSurface::Capture => Some(&mut self.capture_buf),
            ModalSurface::Rename
            | ModalSurface::AddSibling
            | ModalSurface::TagsEdit
            | ModalSurface::PropertyEdit
            | ModalSurface::Palette
            | ModalSurface::InsertLink => Some(&mut self.field_buf),
            ModalSurface::Search
            | ModalSurface::BodySearch
            | ModalSurface::Buffers
            | ModalSurface::Files
            | ModalSurface::TagPick
            | ModalSurface::Refile
            // The list commands are pickers too — they narrow with the
            // same field rather than each growing one.
            | ModalSurface::Headlines
            | ModalSurface::Blocks
            | ModalSurface::UndoHistory
            | ModalSurface::DbView
            | ModalSurface::Graph
            | ModalSurface::FindFile
            | ModalSurface::Messages => Some(&mut self.query),
            ModalSurface::Ex => Some(&mut self.ex_buf),
            ModalSurface::Sync => Some(&mut self.sync_buf),
            ModalSurface::Llm => Some(&mut self.chat_buf),
            _ => None,
        }
    }

    /// The headlines of the selected file, narrowed by the filter.
    fn filtered_headlines(&self, shell: &Shell) -> Vec<HeadlineRow> {
        filtered(
            self.headline_rows(shell),
            self.prompt_text().unwrap_or_default(),
            |row| row.title.clone(),
        )
    }

    /// The vault's source blocks, narrowed by the filter.
    fn filtered_blocks(&self, shell: &Shell) -> Vec<BlockRow> {
        filtered(
            self.block_rows(shell),
            self.prompt_text().unwrap_or_default(),
            |b| format!("{} {} {}", b.file, b.lang, b.line),
        )
    }

    /// The selected file's undo tree, narrowed by the filter.
    fn filtered_history(&self, shell: &Shell) -> Vec<closure_core::HistoryRow> {
        filtered(
            self.undo_history_rows(shell),
            self.prompt_text().unwrap_or_default(),
            |r| r.label.clone(),
        )
    }

    /// How many rows the open picker is showing — what its cursor wraps
    /// around, and what a shell scrolls.
    #[must_use]
    pub fn picker_len(&self, shell: &Shell) -> usize {
        self.picker_view(shell).map_or(0, |v| v.rows.len())
    }

    /// Click row `i` of the open picker: put the cursor there, then
    /// pick it. The mouse path into every picker, so it agrees with
    /// Enter by construction rather than by two implementations.
    pub fn picker_click(&mut self, shell: &mut Shell, i: usize) {
        if self.picker_view(shell).is_none_or(|v| i >= v.rows.len()) {
            return;
        }
        match self.surface {
            ModalSurface::Palette => self.palette_cursor = i,
            ModalSurface::UndoHistory => self.hist_cursor = i,
            _ => self.selected = i,
        }
        self.pick_current(shell);
    }

    /// Act on the row the cursor is on. What Enter does in a picker,
    /// and what a click on a row does.
    fn pick_current(&mut self, shell: &mut Shell) {
        let at = self.picker_cursor();
        match self.surface {
            ModalSurface::Palette => self.commit_palette(shell),
            // "messages enter should copy the selected line to system
            // clipboard an internal 'clipboard' (kill-ring?)"
            //
            // The log is where a trace reading, an error or a saved
            // line ends up — all things you open it in order to paste
            // somewhere else. It goes into the same register `y`, `d`
            // and `C-k` use, which is the seam the system-clipboard
            // mirror already watches: one assignment reaches both,
            // rather than a second clipboard growing beside the first.
            ModalSurface::Messages => {
                let line = self
                    .picker_rows(shell)
                    .and_then(|(_, _, rows)| rows.get(at).map(|row| row.label.clone()));
                if let Some(line) = line {
                    self.set_register_from_clipboard(&line);
                    self.say(format!("copied: {line}"));
                }
            }
            ModalSurface::Buffers | ModalSurface::Files => {
                // The rows on screen are the ones that survived the
                // filter; the click paths address the underlying list.
                let matches: Vec<usize> = if self.surface == ModalSurface::Buffers {
                    self.buffer_rows(shell)
                        .iter()
                        .enumerate()
                        .filter(|(_, r)| r.matches_filter)
                        .map(|(i, _)| i)
                        .collect()
                } else {
                    self.file_rows(shell)
                        .iter()
                        .enumerate()
                        .filter(|(_, r)| r.matches_filter)
                        .map(|(i, _)| i)
                        .collect()
                };
                let Some(&row) = matches.get(at) else {
                    return;
                };
                self.query.clear();
                self.selected = 0;
                if self.surface == ModalSurface::Buffers {
                    self.buffer_click(shell, row);
                } else {
                    self.file_click(shell, row);
                }
            }
            ModalSurface::Headlines => {
                let Some(HeadlineRow { id, .. }) = self.filtered_headlines(shell).get(at).cloned()
                else {
                    return;
                };
                self.query.clear();
                self.go_home();
                self.select_by_id(shell, &id);
            }
            ModalSurface::Blocks => {
                let Some(BlockRow { file, .. }) = self.filtered_blocks(shell).get(at).cloned()
                else {
                    return;
                };
                self.query.clear();
                self.block_out = None;
                self.go_home();
                let path = std::path::PathBuf::from(&file);
                if let Some(idx) = self
                    .rows_shared(shell)
                    .iter()
                    .position(|r| std::path::Path::new(&r.path) == path)
                {
                    self.selected = idx;
                }
            }
            ModalSurface::UndoHistory => {
                // The picker lists the tree in walk order and narrows
                // it; the vault addresses history nodes by insertion
                // order, so the row carries the one to send.
                let Some(index) = self.filtered_history(shell).get(at).map(|r| r.index) else {
                    return;
                };
                self.query.clear();
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let path = std::path::PathBuf::from(&row.path);
                    match shell.vault.jump_history_in(&path, index) {
                        Ok(()) => self.say("jumped"),
                        Err(e) => self.status = format!("jump failed: {e}"),
                    }
                    self.selected = self
                        .selected
                        .min(self.rows_shared(shell).len().saturating_sub(1));
                }
                self.go_home();
            }
            _ => {}
        }
    }

    /// The floating picker for the open surface, or `None` when the
    /// surface is not one.
    ///
    /// Every list the user reaches for by name — the palette, the
    /// buffers, the headlines of this file, its source blocks, its undo
    /// tree, the recent files — is the same gesture: narrow, then pick.
    /// Deriving them all here is what makes them behave the same
    /// without each shell agreeing to (I7).
    #[must_use]
    pub fn picker_view(&self, shell: &Shell) -> Option<PickerView> {
        let (title, hint, mut rows) = self.picker_rows(shell)?;
        // Which characters of each label the filter matched, marked
        // once here rather than by each surface: every picker filters
        // the same way, so every picker highlights the same way.
        let filter = self.prompt_text().unwrap_or_default();
        if !filter.trim().is_empty() {
            for row in &mut rows {
                row.matches = closure_query::match_spans(filter, &row.label);
            }
        }
        // A cursor pointing past the narrowed list is how a picker
        // opens the wrong thing, so it is clamped where it is read
        // rather than everywhere it is moved.
        let cursor = self.picker_cursor().min(rows.len().saturating_sub(1));
        if let Some(row) = rows.get_mut(cursor) {
            row.current = true;
        }
        Some(PickerView {
            title: title.to_owned(),
            hint: hint.to_owned(),
            rows,
            cursor,
        })
    }

    /// `C-c C-l`'s three steps as picker rows: the types, then what
    /// the destination can be completed to, then nothing — a
    /// description is prose and has no candidates.
    fn link_pick_rows(&self, shell: &Shell) -> (&'static str, &'static str, Vec<PickRow>) {
        let row = |label: String, trailing: String| PickRow {
            label,
            detail: String::new(),
            trailing,
            matches: Vec::new(),
            current: false,
        };
        if self.link_kind.is_none() {
            return (
                "link type",
                "TAB or RET picks \u{b7} Esc cancels",
                self.link_types()
                    .into_iter()
                    .map(|kind| row(kind, String::new()))
                    .collect(),
            );
        }
        if self.link_dest.is_some() {
            return (
                "what to call it",
                "RET \u{b7} empty shows the link itself",
                Vec::new(),
            );
        }
        (
            "where it goes",
            "TAB completes \u{b7} RET \u{b7} Esc cancels",
            self.link_completions(shell)
                .into_iter()
                .map(|c| row(c.label, c.value))
                .collect(),
        )
    }

    /// Every headline in the vault, as picker rows.
    ///
    /// The db-view's four columns become the picker's three fields:
    /// the title is what you are looking for, the keyword marks it,
    /// and the priority and tags say the rest.
    fn db_pick_rows(&self, shell: &Shell) -> Vec<PickRow> {
        use std::fmt::Write as _;
        let rows = shell
            .vault
            .iter()
            .flat_map(|(_, doc)| doc.all_headlines())
            .map(|h| {
                let mut detail = String::new();
                if let Some(p) = h.priority() {
                    detail.push_str(&priority_cookie(p));
                }
                if !h.tags().is_empty() {
                    if !detail.is_empty() {
                        detail.push(' ');
                    }
                    let _ = write!(detail, ":{}:", h.tags().join(":"));
                }
                PickRow {
                    label: h.title().to_owned(),
                    detail,
                    trailing: h.todo().unwrap_or_default().to_owned(),
                    matches: Vec::new(),
                    current: false,
                }
            })
            .collect();
        Self::narrow(self.prompt_text().unwrap_or_default(), rows)
    }

    /// The link graph as picker rows.
    ///
    /// The old pane's three labelled sections survive as the trailing
    /// field: a flat list that lost them would be a worse pane, not a
    /// better one.
    fn graph_pick_rows(&self, shell: &Shell) -> Vec<PickRow> {
        let row = |label: String, detail: String, kind: &str| PickRow {
            label,
            detail,
            trailing: kind.to_owned(),
            matches: Vec::new(),
            current: false,
        };
        let mut rows: Vec<PickRow> = Vec::new();
        for (_, title, n) in self.hub_rows(shell) {
            rows.push(row(title, format!("{n} link(s) in"), "hub"));
        }
        for (_, title) in self.orphan_rows(shell) {
            rows.push(row(title, "nothing links here".to_owned(), "orphan"));
        }
        for text in self.dead_link_rows(shell) {
            rows.push(row(text, "points at nothing".to_owned(), "dead link"));
        }
        Self::narrow(self.prompt_text().unwrap_or_default(), rows)
    }

    /// Keep the rows whose label the filter matches.
    ///
    /// Each picker's rows-builder narrows its own list, which is why
    /// two of them shipped without narrowing at all: the db-view and
    /// the graph came from panes that had no filter to forget.
    fn narrow(filter: &str, rows: Vec<PickRow>) -> Vec<PickRow> {
        if filter.trim().is_empty() {
            return rows;
        }
        rows.into_iter()
            .filter(|r| closure_query::fuzzy_score(filter, &r.label).is_some())
            .collect()
    }

    /// The open buffers as picker rows; dirty ones say so and the one
    /// on screen carries a dot.
    fn buffer_pick_rows(&self, shell: &Shell) -> Vec<PickRow> {
        self.buffer_rows(shell)
            .into_iter()
            .filter(|r| r.matches_filter)
            .map(|r| PickRow {
                label: r.name,
                detail: if r.dirty {
                    "unsaved".to_owned()
                } else {
                    String::new()
                },
                trailing: if r.current {
                    "\u{25cf}".to_owned()
                } else {
                    String::new()
                },
                matches: Vec::new(),
                current: false,
            })
            .collect()
    }

    /// The recent files as picker rows.
    fn file_pick_rows(&self, shell: &Shell) -> Vec<PickRow> {
        self.file_rows(shell)
            .into_iter()
            .filter(|r| r.matches_filter)
            .map(|r| PickRow {
                label: r.name,
                detail: r.path.display().to_string(),
                trailing: String::new(),
                matches: Vec::new(),
                current: false,
            })
            .collect()
    }

    /// What the open surface is picking from: its title, what Enter
    /// does, and the rows surviving the filter.
    fn picker_rows(&self, shell: &Shell) -> Option<(&'static str, &'static str, Vec<PickRow>)> {
        let (title, hint, rows) = match self.surface {
            ModalSurface::Palette => (
                "commands",
                "RET runs",
                self.palette_shared()
                    .iter()
                    .map(|e| PickRow {
                        label: e.label.clone(),
                        detail: e.description.clone(),
                        // Every key that runs it, not the first one the
                        // keymap happens to list: the palette is where
                        // you go when you do not know the key, so it is
                        // the one place a second key is worth learning.
                        trailing: e.action.chords().join("  ·  "),
                        matches: Vec::new(),
                        current: false,
                    })
                    .collect::<Vec<_>>(),
            ),
            ModalSurface::Buffers => (
                "buffers",
                "RET opens \u{b7} the one you are in is marked",
                self.buffer_pick_rows(shell),
            ),
            ModalSurface::Files => ("files", "RET opens", self.file_pick_rows(shell)),
            ModalSurface::InsertLink => self.link_pick_rows(shell),
            ModalSurface::Headlines => (
                "headlines in this file",
                "RET goes to it",
                self.filtered_headlines(shell)
                    .into_iter()
                    .map(|HeadlineRow { title, id }| PickRow {
                        label: title,
                        detail: String::new(),
                        trailing: id,
                        matches: Vec::new(),
                        current: false,
                    })
                    .collect(),
            ),
            ModalSurface::Blocks => (
                "source blocks",
                "RET goes to the file it is in",
                self.filtered_blocks(shell)
                    .into_iter()
                    .map(|BlockRow { file, lang, line }| PickRow {
                        label: line,
                        // Vault-relative: the absolute path of every
                        // file in the vault you are looking at is
                        // mostly the same prefix, repeated down the
                        // list and pushing the part that differs off
                        // the end of the row.
                        detail: std::path::Path::new(&file)
                            .strip_prefix(shell.vault.root())
                            .map_or_else(|_| file.clone(), |rel| rel.display().to_string()),
                        trailing: lang,
                        matches: Vec::new(),
                        current: false,
                    })
                    .collect(),
            ),
            ModalSurface::Messages => (
                "messages",
                "the newest is first",
                filtered(
                    self.messages.clone(),
                    self.prompt_text().unwrap_or_default(),
                    Clone::clone,
                )
                .into_iter()
                .map(|text| PickRow {
                    label: text,
                    detail: String::new(),
                    trailing: String::new(),
                    matches: Vec::new(),
                    current: false,
                })
                .collect(),
            ),
            // The last two surfaces painting a list of their own
            // design. A db-view is headlines and a graph is headlines,
            // so both are pickers like every other list of them — one
            // filter, one set of chords, one look.
            ModalSurface::DbView => ("db", "RET jumps to it", self.db_pick_rows(shell)),
            ModalSurface::FindFile => (
                "find file",
                "RET opens, or makes what is not there",
                self.find_file_rows(shell),
            ),
            ModalSurface::Graph => ("graph", "RET jumps to it", self.graph_pick_rows(shell)),
            ModalSurface::UndoHistory => (
                "undo history",
                "RET jumps the document to that edit",
                self.filtered_history(shell)
                    .into_iter()
                    .map(|r| PickRow {
                        label: format!("{}{}", r.graph, r.label),
                        detail: String::new(),
                        trailing: if r.is_current {
                            "now".to_owned()
                        } else {
                            String::new()
                        },
                        matches: Vec::new(),
                        current: false,
                    })
                    .collect(),
            ),
            _ => return None,
        };
        Some((title, hint, rows))
    }

    /// Which row of the open picker the cursor is on.
    ///
    /// The palette and the undo tree have always kept their own. The
    /// rest fell through to `selected`, which is the *outline's*
    /// selection — so walking the message log walked the notes behind
    /// it: "scrolling messages scrolls the background outline tree view
    /// as well".
    ///
    /// The test is whether the list is showing you the outline. Search
    /// filters the outline and Enter opens the row, so its cursor *is*
    /// the outline's and stays that way. A log, a block list or a table
    /// is showing you something else.
    #[must_use]
    pub const fn picker_cursor(&self) -> usize {
        match self.surface {
            ModalSurface::Palette => self.palette_cursor,
            ModalSurface::UndoHistory => self.hist_cursor,
            _ if Self::picker_has_own_cursor(self.surface) => self.pane_cursor,
            _ => self.selected,
        }
    }

    /// Whether this surface's list is something other than the outline.
    const fn picker_has_own_cursor(surface: ModalSurface) -> bool {
        matches!(
            surface,
            ModalSurface::Messages
                | ModalSurface::Blocks
                | ModalSurface::Headlines
                | ModalSurface::DbView
                | ModalSurface::Graph
                | ModalSurface::Agenda
                | ModalSurface::Backlinks
        )
    }

    /// [`Self::active_prompt`] without the borrow, for the shells.
    /// Which history this surface draws on, or `None` for a surface
    /// that is not a prompt.
    ///
    /// The pickers share the outline's filter field but not its
    /// history: a filter typed to find a buffer is not a candidate for
    /// the one that finds a file.
    const fn prompt_kind(surface: ModalSurface) -> Option<&'static str> {
        Some(match surface {
            ModalSurface::Capture => "capture",
            ModalSurface::Rename => "rename",
            ModalSurface::AddSibling => "heading",
            ModalSurface::TagsEdit | ModalSurface::TagPick => "tags",
            ModalSurface::PropertyEdit => "property",
            ModalSurface::Search | ModalSurface::BodySearch => "search",
            ModalSurface::Ex => "ex",
            ModalSurface::Llm => "llm",
            ModalSurface::Refile => "refile",
            ModalSurface::InsertLink => "link",
            _ => return None,
        })
    }

    /// The field a prompt types into, mutably.
    const fn prompt_mut(&mut self) -> Option<&mut LineInput> {
        match self.surface {
            ModalSurface::Capture => Some(&mut self.capture_buf),
            ModalSurface::Rename
            | ModalSurface::AddSibling
            | ModalSurface::TagsEdit
            | ModalSurface::PropertyEdit
            | ModalSurface::Palette
            | ModalSurface::InsertLink => Some(&mut self.field_buf),
            ModalSurface::Search
            | ModalSurface::BodySearch
            | ModalSurface::Buffers
            | ModalSurface::Files
            | ModalSurface::TagPick
            | ModalSurface::Refile
            | ModalSurface::Headlines
            | ModalSurface::Blocks
            | ModalSurface::UndoHistory
            | ModalSurface::DbView
            | ModalSurface::Graph
            | ModalSurface::FindFile
            | ModalSurface::Messages => Some(&mut self.query),
            ModalSurface::Ex => Some(&mut self.ex_buf),
            ModalSurface::Sync => Some(&mut self.sync_buf),
            ModalSurface::Llm => Some(&mut self.chat_buf),
            _ => None,
        }
    }

    /// Remember what this prompt was holding, whichever door it left
    /// by. Called on the way out of every prompt surface.
    fn remember_prompt(&mut self) {
        const KEEP: usize = 100;
        let Some(kind) = Self::prompt_kind(self.surface) else {
            return;
        };
        let Some(text) = self.prompt().map(|f| f.text().to_owned()) else {
            return;
        };
        self.history_walk = None;
        if text.trim().is_empty() {
            return;
        }
        let ring = self.prompt_history.entry(kind).or_default();
        // A repeat is one entry: a history of the same word five times
        // is four keystrokes between you and the one before it.
        ring.retain(|e| *e != text);
        ring.insert(0, text);
        ring.truncate(KEEP);
    }

    /// The words around the open prompt's field, or `None` when the
    /// surface is not a prompt.
    #[must_use]
    pub fn prompt_chrome(&self, shell: &Shell) -> Option<PromptChrome> {
        use PromptTone as T;
        let rows = self.rows_shared(shell).len();
        let (label, hint, tone, icon) = match self.surface {
            ModalSurface::Rename => ("rename".to_owned(), String::new(), T::Edit, "\u{f044}"),
            // One prompt serves all four new-headline chords, so it has
            // to say which one opened it.
            ModalSurface::AddSibling => (
                format!("new {}", self.new_heading_kind()),
                String::new(),
                T::Edit,
                "\u{f067}",
            ),
            ModalSurface::TagsEdit => ("tags".to_owned(), String::new(), T::Edit, "\u{f02c}"),
            ModalSurface::PropertyEdit => {
                ("property".to_owned(), String::new(), T::Edit, "\u{f013}")
            }
            ModalSurface::Capture => (
                "capture".to_owned(),
                self.capture_target_label(shell),
                T::Edit,
                "\u{f040}",
            ),
            ModalSurface::Ex => (
                "command".to_owned(),
                ":w :q :wq :x, or any command name".to_owned(),
                T::Command,
                "\u{f120}",
            ),
            ModalSurface::Search => (
                "search".to_owned(),
                format!("{rows} match(es)"),
                T::Filter,
                "\u{f002}",
            ),
            ModalSurface::BodySearch => (
                "body".to_owned(),
                format!("{} line(s)", self.body_search_rows(shell).len()),
                T::Filter,
                "\u{f002}",
            ),
            ModalSurface::FindFile => (
                "find file".to_owned(),
                "RET opens \u{b7} a new name makes it".to_owned(),
                T::Target,
                "\u{f07b}",
            ),
            ModalSurface::Refile => (
                "refile to".to_owned(),
                "RET files it here".to_owned(),
                T::Target,
                "\u{f07b}",
            ),
            ModalSurface::TagPick => (
                "tags".to_owned(),
                "SPC toggles \u{b7} RET writes".to_owned(),
                T::Target,
                "\u{f02c}",
            ),
            ModalSurface::Buffers => (
                "buffers".to_owned(),
                format!("{} open \u{b7} RET opens", self.buffer_rows(shell).len()),
                T::Filter,
                "\u{f0c5}",
            ),
            ModalSurface::Files => (
                "files".to_owned(),
                format!(
                    "{} in this vault \u{b7} RET opens",
                    self.file_rows(shell).len()
                ),
                T::Filter,
                "\u{f15c}",
            ),
            _ => return None,
        };
        Some(PromptChrome {
            label,
            hint,
            tone,
            icon,
            // Only the new-headline prompt applies one, and only when
            // the chord that opened it was the TODO variant.
            keyword: (self.surface == ModalSurface::AddSibling && self.new_heading.todo).then(
                || {
                    shell
                        .vault
                        .todo_keywords()
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "TODO".to_owned())
                },
            ),
        })
    }

    /// How many times a vault switch has been asked for.
    ///
    /// The window watches this the way it watches the register's
    /// generation: a chord, a palette entry and a click on the header
    /// all arrive the same way, and none of them can raise a dialog
    /// from in here.
    #[must_use]
    pub const fn vault_switch_asked(&self) -> u64 {
        self.vault_switch_asked
    }

    /// The id of the headline the file buffer's caret is inside.
    ///
    /// Most of a file is body text, so a caret on prose means the
    /// headline it belongs to — the nearest one at or above the line —
    /// rather than nothing.
    fn headline_at_caret(&self, shell: &Shell) -> Option<String> {
        let path = self.file_target.clone()?;
        let (line, _) = self.body.cursor_line_col();
        let text = self.body.text();
        let stars = text
            .split('\n')
            .take(line + 1)
            .enumerate()
            .filter(|(_, l)| l.starts_with('*'))
            .map(|(i, _)| i)
            .last()?;
        // The nth headline of the file, by document order: the buffer
        // holds exactly what is on disk, so counting stars is the same
        // walk the parser does.
        let nth = text
            .split('\n')
            .take(stars)
            .filter(|l| l.starts_with('*'))
            .count();
        shell
            .vault
            .document(&path)?
            .all_headlines()
            .nth(nth)
            .map(|h| h.id().to_string())
    }

    /// Put the file buffer's caret on the headline the outline had
    /// selected.
    fn caret_to_selected_headline(&mut self, shell: &Shell) {
        let Some(row) = self.rows_shared(shell).get(self.selected).cloned() else {
            return;
        };
        let text = self.body.text().to_owned();
        // The headline's own line, found by its title on a star line:
        // the buffer is the file, so the title is on exactly one of
        // them unless a vault repeats a heading verbatim — in which
        // case the first is as good an answer as any.
        let Some(line) = text
            .split('\n')
            .position(|l| l.starts_with('*') && l.contains(&row.title))
        else {
            return;
        };
        self.body.goto_line_col(line, 0);
    }

    /// Put an open buffer into the editing mode the new keymap
    /// implies.
    ///
    /// Notion and Emacs have no NORMAL, so a buffer left in one after
    /// a switch is a text field that will not take text — the worst
    /// thing the friendliest mode in the app could do.
    fn settle_editor_mode(&mut self) {
        if !self.surface.is_editor() {
            return;
        }
        if self.modal_editing() {
            self.body.to_normal();
        } else {
            self.body.to_insert();
        }
    }

    /// Run `command` with `arg` — what the `:` line does when a line
    /// has a space in it.
    pub fn run_with_arg(&mut self, shell: &mut Shell, command: &str, arg: &str) {
        self.command_arg = Some(arg.to_owned());
        self.run(shell, command);
        self.command_arg = None;
    }

    /// The argument the running command was given, if any.
    fn arg(&self) -> Option<&str> {
        self.command_arg.as_deref()
    }

    /// The directory a `:open-vault <dir>` named, if the ask came that
    /// way rather than through the dialog.
    pub const fn take_vault_switch_path(&mut self) -> Option<String> {
        self.vault_switch_path.take()
    }

    /// May the app change vaults right now?
    ///
    /// An unwritten buffer belongs to the vault being left, so
    /// switching away from one is throwing it away — the one thing
    /// this must not do quietly.
    #[must_use]
    pub fn can_switch_vault(&self) -> bool {
        !self.body_dirty()
    }

    /// Drop everything that belonged to the vault being left.
    ///
    /// Ids, selections, marks and buffers all name things in one
    /// vault; carried into another they point at nothing, or worse at
    /// something else with the same index.
    pub fn reset_for_vault(&mut self) {
        self.marks.clear();
        self.selected = 0;
        self.query.clear();
        self.field_buf.clear();
        self.edit_target = None;
        self.file_target = None;
        self.special = None;
        self.body.clear();
        self.body_baseline.clear();
        self.body_folds.clear();
        self.body_stash.clear();
        self.buffers.clear();
        self.pane_return = None;
        self.find_dir = std::path::PathBuf::new();
        self.invalidate_rows();
        self.surface = ModalSurface::Browse;
    }

    /// The picture the full-size view is showing, if it is open.
    #[must_use]
    pub fn image_shown(&self) -> Option<&std::path::Path> {
        self.image_view.as_deref()
    }

    /// The colour a diagram is drawn in — the shell's own
    /// foreground, reported the way the viewport is
    /// ([`Self::set_ink`]). A renderer that picked its own would
    /// draw black maths on a dark editor, which is what it did.
    #[must_use]
    pub const fn ink(&self) -> u32 {
        self.ink
    }

    /// Tell the core what colour this shell writes in.
    pub const fn set_ink(&mut self, ink: u32) {
        self.ink = ink;
    }

    /// Where rendered diagrams are kept.
    ///
    /// Under the vault's own dot-directory, not beside the notes: a
    /// rendered picture is a build artefact, and dropping derived
    /// files among the org files puts them into the thing the user
    /// syncs, greps and reads.
    #[must_use]
    pub fn diagram_cache(&self, shell: &Shell) -> std::path::PathBuf {
        shell.vault.root().join(".closure").join("diagrams")
    }

    /// The rendered pictures for the open buffer: `(line, file)`, one
    /// per diagram block that has already been rendered.
    ///
    /// Deliberately a *lookup* and never a render. The painter calls
    /// this, and a painter that shelled out would run `mmdc` once per
    /// diagram per frame. Nothing is returned while the picture toggle
    /// is off, for the same reason inline images vanish: one toggle
    /// for everything painted between the lines.
    #[must_use]
    pub fn diagram_previews(&self, shell: &Shell) -> Vec<(usize, std::path::PathBuf)> {
        if !self.images_shown {
            return Vec::new();
        }
        let cache = self.diagram_cache(shell);
        diagram_blocks(self.body.text())
            .into_iter()
            .filter_map(|b| {
                let kind = closure_eval::diagram_for(&b.lang)?;
                let path = closure_eval::diagram_path(&cache, kind, &b.src, self.ink);
                path.is_file().then_some((b.line, path))
            })
            .collect()
    }

    /// `preview-diagrams`: render every diagram block in the buffer.
    ///
    /// org's `C-c C-x C-l` — you ask for the pictures, they appear.
    /// Rendering runs a program, so it goes through the *same*
    /// eval-trust allowlist as `C-c C-c` and is default-deny like it:
    /// a picture is not a reason to skip the gate. A refusal names the
    /// language and the fix, because a refusal that names only the
    /// concept has already been reported once.
    fn preview_diagrams(&mut self, shell: &Shell) {
        let blocks = diagram_blocks(self.body.text());
        if blocks.is_empty() {
            self.say("no mermaid or latex blocks in this buffer");
            return;
        }
        let cache = self.diagram_cache(shell);
        let trust = shell.vault.eval_trust();
        let (mut drawn, mut cached) = (0usize, 0usize);
        for block in blocks {
            let Some(kind) = closure_eval::diagram_for(&block.lang) else {
                continue;
            };
            if !closure_eval::eval_allowed(&trust, &block.lang) {
                self.say(Self::trust_refusal(shell, &block.lang));
                return;
            }
            if closure_eval::diagram_path(&cache, kind, &block.src, self.ink).is_file() {
                cached += 1;
                continue;
            }
            let tool = Self::diagram_tool(shell, kind);
            match closure_eval::render_diagram(kind, &block.src, &cache, &tool, self.ink) {
                Ok(_) => drawn += 1,
                // Named, with somewhere to get it. Silence here is the
                // one outcome that leaves nothing to act on.
                Err(e) => {
                    self.status = format!("{e}");
                    return;
                }
            }
        }
        self.say(match (drawn, cached) {
            (0, n) => format!("{n} diagram(s) already drawn"),
            (n, 0) => format!("drew {n} diagram(s)"),
            (n, c) => format!("drew {n}, {c} already drawn"),
        });
    }

    /// The program that renders `kind` — config.org's override, or the
    /// language's default. A user with a wrapper script or a pinned
    /// version is not arguing with us about it.
    fn diagram_tool(shell: &Shell, kind: closure_eval::Diagram) -> String {
        shell
            .vault
            .diagram_tool(kind.language())
            .unwrap_or_else(|| kind.tool().to_owned())
    }

    /// Show `path` as large as the window will make it.
    pub fn show_image(&mut self, path: std::path::PathBuf) {
        self.image_return = Some(self.surface);
        self.image_view = Some(path);
        self.surface = ModalSurface::ImageView;
    }

    /// Is this headline marked for a bulk action?
    #[must_use]
    pub fn is_marked(&self, id: &str) -> bool {
        self.marks.contains(id)
    }

    /// How many headlines are marked.
    #[must_use]
    pub fn marked_count(&self) -> usize {
        self.marks.len()
    }

    /// The ids an action applies to: the marks when there are any, and
    /// the row under the cursor when there are none.
    ///
    /// dired's own rule, and the thing that makes `D` safe to press
    /// without first checking what is marked.
    fn action_targets(&self, shell: &Shell) -> Vec<String> {
        if !self.marks.is_empty() {
            return self.marks.iter().cloned().collect();
        }
        self.rows_shared(shell)
            .get(self.selected)
            .map(|r| r.id.clone())
            .into_iter()
            .collect()
    }

    /// How many times the editor's unnamed register has changed.
    ///
    /// What a shell watches to decide whether to write the system
    /// clipboard: without it the mirror would write on every keystroke
    /// and fight whatever else owns the selection.
    #[must_use]
    pub const fn register_generation(&self) -> u64 {
        self.body.register_generation()
    }

    /// What a bare `p` would paste.
    #[must_use]
    pub fn register_text(&self) -> &str {
        self.body.register_text()
    }

    /// Put the system clipboard into the register, so `p` pastes it.
    pub fn set_register_from_clipboard(&mut self, text: &str) {
        self.body.set_register_from_clipboard(text);
    }

    /// How many entries this prompt's history holds.
    ///
    /// A prompt that can recall something says so: a feature nothing
    /// mentions is a feature nobody presses, and this one exists for
    /// the moment *after* the mistake, when you are not exploring.
    #[must_use]
    pub fn prompt_history_len(&self) -> usize {
        Self::prompt_kind(self.surface)
            .and_then(|k| self.prompt_history.get(k))
            .map_or(0, Vec::len)
    }

    /// `M-p` / `M-n`: walk this prompt's history.
    ///
    /// Emacs's own minibuffer keys, and it has to be those rather than
    /// `C-p`/`C-n` — those are the completion cycle in a prompt and the
    /// list walk in a picker.
    fn walk_history(&mut self, back: bool) -> bool {
        let Some(kind) = Self::prompt_kind(self.surface) else {
            return false;
        };
        let ring = self.prompt_history.get(kind).cloned().unwrap_or_default();
        if ring.is_empty() {
            return false;
        }
        let current = self
            .prompt()
            .map(|f| f.text().to_owned())
            .unwrap_or_default();
        let (at, draft) = match self.history_walk.take() {
            Some((i, draft)) => (Some(i), draft),
            // Starting a walk: the line you were typing is the thing to
            // come back to, not the first entry.
            None => (None, current),
        };
        let next = match (at, back) {
            (None, true) => Some(0),
            (Some(i), true) => Some((i + 1).min(ring.len() - 1)),
            (Some(i), false) if i > 0 => Some(i - 1),
            // Forward off the newest entry, or forward without a walk
            // in progress: back to the line you were typing.
            (None | Some(_), false) => None,
        };
        let text = match next {
            Some(i) => {
                self.history_walk = Some((i, draft));
                ring[i].clone()
            }
            None => draft,
        };
        if let Some(field) = self.prompt_mut() {
            field.set_text(&text);
        }
        true
    }

    const fn prompt(&self) -> Option<&LineInput> {
        match self.surface {
            ModalSurface::Capture => Some(&self.capture_buf),
            ModalSurface::Rename
            | ModalSurface::AddSibling
            | ModalSurface::TagsEdit
            | ModalSurface::PropertyEdit
            | ModalSurface::Palette
            // All three of `C-c C-l`'s steps type into one field: the
            // step is which of them is open, not which buffer it uses.
            | ModalSurface::InsertLink => Some(&self.field_buf),
            ModalSurface::Search
            | ModalSurface::BodySearch
            | ModalSurface::Buffers
            | ModalSurface::Files
            | ModalSurface::TagPick
            | ModalSurface::Refile
            // The list commands are pickers too — they narrow with the
            // same field rather than each growing one.
            | ModalSurface::Headlines
            | ModalSurface::Blocks
            | ModalSurface::UndoHistory
            | ModalSurface::DbView
            | ModalSurface::Graph
            | ModalSurface::FindFile
            | ModalSurface::Messages => Some(&self.query),
            ModalSurface::Ex => Some(&self.ex_buf),
            ModalSurface::Sync => Some(&self.sync_buf),
            ModalSurface::Llm => Some(&self.chat_buf),
            _ => None,
        }
    }

    /// The text in whichever field is open, or `None` on a surface with
    /// no field. What a shell paints and what a test asserts on, so
    /// neither has to name a buffer per surface.
    #[must_use]
    pub fn prompt_text(&self) -> Option<&str> {
        self.prompt().map(LineInput::text)
    }

    /// The caret's byte offset in that field; zero when there is none.
    #[must_use]
    pub const fn prompt_cursor(&self) -> usize {
        match self.prompt() {
            Some(field) => field.cursor(),
            None => 0,
        }
    }

    /// `C-n`/`C-p` in a prompt: start or continue a completion cycle
    /// over the word being typed — the body editor's gesture, over the
    /// prompt instead ([`prompt_completions`]).
    fn cycle_prompt_completion(&mut self, shell: &Shell, forward: bool) {
        if let Some(s) = self.prompt_completion.clone() {
            let n = s.items.len();
            let ix = match (s.ix, forward) {
                (Some(i), true) => (i + 1) % n,
                (Some(i), false) => (i + n - 1) % n,
                (None, true) => 0,
                (None, false) => n - 1,
            };
            let text = s.items[ix].clone();
            if let Some(field) = self.active_prompt() {
                field.replace_to_cursor(s.start, &text);
            }
            if let Some(open) = self.prompt_completion.as_mut() {
                open.ix = Some(ix);
            }
            return;
        }
        let Some(field) = self.active_prompt() else {
            return;
        };
        let (start, prefix) = (field.prefix_start(), field.word_prefix().to_owned());
        let mut items = prompt_completions(&prefix, &shell.vault);
        items.truncate(8);
        if items.is_empty() {
            return;
        }
        // Backwards from nothing lands on the last candidate, the way
        // the editor's cycle does.
        let ix = if forward { 0 } else { items.len() - 1 };
        let text = items[ix].clone();
        if let Some(field) = self.active_prompt() {
            field.replace_to_cursor(start, &text);
        }
        self.prompt_completion = Some(CompletionSession {
            start,
            items,
            ix: Some(ix),
        });
    }

    /// Apply candidate `ix` and end the cycle — what clicking one in
    /// the strip does. Out of range is a no-op.
    pub fn pick_prompt_completion(&mut self, ix: usize) {
        let Some(session) = self.prompt_completion.take() else {
            return;
        };
        let Some(text) = session.items.get(ix).cloned() else {
            self.prompt_completion = Some(session);
            return;
        };
        if let Some(field) = self.active_prompt() {
            field.replace_to_cursor(session.start, &text);
        }
    }

    /// TAB in a prompt: accept.
    ///
    /// With a cycle open the candidate on screen stands and the popup
    /// closes; with none, TAB is what starts one and takes the first
    /// candidate — which is what TAB means in every other text field on
    /// the desktop, and a one-line title prompt has no indentation for
    /// it to mean instead.
    fn accept_prompt_completion(&mut self, shell: &Shell) {
        if self.prompt_completion.take().is_none() {
            self.cycle_prompt_completion(shell, true);
            self.prompt_completion = None;
        }
    }

    /// Whether the GUI should auto-open the completion popup after its
    /// typing-idle delay: INSERT in the body editor, no session yet, a
    /// word prefix of at least 3 chars with candidates behind it.
    #[must_use]
    pub fn completion_should_popup(&self, shell: &Shell) -> bool {
        // Every editing surface, not just the pane one: the editor
        // *view* holds the same buffer under a different surface, and
        // pinning this to `EditBody` is why completion never appeared
        // there.
        self.surface.is_editor()
            && self.body.mode() == EditorMode::Insert
            && self.completion.is_none()
            && self.body.word_prefix().chars().count() >= 3
            && !body_completions(self.body.word_prefix(), &shell.vault).is_empty()
    }

    /// Open the completion popup without applying anything: candidates
    /// show, the buffer stays untouched until `C-n`/`C-p`/TAB.
    pub fn open_completion_popup(&mut self, shell: &Shell) {
        if !self.completion_should_popup(shell) {
            return;
        }
        let start = self.body.word_start();
        let mut items = body_completions(self.body.word_prefix(), &shell.vault);
        items.truncate(8);
        self.completion = Some(CompletionSession {
            start,
            items,
            ix: None,
        });
    }

    /// Where the caret is, one-based, or `None` outside a buffer.
    ///
    /// "show cursor position in editor view (like line and row)". The
    /// buffer knew all along — the gutter is built from it — and
    /// nothing said so, which made "which line am I on?" a question
    /// you answered by counting.
    ///
    /// One-based to agree with the gutter two columns to its left. A
    /// zero-based column printed beside a one-based line number is a
    /// small lie told constantly.
    #[must_use]
    pub fn cursor_position(&self) -> Option<(usize, usize)> {
        if !self.surface.is_editor() {
            // The outline has a selection, not a caret. A line and
            // column there would describe something not on screen.
            return None;
        }
        let (line, col) = self.body_cursor();
        Some((line + 1, col + 1))
    }

    /// The same, as the short label a status bar paints.
    ///
    /// A bare `3:5` among a row of counts is ambiguous, so the string
    /// carries its own meaning and every shell paints the same one.
    #[must_use]
    pub fn cursor_position_label(&self) -> Option<String> {
        let (line, col) = self.cursor_position()?;
        Some(format!("L{line}:C{col}"))
    }

    /// The body editor cursor as zero-based `(line, column)`.
    #[must_use]
    pub fn body_cursor(&self) -> (usize, usize) {
        self.body.cursor_line_col()
    }

    /// The body editor's Visual selection byte range for the renderer.
    #[must_use]
    pub fn body_selection(&self) -> Option<(usize, usize)> {
        self.body.visual_selection()
    }

    /// Commit the body buffer to the target headline through the kernel
    /// command (I8), then return to Browse. No-op if not editing.
    pub fn commit_edit_body(&mut self, shell: &mut Shell) {
        // The buffer is gone; a pane must not offer to put it back.
        self.pane_return = None;
        self.write_body(shell);
        self.remember_body_cursor();
        self.edit_target = None;
        self.body.clear();
        self.body_baseline.clear();
        self.surface = ModalSurface::Browse;
    }

    /// Esc in a non-modal mode: close a clean buffer, refuse a
    /// modified one and say what saves and what discards.
    fn escape_closes_buffer(&mut self) {
        if self.body_dirty() {
            self.say("unsaved edit — C-c C-c or :w saves · :q! discards");
        } else {
            self.remember_body_cursor();
            self.edit_target = None;
            self.body.clear();
            self.surface = ModalSurface::Browse;
        }
    }

    /// Close the buffer without writing it, keeping whatever is in it
    /// against its headline — `:q`.
    ///
    /// The caller has already refused when the buffer is modified, so
    /// in practice the stash is empty here; it is taken anyway because
    /// "close without losing text" is the rule, not a special case.
    fn close_editor(&mut self) {
        // The buffer is gone; a pane must not offer to put it back.
        self.pane_return = None;
        self.remember_body_cursor();
        self.stash_body();
        self.edit_target = None;
        self.body.clear();
        self.body_baseline.clear();
        self.go_home();
    }

    /// Throw the buffer away — `:q!`. The stash goes with it, or the
    /// next visit would restore exactly what was just discarded.
    fn discard_editor(&mut self) {
        // The buffer is gone; a pane must not offer to put it back.
        self.pane_return = None;
        self.drop_stash();
        self.remember_body_cursor();
        self.edit_target = None;
        self.body.clear();
        self.body_baseline.clear();
        self.go_home();
        self.say("edit discarded");
    }

    /// Run `cmd` as a shell command (`:!pwd`), behind the vault's
    /// `eval_trust`.
    ///
    /// Running arbitrary shell from an editor is the same capability
    /// the evaluator already default-denies (C1a) — a vault is a file
    /// somebody can send you — so it answers to the same key rather
    /// than inventing a looser rule for the same thing. The working
    /// directory is the vault, because that is the directory the user
    /// is thinking about.
    /// How long a `:!` command may run before it is cut off.
    ///
    /// Long enough for a build or a `git fetch`, short enough that a
    /// runaway cannot own the window. A command that needs longer is a
    /// command for a terminal.
    const SHELL_ESCAPE_TIMEOUT_SECS: u64 = 20;

    /// How many lines of a command's output reach the message log.
    /// Enough for a `git status`; not a whole `find /`.
    const SHELL_ESCAPE_LOG_LINES: usize = 40;

    fn run_shell_escape(&mut self, shell: &Shell, cmd: &str) {
        if cmd.is_empty() {
            self.say(":! needs a command — `:!ls`, `:!git status`");
            return;
        }
        // The vault's own answer, not a second read of a second file:
        // `:!` is the same capability `#+BEGIN_SRC shell` is, so it
        // answers to the same gate.
        let trust = shell.vault.eval_trust();
        if !closure_eval::eval_allowed(&trust, "shell") {
            self.say(format!(
                "refused to run `{cmd}` — {}",
                Self::trust_refusal(shell, "shell")
            ));
            return;
        }
        let quoted = cmd.replace('\'', r"'\''");
        let program = format!("cd '{}' && {}", shell.vault.root().display(), quoted);
        // Bounded, and not held open by a grandchild that inherited the
        // pipe: `:! xdg-open .` used to freeze the whole app, because
        // the file manager it opened kept the write end alive and the
        // read waited for EOF that was never coming.
        match closure_eval::shell_escape(
            &program,
            std::time::Duration::from_secs(Self::SHELL_ESCAPE_TIMEOUT_SECS),
        ) {
            Ok(out) => {
                let mut text = out.stdout;
                if !out.stderr.is_empty() {
                    text.push_str(&out.stderr);
                }
                self.say(if out.exit == 0 {
                    format!("`{cmd}` ok")
                } else {
                    format!("`{cmd}` exited {}", out.exit)
                });
                // Into the message log as well as the pane. "commands
                // `:!` should pipe their output to some echo
                // area/stdout/*MESSAGES* buffer" — a one-line status
                // cannot hold `git status`, and a pane you have to
                // know about is not where you look first. The log is
                // the one place that already outlives the command,
                // scrolls, and has a chord (`g M`).
                for line in text.lines().rev().take(Self::SHELL_ESCAPE_LOG_LINES) {
                    self.say(format!("{cmd}: {line}"));
                }
                self.shell_out = Some(if text.trim().is_empty() {
                    format!("(no output, exit {})", out.exit)
                } else {
                    text
                });
            }
            Err(e) => {
                self.say(format!("`{cmd}` failed: {e}"));
                self.shell_out = None;
            }
        }
    }

    /// Output of the last `:!` command, for a shell to show.
    #[must_use]
    pub fn shell_output(&self) -> Option<&str> {
        self.shell_out.as_deref()
    }

    /// Note where the cursor was left in the body being closed, so the
    /// next visit resumes there.
    fn remember_body_cursor(&mut self) {
        if let Some(id) = &self.edit_target {
            self.body_cursors
                .insert(id.clone(), self.body.cursor_byte());
        }
    }

    /// Write the buffer into the vault, leaving the editor open and the
    /// buffer as it is.
    ///
    /// What `:w` needs and what committing is built out of: the write
    /// and the leaving are two different decisions, and a plain `:w`
    /// only ever meant the first one.
    /// Bring a headline's `[n/m]` / `[p%]` cookie up to date with the
    /// checkboxes in its body, if it has one.
    ///
    /// Renaming is the only way to change a title, so this goes through
    /// the same kernel command every rename does (I8) — and only when
    /// the number actually moved, so a save never costs an edit the
    /// user cannot see.
    fn recount_headline_cookie(&mut self, shell: &mut Shell, id: &str) {
        let bid = closure_core::BlockId::from_existing(id);
        let Some((title, body)) = shell
            .vault
            .find_by_id(&bid)
            .map(|(h, _)| (h.title().to_owned(), h.body_text().to_owned()))
        else {
            return;
        };
        let Some(span) = cookie_span(&title) else {
            return;
        };
        let (done, total) = checkbox_counts(&body);
        if total == 0 {
            return;
        }
        let replacement = if title[span.clone()].ends_with("%]") {
            format!("[{}%]", done * 100 / total)
        } else {
            format!("[{done}/{total}]")
        };
        if title[span.clone()] == replacement {
            return;
        }
        let mut updated = title;
        updated.replace_range(span, &replacement);
        if shell.vault.rename_headline(&bid, &updated).is_ok() {
            self.invalidate_rows();
        }
    }

    fn write_body(&mut self, shell: &mut Shell) {
        let Some(id) = self.edit_target.clone() else {
            return;
        };
        let bid = closure_core::BlockId::from_existing(&id);
        // A body line starting with `*` *is* a headline once it is back
        // in the file. Escaping it with a comma (org's own convention)
        // is right for prose that happens to start with a star, and
        // wrong for what people mean when they type `* Something` into
        // a note — which is that it belongs *under* this one. Typed
        // headlines are filed as children, rebased to this headline's
        // depth; the rest is escaped as before.
        let (prose, typed) = closure_org::split_body_headlines(self.body.text());
        let mut body = closure_org::escape_body(&prose);
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        // The buffer showed every child, so it is the whole truth
        // about what is under this headline: a child deleted from it is
        // deleted, which `set_body_with_children` could not express.
        let written = shell.set_subtree(&bid, &body, &typed);
        match written {
            Ok(()) => {
                // A headline that carries a `[/]` cookie is counting the
                // checkboxes in its body, so saving the body is when the
                // count changes (Q3-V5).
                self.recount_headline_cookie(shell, &id);
                // Which file, and how big it now is. A body is written
                // by rewriting the whole file it lives in, and "body
                // saved" never said which file that was.
                self.say(Self::saved_message(shell, &bid));
                // Saved *is* the new baseline: `body_dirty` compares
                // against what the vault holds, and after a write that
                // is what is in the buffer.
                self.body_baseline = self.body.text().to_owned();
                // …and there is nothing left to restore, or the next
                // visit would bring back an edit the vault already has
                // and the note would read as permanently unsaved.
                self.drop_stash();
                self.refill_from_vault(shell, &id);
            }
            Err(e) => self.status = format!("save failed: {e}"),
        }
    }

    /// Reload the open buffer from what was just written.
    ///
    /// A child typed into the buffer has no `:ID:` in it; the write
    /// stamps one on the way to disk. Leaving the buffer as the user
    /// typed it means the next save sees an id-less child again and
    /// mints a *second* one, so a block's identity would churn on every
    /// save. Reading back what was written is also what puts the
    /// stamped drawer on screen, where the rest of the subtree's are.
    fn refill_from_vault(&mut self, shell: &Shell, id: &str) {
        let bid = closure_core::BlockId::from_existing(id);
        let Some(body) = shell
            .vault
            .find_by_id(&bid)
            .map(|(h, _)| closure_org::unescape_body(h.body_text()))
        else {
            return;
        };
        let mut whole = body;
        if let Ok(kids) = shell.children_source(&bid)
            && !kids.is_empty()
        {
            if !whole.is_empty() && !whole.ends_with('\n') {
                whole.push('\n');
            }
            whole.push_str(&kids);
        }
        let at = self.body.cursor_byte().min(whole.len());
        self.body_baseline.clone_from(&whole);
        self.load_body(whole);
        self.body.set_cursor_byte(at);
    }

    /// Whether the body editor holds something the vault does not.
    ///
    /// A comparison against what was loaded, not a "was touched" bit:
    /// a buffer the user has put back the way they found it — by
    /// undoing, or by retyping the same word — has nothing to save,
    /// and warning about it would train them to ignore the warning.
    #[must_use]
    pub fn body_dirty(&self) -> bool {
        // Three kinds of buffer put text in `self.body`, and only one of
        // them sets `edit_target`. Asking about that one alone is why a
        // file you had typed a page into reported itself clean, and
        // every guard that asks before closing let it go without a word.
        let open =
            self.edit_target.is_some() || self.file_target.is_some() || self.special.is_some();
        open && self.body.text() != self.body_baseline
    }

    /// Put the open buffer aside against its headline, if it holds
    /// anything the vault does not.
    ///
    /// Called on the way out of a buffer — opening another note, or
    /// closing this one without writing. A clean buffer stashes
    /// nothing: there would be nothing to restore.
    fn stash_body(&mut self) {
        if !self.body_dirty() {
            return;
        }
        if let Some(id) = self.edit_target.clone() {
            self.body_stash.insert(
                id,
                (self.body.text().to_owned(), self.body_baseline.clone()),
            );
        }
    }

    /// Forget any stashed buffer for the note being edited — what a
    /// write and an explicit discard both mean.
    fn drop_stash(&mut self) {
        if let Some(id) = &self.edit_target {
            self.body_stash.remove(id);
        }
    }

    /// How many bodies are modified and unwritten, the one on screen
    /// included. Zero means the vault has everything.
    #[must_use]
    pub fn unsaved_bodies(&self) -> usize {
        self.body_stash.len() + usize::from(self.body_dirty())
    }

    /// Write out every body edit still in progress. `true` when
    /// something was saved.
    ///
    /// What a window closing under an unfinished edit calls: the
    /// gesture that closed the window is recoverable, the paragraphs
    /// in the buffers are not, so the text wins — all of them, not
    /// just the one that happened to be on screen.
    pub fn save_pending_edit(&mut self, shell: &mut Shell) -> bool {
        let mut saved = if self.body_dirty() {
            self.commit_edit_body(shell);
            true
        } else {
            false
        };
        // Deterministic order, so a failure reports the same way twice.
        let mut held: Vec<_> = std::mem::take(&mut self.body_stash).into_iter().collect();
        held.sort_by(|(a, _), (b, _)| a.cmp(b));
        for (id, (text, _)) in held {
            let bid = closure_core::BlockId::from_existing(&id);
            let mut body = closure_org::escape_body(&text);
            if !body.is_empty() && !body.ends_with('\n') {
                body.push('\n');
            }
            match shell.set_body(&bid, &body) {
                Ok(()) => saved = true,
                Err(e) => self.status = format!("save failed: {e}"),
            }
        }
        saved
    }

    /// Run one `:` line, for a shell (or a test) that has the text
    /// already rather than a keystroke at a time.
    pub fn run_ex_line(&mut self, shell: &mut Shell, line: &str) {
        self.ex_return = Some(self.surface);
        self.run_ex(shell, line);
    }

    /// Refuse to quit over an unsaved body, and say why.
    ///
    /// `true` when the caller should stop. The bare flag this replaced
    /// meant `:q` in the middle of an edit threw the buffer away
    /// without a word.
    fn refuse_quit_when_dirty(&mut self) -> bool {
        if !self.body_dirty() {
            return false;
        }
        self.say("unsaved body — :w saves, :wq saves and quits, :q! discards it");
        true
    }

    /// Replace a byte range of the body buffer with `text`, leaving the
    /// cursor after it.
    ///
    /// What an input method hands over: a compose sequence or a CJK IME
    /// builds a character over several keystrokes and delivers it as a
    /// replacement over a range, not as a keypress. Out-of-range or
    /// non-boundary offsets are clamped rather than panicking (I5), and
    /// the edit is one undo step like any other.
    pub fn body_replace_range(&mut self, range: std::ops::Range<usize>, text: &str) {
        self.completion = None;
        self.body.replace_range(range, text);
    }

    /// Park the body-editor viewport so `line` is its first visible
    /// one, clamped to the buffer — what a scrollbar drag needs, and
    /// the absolute half of [`Self::body_scroll_by`].
    pub fn body_scroll_to(&mut self, line: usize, viewport: usize) {
        let lines = self.body.text().split('\n').count();
        let max = lines.saturating_sub(viewport);
        let (cl, _) = self.body.cursor_line_col();
        self.body_scroll = Some((line.min(max), cl));
    }

    fn on_search_key(
        &mut self,
        shell: &Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        match key {
            "escape" => {
                // Never mind: the outline goes back to the row it was
                // on, not to whatever index the results left behind.
                self.query.clear();
                self.selected = self.search_return.take().unwrap_or(0);
                self.go_home();
            }
            "enter" => {
                // The row list is *filtered* while the overlay is open,
                // so the cursor is an index into the results; clearing
                // the query unfilters it and that index then points at
                // a different row entirely. The id is the only thing
                // both lists agree on.
                let hit = self.selected_row_id(shell);
                self.query.clear();
                self.search_return = None;
                if let Some(id) = hit {
                    self.select_by_id(shell, &id);
                }
                self.go_home();
            }
            // The arrows and the chords every modal user reaches for
            // walk the results; everything else is the field's, which
            // is where `C-w` and `C-u` come from now — they were
            // hand-rolled here and nowhere else.
            _ => {
                let last = self.rows_shared(shell).len().saturating_sub(1);
                self.filter_key(key, ctrl, alt, text, last);
            }
        }
    }

    fn on_capture_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        match key {
            "escape" => {
                // A thought you abandoned is the one you most often
                // want back, so a cancelled capture is remembered too.
                self.remember_capture();
                self.go_home();
                self.capture_buf.clear();
                self.capture_path_root = None;
            }
            // Enter files the item, so there was no way to type a
            // second line: a captured thought had to fit on one or be
            // reopened afterwards. Shift+Enter is the newline, the way
            // a chat box and a Notion block both do it.
            "shift-enter" => self.capture_buf.insert_char('\n'),
            // Up and back through what you have captured before — the
            // shell-history gesture, and the chords for a modal mode.
            "up" => self.walk_capture_history(1),
            "down" => self.walk_capture_history(-1),
            // `C-k` used to be the modal spelling of history-up here.
            // The user asked on 2026-08-03 for "C-k should add to the
            // system clipboard", which is kill-to-end-of-line — what
            // every other field in the shell and every minibuffer does
            // with it. History keeps the arrows, `C-j`, and `M-p`/
            // `M-n`, so nothing is lost but the duplicate.
            "j" if ctrl => self.walk_capture_history(-1),
            // What a minibuffer with a history has always answered to.
            // `C-j`/`C-k` above are the modal spelling and stay; these
            // are the ones an Emacs hand reaches for, and their absence
            // is why `C-k` had to be the history at all.
            "p" if alt => self.walk_capture_history(1),
            "n" if alt => self.walk_capture_history(-1),
            // Files it and *stays*: same target, empty line, ready for
            // the next one. Filing several thoughts under one headline
            // used to mean reopening the prompt and re-aiming it
            // between each, because plain Enter closes and takes the
            // cursor to what it just made. Shift+Enter was already the
            // newline, so the spare modifier is Ctrl.
            "enter" if ctrl => {
                self.remember_capture();
                if !self.capture_buf.is_empty() {
                    let text = self.capture_buf.take();
                    self.commit_capture(shell, &text, false);
                }
                self.capture_buf.clear();
            }
            "enter" => {
                self.remember_capture();
                if !self.capture_buf.is_empty() {
                    let text = self.capture_buf.take();
                    self.commit_capture(shell, &text, true);
                }
                self.go_home();
                self.capture_buf.clear();
                self.capture_path_root = None;
            }
            // Completion, on the editor's own chords: `C-n`/`C-p`
            // cycle, TAB accepts, anything else ends the session.
            "n" if ctrl => self.cycle_prompt_completion(shell, true),
            "p" if ctrl => self.cycle_prompt_completion(shell, false),
            "tab" => self.accept_prompt_completion(shell),
            // Everything else is the field's: the readline chords, the
            // arrows, and the characters themselves.
            _ => {
                self.prompt_completion = None;
                let kill = self.shared_kill();
                self.capture_buf.set_kill(&kill);
                self.capture_buf.key(key, ctrl, alt, text);
                let after = self.capture_buf.kill().to_owned();
                self.keep_shared_kill(&after);
            }
        }
    }

    /// File `title` where the outline is pointing, and put the cursor
    /// on what was just made.
    ///
    /// Two halves of the same complaint: a capture used to land at the
    /// top of `inbox.org` whatever you were looking at, and the
    /// selection stayed where it was — so the next thing you did
    /// happened to the previous headline. Under the selection when
    /// there is one, top level when Escape has said there is not.
    /// Keep what is in the capture field, so the arrows can bring it
    /// back. Consecutive duplicates are one entry.
    fn remember_capture(&mut self) {
        let text = self.capture_buf.text().trim().to_owned();
        self.capture_hist_at = None;
        if text.is_empty()
            || self
                .capture_history
                .last()
                .is_some_and(|last| *last == text)
        {
            return;
        }
        self.capture_history.push(text);
    }

    /// Step `by` entries back (positive) or forward (negative) through
    /// the capture history, newest first.
    ///
    /// Walking past the newest entry lands on the empty line you were
    /// typing on, which is where a shell's history leaves you too.
    fn walk_capture_history(&mut self, by: i32) {
        if self.capture_history.is_empty() {
            return;
        }
        let last = self.capture_history.len() - 1;
        let next = match (self.capture_hist_at, by.is_positive()) {
            (None, true) => Some(last),
            (None, false) => None,
            (Some(i), true) => Some(i.saturating_sub(1)),
            (Some(i), false) if i >= last => None,
            (Some(i), false) => Some(i + 1),
        };
        self.capture_hist_at = next;
        let text = next.map_or("", |i| self.capture_history[i].as_str());
        self.capture_buf.set_text(text);
    }

    /// The path a capture will file into: the file, then every
    /// headline down to the target.
    ///
    /// "under “Notes”" names one headline, and a vault has several of
    /// those; the path says *which*. It is also the control — each
    /// step is somewhere the capture could go instead, so filing one
    /// level up is a click and not a cancel, a re-select and a retype.
    ///
    /// Always at least one crumb, and exactly one of them is
    /// [`CaptureCrumb::active`]: with nothing selected that is the
    /// capture file, which is where a loose thought goes.
    #[must_use]
    pub fn capture_crumbs(&self, shell: &Shell) -> Vec<CaptureCrumb> {
        // The pinned root while an overlay is open, the live selection
        // otherwise — the prompt has to be right before the pin exists.
        let Some(id) = self.capture_path_root.clone().or_else(|| {
            self.selection_active
                .then(|| self.selected_row_id(shell))
                .flatten()
        }) else {
            return vec![CaptureCrumb {
                id: None,
                label: CAPTURE_FILE.to_owned(),
                active: true,
            }];
        };
        let Some((file, chain)) = capture_chain(shell, &id) else {
            return vec![CaptureCrumb {
                id: None,
                label: CAPTURE_FILE.to_owned(),
                active: true,
            }];
        };
        let mut crumbs = Vec::with_capacity(chain.len() + 1);
        crumbs.push(CaptureCrumb {
            id: None,
            label: file,
            active: false,
        });
        crumbs.extend(chain.into_iter().map(|(id, label)| CaptureCrumb {
            id,
            label,
            active: false,
        }));
        // The selection is the target until the user says otherwise,
        // and a pick that outran the path (the outline moved under it)
        // falls back to the same place rather than to none.
        let last = crumbs.len() - 1;
        let at = self
            .capture_crumb_pick
            .filter(|i| *i <= last)
            .unwrap_or(last);
        crumbs[at].active = true;
        crumbs
    }

    /// The selected headline's file, relative to the vault root — what
    /// the file crumb actually means when it is filed into.
    ///
    /// The crumb shows a file *name* because a path is not a label;
    /// the capture needs the path, because a vault may hold more than
    /// one file with that name.
    fn selected_capture_file(&self, shell: &Shell) -> Option<String> {
        let id = self
            .selection_active
            .then(|| self.selected_row_id(shell))
            .flatten()?;
        let bid = closure_core::BlockId::from_existing(&id);
        let (_, path) = shell.vault.find_by_id(&bid)?;
        let rel = path.strip_prefix(shell.vault.root()).unwrap_or(path);
        Some(rel.to_string_lossy().into_owned())
    }

    /// File the capture being typed into crumb `index` of
    /// [`Self::capture_crumbs`] — the click on a breadcrumb.
    ///
    /// Out-of-range indices are ignored rather than clamped: a click
    /// on a crumb that is no longer there should do nothing, not
    /// silently retarget the capture somewhere the user did not point.
    pub fn pick_capture_crumb(&mut self, shell: &Shell, index: usize) {
        let crumbs = self.capture_crumbs(shell);
        if index >= crumbs.len() {
            return;
        }
        // A crumb with no id and no file behind it cannot be filed
        // into (a headline that never got an `:ID:`); the file crumb
        // always can.
        if index > 0 && crumbs[index].id.is_none() {
            self.say(format!("“{}” has no id to file under", crumbs[index].label));
            return;
        }
        self.capture_crumb_pick = Some(index);
        // The tree follows the path: pointing the capture at a
        // headline and leaving the highlight on a different one makes
        // the screen disagree with itself about where you are. The
        // file crumb names no headline, so there is nothing to move to
        // and the outline stays where it is.
        if let Some(id) = crumbs[index].id.clone() {
            self.select_by_id(shell, &id);
        }
        self.say(format!("capture {}", self.capture_target_label(shell)));
    }

    /// Whether the capture will land as a `child` of the crumb it is
    /// pointing at, or at the `top level` of a file.
    ///
    /// The breadcrumbs name the target and the filled chip says which
    /// step it is, which answers "where" and leaves "as a child of it,
    /// or beside it?" to be guessed ("the capture prefix should mention
    /// if it will be placed as a child to a corresponding element").
    #[must_use]
    pub fn capture_placement(&self, shell: &Shell) -> &'static str {
        let onto_headline = self
            .capture_crumbs(shell)
            .into_iter()
            .find(|c| c.active)
            .is_some_and(|c| c.id.is_some());
        if onto_headline { "child" } else { "top level" }
    }

    /// Where the next capture will be filed, in words: `under “Foo”`,
    /// or `into inbox.org` when nothing is selected.
    ///
    /// Both destinations are right and neither was visible — the
    /// overlay said "capture" either way, so the only way to learn
    /// where a thought had landed was to file it and go look. Derived
    /// from the same crumbs [`Self::commit_capture`] files into, so
    /// the promise and the filing cannot drift apart.
    #[must_use]
    pub fn capture_target_label(&self, shell: &Shell) -> String {
        self.capture_crumbs(shell)
            .into_iter()
            .find(|c| c.active)
            .map_or_else(
                || format!("into {CAPTURE_FILE}"),
                |c| {
                    if c.id.is_some() {
                        format!("under “{}”", c.label)
                    } else {
                        format!("into {}", c.label)
                    }
                },
            )
    }

    fn commit_capture(&mut self, shell: &mut Shell, text: &str, follow: bool) {
        // A capture can be more than one line (Shift+Enter). The first
        // line is the headline — a headline *is* one line in org — and
        // the rest is its body, which is where the thought actually
        // was.
        let (title, body) = text.split_once('\n').unwrap_or((text, ""));
        let title = title.trim_end();
        // The active crumb *is* the target — the same list the prompt
        // showed, so what was on screen is what happens.
        let target = self.capture_crumbs(shell).into_iter().find(|c| c.active);
        let parent = target.as_ref().and_then(|c| c.id.clone());
        let captured = match (&parent, &target) {
            (Some(parent), _) => {
                let id = closure_core::BlockId::from_existing(parent);
                shell.capture_under(&id, title)
            }
            // The file crumb: the top level of the file being looked
            // at, which is only the inbox when nothing is selected.
            // Filed by its path under the vault root, never by the
            // name on the chip — two directories can hold a
            // `notes.org` and only one of them is the one on screen.
            (None, Some(_)) => match self.selected_capture_file(shell) {
                Some(file) => shell.capture_into(&file, title),
                None => shell.capture(title),
            },
            (None, None) => shell.capture(title),
        };
        match captured {
            Ok(id) => {
                // Filing into a folded headline puts the item somewhere
                // you cannot see and leaves the selection on a row that
                // is not in the list. Org opens the target it captures
                // into; so does this.
                if let Some(parent) = &parent
                    && row_is_folded(shell, parent)
                {
                    let pid = closure_core::BlockId::from_existing(parent);
                    let _ = shell.set_property(&pid, "VISIBILITY", "all");
                }
                if !body.trim().is_empty() {
                    let mut body = closure_org::escape_body(body);
                    if !body.ends_with('\n') {
                        body.push('\n');
                    }
                    if let Err(e) = shell.set_body(&id, &body) {
                        self.say(format!("captured, but the body failed: {e}"));
                    }
                }
                self.say(format!("captured: {title}"));
                // The row list is rebuilt from the bumped revision, so
                // the new id is findable the moment we ask. Staying put
                // is what keeps the *target* put: the capture prompt
                // aims at whatever the cursor is on, so a run of
                // `C-Enter`s all land in the same place.
                if follow {
                    self.select_by_id(shell, id.as_str());
                    self.selection_active = true;
                } else {
                    self.say(format!("captured: {title} — still filing here"));
                }
            }
            Err(e) => self.status = format!("capture failed: {e}"),
        }
    }

    /// The block id of the row under the cursor, if there is one.
    fn selected_row_id(&self, shell: &Shell) -> Option<String> {
        self.rows_shared(shell)
            .get(self.selected)
            .map(|r| r.id.clone())
            .filter(|id| !id.is_empty())
    }

    /// Where an overlay returns to when it closes.
    ///
    /// In the clickable view that is the outline; in the editor view it
    /// is the file buffer, which is the whole point of that view. They
    /// all returned to the outline, so opening the palette or a capture
    /// from a full-window buffer dropped you back into the row list —
    /// a different shape of the app than the one you were using.
    const fn home_surface(&self) -> ModalSurface {
        // A pane opened over a buffer goes back to that buffer. Without
        // this the answer was "the outline" for anyone whose view is
        // the clickable one, so the note you were writing vanished from
        // the screen while still being open — reported three times, for
        // three different commands, which is what made it one bug.
        if let Some(back) = self.pane_return {
            return back;
        }
        match self.view {
            ViewMode::Editor => ModalSurface::EditFile,
            ViewMode::Clickable => ModalSurface::Browse,
        }
    }

    /// Close the current overlay, returning to [`Self::home_surface`].
    const fn go_home(&mut self) {
        self.surface = self.home_surface();
    }

    /// Body lines hidden by a fold, in order.
    ///
    /// The fold lives on the line rather than on the document: a shell
    /// paints every line except these, so the kernel decides once what
    /// is hidden and every shell hides the same thing (I7).
    #[must_use]
    pub fn body_hidden_lines(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self
            .body_folds
            .iter()
            .flat_map(|(start, end)| (*start + 1)..=*end)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Fold or unfold whatever the body cursor is in: a `#+BEGIN_…`
    /// block, or a headline's subtree.
    ///
    /// A note with three source blocks is mostly code you are not
    /// reading, and a file opened in the editor view is mostly
    /// headlines you are not editing. Org folds both.
    /// The image the caret's line links to, if it links to one.
    ///
    /// An inline preview is deliberately small; a picture worth
    /// opening is worth the window, and `RET` is the key org uses for
    /// "do the thing this link means".
    fn image_on_caret_line(&self, shell: &Shell) -> Option<std::path::PathBuf> {
        let (line, _) = self.body.cursor_line_col();
        let text = self.body.text();
        let raw = text.split('\n').nth(line)?;
        if let Some(first) = image_links(raw).into_iter().next() {
            let candidate = std::path::Path::new(&first.path);
            let full = if candidate.is_absolute() {
                candidate.to_path_buf()
            } else {
                shell.vault.root().join(candidate)
            };
            return full.is_file().then_some(full);
        }
        // …or the picture a drawn diagram block made. One gesture for
        // "show me that picture" is worth more than the distinction
        // between a picture a note links and one it generates, and a
        // mermaid chart is the thing most worth enlarging: the inline
        // copy is a fixed eight rows on purpose.
        self.diagram_at_line(shell, line)
    }

    /// The rendered picture of the diagram block enclosing `line`, if
    /// there is one and it has been drawn.
    fn diagram_at_line(&self, shell: &Shell, line: usize) -> Option<std::path::PathBuf> {
        let cache = self.diagram_cache(shell);
        diagram_blocks(self.body.text()).into_iter().find_map(|b| {
            // A block spans from its opening fence to `b.line`; the
            // source lines are what the caret is realistically on.
            let starts_at = b.line.checked_sub(b.src.lines().count() + 1)?;
            if !(starts_at..=b.line).contains(&line) {
                return None;
            }
            let kind = closure_eval::diagram_for(&b.lang)?;
            let path = closure_eval::diagram_path(&cache, kind, &b.src, self.ink);
            path.is_file().then_some(path)
        })
    }

    /// A newline that carries the list on, org's way.
    ///
    /// `RET` at the end of `- milk` opens `- `; at the end of `1.
    /// first` it opens `2. ` and renumbers what follows, so inserting
    /// into the middle of a list does not leave two `3.`s. On an
    /// *empty* item it ends the list instead of making another empty
    /// one — without that rule every list finishes with a stray bullet
    /// to go back and delete, which is what made the other rules not
    /// worth having.
    fn newline_continuing_list(&mut self) {
        let (line, col) = self.body.cursor_line_col();
        let text = self.body.text().to_owned();
        let current = text.split('\n').nth(line).unwrap_or_default().to_owned();
        // Only at the end of the line: splitting an item in the middle
        // is splitting a sentence, and org does not put a bullet there.
        let at_end = col >= current.chars().count();
        let Some(marker) = (if at_end {
            list_continuation(&current)
        } else {
            None
        }) else {
            // An empty item ends the list: the marker goes, and the
            // caret is left on the blank line it leaves behind.
            if at_end && split_list_marker(current.trim_start()).is_some() {
                let indent = current.len() - current.trim_start().len();
                self.body.goto_line_col(line, 0);
                for _ in 0..current.chars().count() - indent {
                    self.body.delete_at();
                }
            }
            self.body.insert_char('\n');
            return;
        };
        self.body.insert_char('\n');
        for c in marker.chars() {
            self.body.insert_char(c);
        }
        // Counting only matters for a counter, and `renumber_list`
        // leaves everything else alone.
        let after = self.body.text().to_owned();
        let renumbered = renumber_list(&after, line + 1);
        if renumbered != after {
            let (l, c) = self.body.cursor_line_col();
            self.load_body(renumbered);
            self.body.goto_line_col(l, c);
        }
    }

    /// Move the caret off a hidden line, if it is on one.
    ///
    /// A shell paints every line except the hidden ones, so a caret
    /// inside a fold is a caret that is not drawn — it has not moved
    /// and it still takes your typing, which is worse than losing it.
    /// It surfaces at the fold's own first line, which is the line
    /// that stands for what is inside it.
    ///
    /// `downwards` says which way the caret was travelling, so a
    /// motion *through* a fold comes out the far side rather than
    /// sticking to its head and refusing to move.
    fn leave_hidden_line(&mut self, downwards: bool) {
        let (line, col) = self.body.cursor_line_col();
        let Some(&(start, end)) = self
            .body_folds
            .iter()
            .find(|(s, e)| (*s + 1..=*e).contains(&line))
        else {
            return;
        };
        let target = if downwards { end + 1 } else { start };
        let last = self.body.text().split('\n').count().saturating_sub(1);
        self.body.goto_line_col(target.min(last), col);
    }

    fn toggle_body_fold(&mut self) {
        let (line, _) = self.body.cursor_line_col();
        if let Some(i) = self
            .body_folds
            .iter()
            .position(|(s, e)| *s == line || (*s..=*e).contains(&line))
        {
            let (start, _) = self.body_folds.remove(i);
            self.say(format!("unfolded line {}", start + 1));
            return;
        }
        // Fold first, then rescue the caret: folding the range the
        // caret is in is the common case, and leaving it inside is the
        // "caret disappears" report.
        let text = self.body.text().to_owned();
        let lines: Vec<&str> = text.split('\n').collect();
        match fold_range(&lines, line) {
            Some((start, end)) => {
                self.body_folds.push((start, end));
                self.leave_hidden_line(false);
                self.say(format!("folded {} line(s)", end - start));
            }
            None => {
                self.say("nothing to fold here — a block or a headline folds");
            }
        }
    }

    /// Whether image links are painted as pictures — what a shell asks
    /// before it loads any of them.
    #[must_use]
    pub const fn images_shown(&self) -> bool {
        self.images_shown
    }

    /// File an image that arrived on the clipboard and put a link to it
    /// where the cursor is. Returns the link that was inserted.
    ///
    /// The bytes go in the vault, under `assets_dir` (`assets` by
    /// default), and the link written into the note is *relative*, so
    /// the file still resolves when the vault is opened in Emacs or
    /// synced to another machine. `None` when there is no buffer open:
    /// there would be nowhere to put the link, and a picture in the
    /// vault that nothing refers to is litter.
    pub fn paste_image(&mut self, shell: &Shell, extension: &str, bytes: &[u8]) -> Option<String> {
        if !self.surface.is_editor() {
            return None;
        }
        let root = shell.vault.root();
        let dir = closure_config::Config::from_path(&root.join(closure_config::CONFIG_FILE))
            .ok()
            .and_then(|c| c.assets_dir)
            .unwrap_or_else(|| std::path::PathBuf::from("assets"));
        let name = asset_file_name(extension);
        let target = root.join(&dir);
        if let Err(e) = std::fs::create_dir_all(&target) {
            self.say(format!("could not make {}: {e}", target.display()));
            return None;
        }
        if let Err(e) = std::fs::write(target.join(&name), bytes) {
            self.say(format!("could not write the image: {e}"));
            return None;
        }
        let link = format!("[[file:{}/{name}]]", dir.display());
        // Through `replace_all` rather than `insert_str`: a paste is one
        // edit whatever mode the buffer is in, and `insert_str`'s
        // checkpoint is the INSERT-burst one.
        let mut text = self.body.text().to_owned();
        let at = self.body.cursor_byte().min(text.len());
        text.insert_str(at, &link);
        self.body.replace_all(text, at + link.len());
        self.say(format!("filed {}/{name}", dir.display()));
        Some(link)
    }

    /// Where the disk-file courier drops and looks for bundles, from
    /// the vault's `config.org`.
    fn sync_dir(shell: &Shell) -> Option<std::path::PathBuf> {
        closure_config::Config::from_path(&shell.vault.root().join(closure_config::CONFIG_FILE))
            .ok()?
            .sync_dir
    }

    /// Leave our replica in the shared folder — `sync_dir` in
    /// config.org, which is a Syncthing share, a Dropbox, a mounted
    /// drive, a USB stick.
    fn sync_export(&mut self, shell: &Shell) {
        let Some(dir) = Self::sync_dir(shell) else {
            self.say("no sync_dir in config.org — set it to a folder both machines can see");
            return;
        };
        self.sync_mut().snapshot(shell);
        let said = match self.sync_mut().export_bundle(&dir) {
            Ok(path) => format!("left a bundle in {}", path.display()),
            Err(e) => format!("export failed: {e}"),
        };
        self.say(said);
    }

    /// Pick up every bundle a paired peer left in the shared folder and
    /// write what converged back into the vault.
    fn sync_import(&mut self, shell: &mut Shell) {
        let Some(dir) = Self::sync_dir(shell) else {
            self.say("no sync_dir in config.org — set it to a folder both machines can see");
            return;
        };
        self.sync_mut().snapshot(shell);
        match self.sync_mut().import_bundles(&dir) {
            Ok((0, _)) => {
                self.say("nothing new in the sync folder");
            }
            Ok((n, conflicts)) => {
                // The replica converging is half a sync; the vault is
                // what gets opened in Emacs and committed to git.
                let applied = self.sync_mut().apply_to_vault(shell);
                let mut pending_list = self.conflicts.conflicts().to_vec();
                pending_list.extend(conflicts);
                let pending = pending_list.len();
                self.set_conflicts(pending_list);
                self.say(if pending > 0 {
                    format!("{n} bundle(s), {applied} field(s), {pending} conflict(s) to review")
                } else {
                    format!("{n} bundle(s), {applied} field(s) merged")
                });
            }
            Err(e) => self.status = format!("import failed: {e}"),
        }
    }

    /// Paste back the peers this vault has paired with before.
    ///
    /// Pairing that has to be redone every session is not pairing, so
    /// the tickets live in `config.org` — a ticket is an address and a
    /// public key, nothing secret, and the vault is plain files (I1).
    pub fn load_peers(&mut self, shell: &Shell) {
        // Our own identity first. The comment above was already true of
        // the peer list and quietly false of us: the tickets survived a
        // restart and the key that made ours meaningful did not, so a
        // peer coming back was refused rather than merely unknown.
        let root = shell.vault.root().to_path_buf();
        // Named after the vault, which is what the other side has to
        // recognise — "local" told a peer nothing, and told two peers
        // the same nothing.
        if let Some(name) = root.file_name().map(|n| n.to_string_lossy().into_owned()) {
            self.sync_mut().set_name(&name);
        }
        self.sync_mut().load_identity(&root);
        let Ok(cfg) = closure_config::Config::from_path(&root.join(closure_config::CONFIG_FILE))
        else {
            return;
        };
        for ticket in &cfg.sync_peers {
            // A ticket that no longer parses is skipped rather than
            // fatal: the file is the user's to edit.
            let _ = self.sync_mut().add_peer(ticket);
        }
    }

    /// Open the outline on the headline the last session was in.
    ///
    /// The cursor inside a body was already remembered, which made the
    /// outline forgetting the *note* the odd one out. An id that no
    /// longer resolves leaves the cursor at the top: a vault is edited
    /// elsewhere too, and a missing note is not an error.
    pub fn restore_last_place(&mut self, shell: &Shell) {
        let Ok(cfg) = closure_config::Config::from_path(
            &shell.vault.root().join(closure_config::CONFIG_FILE),
        ) else {
            return;
        };
        if let Some(id) = &cfg.last_place {
            self.select_by_id(shell, id);
        }
        self.outline_width = cfg.outline_width;
        // The files recent sessions were in come back with it: the
        // picker's whole point is that the note you were in yesterday
        // is the first thing it offers (Q1-B4).
        self.recent_files.clone_from(&cfg.recent_files);
    }

    /// Write where this session was back to `config.org` — what the
    /// window calls on the way out.
    ///
    /// The note last *edited* wins over the one merely under the
    /// cursor: opening a body is the strongest statement there is
    /// about where you were. With neither, the file is left alone
    /// rather than cleared — dropping the selection (Esc) means "do
    /// not move me next time", not "I was nowhere".
    pub fn save_last_place(&mut self, shell: &Shell) {
        let place = self
            .last_edited
            .clone()
            .or_else(|| self.selection_active.then(|| self.selected_row_id(shell))?);
        let path = shell.vault.root().join(closure_config::CONFIG_FILE);
        let mut source = std::fs::read_to_string(&path).unwrap_or_default();
        if let Some(place) = place {
            match closure_config::set_config_key(&source, "last_place", &place) {
                Ok(updated) => source = updated,
                Err(e) => {
                    self.say(format!("could not remember where you were: {e}"));
                    return;
                }
            }
        }
        // The pane you sized is the pane you get back. Only when a
        // session actually moved it: a key appearing the first time you
        // close the window, holding the default, is noise in a file you
        // read.
        if let Some(width) = self.outline_width {
            match closure_config::set_config_key(&source, "outline_width", &format!("{width}")) {
                Ok(updated) => source = updated,
                Err(e) => {
                    self.say(format!("could not remember the pane width: {e}"));
                    return;
                }
            }
        }
        // Likewise the rail: written only once a session has actually
        // docked or undocked it.
        if let Some(docked) = self.rail_docked {
            match closure_config::set_config_key(&source, "rail_docked", &format!("{docked}")) {
                Ok(updated) => source = updated,
                Err(e) => {
                    self.say(format!("could not remember the rail: {e}"));
                    return;
                }
            }
        }
        // The recent-files list is written even when the session ended
        // with no selection: which files you were in is true regardless
        // of where the cursor happened to rest (Q1-B4).
        if !self.recent_files.is_empty() {
            let files: Vec<String> = self
                .recent_files
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            match closure_config::set_config_key(&source, "recent_files", &files.join(", ")) {
                Ok(updated) => source = updated,
                Err(e) => {
                    self.say(format!("could not remember which files you were in: {e}"));
                    return;
                }
            }
        }
        if let Err(e) = std::fs::write(&path, source) {
            self.say(format!("could not remember where you were: {e}"));
        }
    }

    /// Whether long body lines wrap instead of scrolling sideways.
    #[must_use]
    pub const fn wrap(&self) -> bool {
        self.wrap
    }

    /// Set wrapping — what `config.org`'s `wrap` key does at launch.
    pub const fn set_wrap(&mut self, wrap: bool) {
        self.wrap = wrap;
    }

    /// How many times this session has started over — what a window
    /// watches to redo the parts of a launch only it can do.
    #[must_use]
    pub const fn reloads(&self) -> u64 {
        self.reloads
    }

    /// Start over without quitting: the whole of a close, then the
    /// whole of a launch, with no process dying in between.
    ///
    /// Saves what the window closing saves, re-reads the vault, and
    /// then drops the session — buffers, stashes, jumps, whichever
    /// surface was open — coming back the way a launch does, out of
    /// `config.org`. That last part is the point of having the command
    /// at all: a keymap or a theme edited in another window takes
    /// effect here without a restart.
    ///
    /// The re-read is the full walk rather than the incremental poll.
    /// This is the command pressed *because* what is on screen looks
    /// wrong, and an mtime is exactly the thing that would then be
    /// trusted wrongly.
    pub fn reload_session(&mut self, shell: &mut Shell) {
        self.save_pending_edit(shell);
        self.save_last_place(shell);
        if let Err(e) = shell.vault.reload() {
            self.say(format!("reload failed: {e}"));
            self.notify(ToastLevel::Error, format!("reload failed: {e}"));
            return;
        }
        // `config.org` picks the editing mode at launch, so it picks it
        // here — but only when it actually names one. `Config` hands
        // back a fully-defaulted struct, and taking the mode from that
        // would quietly drop a session into Doom every time the file
        // said nothing about it, which is a worse answer than the mode
        // already on screen.
        let cfg = std::fs::read_to_string(shell.vault.root().join(closure_config::CONFIG_FILE))
            .unwrap_or_default();
        let mode = closure_config::config_key(&cfg, "input_mode")
            .and_then(|_| closure_config::Config::from_org_source(&cfg).ok())
            .map_or(self.mode, |c| c.input_mode);
        // The window's measurements are not session state — nothing
        // reports them again until the next frame, and a viewport of
        // zero rows leaves every framing chord a no-op until then. The
        // clock is the shell's too, for the same reason.
        let (body_rows, outline_rows) = (self.body_viewport, self.outline_viewport);
        let (today, now) = (
            std::mem::take(&mut self.today),
            std::mem::take(&mut self.now),
        );
        let (bind, advertise) = (self.sync_bind, self.sync_advertise);
        let reloads = self.reloads;
        *self = Self::new(mode);
        self.reloads = reloads.wrapping_add(1);
        self.body_viewport = body_rows;
        self.outline_viewport = outline_rows;
        self.today = today;
        self.now = now;
        self.wrap = closure_config::Config::from_path(
            &shell.vault.root().join(closure_config::CONFIG_FILE),
        )
        .is_ok_and(|c| c.wrap);
        self.configure_sync(bind, advertise);
        self.load_peers(shell);
        self.restore_last_place(shell);
        self.say("reloaded — vault and config re-read from disk");
        self.notify(ToastLevel::Success, "reloaded");
    }

    /// Forget the peer at `at`, in this session and in `config.org`.
    ///
    /// "Is the peer ticket input field append only to the config? We
    /// may need some add/deactivate/delete UI component" — it was.
    /// A ticket pasted by accident, or a machine that no longer
    /// exists, stayed in the file forever and there was no way to see
    /// or change that from the screen it was added on.
    ///
    /// Out of range is harmless: the pane and the list can disagree by
    /// a frame, and a click landing on a row that has just gone is not
    /// worth an error.
    pub fn forget_peer(&mut self, at: usize, shell: &Shell) {
        let gone = {
            let sync = self.sync_mut();
            if at >= sync.peers().len() {
                return;
            }
            sync.forget_peer(at)
        };
        self.save_peers(shell);
        if let Some(addr) = gone {
            self.say(format!("forgot {addr}"));
        }
    }

    /// Where this shell is accepting connections, if it is.
    ///
    /// "What does the listen button really do? It does something, but
    /// what?" It binds a socket. Nothing said so and nothing said
    /// whether it already had, so pressing it twice looked exactly
    /// like never having pressed it.
    #[must_use]
    pub fn listening_on(&self) -> Option<std::net::SocketAddr> {
        self.sync
            .as_ref()
            .and_then(SyncApp::listener)
            .and_then(|l| l.local_addr().ok())
    }

    /// Write the current peer set back to `config.org`.
    fn save_peers(&mut self, shell: &Shell) {
        let Some(sync) = self.sync.as_ref() else {
            return;
        };
        let tickets: Vec<String> = sync
            .peers()
            .iter()
            .map(|p| {
                closure_sync::SyncTicket {
                    addr: p.addr,
                    pubkey: p.key,
                }
                .encode()
            })
            .collect();
        let path = shell.vault.root().join(closure_config::CONFIG_FILE);
        let source = std::fs::read_to_string(&path).unwrap_or_default();
        match closure_config::set_config_key(&source, "sync_peers", &tickets.join(", ")) {
            Ok(updated) => {
                if let Err(e) = std::fs::write(&path, updated) {
                    self.say(format!("peer saved for this session only: {e}"));
                }
            }
            Err(e) => self.status = format!("peer saved for this session only: {e}"),
        }
    }

    /// Whether the headline tree is pinned beside a full-window buffer
    /// (`toggle-tree`).
    #[must_use]
    pub const fn tree_open(&self) -> bool {
        self.tree_open
    }

    /// Whether a row is selected, as opposed to the cursor merely
    /// resting on one. Escape clears it; a motion or a capture makes
    /// it true again.
    #[must_use]
    pub const fn selection_active(&self) -> bool {
        self.selection_active
    }

    /// Drop the selection without moving the cursor.
    pub const fn clear_selection(&mut self) {
        self.selection_active = false;
    }

    fn on_browse_key(
        &mut self,
        shell: &mut Shell,
        key: &str,
        ctrl: bool,
        alt: bool,
        text: Option<char>,
    ) {
        // Escape in the outline drops the selection (and any half-typed
        // chord): the way to tell a capture "file this loose, not under
        // whatever the cursor happens to be resting on".
        if key == "escape" {
            self.pending.clear();
            self.clear_selection();
            return;
        }
        let stroke = modal_stroke(key, ctrl, alt, text);
        let Some(stroke) = stroke else {
            self.pending.clear();
            return;
        };
        self.pending.push(stroke);
        let chord = self.pending.join(" ");
        if let Some(cmd) = self.command_for(&chord).map(ToOwned::to_owned) {
            self.pending.clear();
            self.run_command(shell, &cmd);
        } else if self
            .keys
            .iter()
            .any(|(c, _)| c.starts_with(&format!("{chord} ")))
        {
            // Valid prefix — keep the pending strokes.
        } else {
            self.pending.clear();
        }
    }

    fn run_command(&mut self, shell: &mut Shell, cmd: &str) {
        // Every door into a command comes through here — the mouse via
        // `run`, a chord, and the `:` line — which is why the way back
        // to a buffer is recorded here rather than in any one of them.
        // Hooked on `run` alone it worked for the tests and not on
        // screen, because `:messages` never touches `run`.
        //
        // And why the argument is split off here: a command line is a
        // name and a rest, and every caller that has a rest — a cron
        // entry, the `:` line, a key bound in config.org — comes
        // through this door too.
        let (cmd, args) = split_command(cmd);
        if !args.is_empty() && command_argument(cmd).is_none() {
            self.say(format!("{cmd}: takes no argument (got `{args}`)"));
            return;
        }
        self.command_args.clear();
        self.command_args.push_str(args);
        let from = self.surface;
        // Which file this command is about to touch, and whether it
        // touched anything. Undo has to follow the *edit*, not the
        // cursor: `d` is precisely the command that moves the
        // selection off what it just changed, so asking the row that
        // is selected afterwards sends `u` to another file — or, once
        // the last headline is gone, to no row at all and nowhere.
        //
        // Recorded here for the same reason the pane-return is: every
        // door into a command comes through this one function, and one
        // omission per mutating command was how the last version of
        // this went wrong.
        let touching = self
            .rows_shared(shell)
            .get(self.selected)
            .map(|r| std::path::PathBuf::from(&r.path));
        let revision = shell.vault.revision();
        self.run_command_inner(shell, cmd);
        // …but undo and redo *act on* that file, they do not choose a
        // new one. Letting them record would have `u` retarget itself
        // to wherever the cursor happened to be, so the `C-r` after it
        // went somewhere else entirely.
        if shell.vault.revision() != revision
            && !matches!(cmd, "undo" | "redo")
            && let Some(path) = touching
        {
            self.last_edited_file = Some(path);
        }
        self.note_pane_return(from);
    }

    // A flat one-arm-per-command dispatch reads clearest as one match;
    // the same precedent as `view_to_json` / `qml_item`.
    #[allow(clippy::too_many_lines)]
    fn run_command_inner(&mut self, shell: &mut Shell, cmd: &str) {
        let last = self.rows_shared(shell).len().saturating_sub(1);
        // Moving the cursor *is* selecting: Escape drops the selection
        // so a capture goes to the top level, and the next motion is
        // how you say you are looking at something again. Opening a
        // surface is not a motion — `Esc` then `c` must still capture
        // loose.
        if matches!(
            cmd,
            "next-file"
                | "prev-file"
                | "first-file"
                | "last-file"
                | "next-sibling"
                | "prev-sibling"
                | "parent"
                | "child"
        ) {
            self.selection_active = true;
        }
        match cmd {
            "next-file" => {
                self.scroll_override = None;
                self.selected = (self.selected + 1).min(last);
            }
            "prev-file" => {
                self.scroll_override = None;
                self.selected = self.selected.saturating_sub(1);
            }
            "first-file" => {
                self.scroll_override = None;
                self.selected = 0;
            }
            "last-file" => {
                self.scroll_override = None;
                self.selected = last;
            }
            "quit" => {
                if !self.refuse_quit_when_dirty() {
                    self.quit = true;
                }
            }
            "reload-shell" => self.reload_session(shell),
            // org-edit-special's pair. `C-Enter` used to do the first
            // of these from inside the editor's key handler, which
            // stood on the chord org wants for its table commands.
            // One command, the right meaning for the surface: a source
            // block writes back to where it came from, a file buffer
            // writes the file, a body commits the headline. org's
            // `C-c C-c` is context-sensitive in exactly this way.
            // The readline set, with names, so `bind` can reach it.
            // They act on whatever is taking text: the buffer when one
            // is open, otherwise the prompt's field — the same rule the
            // keys themselves follow, and the reason one binding is
            // enough for both.
            "line-start" | "line-end" | "char-left" | "char-right" | "char-up" | "char-down"
            | "word-left" | "word-right" | "delete-char" | "delete-char-back" | "kill-line"
            | "kill-line-back" | "kill-word-back" | "kill-word-forward" | "yank" => {
                self.text_motion(cmd);
            }
            // org's `C-c C-c` is context-sensitive, and a source block
            // is the context it is most famous for: there it is
            // `org-babel-execute-src-block`. Taking the chord
            // unconditionally for "save and close" meant pressing it on
            // a block said "body saved" and ran nothing.
            "commit-edit"
                if self.surface.is_editor()
                    && code_block_at(self.body.text(), self.body.cursor_line_col().0).is_some() =>
            {
                self.eval_block_in_buffer(shell);
            }
            "commit-edit" => match self.surface {
                ModalSurface::EditBlock => self.commit_edit_special(shell),
                ModalSurface::EditFile => self.commit_file_buffer(shell),
                _ => self.commit_edit_body(shell),
            },
            "discard-edit" => {
                if self.surface == ModalSurface::EditFile {
                    self.close_file_buffer();
                    self.view = ViewMode::Clickable;
                } else {
                    self.discard_editor();
                }
            }
            // The dialog is the window's — a dep-free core cannot
            // raise one — so this records the ask and the shell
            // answers it, the same shape as the clipboard mirror.
            "open-vault" => {
                if let Some(dir) = self.arg().map(str::trim).filter(|d| !d.is_empty()) {
                    self.vault_switch_path = Some(dir.to_owned());
                    self.vault_switch_asked = self.vault_switch_asked.wrapping_add(1);
                    return;
                }
                if self.can_switch_vault() {
                    self.vault_switch_asked = self.vault_switch_asked.wrapping_add(1);
                    self.say("choose a vault directory\u{2026}");
                } else {
                    self.say("unsaved edit — C-c C-c saves it first");
                }
            }
            "find-file" => {
                self.query.clear();
                self.selected = 0;
                self.find_dir = std::path::PathBuf::new();
                self.surface = ModalSurface::FindFile;
            }
            "toggle-mark" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    if !self.marks.remove(&row.id) {
                        self.marks.insert(row.id);
                    }
                    // dired steps on, so a run of rows is marked by
                    // holding one key rather than alternating with the
                    // arrow.
                    let last = self.rows_shared(shell).len().saturating_sub(1);
                    self.selected = (self.selected + 1).min(last);
                    self.say(format!("{} marked", self.marks.len()));
                }
            }
            "unmark-all" => {
                let n = self.marks.len();
                self.marks.clear();
                self.say(format!("{n} mark(s) cleared"));
            }
            "delete-marked" => {
                let targets = self.action_targets(shell);
                let mut gone = 0usize;
                for id in &targets {
                    let bid = closure_core::BlockId::from_existing(id);
                    match shell.cut_subtree(&bid) {
                        Ok(()) => gone += 1,
                        Err(e) => self.say(format!("delete failed: {e}")),
                    }
                }
                // A mark that outlives what it pointed at is a mark
                // that deletes something else next time.
                self.marks.clear();
                self.invalidate_rows();
                self.selected = self
                    .selected
                    .min(self.rows_shared(shell).len().saturating_sub(1));
                self.say(format!("deleted {gone} — p pastes the last one"));
            }
            "messages" => {
                self.query.clear();
                // The log's own cursor starts at the top. Opening a
                // list is not an edit to where you were reading.
                self.pane_cursor = 0;
                self.surface = ModalSurface::Messages;
            }
            "toggle-wrap" => {
                self.wrap = !self.wrap;
                self.say(if self.wrap {
                    "wrap on — long lines fold at the pane edge".to_owned()
                } else {
                    "wrap off — long lines scroll sideways".to_owned()
                });
            }
            // `capture <title>` files it; bare `capture` opens the bar
            // to type one into. The chord is the bare form and means
            // exactly what it did.
            "capture" if !self.command_args.is_empty() => {
                let title = std::mem::take(&mut self.command_args);
                self.commit_capture(shell, &title, false);
            }
            "capture" => {
                self.surface = ModalSurface::Capture;
                self.capture_buf.clear();
                // A pick belongs to the thought it was made for.
                self.capture_crumb_pick = None;
                self.capture_path_root = self
                    .selection_active
                    .then(|| self.selected_row_id(shell))
                    .flatten();
                self.say(format!("capture {}", self.capture_target_label(shell)));
            }
            // `search <text>` runs it; bare `search` opens the prompt.
            "search" | "search-headlines" if !self.command_args.is_empty() => {
                self.surface = ModalSurface::Search;
                self.search_return = Some(self.selected);
                self.query.set_text(&std::mem::take(&mut self.command_args));
                self.selected = 0;
            }
            "search" | "search-headlines" => {
                // Doom's `SPC s s` is search-*buffer*: swiper over the
                // thing you are looking at. Bound to the vault-wide
                // headline search it threw you out of the buffer to look
                // somewhere else entirely, which is not what a search
                // from inside an editor can mean.
                if self.surface.is_editor() {
                    self.body.modal_key("/");
                } else {
                    self.surface = ModalSurface::Search;
                    self.query.clear();
                    // Remembered so Esc is a real "never mind".
                    self.search_return = Some(self.selected);
                    self.selected = 0;
                }
            }
            // Enter *opens* the row. It used to report the row in the
            // status line, which is not what Enter means in any other
            // list in this app or any other: the selection is already
            // where the cursor is, so naming it says nothing new.
            "open-file" => {
                if self.rows_shared(shell).get(self.selected).is_some() {
                    self.selection_active = true;
                    self.run_command(shell, "edit-body");
                }
            }
            "backlinks" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    self.link_target = Some(row.id);
                    // The pane's own cursor. Writing the outline's here
                    // is what "selection at top of outline headings list
                    // when switching from any element to the Jobs panel
                    // and back" was: a glance at another pane cost you
                    // your place in the vault.
                    self.pane_cursor = 0;
                    self.surface = ModalSurface::Backlinks;
                }
            }
            "undo-history" => {
                self.surface = ModalSurface::UndoHistory;
                // The cursor opens on the active node (Q2-U3).
                self.hist_cursor = self
                    .undo_history_rows(shell)
                    .iter()
                    .position(|r| r.is_current)
                    .unwrap_or(0);
                self.say("undo history — type to filter · RET jumps there");
            }
            "agenda" => {
                // The pane's own cursor. Writing the outline's here
                // is what "selection at top of outline headings list
                // when switching from any element to the Jobs panel
                // and back" was: a glance at another pane cost you
                // your place in the vault.
                self.pane_cursor = 0;
                self.surface = ModalSurface::Agenda;
            }
            "list-blocks" => {
                // The list's own cursor; the outline stays where it is.
                self.pane_cursor = 0;
                self.surface = ModalSurface::Blocks;
            }
            // In a buffer, folding is about the buffer: the block or
            // the subtree the cursor is in, not the outline row behind
            // it.
            "toggle-fold" if self.surface.is_editor() => self.toggle_body_fold(),
            "toggle-fold" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    // Say which way it went, in the same words
                    // [`OutlineApp::toggle_fold`] uses. This said
                    // nothing at all, which left the shells' toast
                    // rules for `folded:`/`unfolded:` matching a status
                    // no modal shell ever produced.
                    self.say(match toggle_visibility(shell, &bid) {
                        Some(true) => format!("folded: {}", row.title),
                        Some(false) => format!("unfolded: {}", row.title),
                        None => format!("fold failed: {}", row.title),
                    });
                    self.selected = self
                        .selected
                        .min(self.rows_shared(shell).len().saturating_sub(1));
                }
            }
            "toggle-todo" => self.cycle_todo(shell, 1),
            "todo-back" => self.cycle_todo(shell, -1),
            "cycle-priority" => self.cycle_priority(shell, 1),
            "priority-down" => self.step_priority(shell, 1),
            "priority-up" => self.step_priority(shell, -1),
            "toggle-checkbox" => self.toggle_checkbox(),
            // A refused level change (promoting a level-1 headline: no
            // level 0 exists) used to be dropped on the floor, so the
            // key did nothing and said nothing — which is exactly what
            // "the UI doesn't refresh" feels like from the outside.
            "promote" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    self.say(match shell.promote(&bid) {
                        Ok(()) => format!("promoted: {}", row.title),
                        Err(_) if row.level <= 1 => {
                            "already at the top level — nothing to promote into".to_owned()
                        }
                        Err(e) => format!("promote failed: {e}"),
                    });
                }
            }
            "demote" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    self.say(match shell.demote(&bid) {
                        Ok(()) => format!("demoted: {}", row.title),
                        Err(e) => format!("demote failed: {e}"),
                    });
                }
            }
            // Moving up = moving the previous sibling below us; the
            // selection follows the moved heading (org rule). At the
            // end of a sibling run there is nobody to swap with, so the
            // subtree walks out of its parent instead.
            "move-subtree-up" => {
                if !self.swap_with_sibling(shell, false) {
                    self.escape_parent(shell, false);
                }
            }
            "move-subtree-down" => {
                if !self.swap_with_sibling(shell, true) {
                    self.escape_parent(shell, true);
                }
            }
            // org's four new-headline chords, plus the outline's own
            // `add-sibling`, which is the plain-sibling one by another
            // name. All of them ask for a title: `M-RET` used to make a
            // headline called "untitled" without asking, so the only
            // chord that existed was also one you had to undo.
            "manual" => {
                self.pane_cursor = 0;
                self.surface = ModalSurface::Manual;
                self.say("manual — generated from the keymap you are using · Esc back");
            }
            "describe-key" => {
                self.prompt_from = self.surface.is_editor().then_some(self.surface);
                self.surface = ModalSurface::DescribeKey;
                self.say("describe key — press one · Esc cancels");
            }
            "toggle-rail" => {
                let docked = !self.rail_docked();
                self.rail_docked = Some(docked);
                self.say(if docked {
                    "rail docked — icons only, `M-x toggle-rail` brings the labels back"
                } else {
                    "rail expanded"
                });
            }
            "trust-language" => self.trust_language(shell),
            "toggle-line-comment" => self.toggle_line_comment(),
            "add-heading"
            | "add-heading-above"
            | "add-todo-heading"
            | "add-child-heading"
            | "add-todo-child-heading"
            | "add-sibling" => self.begin_new_heading(shell, cmd),
            "edit-tags" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let tags = self.detail(shell).map(|d| d.tags).unwrap_or_default();
                    self.field_target = Some(row.id);
                    self.field_buf.set_text(&tags.join(" "));
                    self.surface = ModalSurface::TagsEdit;
                }
            }
            "edit-property" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    self.field_target = Some(row.id);
                    self.field_buf.clear();
                    self.surface = ModalSurface::PropertyEdit;
                }
            }
            "edit-body" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let place = self.current_place(shell);
                    self.push_jump(place);
                    self.open_body_by_id(shell, &row.id);
                }
            }
            "refile" => self.open_refile(shell),
            "insert-link" => self.open_insert_link(),
            "tag-picker" => self.open_tag_picker(shell),
            "zoom-in" | "zoom-out" | "zoom-reset" => {
                self.zoom_command(cmd);
            }
            "clock-in" | "clock-out" | "clock-cancel" => self.clock(shell, cmd),
            "clock-goto" => self.clock_goto(shell),
            "archive" => self.archive_selected(shell),
            "schedule" => self.open_date_pick(shell, PlanField::Scheduled),
            "deadline" => self.open_date_pick(shell, PlanField::Deadline),
            "list-buffers" => {
                self.surface = ModalSurface::Buffers;
                self.query.clear();
                self.selected = 0;
                self.say("buffers — type to filter · RET opens · Esc back");
            }
            "recent-files" => {
                self.surface = ModalSurface::Files;
                self.query.clear();
                self.selected = 0;
                self.say("files — type to filter · RET opens · Esc back");
            }
            "next-buffer" => self.cycle_buffer(shell, 1),
            "prev-buffer" => self.cycle_buffer(shell, -1),
            "alternate-buffer" => {
                if let Some(target) = self.alternate_buffer() {
                    self.open_buffer(shell, &target, true);
                } else {
                    self.say("no other buffer to switch to");
                }
            }
            "close-buffer" => self.close_current_buffer(shell, false),
            "close-buffer-force" => self.close_current_buffer(shell, true),
            "jump-back" => {
                if self.jump_at == 0 && self.jumps.is_empty() {
                    self.say("no jumps yet");
                } else {
                    // Standing at the present, the present itself has to
                    // go on the list first, or there would be nothing to
                    // come forward to.
                    if self.jump_at == self.jumps.len() {
                        let here = self.current_place(shell);
                        self.jumps.push(here);
                    }
                    if self.jump_at == 0 {
                        self.say("no older jump");
                    } else {
                        self.jump_at -= 1;
                        let place = self.jumps[self.jump_at].clone();
                        self.goto_place(shell, &place);
                    }
                }
            }
            "jump-forward" => {
                if self.jump_at + 1 < self.jumps.len() {
                    self.jump_at += 1;
                    let place = self.jumps[self.jump_at].clone();
                    self.goto_place(shell, &place);
                } else {
                    self.say("no newer jump");
                }
            }
            "set-input-mode" => {
                let Some(name) = self.arg().map(str::trim).map(ToOwned::to_owned) else {
                    self.say("set-input-mode: name one — emacs vim doom helix notion");
                    return;
                };
                match name.to_ascii_lowercase().as_str() {
                    "emacs" => self.mode = InputMode::Emacs,
                    "vim" => self.mode = InputMode::Vim,
                    "doom" => self.mode = InputMode::Doom,
                    "helix" => self.mode = InputMode::Helix,
                    "notion" => self.mode = InputMode::Notion,
                    _ => {
                        self.say(format!("{name}: no such input mode"));
                        return;
                    }
                }
                self.rebuild_keymap();
                self.settle_editor_mode();
                self.say(format!("input mode: {:?}", self.mode));
            }
            "next-input-mode" => {
                self.mode = match self.mode {
                    InputMode::Notion => InputMode::Emacs,
                    InputMode::Emacs => InputMode::Vim,
                    InputMode::Vim => InputMode::Doom,
                    InputMode::Doom => InputMode::Helix,
                    InputMode::Helix => InputMode::Notion,
                };
                self.rebuild_keymap();
                self.settle_editor_mode();
                // The view deliberately does *not* follow. It used to:
                // a mode with a NORMAL was assumed to want the file and
                // one without it the rows. In practice that means
                // clicking the mode chip to try Vim throws away the
                // pane you were reading — a large surprise for a small
                // chord, and `toggle-view` is right there for anyone
                // who wants the other shape ("switching the mode in the
                // top left corner will show the full view body editor.
                // Disable this behavior").
            }
            // `rename <title>` renames the selected row; bare `rename`
            // opens the field with the old title in it.
            "rename" if !self.command_args.is_empty() => {
                let title = std::mem::take(&mut self.command_args);
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let id = closure_core::BlockId::from_existing(&row.id);
                    match shell.rename_headline(&id, &title) {
                        Ok(()) => self.say(format!("renamed to {title}")),
                        Err(e) => self.say(format!("rename: {e}")),
                    }
                } else {
                    self.say("rename: nothing is selected");
                }
            }
            "rename" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    self.field_target = Some(row.id);
                    self.field_buf.set_text(&row.title);
                    self.surface = ModalSurface::Rename;
                    self.say("rename — Enter save, Esc cancel");
                }
            }
            // `goto <id>` — the one command that is only useful with an
            // argument, which is why it did not exist until there was a
            // way to give it one.
            "goto" => {
                let id = std::mem::take(&mut self.command_args);
                if id.is_empty() {
                    self.say("goto: which id?");
                } else if let Some(i) = self.rows_shared(shell).iter().position(|r| r.id == id) {
                    self.selected = i;
                    self.selection_active = true;
                    self.scroll_override = None;
                } else {
                    self.say(format!("goto: no headline with id {id}"));
                }
            }
            // A delete is a *cut*: the subtree goes on the kill ring on
            // its way out, so `d` then `p` moves it the way vim moves a
            // line. Dropping the text left undo as the only way back,
            // and undo is not a way to move something.
            "delete" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    match shell.cut_subtree(&bid) {
                        Ok(()) => {
                            // Onto the register too, so the window's
                            // existing watcher offers it to the system
                            // clipboard: "sync with system clipboard
                            // (two way)" was one way only outside a
                            // buffer, and a headline you cut could not
                            // be pasted anywhere else.
                            if let Some(text) = shell.ring_top() {
                                let text = text.to_owned();
                                self.set_register_from_clipboard(&text);
                            }
                            self.status = format!("cut: {} — p pastes it", row.title);
                        }
                        Err(e) => self.status = format!("delete failed: {e}"),
                    }
                    self.selected = self
                        .selected
                        .min(self.rows_shared(shell).len().saturating_sub(1));
                }
            }
            "paste-subtree" => {
                if let Some(row) = self.rows_shared(shell).get(self.selected).cloned() {
                    let bid = closure_core::BlockId::from_existing(&row.id);
                    match shell.paste_subtree(&bid) {
                        Ok(()) => self.status = format!("pasted after {}", row.title),
                        // Nothing was cut here, but something may be on
                        // the clipboard — org copied from a browser, a
                        // subtree from another window. The ring wins
                        // when it has anything, so cut-and-paste inside
                        // the outline never starts pasting whatever an
                        // unrelated application last held.
                        Err(_) => self.paste_clipboard_subtree(shell, &bid, &row.title),
                    }
                } else {
                    self.say("nothing selected — put the cursor where it should land");
                }
            }
            "undo" => {
                if let Some(path) = self.undo_target(shell) {
                    match shell.vault.undo_in(&path) {
                        Ok(()) => self.say("undo"),
                        Err(e) => self.status = format!("undo failed: {e}"),
                    }
                    self.selected = self
                        .selected
                        .min(self.rows_shared(shell).len().saturating_sub(1));
                } else {
                    self.say("nothing to undo");
                }
            }
            "redo" => {
                if let Some(path) = self.undo_target(shell) {
                    match shell.vault.redo_in(&path) {
                        Ok(()) => self.say("redo"),
                        Err(e) => self.status = format!("redo failed: {e}"),
                    }
                    self.selected = self
                        .selected
                        .min(self.rows_shared(shell).len().saturating_sub(1));
                } else {
                    self.say("nothing to redo");
                }
            }
            "palette" => self.open_palette(),
            "toggle-which-key" => self.which_key_open = !self.which_key_open,
            "dismiss-notifications" => self.notifications.clear(),
            // Half a screen: the step between one row at a time and
            // jumping to an end, and the one vim put on these chords.
            "half-page-down" | "half-page-up" => {
                let step = (self.outline_viewport / 2).max(1);
                let last = self.rows_shared(shell).len().saturating_sub(1);
                self.selected = if cmd == "half-page-down" {
                    (self.selected + step).min(last)
                } else {
                    self.selected.saturating_sub(step)
                };
                self.selection_active = true;
            }
            "ex-command" => self.begin_ex(),
            "llm" => {
                self.chat_buf.clear();
                self.surface = ModalSurface::Llm;
                self.say("assistant — type a question, Enter sends, Esc back");
            }
            "graph" | "journal" | "cron" => {
                // The pane's cursor starts at the top; the outline's
                // does not move. Opening a pane is not an edit to where
                // you were reading.
                self.pane_cursor = 0;
                self.surface = match cmd {
                    "graph" => ModalSurface::Graph,
                    "journal" => ModalSurface::Journal,
                    _ => ModalSurface::Cron,
                };
                self.say(format!("{cmd} — Esc back"));
            }
            "sync" => {
                self.sync_buf.clear();
                self.sync_mut();
                self.surface = ModalSurface::Sync;
                self.say("sync — hand over your ticket, paste theirs, Esc back");
            }
            "preview-diagrams" => self.preview_diagrams(shell),
            // "Create a function/command that returns these values or
            // alternatively prints them to the stdout/*MESSAGES*
            // buffer." Both: `closure_core::build_info()` is the
            // function, and this puts it where a bug report can copy
            // it from.
            "open-config" => self.open_config(shell),
            "assistant-setup" => {
                self.settings_cursor = 0;
                self.editing_setting = None;
                self.surface = ModalSurface::Settings;
            }
            "build-info" => {
                let line = format!("closure {}", closure_core::build_info().describe());
                self.say(line);
            }
            "toggle-trace" => {
                self.tracing = !self.tracing;
                self.say(if self.tracing {
                    "tracing on — slow keys land in the message log (g M, or :messages)"
                } else {
                    "tracing off"
                });
            }
            "toggle-inline-images" => {
                self.images_shown = !self.images_shown;
                self.say(if self.images_shown {
                    "inline images shown".to_owned()
                } else {
                    "inline images hidden — the links stay".to_owned()
                });
            }
            "sync-export" => self.sync_export(shell),
            "sync-import" => self.sync_import(shell),
            "edit-special" => self.begin_edit_special(shell),
            // Where it was pressed decides what runs. In a buffer that
            // is the block under the cursor (org's `C-c C-c`); in the
            // Blocks list it is the row the cursor is on. Anywhere else
            // there is no block in view, and the old code read the
            // *outline's* selection as an index into the vault-wide
            // block list — pressing it on the third headline ran the
            // third block in the vault, whatever that was.
            "execute-block" => {
                if self.surface.is_editor() {
                    self.eval_block_in_buffer(shell);
                } else if self.surface == ModalSurface::Blocks {
                    self.eval_selected_block(shell);
                } else {
                    self.say(
                        "no source block under the cursor — open the note, or list the blocks",
                    );
                }
            }
            "list-headlines" => {
                // The list's own cursor; the outline stays where it is.
                self.pane_cursor = 0;
                self.surface = ModalSurface::Headlines;
                self.say("headlines — type to filter · RET goes to it");
            }
            "db-view" => {
                self.pane_cursor = 0;
                self.surface = ModalSurface::DbView;
                self.say("database — RET jump, Esc back");
            }
            "body-search" => {
                self.query.clear();
                self.selected = 0;
                self.surface = ModalSurface::BodySearch;
                self.say("body search — type to filter, RET jump, Esc back");
            }
            "toggle-llm-render" => {
                self.llm_render = !self.llm_render;
                self.say(format!(
                    "LLM render access {}",
                    if self.llm_render {
                        "granted"
                    } else {
                        "revoked"
                    }
                ));
            }
            "sniffer" => {
                // The sniffer keeps its own cursor (`sniffer_cursor`);
                // this only ever moved the outline behind it.
                self.surface = ModalSurface::Sniffer;
                // Where the flows come from: the vault's own capture
                // log. Read on open, so the pane shows what is there
                // rather than telling you to go and run another
                // program.
                let n = self.sniffer.load(&shell.vault);
                self.say(if n == 0 {
                    "no flows in network.org yet — `closure sniff --live <iface>` writes it"
                        .to_owned()
                } else {
                    format!("{n} flow(s) — a allow, b block, r reload, Esc back")
                });
            }
            "reload-flows" => {
                let n = self.sniffer.load(&shell.vault);
                self.say(format!("{n} flow(s) from network.org"));
            }
            "debug-flow" => {
                self.sniffer_debug = !self.sniffer_debug;
                self.surface = ModalSurface::Sniffer;
                self.say(if self.sniffer_debug {
                    "debug — what was recorded, and what it was matched against"
                } else {
                    "debug off"
                });
            }
            "allow-flow" | "block-flow" => {
                if self.sniffer.events().is_empty() {
                    self.say("no captured flows");
                } else {
                    if cmd == "allow-flow" {
                        self.sniffer.allow_selected();
                    } else {
                        self.sniffer.block_selected();
                    }
                    self.surface = ModalSurface::Sniffer;
                    self.say(
                        self.sniffer
                            .detail()
                            .unwrap_or_else(|| "flow rule updated".to_owned()),
                    );
                }
            }
            "conflicts" => {
                // Likewise: the resolver's cursor is its own.
                self.surface = ModalSurface::Conflicts;
                self.say("conflicts — o ours, t theirs, Esc back");
            }
            // The way home. Esc has always walked back out of a pane,
            // but Esc is a keyboard-only door: the rail's home button
            // needs a command of its own, and a `g h` for the users who
            // would rather not reach for Esc.
            "browse" => {
                self.slash = None;
                self.surface = ModalSurface::Browse;
                self.say("outline");
            }
            // `SPC f s` / `:w`: write whatever buffer is open. Which
            // one that is decides what "write" means — a body commits
            // through the kernel command, a file writes its whole
            // source — and outside a buffer there is nothing to write,
            // because every other edit is already on disk.
            "save-buffer" => match self.surface {
                ModalSurface::EditFile => self.commit_file_buffer(shell),
                // Write and carry on, the way `C-s` does in every
                // editor and the way the file buffer beside it already
                // did. Closing on save is what made a headline you had
                // just typed go out of sight.
                ModalSurface::EditBody => self.write_body(shell),
                ModalSurface::EditBlock => self.write_edit_special(shell),
                _ => self.say("no buffer open — every edit is already written"),
            },
            // The switch between the two shapes of the shell: rows you
            // click, or the file itself in one buffer.
            "toggle-file-view" => {
                // Keyed off the surface rather than the stored view: a
                // modal mode *starts* in the editor view without a
                // buffer open yet (nothing has a shell to open one
                // with until the shell hands us one), and a toggle that
                // trusted the flag would close a buffer that was never
                // opened.
                if self.surface == ModalSurface::EditFile {
                    // The toggle is the most easily mistyped way out of
                    // a file buffer, so it refuses like the deliberate
                    // ones rather than dropping the file quietly.
                    if self.body_dirty() {
                        self.say("unsaved file — C-s writes it · C-c C-k discards");
                    } else {
                        // Two views of one document should agree about
                        // where you are in it: leaving the buffer
                        // selects the headline the caret was in.
                        let landed = self.headline_at_caret(shell);
                        self.view = ViewMode::Clickable;
                        self.close_file_buffer();
                        if let Some(id) = landed {
                            self.select_by_id(shell, &id);
                        }
                        self.say("outline view");
                    }
                } else {
                    self.view = ViewMode::Editor;
                    self.open_file_buffer(shell);
                    // …and entering it puts the caret on the headline
                    // the outline was showing, rather than at line one.
                    self.caret_to_selected_headline(shell);
                }
            }
            // The tree beside a full-window buffer: writing *into* an
            // outline is a different job from reading one, and this is
            // how you get the shape back without leaving the buffer.
            // The shells own the panel, so the core owns the flag.
            "toggle-tree" => {
                self.tree_open = !self.tree_open;
                self.say(if self.tree_open {
                    "headline tree shown".to_owned()
                } else {
                    "headline tree hidden".to_owned()
                });
            }
            "resolve-ours" | "resolve-theirs" => {
                if self.conflicts.conflicts().is_empty() {
                    self.say("no conflicts to resolve");
                } else {
                    let ours = cmd == "resolve-ours";
                    let result = if ours {
                        self.conflicts.resolve_ours(shell)
                    } else {
                        self.conflicts.resolve_theirs(shell)
                    };
                    let side = if ours { "ours" } else { "theirs" };
                    self.say(match result {
                        Ok(()) => format!("resolved {side}"),
                        Err(e) => format!("resolve failed: {e}"),
                    });
                    self.surface = ModalSurface::Conflicts;
                }
            }
            other => self.status = format!("{other}: unknown command"),
        }
    }

    /// The chord bound to `command` in the *active* mode, if any.
    ///
    /// The one lookup a shell calls while painting any affordance, so
    /// that every button and menu entry can carry its keybinding — and
    /// so that they all change together when the mode does. Sourced
    /// from [`closure_input::chord_for_command`] (I4).
    #[must_use]
    pub fn chord_for(&self, command: &str) -> Option<&str> {
        self.chords_for(command).first().copied()
    }

    /// What `chord` runs in the mode in force, or `None` when nothing
    /// is bound to it.
    ///
    /// Emacs answers "M-# is undefined" rather than doing nothing, and
    /// this is the same answer: silence reads as a broken keyboard.
    #[must_use]
    pub fn describe_key(&self, chord: &str) -> Option<KeyDescription> {
        let command = self.command_for(chord)?.to_owned();
        let (description, section) = Self::registry_entry(&command)?;
        Some(KeyDescription {
            chord: chord.to_owned(),
            command,
            description,
            section,
        })
    }

    /// What `command` is, and every chord that reaches it.
    ///
    /// Both halves at once because "what is this" and "how do I run it"
    /// are the same question asked from either end.
    #[must_use]
    pub fn describe_command(&self, command: &str) -> Option<CommandDescription> {
        let (description, section) = Self::registry_entry(command)?;
        Some(CommandDescription {
            command: command.to_owned(),
            description,
            section,
            chords: self
                .chords_for(command)
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
        })
    }

    /// The registry's `(description, section)` for a canonical name.
    fn registry_entry(command: &str) -> Option<(String, String)> {
        PALETTE_COMMANDS
            .iter()
            .find(|(_, name, ..)| *name == command)
            .map(|(_, _, section, desc)| ((*desc).to_owned(), (*section).to_owned()))
    }

    /// Every chord bound to `command` in the active mode, primary
    /// first — what a pane with room for more than one shows.
    #[must_use]
    pub fn chords_for(&self, command: &str) -> Vec<&str> {
        self.keys
            .iter()
            .filter(|(_, cmd)| cmd == command)
            .map(|(chord, _)| chord.as_str())
            .collect()
    }

    /// The command `chord` runs in the active keymap, if any.
    #[must_use]
    pub fn command_for(&self, chord: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|(c, _)| c == chord)
            .map(|(_, cmd)| cmd.as_str())
    }

    /// The keymap in force: the chosen mode plus whatever `config.org`
    /// said about it.
    #[must_use]
    pub fn keymap(&self) -> &[(String, String)] {
        &self.keys
    }

    /// Apply `config.org`'s `bind` lines on top of the current mode.
    ///
    /// Kept as the overrides rather than as the result, because
    /// `cycle-mode` has to reapply them to the next keymap: a rebind
    /// the user wrote down is about their fingers, not about whichever
    /// of the five schemes they are looking at.
    pub fn set_key_overrides(&mut self, overrides: Vec<(String, String)>) {
        self.key_overrides = overrides;
        self.rebuild_keymap();
        // A command name this shell does not have cannot be caught when
        // the file is parsed — closure-config does not know the command
        // list — so it is said out loud here rather than binding a key
        // to nothing and leaving it to be found by pressing it.
        let unknown: Vec<String> = self
            .key_overrides
            .iter()
            .filter(|(_, cmd)| !cmd.is_empty() && !command_exists(cmd))
            .map(|(chord, cmd)| format!("{chord} → {cmd}"))
            .collect();
        if !unknown.is_empty() {
            self.say(format!(
                "config.org: no such command — {}",
                unknown.join(", ")
            ));
        }
    }

    /// Run one of the named readline motions against whatever is
    /// taking text right now.
    ///
    /// A buffer if one is open, otherwise the prompt's own field. When
    /// neither is, the command says so: a chord you can press anywhere
    /// has to answer where it means nothing.
    fn text_motion(&mut self, command: &str) {
        if self.surface.is_editor() {
            match command {
                "line-start" => self.body.line_home(),
                "line-end" => self.body.line_end_motion(),
                "char-left" => self.body.left(),
                "char-right" => self.body.right(),
                "char-up" => self.body.up(),
                "char-down" => self.body.down(),
                "word-left" => self.body.word_backward(),
                "word-right" => self.body.word_end_forward(),
                "delete-char" => self.body.delete_at(),
                "delete-char-back" => self.body.backspace(),
                "kill-line" => self.body.kill_rest_of_line(),
                "kill-line-back" => self.body.kill_to_line_start(),
                "kill-word-back" => self.kill_word(false),
                "kill-word-forward" => self.kill_word(true),
                "yank" => self.body.yank_insert(),
                _ => {}
            }
            return;
        }
        // The prompts share one readline implementation, so the motion
        // is spelled as the stroke it stands for and handed to it —
        // rather than a second copy of the same fifteen answers.
        let stroke: (&str, bool, bool) = match command {
            "line-start" => ("a", true, false),
            "line-end" => ("e", true, false),
            "char-left" => ("b", true, false),
            "char-right" => ("f", true, false),
            "char-up" => ("up", false, false),
            "char-down" => ("down", false, false),
            "word-left" => ("left", false, true),
            "word-right" => ("right", false, true),
            "delete-char" => ("d", true, false),
            "delete-char-back" => ("backspace", false, false),
            "kill-line" => ("k", true, false),
            "kill-line-back" => ("u", true, false),
            "kill-word-back" => ("w", true, false),
            "kill-word-forward" => ("d", false, true),
            "yank" => ("y", true, false),
            _ => return,
        };
        let mut kill = self.shared_kill();
        let field = match self.surface {
            ModalSurface::Search | ModalSurface::BodySearch => Some(&mut self.query),
            ModalSurface::Capture => Some(&mut self.capture_buf),
            ModalSurface::Ex => Some(&mut self.ex_buf),
            ModalSurface::Llm => Some(&mut self.chat_buf),
            ModalSurface::Sync => Some(&mut self.sync_buf),
            // Every picker and every one-line prompt types into the
            // same field, which is why they all answer to the same
            // chords in the first place.
            ModalSurface::Palette
            | ModalSurface::TagsEdit
            | ModalSurface::PropertyEdit
            | ModalSurface::Rename
            | ModalSurface::AddSibling
            | ModalSurface::Headlines
            | ModalSurface::Blocks
            | ModalSurface::Messages
            | ModalSurface::UndoHistory
            | ModalSurface::Files
            | ModalSurface::Buffers => Some(&mut self.field_buf),
            // A list you only walk, or the outline itself: there is no
            // caret here for a motion to move, and saying so beats
            // moving one you cannot see.
            _ => None,
        };
        let Some(field) = field else {
            self.keep_shared_kill(&kill);
            self.say(format!("{command}: nothing here is taking text"));
            return;
        };
        let claimed = line_key(field, &mut kill, stroke.0, stroke.1, stroke.2, None);
        self.keep_shared_kill(&kill);
        if !claimed {
            self.say(format!("{command}: that key does nothing here"));
        }
    }

    /// Rebuild [`Self::keys`] from the mode and the overrides.
    fn rebuild_keymap(&mut self) {
        self.keys = closure_input::keymap_with(self.mode, &self.key_overrides);
    }

    /// Output of the last source block run, while the Blocks surface
    /// still shows the block that produced it.
    #[must_use]
    pub fn block_output(&self) -> Option<&str> {
        self.block_out.as_deref()
    }

    /// Run the source block under the Blocks cursor through the kernel
    /// (org-babel), keeping its output for the pane to paint.
    ///
    /// The execution path — and in particular the `eval_trust`
    /// allowlist, the reason opening a file cannot run its code — is
    /// the kernel's [`closure_store::Vault::eval_block`]. A refusal is
    /// reported, never worked around.
    /// `C-c C-c`: run the block the cursor is in, from the buffer as it
    /// stands rather than from the file on disk.
    ///
    /// Org runs what you are looking at, unsaved edits included, and
    /// writes `#+RESULTS:` back into the buffer — so a run is an edit
    /// like any other and `u` undoes it. The trust gate is the same
    /// one [`closure_store::Vault::eval_block`] consults: a second
    /// route to evaluation must not be a way around the policy.
    fn eval_block_in_buffer(&mut self, shell: &Shell) {
        self.block_out = None;
        let (line, _) = self.body.cursor_line_col();
        let Some(block) = code_block_at(self.body.text(), line) else {
            self.say("no source block under the cursor");
            return;
        };
        if !closure_eval::eval_allowed(&shell.vault.eval_trust(), &block.lang) {
            // Naming the concept is not naming the fix: the companion
            // report is somebody who could not work out the remedy
            // from a refusal that mentioned `eval_trust` and stopped.
            self.say(Self::trust_refusal(shell, &block.lang));
            return;
        }
        let Some(backend) = closure_eval::backend_for(&block.lang) else {
            self.say(format!("no backend for `{}`", block.lang));
            return;
        };
        let header = closure_eval::HeaderArgs::parse(&block.args);
        let program = format!(
            "{}{}",
            closure_eval::var_prelude(&block.lang, &header.vars),
            block.program
        );
        match backend.eval_bounded(&program, closure_eval::Bounds::default()) {
            Ok(out) => {
                if header.is_silent() {
                    self.say(format!("ran the {} block · results silent", block.lang));
                } else {
                    let text = attach_results(self.body.text(), block.end, &out.stdout);
                    let at = self.body.cursor_byte();
                    self.body.replace_all(text, at);
                    self.say(format!("ran the {} block", block.lang));
                }
                self.block_out = Some(out.stdout);
            }
            Err(e) => self.status = format!("the block failed: {e}"),
        }
    }

    fn eval_selected_block(&mut self, shell: &mut Shell) {
        self.block_out = None;
        let rows = self.block_rows(shell);
        // The block list's own cursor, not the outline's — see
        // [`Self::picker_cursor`]. They were the same field until the
        // message log turned out to be scrolling the notes behind it.
        //
        // The field rather than `picker_cursor()`: that accessor asks
        // which surface is open, and by the time a command runs the
        // surface may already have been left. This is only ever reached
        // from the block list, so the list's cursor is the answer.
        let Some(BlockRow { file: path, .. }) = rows.get(self.pane_cursor).cloned() else {
            self.say("no source blocks in this vault");
            return;
        };
        // `block_rows` is flat across files; `eval_block` counts within
        // one, so rebase the cursor onto the block's own file.
        let index = rows[..self.pane_cursor]
            .iter()
            .filter(|b| b.file == path)
            .count();
        self.surface = ModalSurface::Blocks;
        match shell.vault.eval_block(std::path::Path::new(&path), index) {
            Ok(out) => {
                self.say(format!("ran block #{index} of {path}"));
                self.block_out = Some(out);
            }
            Err(e) => self.status = format!("{e}"),
        }
    }

    /// Every headline in the selected row's file, as `(title, id)` —
    /// the flat per-file listing behind the Headlines surface.
    #[must_use]
    pub fn headline_rows(&self, shell: &Shell) -> Vec<HeadlineRow> {
        let rows = self.rows_shared(shell);
        let Some(row) = rows.get(self.selected.min(rows.len().saturating_sub(1))) else {
            return Vec::new();
        };
        let path = std::path::PathBuf::from(&row.path);
        shell
            .vault
            .iter()
            .filter(|(p, _)| *p == path.as_path())
            .flat_map(|(_, doc)| doc.all_headlines())
            .map(|h| HeadlineRow {
                title: h.title().to_owned(),
                id: h.id().to_string(),
            })
            .collect()
    }

    /// The Notion-style database table over the whole vault: a header
    /// row plus one cell row per headline.
    ///
    /// Deliberately loose, the way Notion's databases are: every
    /// headline is a row, a missing value is an empty cell rather than
    /// an error, and the columns are the properties an org headline
    /// always has.
    #[must_use]
    pub fn db_rows(&self, shell: &Shell) -> (Vec<String>, Vec<Vec<String>>) {
        let header = ["title", "todo", "priority", "tags"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let rows = shell
            .vault
            .iter()
            .flat_map(|(_, doc)| doc.all_headlines())
            .map(|h| {
                vec![
                    h.title().to_owned(),
                    h.todo().unwrap_or_default().to_owned(),
                    h.priority().map(String::from).unwrap_or_default(),
                    h.tags().join(" "),
                ]
            })
            .collect();
        (header, rows)
    }

    /// Body-text hits for the current query, as `(id, "title — line")`.
    ///
    /// The outline search matches titles; this one matches the text
    /// underneath them, which is the only way to find a note you
    /// remember the contents but not the heading of. An empty query
    /// matches nothing — dumping the vault is not a search result.
    #[must_use]
    pub fn body_search_rows(&self, shell: &Shell) -> Vec<(String, String)> {
        if self.query.is_empty() {
            return Vec::new();
        }
        let needle = self.query.text().to_lowercase();
        let mut out = Vec::new();
        for (_, doc) in shell.vault.iter() {
            for h in doc.all_headlines() {
                // The comma escape is an on-disk spelling: showing it
                // in a hit means the line the user reads is not the
                // line they typed.
                let body = closure_org::unescape_body(h.body_text());
                for line in body
                    .lines()
                    .filter(|l| l.to_lowercase().contains(needle.as_str()))
                {
                    out.push((
                        h.id().to_string(),
                        format!("{} — {}", h.title(), line.trim()),
                    ));
                }
            }
        }
        out
    }

    /// Whether the LLM may read the rendered view (V3b). Off until
    /// `toggle-llm-render` grants it.
    #[must_use]
    pub const fn llm_render_access(&self) -> bool {
        self.llm_render
    }

    /// Whether the sniffer pane is showing the raw record behind the
    /// selected flow.
    #[must_use]
    pub const fn sniffer_debug(&self) -> bool {
        self.sniffer_debug
    }

    /// The sniffer surface's state, for painting and for feeding
    /// captures in.
    #[must_use]
    pub const fn sniffer(&self) -> &SnifferApp {
        &self.sniffer
    }

    /// Mutable sniffer state — a shell records live captures here.
    pub const fn sniffer_mut(&mut self) -> &mut SnifferApp {
        &mut self.sniffer
    }

    /// The pending CRDT conflicts, for painting.
    #[must_use]
    pub const fn conflicts(&self) -> &ConflictApp {
        &self.conflicts
    }

    /// Load the conflicts a merge produced, so the Conflicts surface
    /// has something to resolve.
    pub fn set_conflicts(&mut self, conflicts: Vec<closure_crdt::FieldConflict>) {
        self.conflicts = ConflictApp::new(conflicts, self.mode);
    }
}

/// One step through a popup's list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListStep {
    /// Down the list.
    Next,
    /// Up the list.
    Prev,
}

/// Which way a popup's list steps for this key, if it steps at all.
///
/// Three pairs mean the same thing to a hand in front of a list: the
/// arrows, Emacs' `C-n`/`C-p`, and Doom's `C-j`/`C-k` — the pair evil
/// puts on company's and vertico's maps so a modal user never leaves
/// the home row to pick a candidate. One function so every popup
/// answers to all three (I4) instead of each surface picking one.
///
/// Callers where `C-k` already means kill-to-end-of-line ask this only
/// while a popup is actually showing, which is the scope company's own
/// map has.
fn list_step(key: &str, ctrl: bool) -> Option<ListStep> {
    match key {
        "down" => Some(ListStep::Next),
        "up" => Some(ListStep::Prev),
        "n" | "j" if ctrl => Some(ListStep::Next),
        "p" | "k" if ctrl => Some(ListStep::Prev),
        _ => None,
    }
}

/// Move a popup cursor one place, wrapping at both ends.
///
/// "when you are at the end or start and want to go beyond the limit it
/// should overflow". A list that stops dead makes the entry one past
/// the end — the one you were reaching for — cost the whole trip back,
/// and every completion popup worth using wraps instead.
///
/// Popups only. The outline is a document, not a candidate list, and a
/// `j` at the last headline that jumped to the first would lose your
/// place in what you are reading.
const fn step_wrapping(cursor: usize, len: usize, step: ListStep) -> usize {
    if len == 0 {
        return 0;
    }
    match step {
        ListStep::Next => {
            if cursor + 1 >= len {
                0
            } else {
                cursor + 1
            }
        }
        ListStep::Prev => {
            if cursor == 0 {
                len - 1
            } else {
                cursor - 1
            }
        }
    }
}

/// Translate a GUI key event into a keymap chord stroke (`C-n`, `M-<`,
/// `<down>`, `RET`, bare `g`/`G`). Returns `None` for keys with no
/// stroke representation.
fn modal_stroke(key: &str, ctrl: bool, alt: bool, text: Option<char>) -> Option<String> {
    // A shell spells the keys where shift *changes the command* as
    // `shift-<key>` — Enter, TAB and the arrows, the ones with no
    // shifted character of their own. The keymaps spell that idea org's
    // way, modifiers outermost: `M-S-RET` is org-insert-todo-heading,
    // `M-S-<right>` inserts a table column. A shifted *letter* never
    // comes through here as `shift-a`; it arrives as `A`.
    let (shift, key) = key
        .strip_prefix("shift-")
        .map_or((false, key), |rest| (true, rest));
    let base = match key {
        "enter" => "RET".to_owned(),
        "escape" => "ESC".to_owned(),
        "backspace" => "DEL".to_owned(),
        "tab" => "TAB".to_owned(),
        "space" => "SPC".to_owned(),
        "down" => "<down>".to_owned(),
        "up" => "<up>".to_owned(),
        "left" => "<left>".to_owned(),
        "right" => "<right>".to_owned(),
        _ => {
            if let Some(c) = text {
                c.to_string()
            } else if ctrl || alt {
                key.to_ascii_lowercase()
            } else {
                return None;
            }
        }
    };
    // Modifiers outermost, org's own order: `C-M-S-<return>`. Ctrl
    // used to win outright and drop Alt, so no chord with both could
    // ever be produced — `C-M-RET` (org-insert-subheading) resolved as
    // plain `C-RET` and quietly ran the wrong command.
    let base = if shift { format!("S-{base}") } else { base };
    let base = if alt { format!("M-{base}") } else { base };
    Some(if ctrl { format!("C-{base}") } else { base })
}

/// The git widget's last answer, and when it was taken.
#[derive(Debug, Clone)]
struct GitMemo {
    /// Vault revision the answer was taken at.
    revision: u64,
    /// When it was taken, for the rate limit.
    taken: std::time::Instant,
    /// The answer; `None` when the vault is not a repository.
    state: Option<closure_store::GitStatus>,
}

/// One row of the assistant's setup screen: a config key, what it is
/// set to, and what the shell can tell the user about it.
#[derive(Debug, Clone)]
pub struct SettingField {
    /// The `config.org` key this row writes.
    pub key: &'static str,
    /// Short human label.
    pub label: &'static str,
    /// One line on what the setting is for.
    pub help: &'static str,
    /// What it is set to now — empty when unset.
    pub value: String,
    /// Shown in place of an empty value, so "unset" and "set to
    /// nothing" do not look the same.
    pub placeholder: &'static str,
    /// Live commentary: what the value *means* right now. Where a
    /// request will really go; whether the named key variable actually
    /// exists in the environment. Never the key itself.
    pub detail: String,
    /// The values worth offering, when the setting is a closed set.
    pub choices: Vec<String>,
}

/// The assistant's settings, in the order they are worth filling in.
///
/// The key never appears here. `llm_key_env` names an *environment
/// variable*, so this screen can be photographed without leaking
/// anything — but it does say whether that variable is currently set,
/// because "provider configured, key never exported" is the failure
/// people actually hit and it is invisible from the config alone.
#[must_use]
pub fn assistant_settings(cfg: &closure_config::Config) -> Vec<SettingField> {
    assistant_settings_with(cfg, &|name| std::env::var(name).ok())
}

/// The same, reading the environment through `lookup`.
///
/// The shell passes the real environment. Taking it as an argument is
/// what lets "the named key variable is missing" be tested at all: the
/// workspace forbids `unsafe`, and `set_var` is unsafe for good
/// reasons — it is process-wide and racy against every other thread.
#[must_use]
pub fn assistant_settings_with(
    cfg: &closure_config::Config,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Vec<SettingField> {
    let provider = cfg.llm_provider.clone().unwrap_or_default();
    let kind = closure_llm::provider_kind(cfg.llm_provider.as_deref());
    let needs_key = matches!(
        kind,
        closure_llm::ProviderKind::OpenAi | closure_llm::ProviderKind::Anthropic
    );

    let key_detail = match cfg.llm_key_env.as_deref() {
        _ if !needs_key => "this provider needs no key".to_owned(),
        None | Some("") => "no variable named — required for this provider".to_owned(),
        Some(var) => {
            if lookup(var).is_some_and(|v| !v.is_empty()) {
                format!("${var} is set")
            } else {
                format!("${var} is not set in this environment")
            }
        }
    };

    let endpoint_detail = cfg.llm_endpoint.as_deref().map_or_else(
        || {
            let default = match kind {
                closure_llm::ProviderKind::OpenAi => closure_llm::OPENAI_URL,
                closure_llm::ProviderKind::Anthropic => closure_llm::ANTHROPIC_URL,
                closure_llm::ProviderKind::Ollama => "http://localhost:11434",
                closure_llm::ProviderKind::Echo => "nowhere — echo never leaves the process",
                closure_llm::ProviderKind::Unknown => {
                    "nowhere — the provider name is not one of these"
                }
            };
            format!("unset, so requests go to {default}")
        },
        |url| format!("requests go to {url}"),
    );

    vec![
        SettingField {
            key: "llm_provider",
            label: "Provider",
            help: "Which service answers — echo, ollama, openai, anthropic. echo never leaves the process.",
            value: provider,
            placeholder: "unset — echo",
            detail: String::new(),
            choices: ["echo", "ollama", "openai", "anthropic"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
        },
        SettingField {
            key: "llm_model",
            label: "Model",
            help: "The model name, as the provider spells it — e.g. claude-sonnet-4-5, gpt-4o, llama3.",
            value: cfg.llm_model.clone().unwrap_or_default(),
            placeholder: "unset — the provider's default",
            detail: String::new(),
            choices: Vec::new(),
        },
        SettingField {
            key: "llm_key_env",
            label: "Key variable",
            help: "Names the environment variable holding the key — e.g. ANTHROPIC_API_KEY. The key itself never lives in this file.",
            value: cfg.llm_key_env.clone().unwrap_or_default(),
            placeholder: "unset",
            detail: key_detail,
            choices: ["ANTHROPIC_API_KEY", "OPENAI_API_KEY"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
        },
        SettingField {
            key: "llm_endpoint",
            label: "Endpoint",
            help: "Point at any compatible server — e.g. http://localhost:11434 or http://localhost:8080/v1/chat/completions.",
            value: cfg.llm_endpoint.clone().unwrap_or_default(),
            placeholder: "unset — the provider's own",
            detail: endpoint_detail,
            choices: Vec::new(),
        },
        SettingField {
            key: "llm_tools",
            label: "Tools",
            help: "Which vault commands the assistant may run — e.g. read, search, capture. Empty allows every non-render tool.",
            value: cfg.llm_tools.clone().unwrap_or_default().join(", "),
            placeholder: "unset — all but render",
            detail: String::new(),
            choices: Vec::new(),
        },
    ]
}

/// A byte count as a person reads it: `B`, `KB`, `MB`, `GB`.
///
/// 1024 to the step rather than 1000, because every other tool the
/// user has in front of them (`ls -h`, `du -h`) says KB for 1024 and a
/// file manager that disagreed would just look wrong.
#[must_use]
pub fn human_bytes(bytes: u64) -> String {
    const STEP: f64 = 1024.0;
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a display size, not an accounting figure"
    )]
    let mut size = bytes as f64;
    for unit in ["KB", "MB", "GB"] {
        size /= STEP;
        // The last unit takes whatever is left rather than running out
        // and printing a bare number with no unit at all.
        if size < STEP || unit == "GB" {
            return format!("{size:.1} {unit}");
        }
    }
    unreachable!()
}

/// What the shell says after a save.
///
/// "Does the whole inbox.org file gets rewritten if I save a body?"
/// Yes — every mutation writes the whole file from the in-memory
/// document. `body saved` hid exactly that: a note lives in a file
/// with other notes in it, and the message never said which file it
/// had just rewritten, so there was no way to tell where the writing
/// went.
///
/// The size is the *file's*, because the file is what was written.
#[must_use]
pub fn save_report(file: &str, bytes: u64) -> String {
    format!("wrote {file} — {}", human_bytes(bytes))
}
