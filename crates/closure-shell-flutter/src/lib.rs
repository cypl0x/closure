//! Flutter embedder for closure.
//!
//! Cross-platform (iOS/Android/desktop/web) via Flutter engine + platform channels
//! or FFI to the kernel (I7). Per vision "suggestion-tier"; evaluate vs Tauri/egui.
//! Stub for matrix/ROADMAP; body would provide Dart FFI + embed views over
//! the pure respond/export or direct Vault.

#![forbid(unsafe_code)]

/// Marker.
pub const FLUTTER_SHELL: &str = "flutter";
