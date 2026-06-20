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

// ===========================================================================
// Canonical shell keymap — the single source of truth every UI shell
// (TUI, gpui, egui, web) consumes so the five editing modes and their
// which-key listings are identical everywhere (vision: "every mode
// consistently in every UI element"; spec I4). Each mode binds the same
// command set; only the chords differ.
// ===========================================================================

/// Doom/default bindings: SPC-leader friendly, single-key navigation.
const DOOM_KEYMAP: &[(&str, &str)] = &[
    ("j", "next-file"),
    ("k", "prev-file"),
    ("<down>", "next-file"),
    ("<up>", "prev-file"),
    ("g g", "first-file"),
    ("G", "last-file"),
    ("q", "quit"),
    ("ESC", "quit"),
    ("/", "search-start"),
    ("s", "search-headline-start"),
    ("RET", "open-file"),
    ("b", "backlinks"),
    ("c", "capture-start"),
    ("l", "headline-list"),
    ("u", "undo"),
    ("C-r", "redo"),
    ("M", "cycle-mode"),
    (":", "palette"),
    ("v", "db-view"),
    ("e", "block-list"),
    ("g a", "agenda"),
    ("S", "body-search"),
    ("a", "add-sibling"),
    ("r", "rename"),
    ("d", "delete"),
    ("i", "edit-body"),
    ("t", "toggle-todo"),
    ("p", "cycle-priority"),
    ("y", "edit-tags"),
    ("o", "edit-property"),
    ("g r", "toggle-llm-render"),
];

/// Emacs bindings: Ctrl/Meta chords, `C-x C-c` quits.
const EMACS_KEYMAP: &[(&str, &str)] = &[
    ("C-n", "next-file"),
    ("C-p", "prev-file"),
    ("<down>", "next-file"),
    ("<up>", "prev-file"),
    ("M-<", "first-file"),
    ("M->", "last-file"),
    ("C-x C-c", "quit"),
    ("C-s", "search-start"),
    ("C-c s", "search-headline-start"),
    ("RET", "open-file"),
    ("C-c b", "backlinks"),
    ("C-c c", "capture-start"),
    ("C-c l", "headline-list"),
    ("C-x u", "undo"),
    ("C-x r", "redo"),
    ("C-c m", "cycle-mode"),
    (":", "palette"),
    ("v", "db-view"),
    ("e", "block-list"),
    ("g a", "agenda"),
    ("S", "body-search"),
    ("C-c a", "add-sibling"),
    ("C-c r", "rename"),
    ("C-c d", "delete"),
    ("C-c e", "edit-body"),
    ("C-c t", "toggle-todo"),
    ("C-c p", "cycle-priority"),
    ("C-c y", "edit-tags"),
    ("C-c o", "edit-property"),
    ("C-c g", "toggle-llm-render"),
];

/// Vim bindings: modal navigation keys.
const VIM_KEYMAP: &[(&str, &str)] = &[
    ("j", "next-file"),
    ("k", "prev-file"),
    ("<down>", "next-file"),
    ("<up>", "prev-file"),
    ("g g", "first-file"),
    ("G", "last-file"),
    ("Z Z", "quit"),
    ("q", "quit"),
    ("/", "search-start"),
    ("s", "search-headline-start"),
    ("RET", "open-file"),
    ("b", "backlinks"),
    ("c", "capture-start"),
    ("l", "headline-list"),
    ("u", "undo"),
    ("C-r", "redo"),
    ("M", "cycle-mode"),
    (":", "palette"),
    ("v", "db-view"),
    ("e", "block-list"),
    ("g a", "agenda"),
    ("S", "body-search"),
    ("a", "add-sibling"),
    ("r", "rename"),
    ("d", "delete"),
    ("i", "edit-body"),
    ("t", "toggle-todo"),
    ("p", "cycle-priority"),
    ("y", "edit-tags"),
    ("o", "edit-property"),
    ("g r", "toggle-llm-render"),
];

/// Helix bindings: vim-like with `U` redo and `g e` for end.
const HELIX_KEYMAP: &[(&str, &str)] = &[
    ("j", "next-file"),
    ("k", "prev-file"),
    ("<down>", "next-file"),
    ("<up>", "prev-file"),
    ("g g", "first-file"),
    ("g e", "last-file"),
    ("q", "quit"),
    ("ESC", "quit"),
    ("/", "search-start"),
    ("s", "search-headline-start"),
    ("RET", "open-file"),
    ("b", "backlinks"),
    ("c", "capture-start"),
    ("l", "headline-list"),
    ("u", "undo"),
    ("U", "redo"),
    ("M", "cycle-mode"),
    (":", "palette"),
    ("v", "db-view"),
    ("e", "block-list"),
    ("g a", "agenda"),
    ("S", "body-search"),
    ("a", "add-sibling"),
    ("r", "rename"),
    ("d", "delete"),
    ("i", "edit-body"),
    ("t", "toggle-todo"),
    ("p", "cycle-priority"),
    ("y", "edit-tags"),
    ("o", "edit-property"),
    ("g r", "toggle-llm-render"),
];

/// Notion bindings: mouse + arrows + slash command, minimal chords.
const NOTION_KEYMAP: &[(&str, &str)] = &[
    ("<down>", "next-file"),
    ("<up>", "prev-file"),
    ("g g", "first-file"),
    ("G", "last-file"),
    ("ESC", "quit"),
    ("q", "quit"),
    ("/", "palette"),
    ("s", "search-headline-start"),
    ("C-s", "search-start"),
    ("RET", "open-file"),
    ("b", "backlinks"),
    ("c", "capture-start"),
    ("l", "headline-list"),
    ("u", "undo"),
    ("C-r", "redo"),
    ("M", "cycle-mode"),
    (":", "palette"),
    ("v", "db-view"),
    ("e", "block-list"),
    ("g a", "agenda"),
    ("S", "body-search"),
    ("a", "add-sibling"),
    ("r", "rename"),
    ("d", "delete"),
    ("i", "edit-body"),
    ("t", "toggle-todo"),
    ("p", "cycle-priority"),
    ("y", "edit-tags"),
    ("o", "edit-property"),
    ("g r", "toggle-llm-render"),
];

/// The canonical `(chord, command)` keymap for an input mode — the
/// single source of truth shared by every shell. Every mode binds the
/// same command set (I4); only the chords differ.
#[must_use]
pub const fn mode_keymap(mode: InputMode) -> &'static [(&'static str, &'static str)] {
    match mode {
        InputMode::Emacs => EMACS_KEYMAP,
        InputMode::Vim => VIM_KEYMAP,
        InputMode::Doom => DOOM_KEYMAP,
        InputMode::Helix => HELIX_KEYMAP,
        InputMode::Notion => NOTION_KEYMAP,
    }
}

/// Resolve a full chord string (e.g. `"C-x C-c"`) to its command in
/// `mode`, or `None` if unbound.
#[must_use]
pub fn command_for(mode: InputMode, chord: &str) -> Option<&'static str> {
    mode_keymap(mode)
        .iter()
        .find(|(c, _)| *c == chord)
        .map(|(_, cmd)| *cmd)
}

/// Reverse of [`command_for`]: the first chord bound to `command`.
///
/// Returns `None` if the command is unbound in `mode`. Lets a shell
/// render the real keybinding next to a command (vision: every UI
/// element shows its chord) from the keymap source of truth (I4).
#[must_use]
pub fn chord_for_command(mode: InputMode, command: &str) -> Option<&'static str> {
    mode_keymap(mode)
        .iter()
        .find(|(_, cmd)| *cmd == command)
        .map(|(chord, _)| *chord)
}

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

    /// Maximum stroke count across bound chords (`None` when empty).
    #[must_use]
    pub fn max_chord_strokes(&self) -> Option<usize> {
        self.bindings
            .keys()
            .map(|k| k.split_whitespace().count())
            .max()
    }

    /// Minimum stroke count across bound chords (`None` when empty).
    #[must_use]
    pub fn min_chord_strokes(&self) -> Option<usize> {
        self.bindings
            .keys()
            .map(|k| k.split_whitespace().count())
            .min()
    }

    /// Integer mean stroke count across bound chords (`0` when empty).
    #[must_use]
    pub fn mean_chord_strokes(&self) -> usize {
        self.total_chord_strokes()
            .checked_div(self.bindings.len())
            .unwrap_or(0)
    }

    /// Total stroke count summed across every bound chord.
    #[must_use]
    pub fn total_chord_strokes(&self) -> usize {
        self.bindings
            .keys()
            .map(|k| k.split_whitespace().count())
            .sum()
    }

    /// Median chord stroke count (`None` when empty).
    #[must_use]
    pub fn median_chord_strokes(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .bindings
            .keys()
            .map(|k| k.split_whitespace().count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Maximum chord byte length across bound chords (`None` when empty).
    #[must_use]
    pub fn max_chord_byte_len(&self) -> Option<usize> {
        self.bindings.keys().map(String::len).max()
    }

    /// Minimum chord byte length across bound chords (`None` when empty).
    #[must_use]
    pub fn min_chord_byte_len(&self) -> Option<usize> {
        self.bindings.keys().map(String::len).min()
    }

    /// Total chord byte length summed across bound chords.
    #[must_use]
    pub fn total_chord_byte_len(&self) -> usize {
        self.bindings.keys().map(String::len).sum()
    }

    /// Integer mean chord byte length (`0` when empty).
    #[must_use]
    pub fn mean_chord_byte_len(&self) -> usize {
        self.total_chord_byte_len()
            .checked_div(self.bindings.len())
            .unwrap_or(0)
    }

    /// Median chord byte length (`None` when empty).
    #[must_use]
    pub fn median_chord_byte_len(&self) -> Option<usize> {
        let mut v: Vec<usize> = self.bindings.keys().map(String::len).collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of chord byte lengths to occurrence count.
    #[must_use]
    pub fn chord_byte_len_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for k in self.bindings.keys() {
            *m.entry(k.len()).or_insert(0) += 1;
        }
        m
    }

    /// Most common chord byte length (lowest wins ties; `None` when empty).
    #[must_use]
    pub fn mode_chord_byte_len(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (len, c) in self.chord_byte_len_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((len, c));
            }
        }
        best.map(|(len, _)| len)
    }

    /// Maximum chord character length across bound chords (`None` when empty).
    #[must_use]
    pub fn max_chord_char_len(&self) -> Option<usize> {
        self.bindings.keys().map(|k| k.chars().count()).max()
    }

    /// Minimum chord character length across bound chords (`None` when empty).
    #[must_use]
    pub fn min_chord_char_len(&self) -> Option<usize> {
        self.bindings.keys().map(|k| k.chars().count()).min()
    }

    /// Total chord character length summed across bound chords.
    #[must_use]
    pub fn total_chord_char_len(&self) -> usize {
        self.bindings.keys().map(|k| k.chars().count()).sum()
    }

    /// Integer mean chord character length (`0` when empty).
    #[must_use]
    pub fn mean_chord_char_len(&self) -> usize {
        self.total_chord_char_len()
            .checked_div(self.bindings.len())
            .unwrap_or(0)
    }

    /// Median chord character length (`None` when empty).
    #[must_use]
    pub fn median_chord_char_len(&self) -> Option<usize> {
        let mut v: Vec<usize> = self.bindings.keys().map(|k| k.chars().count()).collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of chord character lengths to occurrence count.
    #[must_use]
    pub fn chord_char_len_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for k in self.bindings.keys() {
            *m.entry(k.chars().count()).or_insert(0) += 1;
        }
        m
    }

    /// Most common chord character length (lowest wins ties; `None` when empty).
    #[must_use]
    pub fn mode_chord_char_len(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (len, c) in self.chord_char_len_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((len, c));
            }
        }
        best.map(|(len, _)| len)
    }

    /// Histogram of chord stroke counts to occurrence count.
    #[must_use]
    pub fn chord_stroke_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for k in self.bindings.keys() {
            *m.entry(k.split_whitespace().count()).or_insert(0) += 1;
        }
        m
    }

    /// Max byte length over distinct command names. None if no commands.
    #[must_use]
    pub fn max_command_byte_len(&self) -> Option<usize> {
        self.command_names().iter().map(|n| n.len()).max()
    }

    /// Min byte length over distinct command names. None if no commands.
    #[must_use]
    pub fn min_command_byte_len(&self) -> Option<usize> {
        self.command_names().iter().map(|n| n.len()).min()
    }

    /// Sum of byte lengths over distinct command names.
    #[must_use]
    pub fn total_command_byte_len(&self) -> usize {
        self.command_names().iter().map(|n| n.len()).sum()
    }

    /// Integer mean byte length over distinct command names. 0 when empty.
    #[must_use]
    pub fn mean_command_byte_len(&self) -> usize {
        let names = self.command_names();
        self.total_command_byte_len()
            .checked_div(names.len())
            .unwrap_or(0)
    }

    /// Median byte length over distinct command names. None if empty.
    #[must_use]
    pub fn median_command_byte_len(&self) -> Option<usize> {
        let mut v: Vec<usize> = self.command_names().iter().map(|n| n.len()).collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of command name byte length -> count.
    #[must_use]
    pub fn command_byte_len_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for n in self.command_names() {
            *m.entry(n.len()).or_insert(0) += 1;
        }
        m
    }

    /// Most common command name byte length. None if empty. Lowest wins ties.
    #[must_use]
    pub fn mode_command_byte_len(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (len, c) in self.command_byte_len_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((len, c));
            }
        }
        best.map(|(len, _)| len)
    }

    /// Max char length over distinct command names. None if no commands.
    #[must_use]
    pub fn max_command_char_len(&self) -> Option<usize> {
        self.command_names().iter().map(|n| n.chars().count()).max()
    }

    /// Min char length over distinct command names. None if no commands.
    #[must_use]
    pub fn min_command_char_len(&self) -> Option<usize> {
        self.command_names().iter().map(|n| n.chars().count()).min()
    }

    /// Sum of char lengths over distinct command names.
    #[must_use]
    pub fn total_command_char_len(&self) -> usize {
        self.command_names().iter().map(|n| n.chars().count()).sum()
    }

    /// Integer mean char length over distinct command names. 0 when empty.
    #[must_use]
    pub fn mean_command_char_len(&self) -> usize {
        let names = self.command_names();
        self.total_command_char_len()
            .checked_div(names.len())
            .unwrap_or(0)
    }

    /// Median char length over distinct command names. None if empty.
    #[must_use]
    pub fn median_command_char_len(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .command_names()
            .iter()
            .map(|n| n.chars().count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of command name char length -> count.
    #[must_use]
    pub fn command_char_len_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for n in self.command_names() {
            *m.entry(n.chars().count()).or_insert(0) += 1;
        }
        m
    }

    /// Most common command name char length. None if empty. Lowest wins ties.
    #[must_use]
    pub fn mode_command_char_len(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (len, c) in self.command_char_len_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((len, c));
            }
        }
        best.map(|(len, _)| len)
    }

    /// Count of single-stroke bound chords.
    #[must_use]
    pub fn single_stroke_count(&self) -> usize {
        self.bindings
            .keys()
            .filter(|k| k.split_whitespace().count() == 1)
            .count()
    }

    /// Count of multi-stroke bound chords (more than one stroke).
    #[must_use]
    pub fn multi_stroke_count(&self) -> usize {
        self.bindings
            .keys()
            .filter(|k| k.split_whitespace().count() > 1)
            .count()
    }

    /// Percentage of bound chords that are single-stroke (`0..=100`).
    #[must_use]
    pub fn single_stroke_pct(&self) -> usize {
        (self.single_stroke_count() * 100)
            .checked_div(self.bindings.len())
            .unwrap_or(0)
    }

    /// Percentage of bound chords that are multi-stroke (`0..=100`).
    #[must_use]
    pub fn multi_stroke_pct(&self) -> usize {
        (self.multi_stroke_count() * 100)
            .checked_div(self.bindings.len())
            .unwrap_or(0)
    }

    /// Sorted distinct stroke list across all bound chords.
    #[must_use]
    pub fn distinct_strokes(&self) -> Vec<String> {
        let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for chord in self.bindings.keys() {
            for stroke in chord.split_whitespace() {
                s.insert(stroke.to_owned());
            }
        }
        s.into_iter().collect()
    }

    /// Count of distinct strokes across all bound chords.
    #[must_use]
    pub fn distinct_stroke_count(&self) -> usize {
        self.distinct_strokes().len()
    }

    /// Histogram of stroke usage frequency across bound chords.
    #[must_use]
    pub fn stroke_counts(&self) -> std::collections::BTreeMap<String, usize> {
        let mut m = std::collections::BTreeMap::new();
        for chord in self.bindings.keys() {
            for stroke in chord.split_whitespace() {
                *m.entry(stroke.to_owned()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most frequently used stroke across bound chords (lowest name wins ties).
    #[must_use]
    pub fn most_common_stroke(&self) -> Option<String> {
        let mut best: Option<(String, usize)> = None;
        for (s, c) in self.stroke_counts() {
            if best.as_ref().is_none_or(|(_, bc)| c > *bc) {
                best = Some((s, c));
            }
        }
        best.map(|(s, _)| s)
    }

    /// Max usage frequency of any stroke across bound chords. None if empty.
    #[must_use]
    pub fn max_stroke_freq(&self) -> Option<usize> {
        self.stroke_counts().values().copied().max()
    }

    /// Min usage frequency of any stroke across bound chords. None if empty.
    #[must_use]
    pub fn min_stroke_freq(&self) -> Option<usize> {
        self.stroke_counts().values().copied().min()
    }

    /// Sum of stroke occurrences across bound chords.
    #[must_use]
    pub fn total_stroke_occurrences(&self) -> usize {
        self.stroke_counts().values().sum()
    }

    /// Mean stroke frequency. 0 when empty.
    #[must_use]
    pub fn mean_stroke_freq(&self) -> usize {
        let m = self.stroke_counts();
        self.total_stroke_occurrences()
            .checked_div(m.len())
            .unwrap_or(0)
    }

    /// Median stroke frequency. None when empty.
    #[must_use]
    pub fn median_stroke_freq(&self) -> Option<usize> {
        let mut v: Vec<usize> = self.stroke_counts().values().copied().collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Most common stroke-frequency value. None when empty. Lowest wins ties.
    #[must_use]
    pub fn mode_stroke_freq(&self) -> Option<usize> {
        let mut hist: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
        for f in self.stroke_counts().values() {
            *hist.entry(*f).or_insert(0) += 1;
        }
        let mut best: Option<(usize, usize)> = None;
        for (f, c) in hist {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((f, c));
            }
        }
        best.map(|(f, _)| f)
    }

    /// True iff `chord` is bound to a command.
    #[must_use]
    pub fn has_chord(&self, chord: &str) -> bool {
        self.bindings.contains_key(chord)
    }

    /// Command name bound to `chord`, if any.
    #[must_use]
    pub fn command_for_chord(&self, chord: &str) -> Option<&str> {
        self.bindings.get(chord).map(String::as_str)
    }

    /// Sorted chord strings beginning with `prefix`.
    #[must_use]
    pub fn chords_with_prefix(&self, prefix: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .bindings
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        out.sort();
        out
    }

    /// Sorted distinct command names whose chord begins with `prefix`.
    #[must_use]
    pub fn commands_with_prefix(&self, prefix: &str) -> Vec<String> {
        let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (chord, cmd) in &self.bindings {
            if chord.starts_with(prefix) {
                s.insert(cmd.clone());
            }
        }
        s.into_iter().collect()
    }

    /// Count of chord strings beginning with `prefix`.
    #[must_use]
    pub fn chords_with_prefix_count(&self, prefix: &str) -> usize {
        self.bindings
            .keys()
            .filter(|k| k.starts_with(prefix))
            .count()
    }

    /// Count of distinct command names whose chord begins with `prefix`.
    #[must_use]
    pub fn commands_with_prefix_count(&self, prefix: &str) -> usize {
        self.commands_with_prefix(prefix).len()
    }

    /// True iff any bound chord begins with `prefix`.
    #[must_use]
    pub fn has_prefix(&self, prefix: &str) -> bool {
        self.bindings.keys().any(|k| k.starts_with(prefix))
    }

    /// Number of bound chords (alias of [`Self::binding_count`]).
    #[must_use]
    pub fn chord_count(&self) -> usize {
        self.binding_count()
    }

    /// Maximum chord depth across bound chords. 0 when empty.
    #[must_use]
    pub fn max_depth(&self) -> usize {
        self.max_chord_strokes().unwrap_or(0)
    }

    /// Minimum chord depth across bound chords. 0 when empty.
    #[must_use]
    pub fn min_depth(&self) -> usize {
        self.min_chord_strokes().unwrap_or(0)
    }

    /// Total chord depth across bound chords.
    #[must_use]
    pub fn total_depth(&self) -> usize {
        self.total_chord_strokes()
    }

    /// Integer mean chord depth across bound chords. 0 when empty.
    #[must_use]
    pub fn mean_depth(&self) -> usize {
        self.mean_chord_strokes()
    }

    /// Median chord depth (alias of [`Self::median_chord_strokes`]).
    #[must_use]
    pub fn median_depth(&self) -> Option<usize> {
        self.median_chord_strokes()
    }

    /// Mode chord depth (alias of [`Self::mode_chord_strokes`]).
    #[must_use]
    pub fn mode_depth(&self) -> Option<usize> {
        self.mode_chord_strokes()
    }

    /// Depth histogram (alias of [`Self::chord_stroke_counts`]).
    #[must_use]
    pub fn depth_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        self.chord_stroke_counts()
    }

    /// True iff `command` is bound (alias of [`Self::has_command`]).
    #[must_use]
    pub fn contains_command(&self, command: &str) -> bool {
        self.has_command(command)
    }

    /// True iff `chord` is bound (alias of [`Self::has_chord`]).
    #[must_use]
    pub fn contains_chord(&self, chord: &str) -> bool {
        self.has_chord(chord)
    }

    /// Count of bound chords (alias of [`Self::chord_count`]).
    #[must_use]
    pub fn count_chords(&self) -> usize {
        self.chord_count()
    }

    /// Count of distinct commands (alias of [`Self::command_count`]).
    #[must_use]
    pub fn count_commands(&self) -> usize {
        self.command_count()
    }

    /// Sorted bound chord strings at exactly `depth`.
    #[must_use]
    pub fn chords_at_depth(&self, depth: usize) -> Vec<String> {
        let mut out: Vec<String> = self
            .bindings
            .keys()
            .filter(|k| k.split_whitespace().count() == depth)
            .cloned()
            .collect();
        out.sort();
        out
    }

    /// True iff any bound chord sits at exactly `depth`.
    #[must_use]
    pub fn has_chord_at_depth(&self, depth: usize) -> bool {
        self.bindings
            .keys()
            .any(|k| k.split_whitespace().count() == depth)
    }

    /// Count of bound chords at exactly `depth`.
    #[must_use]
    pub fn chords_at_depth_count(&self, depth: usize) -> usize {
        self.chords_at_depth(depth).len()
    }

    /// True iff any bound command name begins with `prefix`.
    #[must_use]
    pub fn has_command_with_prefix(&self, prefix: &str) -> bool {
        self.bindings.values().any(|c| c.starts_with(prefix))
    }

    /// Sorted distinct command names beginning with `prefix`.
    #[must_use]
    pub fn command_names_with_prefix(&self, prefix: &str) -> Vec<String> {
        let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for cmd in self.bindings.values() {
            if cmd.starts_with(prefix) {
                s.insert(cmd.clone());
            }
        }
        s.into_iter().collect()
    }

    /// Most common chord stroke count (lowest wins ties; `None` when empty).
    #[must_use]
    pub fn mode_chord_strokes(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (sc, c) in self.chord_stroke_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((sc, c));
            }
        }
        best.map(|(sc, _)| sc)
    }

    /// True iff `command` is bound to at least one chord.
    #[must_use]
    pub fn has_command(&self, command: &str) -> bool {
        self.bindings.values().any(|v| v == command)
    }

    /// Map of command name to the number of chords bound to it.
    #[must_use]
    pub fn command_chord_counts(&self) -> std::collections::BTreeMap<String, usize> {
        let mut m = std::collections::BTreeMap::new();
        for cmd in self.bindings.values() {
            *m.entry(cmd.clone()).or_insert(0) += 1;
        }
        m
    }

    /// Max chord count over distinct commands. None if empty.
    #[must_use]
    pub fn max_chords_per_command(&self) -> Option<usize> {
        self.command_chord_counts().values().copied().max()
    }

    /// Min chord count over distinct commands. None if empty.
    #[must_use]
    pub fn min_chords_per_command(&self) -> Option<usize> {
        self.command_chord_counts().values().copied().min()
    }

    /// Sum of chord counts over distinct commands.
    #[must_use]
    pub fn total_chords_per_command(&self) -> usize {
        self.command_chord_counts().values().sum()
    }

    /// Integer mean chord count over distinct commands. 0 when empty.
    #[must_use]
    pub fn mean_chords_per_command(&self) -> usize {
        let m = self.command_chord_counts();
        self.total_chords_per_command()
            .checked_div(m.len())
            .unwrap_or(0)
    }

    /// Median chord count over distinct commands. None when empty.
    #[must_use]
    pub fn median_chords_per_command(&self) -> Option<usize> {
        let mut v: Vec<usize> = self.command_chord_counts().values().copied().collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of chords-per-command counts.
    #[must_use]
    pub fn chords_per_command_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for c in self.command_chord_counts().values() {
            *m.entry(*c).or_insert(0) += 1;
        }
        m
    }

    /// Most common chord-count per command. None when empty. Lowest wins ties.
    #[must_use]
    pub fn mode_chords_per_command(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (cc, c) in self.chords_per_command_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((cc, c));
            }
        }
        best.map(|(cc, _)| cc)
    }

    /// Command bound to the most chords (lowest name wins ties).
    #[must_use]
    pub fn most_bound_command(&self) -> Option<String> {
        let mut best: Option<(String, usize)> = None;
        for (cmd, c) in self.command_chord_counts() {
            if best.as_ref().is_none_or(|(_, bc)| c > *bc) {
                best = Some((cmd, c));
            }
        }
        best.map(|(cmd, _)| cmd)
    }

    /// Longest bound chord by stroke count (sorted-first on ties).
    #[must_use]
    pub fn longest_chord(&self) -> Option<String> {
        let mut keys: Vec<&String> = self.bindings.keys().collect();
        keys.sort();
        let mut best: Option<&String> = None;
        for k in keys {
            if best.is_none_or(|b| k.split_whitespace().count() > b.split_whitespace().count()) {
                best = Some(k);
            }
        }
        best.cloned()
    }

    /// Shortest bound chord by stroke count (sorted-first on ties).
    #[must_use]
    pub fn shortest_chord(&self) -> Option<String> {
        let mut keys: Vec<&String> = self.bindings.keys().collect();
        keys.sort();
        let mut best: Option<&String> = None;
        for k in keys {
            if best.is_none_or(|b| k.split_whitespace().count() < b.split_whitespace().count()) {
                best = Some(k);
            }
        }
        best.cloned()
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

    /// Count of single-stroke bound chords (depth == 1).
    #[must_use]
    pub fn single_stroke_count(&self) -> usize {
        self.chord_depth_counts().get(&1).copied().unwrap_or(0)
    }

    /// Count of multi-stroke bound chords (depth > 1).
    #[must_use]
    pub fn multi_stroke_count(&self) -> usize {
        self.chord_depth_counts()
            .iter()
            .filter(|(d, _)| **d > 1)
            .map(|(_, c)| *c)
            .sum()
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

    /// Sum of depths over every bound chord.
    #[must_use]
    pub fn total_depth(&self) -> usize {
        self.chord_depth_counts().iter().map(|(d, c)| d * c).sum()
    }

    /// Median chord depth (`None` when empty).
    #[must_use]
    pub fn median_depth(&self) -> Option<usize> {
        let mut v: Vec<usize> = Vec::new();
        for (d, c) in self.chord_depth_counts() {
            for _ in 0..c {
                v.push(d);
            }
        }
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
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
            if cur == target
                && let Some(c) = &n.command
            {
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
        self.nodes
            .iter()
            .any(|n| n.command.as_deref() == Some(command))
    }

    /// True iff `chord` is bound in the trie (alias of [`Self::has_chord`]).
    #[must_use]
    pub fn contains_chord(&self, chord: &str) -> bool {
        self.has_chord(chord)
    }

    /// Count of bound chords (alias of [`Self::chord_count`]).
    #[must_use]
    pub fn count_chords(&self) -> usize {
        self.chord_count()
    }

    /// Count of distinct commands (alias of [`Self::all_commands`].len).
    #[must_use]
    pub fn count_commands(&self) -> usize {
        self.all_commands().len()
    }

    /// Number of distinct chords bound (each leaf with a command).
    #[must_use]
    pub fn chord_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.command.is_some()).count()
    }

    /// Number of bound chords (alias of [`Self::chord_count`]).
    #[must_use]
    pub fn binding_count(&self) -> usize {
        self.chord_count()
    }

    /// Maximum chord depth (alias of [`Self::max_depth`]).
    #[must_use]
    pub fn depth(&self) -> usize {
        self.max_depth()
    }

    /// True iff any bound command sits at exactly `depth`.
    #[must_use]
    pub fn has_command_at_depth(&self, depth: usize) -> bool {
        !self.commands_at_depth(depth).is_empty()
    }

    /// Count of bound commands at exactly `depth`.
    #[must_use]
    pub fn commands_at_depth_count(&self, depth: usize) -> usize {
        self.commands_at_depth(depth).len()
    }

    /// Sorted bound chord strings at exactly `depth`.
    #[must_use]
    pub fn chords_at_depth(&self, depth: usize) -> Vec<String> {
        let mut out: Vec<String> = self
            .all_chords()
            .into_iter()
            .filter(|c| c.split_whitespace().count() == depth)
            .collect();
        out.sort();
        out
    }

    /// True iff any bound chord sits at exactly `depth`.
    #[must_use]
    pub fn has_chord_at_depth(&self, depth: usize) -> bool {
        self.all_chords()
            .iter()
            .any(|c| c.split_whitespace().count() == depth)
    }

    /// Count of bound chords at exactly `depth`.
    #[must_use]
    pub fn chords_at_depth_count(&self, depth: usize) -> usize {
        self.chords_at_depth(depth).len()
    }

    /// True iff any trie node sits at exactly `depth`.
    #[must_use]
    pub fn has_node_at_depth(&self, depth: usize) -> bool {
        self.nodes_at_depth_count(depth) > 0
    }

    /// Count of trie nodes at exactly `depth`.
    #[must_use]
    pub fn nodes_at_depth_count(&self, depth: usize) -> usize {
        fn walk(idx: usize, current: usize, target: usize, nodes: &[TrieNode], count: &mut usize) {
            if current == target {
                *count += 1;
                return;
            }
            for &child in nodes[idx].children.values() {
                walk(child, current + 1, target, nodes, count);
            }
        }
        let mut c = 0usize;
        if !self.nodes.is_empty() {
            walk(0, 0, depth, &self.nodes, &mut c);
        }
        c
    }

    /// Total node count in the trie (including the root).
    #[must_use]
    pub const fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Count of nodes that carry no command (root + intermediate prefixes).
    #[must_use]
    pub fn prefix_node_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.command.is_none()).count()
    }

    /// Count of leaf nodes (no children).
    #[must_use]
    pub fn leaf_node_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.children.is_empty()).count()
    }

    /// Count of branch nodes (with at least one child).
    #[must_use]
    pub fn branch_node_count(&self) -> usize {
        self.nodes.iter().filter(|n| !n.children.is_empty()).count()
    }

    /// Count of nodes that carry a command.
    #[must_use]
    pub fn command_node_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.command.is_some()).count()
    }

    /// Percentage of nodes that are leaves (`0..=100`).
    #[must_use]
    pub fn leaf_node_pct(&self) -> usize {
        (self.leaf_node_count() * 100)
            .checked_div(self.node_count())
            .unwrap_or(0)
    }

    /// Percentage of nodes that are branches (`0..=100`).
    #[must_use]
    pub fn branch_node_pct(&self) -> usize {
        (self.branch_node_count() * 100)
            .checked_div(self.node_count())
            .unwrap_or(0)
    }

    /// Percentage of nodes that carry a command (`0..=100`).
    #[must_use]
    pub fn command_node_pct(&self) -> usize {
        (self.command_node_count() * 100)
            .checked_div(self.node_count())
            .unwrap_or(0)
    }

    /// Percentage of nodes that carry no command (`0..=100`).
    #[must_use]
    pub fn prefix_node_pct(&self) -> usize {
        (self.prefix_node_count() * 100)
            .checked_div(self.node_count())
            .unwrap_or(0)
    }

    /// Percentage of bound chords that are single-stroke (`0..=100`).
    #[must_use]
    pub fn single_stroke_pct(&self) -> usize {
        (self.single_stroke_count() * 100)
            .checked_div(self.chord_count())
            .unwrap_or(0)
    }

    /// Percentage of bound chords that are multi-stroke (`0..=100`).
    #[must_use]
    pub fn multi_stroke_pct(&self) -> usize {
        (self.multi_stroke_count() * 100)
            .checked_div(self.chord_count())
            .unwrap_or(0)
    }

    /// Sorted distinct stroke list across all bound chords.
    #[must_use]
    pub fn distinct_strokes(&self) -> Vec<String> {
        let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for c in self.all_chords() {
            for stroke in c.split_whitespace() {
                s.insert(stroke.to_owned());
            }
        }
        s.into_iter().collect()
    }

    /// Count of distinct strokes across all bound chords.
    #[must_use]
    pub fn distinct_stroke_count(&self) -> usize {
        self.distinct_strokes().len()
    }

    /// Histogram of stroke usage frequency across all bound chords.
    #[must_use]
    pub fn stroke_counts(&self) -> std::collections::BTreeMap<String, usize> {
        let mut m = std::collections::BTreeMap::new();
        for c in self.all_chords() {
            for stroke in c.split_whitespace() {
                *m.entry(stroke.to_owned()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Most frequently used stroke across all bound chords (lowest name wins ties).
    #[must_use]
    pub fn most_common_stroke(&self) -> Option<String> {
        let mut best: Option<(String, usize)> = None;
        for (s, c) in self.stroke_counts() {
            if best.as_ref().is_none_or(|(_, bc)| c > *bc) {
                best = Some((s, c));
            }
        }
        best.map(|(s, _)| s)
    }

    /// Max usage frequency of any stroke. None if empty.
    #[must_use]
    pub fn max_stroke_freq(&self) -> Option<usize> {
        self.stroke_counts().values().copied().max()
    }

    /// Min usage frequency of any stroke. None if empty.
    #[must_use]
    pub fn min_stroke_freq(&self) -> Option<usize> {
        self.stroke_counts().values().copied().min()
    }

    /// Sum of stroke occurrences across all bound chords.
    #[must_use]
    pub fn total_stroke_occurrences(&self) -> usize {
        self.stroke_counts().values().sum()
    }

    /// Mean stroke frequency. 0 when empty.
    #[must_use]
    pub fn mean_stroke_freq(&self) -> usize {
        let m = self.stroke_counts();
        self.total_stroke_occurrences()
            .checked_div(m.len())
            .unwrap_or(0)
    }

    /// Median stroke frequency. None when empty.
    #[must_use]
    pub fn median_stroke_freq(&self) -> Option<usize> {
        let mut v: Vec<usize> = self.stroke_counts().values().copied().collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Most common stroke-frequency value. None when empty. Lowest wins ties.
    #[must_use]
    pub fn mode_stroke_freq(&self) -> Option<usize> {
        let mut hist: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
        for f in self.stroke_counts().values() {
            *hist.entry(*f).or_insert(0) += 1;
        }
        let mut best: Option<(usize, usize)> = None;
        for (f, c) in hist {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((f, c));
            }
        }
        best.map(|(f, _)| f)
    }

    /// True iff `command` is bound somewhere in the trie.
    #[must_use]
    pub fn has_command(&self, command: &str) -> bool {
        self.all_commands().contains(&command)
    }

    /// True iff `chord` is bound to a command in the trie.
    #[must_use]
    pub fn has_chord(&self, chord: &str) -> bool {
        self.all_chords().iter().any(|c| c == chord)
    }

    /// Command name bound to `chord` in the trie, if any.
    #[must_use]
    pub fn command_for_chord(&self, chord: &str) -> Option<String> {
        for (c, cmd) in self.bindings() {
            if c == chord {
                return Some(cmd);
            }
        }
        None
    }

    /// Sorted distinct chord strings bound to `command`.
    #[must_use]
    pub fn chords_for_command(&self, command: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .bindings()
            .into_iter()
            .filter(|(_, c)| c == command)
            .map(|(ch, _)| ch)
            .collect();
        out.sort();
        out
    }

    /// Sorted chord strings beginning with `prefix`.
    #[must_use]
    pub fn chords_with_prefix(&self, prefix: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .all_chords()
            .into_iter()
            .filter(|c| c.starts_with(prefix))
            .collect();
        out.sort();
        out
    }

    /// Sorted distinct command names whose chord begins with `prefix`.
    #[must_use]
    pub fn commands_with_prefix(&self, prefix: &str) -> Vec<String> {
        let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (chord, cmd) in self.bindings() {
            if chord.starts_with(prefix) {
                s.insert(cmd);
            }
        }
        s.into_iter().collect()
    }

    /// Count of chord strings beginning with `prefix`.
    #[must_use]
    pub fn chords_with_prefix_count(&self, prefix: &str) -> usize {
        self.chords_with_prefix(prefix).len()
    }

    /// Count of distinct command names whose chord begins with `prefix`.
    #[must_use]
    pub fn commands_with_prefix_count(&self, prefix: &str) -> usize {
        self.commands_with_prefix(prefix).len()
    }

    /// True iff any bound chord begins with `prefix`.
    #[must_use]
    pub fn has_prefix(&self, prefix: &str) -> bool {
        self.all_chords().iter().any(|c| c.starts_with(prefix))
    }

    /// True iff any bound command name begins with `prefix`.
    #[must_use]
    pub fn has_command_with_prefix(&self, prefix: &str) -> bool {
        self.all_commands().iter().any(|c| c.starts_with(prefix))
    }

    /// Sorted distinct command names beginning with `prefix`.
    #[must_use]
    pub fn command_names_with_prefix(&self, prefix: &str) -> Vec<String> {
        let mut s: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for c in self.all_commands() {
            if c.starts_with(prefix) {
                s.insert(c.to_owned());
            }
        }
        s.into_iter().collect()
    }

    /// Maximum stroke count across bound chords. None if empty.
    #[must_use]
    pub fn max_chord_strokes(&self) -> Option<usize> {
        self.all_chords()
            .iter()
            .map(|c| c.split_whitespace().count())
            .max()
    }

    /// Minimum stroke count across bound chords. None if empty.
    #[must_use]
    pub fn min_chord_strokes(&self) -> Option<usize> {
        self.all_chords()
            .iter()
            .map(|c| c.split_whitespace().count())
            .min()
    }

    /// Sum of stroke counts across bound chords.
    #[must_use]
    pub fn total_chord_strokes(&self) -> usize {
        self.all_chords()
            .iter()
            .map(|c| c.split_whitespace().count())
            .sum()
    }

    /// Integer mean stroke count across bound chords. 0 when empty.
    #[must_use]
    pub fn mean_chord_strokes(&self) -> usize {
        let chords = self.all_chords();
        self.total_chord_strokes()
            .checked_div(chords.len())
            .unwrap_or(0)
    }

    /// Median chord stroke count. None when empty.
    #[must_use]
    pub fn median_chord_strokes(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .all_chords()
            .iter()
            .map(|c| c.split_whitespace().count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of chord stroke counts.
    #[must_use]
    pub fn chord_stroke_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for c in self.all_chords() {
            *m.entry(c.split_whitespace().count()).or_insert(0) += 1;
        }
        m
    }

    /// Most common chord stroke count. None when empty. Lowest wins ties.
    #[must_use]
    pub fn mode_chord_strokes(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (sc, c) in self.chord_stroke_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((sc, c));
            }
        }
        best.map(|(sc, _)| sc)
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
        fn walk(idx: usize, prefix: &mut Vec<String>, nodes: &[TrieNode], out: &mut Vec<String>) {
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

    /// Map of command name to the number of chords bound to it.
    #[must_use]
    pub fn command_chord_counts(&self) -> std::collections::BTreeMap<String, usize> {
        let mut m = std::collections::BTreeMap::new();
        for (_, cmd) in self.bindings() {
            *m.entry(cmd).or_insert(0) += 1;
        }
        m
    }

    /// Command bound to the most chords (lowest name wins ties).
    #[must_use]
    pub fn most_bound_command(&self) -> Option<String> {
        let mut best: Option<(String, usize)> = None;
        for (cmd, c) in self.command_chord_counts() {
            if best.as_ref().is_none_or(|(_, bc)| c > *bc) {
                best = Some((cmd, c));
            }
        }
        best.map(|(cmd, _)| cmd)
    }

    /// Longest bound chord by stroke count (sorted-first on ties).
    #[must_use]
    pub fn longest_chord(&self) -> Option<String> {
        let mut best: Option<String> = None;
        for c in self.all_chords() {
            let take = best
                .as_ref()
                .is_none_or(|b| c.split_whitespace().count() > b.split_whitespace().count());
            if take {
                best = Some(c);
            }
        }
        best
    }

    /// Shortest bound chord by stroke count (sorted-first on ties).
    #[must_use]
    pub fn shortest_chord(&self) -> Option<String> {
        let mut best: Option<String> = None;
        for c in self.all_chords() {
            let take = best
                .as_ref()
                .is_none_or(|b| c.split_whitespace().count() < b.split_whitespace().count());
            if take {
                best = Some(c);
            }
        }
        best
    }

    /// Max chord byte length over bound chords. None if empty.
    #[must_use]
    pub fn max_chord_byte_len(&self) -> Option<usize> {
        self.all_chords().iter().map(String::len).max()
    }

    /// Min chord byte length over bound chords. None if empty.
    #[must_use]
    pub fn min_chord_byte_len(&self) -> Option<usize> {
        self.all_chords().iter().map(String::len).min()
    }

    /// Sum of chord byte lengths over bound chords.
    #[must_use]
    pub fn total_chord_byte_len(&self) -> usize {
        self.all_chords().iter().map(String::len).sum()
    }

    /// Mean chord byte length (integer division). 0 if empty.
    #[must_use]
    pub fn mean_chord_byte_len(&self) -> usize {
        let chords = self.all_chords();
        self.total_chord_byte_len()
            .checked_div(chords.len())
            .unwrap_or(0)
    }

    /// Median chord byte length. None if empty. Even count takes midpoint.
    #[must_use]
    pub fn median_chord_byte_len(&self) -> Option<usize> {
        let mut v: Vec<usize> = self.all_chords().iter().map(String::len).collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of chord byte length -> count.
    #[must_use]
    pub fn chord_byte_len_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for c in self.all_chords() {
            *m.entry(c.len()).or_insert(0) += 1;
        }
        m
    }

    /// Most common chord byte length. None if empty. Lowest wins ties.
    #[must_use]
    pub fn mode_chord_byte_len(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (len, c) in self.chord_byte_len_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((len, c));
            }
        }
        best.map(|(len, _)| len)
    }

    /// Max chord char length over bound chords. None if empty.
    #[must_use]
    pub fn max_chord_char_len(&self) -> Option<usize> {
        self.all_chords().iter().map(|c| c.chars().count()).max()
    }

    /// Min chord char length over bound chords. None if empty.
    #[must_use]
    pub fn min_chord_char_len(&self) -> Option<usize> {
        self.all_chords().iter().map(|c| c.chars().count()).min()
    }

    /// Sum of chord char lengths over bound chords.
    #[must_use]
    pub fn total_chord_char_len(&self) -> usize {
        self.all_chords().iter().map(|c| c.chars().count()).sum()
    }

    /// Mean chord char length (integer division). 0 if empty.
    #[must_use]
    pub fn mean_chord_char_len(&self) -> usize {
        let chords = self.all_chords();
        self.total_chord_char_len()
            .checked_div(chords.len())
            .unwrap_or(0)
    }

    /// Max byte length over distinct command names. None if empty.
    #[must_use]
    pub fn max_command_byte_len(&self) -> Option<usize> {
        self.all_commands().iter().map(|c| c.len()).max()
    }

    /// Min byte length over distinct command names. None if empty.
    #[must_use]
    pub fn min_command_byte_len(&self) -> Option<usize> {
        self.all_commands().iter().map(|c| c.len()).min()
    }

    /// Sum of byte lengths over distinct command names.
    #[must_use]
    pub fn total_command_byte_len(&self) -> usize {
        self.all_commands().iter().map(|c| c.len()).sum()
    }

    /// Integer mean byte length over distinct command names. 0 when empty.
    #[must_use]
    pub fn mean_command_byte_len(&self) -> usize {
        let cmds = self.all_commands();
        self.total_command_byte_len()
            .checked_div(cmds.len())
            .unwrap_or(0)
    }

    /// Median byte length over distinct command names. None if empty.
    #[must_use]
    pub fn median_command_byte_len(&self) -> Option<usize> {
        let mut v: Vec<usize> = self.all_commands().iter().map(|c| c.len()).collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of command name byte length -> count.
    #[must_use]
    pub fn command_byte_len_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for c in self.all_commands() {
            *m.entry(c.len()).or_insert(0) += 1;
        }
        m
    }

    /// Most common command name byte length. None if empty. Lowest wins ties.
    #[must_use]
    pub fn mode_command_byte_len(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (len, c) in self.command_byte_len_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((len, c));
            }
        }
        best.map(|(len, _)| len)
    }

    /// Max char length over distinct command names. None if empty.
    #[must_use]
    pub fn max_command_char_len(&self) -> Option<usize> {
        self.all_commands().iter().map(|c| c.chars().count()).max()
    }

    /// Min char length over distinct command names. None if empty.
    #[must_use]
    pub fn min_command_char_len(&self) -> Option<usize> {
        self.all_commands().iter().map(|c| c.chars().count()).min()
    }

    /// Sum of char lengths over distinct command names.
    #[must_use]
    pub fn total_command_char_len(&self) -> usize {
        self.all_commands().iter().map(|c| c.chars().count()).sum()
    }

    /// Integer mean char length over distinct command names. 0 when empty.
    #[must_use]
    pub fn mean_command_char_len(&self) -> usize {
        let cmds = self.all_commands();
        self.total_command_char_len()
            .checked_div(cmds.len())
            .unwrap_or(0)
    }

    /// Median char length over distinct command names. None if empty.
    #[must_use]
    pub fn median_command_char_len(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .all_commands()
            .iter()
            .map(|c| c.chars().count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of command name char length -> count.
    #[must_use]
    pub fn command_char_len_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for c in self.all_commands() {
            *m.entry(c.chars().count()).or_insert(0) += 1;
        }
        m
    }

    /// Most common command name char length. None if empty. Lowest wins ties.
    #[must_use]
    pub fn mode_command_char_len(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (len, c) in self.command_char_len_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((len, c));
            }
        }
        best.map(|(len, _)| len)
    }

    /// Median chord char length. None if empty. Even count takes midpoint.
    #[must_use]
    pub fn median_chord_char_len(&self) -> Option<usize> {
        let mut v: Vec<usize> = self
            .all_chords()
            .iter()
            .map(|c| c.chars().count())
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of chord char length -> count.
    #[must_use]
    pub fn chord_char_len_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for c in self.all_chords() {
            *m.entry(c.chars().count()).or_insert(0) += 1;
        }
        m
    }

    /// Most common chord char length. None if empty. Lowest wins ties.
    #[must_use]
    pub fn mode_chord_char_len(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (len, c) in self.chord_char_len_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((len, c));
            }
        }
        best.map(|(len, _)| len)
    }

    /// Max chord count over distinct commands. None if empty.
    #[must_use]
    pub fn max_chords_per_command(&self) -> Option<usize> {
        self.command_chord_counts().values().copied().max()
    }

    /// Min chord count over distinct commands. None if empty.
    #[must_use]
    pub fn min_chords_per_command(&self) -> Option<usize> {
        self.command_chord_counts().values().copied().min()
    }

    /// Sum of chord counts over distinct commands.
    #[must_use]
    pub fn total_chords_per_command(&self) -> usize {
        self.command_chord_counts().values().sum()
    }

    /// Integer mean chord count over distinct commands. 0 when empty.
    #[must_use]
    pub fn mean_chords_per_command(&self) -> usize {
        let m = self.command_chord_counts();
        self.total_chords_per_command()
            .checked_div(m.len())
            .unwrap_or(0)
    }

    /// Median chord count over distinct commands. None when empty.
    #[must_use]
    pub fn median_chords_per_command(&self) -> Option<usize> {
        let mut v: Vec<usize> = self.command_chord_counts().values().copied().collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 1 {
            v[mid]
        } else {
            v[mid - 1].midpoint(v[mid])
        })
    }

    /// Histogram of chords-per-command counts.
    #[must_use]
    pub fn chords_per_command_counts(&self) -> std::collections::BTreeMap<usize, usize> {
        let mut m = std::collections::BTreeMap::new();
        for c in self.command_chord_counts().values() {
            *m.entry(*c).or_insert(0) += 1;
        }
        m
    }

    /// Most common chord-count per command. None when empty. Lowest wins ties.
    #[must_use]
    pub fn mode_chords_per_command(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (cc, c) in self.chords_per_command_counts() {
            if best.is_none_or(|(_, bc)| c > bc) {
                best = Some((cc, c));
            }
        }
        best.map(|(cc, _)| cc)
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

/// True iff `s` parses as a chord under [`parse_chord`] auto-detection.
#[must_use]
pub fn is_valid_chord(s: &str) -> bool {
    parse_chord(s).is_ok()
}

/// True iff `s` uses vim bracket syntax (`<...>` strokes), matching
/// the auto-detection rule [`parse_chord`] applies.
#[must_use]
pub fn is_vim_syntax(s: &str) -> bool {
    s.contains('<')
}

/// True iff `s` uses Emacs syntax (no `<` bracket).
#[must_use]
pub fn is_emacs_syntax(s: &str) -> bool {
    !s.contains('<')
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
