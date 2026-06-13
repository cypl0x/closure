//! Tauri shell for closure.
//!
//! Wraps the self-contained web shell (or localhost server) in a native Tauri
//! app (menu, window, FS bridges). Contributes to the capability matrix and
//! multi-UI vision (type-level venn + platform specifics).
//!
//! Status: stub for matrix/ROADMAP (Tauri entry in cli shells); full app
//! (src-tauri, tauri.conf, invoke handlers for vault ops) to be bodied in
//! follow-up TDD cycle. Reuses closure-shell-web where possible (I7).

#![forbid(unsafe_code)]

// Placeholder: in full, pub use or a TauriShell that hosts the webview
// and forwards commands through the kernel (via the web respond or direct
// if embedded). For now the crate exists so workspace + `cargo check` +
// the type-level matrix treat Tauri as a first-class shell variant.

/// Marker for the capability matrix and future integration.
pub const TAURI_SHELL: &str = "tauri";
