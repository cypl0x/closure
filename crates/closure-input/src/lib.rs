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
