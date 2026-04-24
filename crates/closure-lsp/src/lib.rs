//! LSP bridge. Surfaces the command registry as an LSP server so
//! editors (neovim, `VSCode`, helix, emacs-lsp) can drive closure
//! through the same code path as the TUI.

#![forbid(unsafe_code)]

use thiserror::Error;

/// Placeholder LSP server handle.
#[derive(Debug, Default)]
pub struct Server;

/// LSP bridge error.
#[derive(Debug, Error)]
pub enum LspError {
    /// Transport error.
    #[error("transport: {0}")]
    Transport(String),
}
