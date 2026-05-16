//! Keybinding input modes.
//!
//! An input mode is a stateful keybinding trie over the single command
//! registry (spec invariant I4). Each mode translates user key strokes
//! into command names; the registry then executes the command.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use closure_config::InputMode;
use closure_core::{KeyChord, Registry};
use thiserror::Error;

/// Active modal state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeState {
    /// Accepting ordinary edits.
    Normal,
    /// Insert-mode (vim/doom).
    Insert,
    /// Leader-prefix pressed (doom/helix/vim).
    Leader,
}

/// A dispatcher that maps key chords to registered command names for a
/// given [`InputMode`].
pub struct Dispatcher {
    mode: InputMode,
    state: ModeState,
    bindings: HashMap<String, String>,
}

impl Dispatcher {
    /// Build a dispatcher for `mode` that maps every chord declared by
    /// a registered command to that command's name. When the same
    /// chord is claimed by multiple commands the last one registered
    /// wins.
    #[must_use]
    pub fn from_registry(registry: &Registry, mode: InputMode) -> Self {
        let mut bindings: HashMap<String, String> = HashMap::new();
        for (name, cmd) in registry.entries() {
            for chord in cmd.keys() {
                bindings.insert(chord.to_string(), name.to_owned());
            }
        }
        Self {
            mode,
            state: ModeState::Normal,
            bindings,
        }
    }

    /// Currently-active mode.
    #[must_use]
    pub const fn mode(&self) -> InputMode {
        self.mode
    }

    /// Current modal state.
    #[must_use]
    pub const fn state(&self) -> ModeState {
        self.state
    }

    /// Transition modal state.
    pub const fn set_state(&mut self, s: ModeState) {
        self.state = s;
    }

    /// Lookup the command name bound to a chord, if any.
    #[must_use]
    pub fn resolve(&self, chord: &KeyChord) -> Option<&str> {
        self.bindings.get(&chord.to_string()).map(String::as_str)
    }

    /// All bindings as `(chord, command)` pairs, sorted by chord.
    #[must_use]
    pub fn bindings(&self) -> Vec<(&str, &str)> {
        let mut v: Vec<(&str, &str)> = self
            .bindings
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        v.sort_by_key(|&(k, _)| k);
        v
    }

    /// Number of registered chord bindings.
    #[must_use]
    pub fn binding_count(&self) -> usize {
        self.bindings.len()
    }

    /// True iff the dispatcher has zero registered chord bindings.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// True iff `chord` is bound to a command.
    #[must_use]
    pub fn is_bound(&self, chord: &KeyChord) -> bool {
        self.bindings.contains_key(&chord.to_string())
    }

    /// Distinct command names reachable through this dispatcher, sorted.
    #[must_use]
    pub fn command_names(&self) -> Vec<&str> {
        let mut s: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for v in self.bindings.values() {
            s.insert(v.as_str());
        }
        s.into_iter().collect()
    }

    /// Sorted chord strings bound to `command`.
    #[must_use]
    pub fn chords_for_command(&self, command: &str) -> Vec<&str> {
        let mut v: Vec<&str> = self
            .bindings
            .iter()
            .filter(|(_, c)| c.as_str() == command)
            .map(|(k, _)| k.as_str())
            .collect();
        v.sort_unstable();
        v
    }

    /// Number of distinct commands bound in this dispatcher.
    #[must_use]
    pub fn command_count(&self) -> usize {
        self.command_names().len()
    }
}

/// Input-mode errors.
#[derive(Debug, Error)]
pub enum InputError {
    /// A chord was pressed that has no bound command.
    #[error("unbound chord: {0}")]
    Unbound(String),
    /// Vim-style chord notation could not be parsed.
    #[error("invalid chord notation: {0}")]
    BadChord(String),
}

/// Outcome of feeding one stroke into a [`ChordTrie`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrieStep {
    /// The stroke completed a full chord; carries the bound command
    /// name and resets the input state.
    Resolved(String),
    /// The stroke is a valid prefix; the vector lists the stroke
    /// alternatives that can extend it (sorted).
    Pending(Vec<String>),
    /// The stroke is unbound; input state resets.
    Unbound,
}

/// A chord-prefix trie. Build it once from a flat list of
/// `(chord, command-name)` pairs; feed strokes one at a time via
/// [`Self::step`].
#[derive(Debug, Default, Clone)]
pub struct ChordTrie {
    nodes: Vec<TrieNode>,
    cursor: usize,
}

#[derive(Debug, Default, Clone)]
struct TrieNode {
    children: HashMap<String, usize>,
    command: Option<String>,
}

impl ChordTrie {
    /// Build a trie from `(chord, command)` pairs.
    #[must_use]
    pub fn build(bindings: &[(&str, &str)]) -> Self {
        let mut nodes: Vec<TrieNode> = vec![TrieNode::default()];
        for (chord, cmd) in bindings {
            let mut idx = 0usize;
            for stroke in chord.split_whitespace() {
                idx = nodes[idx].children.get(stroke).copied().unwrap_or_else(|| {
                    let new_idx = nodes.len();
                    nodes.push(TrieNode::default());
                    nodes[idx].children.insert(stroke.to_owned(), new_idx);
                    new_idx
                });
            }
            nodes[idx].command = Some((*cmd).to_owned());
        }
        Self { nodes, cursor: 0 }
    }

    /// Reset the cursor to the root.
    pub const fn reset(&mut self) {
        self.cursor = 0;
    }

    /// True iff the trie has no bound chords.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.iter().all(|n| n.command.is_none())
    }

    /// Number of distinct commands bound in the trie.
    #[must_use]
    pub fn command_count(&self) -> usize {
        let mut s: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for n in &self.nodes {
            if let Some(c) = &n.command {
                s.insert(c.as_str());
            }
        }
        s.len()
    }

    /// True iff the cursor is at the root (no in-progress chord).
    #[must_use]
    pub const fn is_at_root(&self) -> bool {
        self.cursor == 0
    }

    /// Sorted distinct command names bound in the trie.
    #[must_use]
    pub fn all_commands(&self) -> Vec<&str> {
        let mut s: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for n in &self.nodes {
            if let Some(c) = &n.command {
                s.insert(c.as_str());
            }
        }
        s.into_iter().collect()
    }

    /// Maximum chord depth (number of strokes in the longest bound chord).
    #[must_use]
    pub fn max_depth(&self) -> usize {
        fn walk(idx: usize, depth: usize, nodes: &[TrieNode], best: &mut usize) {
            let n = &nodes[idx];
            if n.command.is_some() {
                *best = (*best).max(depth);
            }
            for &child in n.children.values() {
                walk(child, depth + 1, nodes, best);
            }
        }
        let mut best = 0usize;
        walk(0, 0, &self.nodes, &mut best);
        best
    }

    /// Minimum chord depth (strokes in the shortest bound chord; `0` when empty).
    #[must_use]
    pub fn min_depth(&self) -> usize {
        fn walk(idx: usize, depth: usize, nodes: &[TrieNode], best: &mut Option<usize>) {
            let n = &nodes[idx];
            if n.command.is_some() {
                *best = Some(best.map_or(depth, |b: usize| b.min(depth)));
            }
            for &child in n.children.values() {
                walk(child, depth + 1, nodes, best);
            }
        }
        let mut best = None;
        walk(0, 0, &self.nodes, &mut best);
        best.unwrap_or(0)
    }

    /// Histogram of chord depths (strokes) to occurrence count.
    #[must_use]
    pub fn chord_depth_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        fn walk(
            idx: usize,
            depth: usize,
            nodes: &[TrieNode],
            m: &mut std::collections::BTreeMap<usize, usize>,
        ) {
            let n = &nodes[idx];
            if n.command.is_some() {
                *m.entry(depth).or_insert(0) += 1;
            }
            for &child in n.children.values() {
                walk(child, depth + 1, nodes, m);
            }
        }
        let mut m = std::collections::BTreeMap::new();
        walk(0, 0, &self.nodes, &mut m);
        m
    }

    /// Integer mean chord depth (strokes per bound chord; `0` when empty).
    #[must_use]
    pub fn mean_depth(&self) -> usize {
        fn walk(idx: usize, depth: usize, nodes: &[TrieNode], total: &mut usize, n: &mut usize) {
            let node = &nodes[idx];
            if node.command.is_some() {
                *total += depth;
                *n += 1;
            }
            for &child in node.children.values() {
                walk(child, depth + 1, nodes, total, n);
            }
        }
        let mut total = 0usize;
        let mut n = 0usize;
        walk(0, 0, &self.nodes, &mut total, &mut n);
        total.checked_div(n).unwrap_or(0)
    }

    /// Most common chord depth (lowest depth wins ties; `None` when empty).
    #[must_use]
    pub fn mode_depth(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (depth, c) in self.chord_depth_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((depth, c));
            }
        }
        best.map(|(depth, _)| depth)
    }

    /// Sorted command names bound at exactly `depth` strokes.
    #[must_use]
    pub fn commands_at_depth(&self, depth: usize) -> Vec<&str> {
        fn walk<'a>(
            idx: usize,
            cur: usize,
            target: usize,
            nodes: &'a [TrieNode],
            out: &mut Vec<&'a str>,
        ) {
            let n = &nodes[idx];
            if cur == target && let Some(c) = &n.command {
                out.push(c.as_str());
            }
            for &child in n.children.values() {
                walk(child, cur + 1, target, nodes, out);
            }
        }
        let mut out = Vec::new();
        walk(0, 0, depth, &self.nodes, &mut out);
        out.sort_unstable();
        out
    }

    /// True iff `command` is bound somewhere in the trie.
    #[must_use]
    pub fn contains_command(&self, command: &str) -> bool {
        self.nodes.iter().any(|n| n.command.as_deref() == Some(command))
    }

    /// Number of distinct chords bound (each leaf with a command).
    #[must_use]
    pub fn chord_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.command.is_some()).count()
    }

    /// Total node count in the trie (including the root).
    #[must_use]
    pub const fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// All bindings as `(chord, command)` pairs, sorted by chord.
    #[must_use]
    pub fn bindings(&self) -> Vec<(String, String)> {
        fn walk(
            idx: usize,
            prefix: &mut Vec<String>,
            nodes: &[TrieNode],
            out: &mut Vec<(String, String)>,
        ) {
            let n = &nodes[idx];
            if let Some(c) = &n.command {
                out.push((prefix.join(" "), c.clone()));
            }
            for (stroke, &child) in &n.children {
                prefix.push(stroke.clone());
                walk(child, prefix, nodes, out);
                prefix.pop();
            }
        }
        let mut out: Vec<(String, String)> = Vec::new();
        walk(0, &mut Vec::new(), &self.nodes, &mut out);
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Sorted distinct chord strings bound in the trie. Strokes joined by spaces.
    #[must_use]
    pub fn all_chords(&self) -> Vec<String> {
        fn walk(
            idx: usize,
            prefix: &mut Vec<String>,
            nodes: &[TrieNode],
            out: &mut Vec<String>,
        ) {
            let n = &nodes[idx];
            if n.command.is_some() {
                out.push(prefix.join(" "));
            }
            for (stroke, &child) in &n.children {
                prefix.push(stroke.clone());
                walk(child, prefix, nodes, out);
                prefix.pop();
            }
        }
        let mut out: Vec<String> = Vec::new();
        walk(0, &mut Vec::new(), &self.nodes, &mut out);
        out.sort();
        out
    }

    /// Feed one stroke. [`TrieStep::Resolved`] and
    /// [`TrieStep::Unbound`] both reset the cursor.
    pub fn step(&mut self, stroke: &str) -> TrieStep {
        let here = &self.nodes[self.cursor];
        let Some(&next) = here.children.get(stroke) else {
            self.cursor = 0;
            return TrieStep::Unbound;
        };
        self.cursor = next;
        let here = &self.nodes[self.cursor];
        if let Some(cmd) = &here.command {
            let cmd = cmd.clone();
            self.cursor = 0;
            return TrieStep::Resolved(cmd);
        }
        let mut next_strokes: Vec<String> = here.children.keys().cloned().collect();
        next_strokes.sort();
        TrieStep::Pending(next_strokes)
    }
}

/// Parse vim-style chord notation `<leader>ff`, `<C-c>`, `<Esc>`,
/// `<SPC>` into a [`KeyChord`] of its constituent strokes.
///
/// Examples:
///
/// - `"<leader>ff"`     → `["<leader>", "f", "f"]`
/// - `"<C-c><C-x>r"`    → `["<C-c>", "<C-x>", "r"]`
/// - `"abc"`            → `["a", "b", "c"]`
#[allow(clippy::missing_errors_doc)]
pub fn parse_vim_chord(s: &str) -> Result<KeyChord, InputError> {
    let mut strokes: Vec<String> = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let Some(close) = s[i..].find('>') else {
                return Err(InputError::BadChord(s.to_owned()));
            };
            strokes.push(s[i..=i + close].to_owned());
            i += close + 1;
        } else {
            // Skip ASCII whitespace silently.
            if bytes[i] == b' ' || bytes[i] == b'\t' {
                i += 1;
                continue;
            }
            // Single ASCII / UTF-8 char.
            let ch = s[i..].chars().next().unwrap_or('\0');
            strokes.push(ch.to_string());
            i += ch.len_utf8();
        }
    }
    if strokes.is_empty() {
        return Err(InputError::BadChord(s.to_owned()));
    }
    let refs: Vec<&str> = strokes.iter().map(String::as_str).collect();
    Ok(KeyChord::from_strokes(&refs))
}

/// Auto-detect chord syntax: if `s` contains `<`, route through
/// [`parse_vim_chord`]; otherwise [`parse_emacs_chord`]. Lets callers
/// accept both `C-c C-x r` and `<C-c><C-x>r` from the same option.
#[allow(clippy::missing_errors_doc)]
pub fn parse_chord(s: &str) -> Result<KeyChord, InputError> {
    if s.contains('<') {
        parse_vim_chord(s)
    } else {
        parse_emacs_chord(s)
    }
}

/// Parse an Emacs-style chord like `C-c C-x r` (whitespace separated,
/// no brackets). Returns a [`KeyChord`] where each stroke is one of
/// `C-x`, `M-x`, `S-x`, or a bare key. Empty input is rejected.
#[allow(clippy::missing_errors_doc)]
pub fn parse_emacs_chord(s: &str) -> Result<KeyChord, InputError> {
    let strokes: Vec<&str> = s.split_whitespace().collect();
    if strokes.is_empty() {
        return Err(InputError::BadChord(s.to_owned()));
    }
    Ok(KeyChord::from_strokes(&strokes))
}
