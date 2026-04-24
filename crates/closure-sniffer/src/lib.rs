//! Network sniffer. Observation-only at first. Composable filters
//! (Little Snitch / mitmproxy / Wireshark style).
//!
//! Lives in the closure ecosystem but is its own binary — it does not
//! link into any shell.

#![forbid(unsafe_code)]

use thiserror::Error;

/// Configured filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Human-readable identifier.
    pub id: String,
    /// Match pattern.
    pub pattern: String,
}

/// Sniffer error.
#[derive(Debug, Error)]
pub enum SnifferError {
    /// Binding a socket failed.
    #[error("bind: {0}")]
    Bind(String),
}
