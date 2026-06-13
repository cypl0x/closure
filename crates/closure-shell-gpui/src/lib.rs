//! gpui shell for closure (Zed's native UI framework).
//!
//! High-performance immediate-mode native desktop (mac/linux/win).
//! Evaluate against egui for power users (perf, input lag budget I from vision).
//! Stub crate for ROADMAP GUI + capability matrix (const in cli); full
//! ShellAdapter impl + eframe-like run loop in follow-up.

#![forbid(unsafe_code)]

/// Marker.
pub const GPUI_SHELL: &str = "gpui";
