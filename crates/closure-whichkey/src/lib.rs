//! Auto-generated which-key popup content.
//!
//! Invariant I4: every command carries its own keybinding in the
//! registry, and the which-key popup is rendered directly from that
//! data — there is no hand-maintained keybinding table.

#![forbid(unsafe_code)]

use std::fmt::Write as _;

use closure_input::Dispatcher;

/// Render a which-key style listing: one `chord → command` line per
/// binding, sorted by chord.
#[must_use]
pub fn render(dispatcher: &Dispatcher) -> String {
    let mut out = String::new();
    for (chord, cmd) in dispatcher.bindings() {
        let _ = writeln!(out, "{chord:20} → {cmd}");
    }
    out
}

/// Only the bindings whose chord starts with `prefix` (e.g. `SPC f`
/// to list file-related commands under the doom leader).
#[must_use]
pub fn render_prefix(dispatcher: &Dispatcher, prefix: &str) -> String {
    let mut out = String::new();
    for (chord, cmd) in dispatcher.bindings() {
        if chord.starts_with(prefix) {
            let _ = writeln!(out, "{chord:20} → {cmd}");
        }
    }
    out
}
