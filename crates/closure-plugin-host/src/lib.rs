//! Wasm plugin host.
//!
//! Plugins are sandboxed wasm modules. The core API surface they can
//! import is semver-pinned; adding to it is a minor version bump,
//! removing is a major. Plugins mutate the document only through the
//! command registry (I8).

#![forbid(unsafe_code)]

use thiserror::Error;

/// Plugin host handle.
#[derive(Debug, Default)]
pub struct Host;

/// Plugin host error.
#[derive(Debug, Error)]
pub enum PluginError {
    /// The plugin failed to load.
    #[error("load: {0}")]
    Load(String),
    /// The plugin tried to call an undefined import.
    #[error("undefined import: {0}")]
    UndefinedImport(String),
}
