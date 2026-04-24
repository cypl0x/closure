//! ACP (Agent Communication Protocol) bridge. Allows external agents
//! to drive the command registry over ACP.

#![forbid(unsafe_code)]

use thiserror::Error;

/// Placeholder ACP adapter.
#[derive(Debug, Default)]
pub struct Adapter;

/// ACP bridge error.
#[derive(Debug, Error)]
pub enum AcpError {
    /// Transport error.
    #[error("transport: {0}")]
    Transport(String),
}
