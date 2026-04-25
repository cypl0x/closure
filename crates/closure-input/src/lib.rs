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

/// Parse vim-style chord notation `<leader>ff`, `<C-c>`, `<Esc>`,
/// `<SPC>` into a [`KeyChord`] of its constituent strokes.
///
/// Examples:
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
