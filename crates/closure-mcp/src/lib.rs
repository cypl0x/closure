//! MCP (Model Context Protocol) bridge.
//!
//! Exposes the command registry as MCP tools. External MCP-speaking
//! clients (agents, IDEs) invoke commands through this bridge only —
//! never reaching the Document directly (I8).

#![forbid(unsafe_code)]

use thiserror::Error;

/// Placeholder MCP server handle.
#[derive(Debug, Default)]
pub struct Server;

/// Start a server binding on a unix socket path.
#[allow(clippy::missing_errors_doc)]
pub const fn serve(_socket_path: &str) -> Result<Server, McpError> {
    Ok(Server)
}

/// MCP bridge error.
#[derive(Debug, Error)]
pub enum McpError {
    /// Socket bind failed.
    #[error("bind: {0}")]
    Bind(String),
}
