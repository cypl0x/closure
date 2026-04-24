//! Web shell. Two delivery modes:
//!
//! * A single-file self-contained HTML bundle (no server), with the
//!   wasm kernel embedded.
//! * A localhost HTTP server that serves the shell and proxies the
//!   command registry.

#![forbid(unsafe_code)]

/// Web shell handle.
#[derive(Debug, Default)]
pub struct Shell;
