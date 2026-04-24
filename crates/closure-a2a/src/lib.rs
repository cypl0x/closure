//! A2A (Agent-to-Agent) bridge. Lets closure participate in agent
//! swarms as a first-class peer.

#![forbid(unsafe_code)]

use thiserror::Error;

/// Placeholder A2A adapter.
#[derive(Debug, Default)]
pub struct Adapter;

/// A2A bridge error.
#[derive(Debug, Error)]
pub enum A2aError {
    /// Transport error.
    #[error("transport: {0}")]
    Transport(String),
}
