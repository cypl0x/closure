//! Wasm plugin host.
//!
//! Plugins are sandboxed wasm modules. The core API surface they can
//! import is semver-pinned; adding to it is a minor version bump,
//! removing is a major. Plugins mutate the document only through the
//! command registry (I8).
//!
//! M10 skeleton: a `Plugin` trait + parsed `Manifest` structure. Wasm
//! runtime hookup (wasmtime / wasmer) lives behind a feature flag
//! once chosen.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use thiserror::Error;

/// Parsed plugin manifest.
///
/// The on-disk format is a `#+BEGIN_SRC closure-plugin` org code block
/// containing `key = value` lines, so plugins are introspectable from
/// the same vault that loads them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Unique plugin identifier (kebab-case).
    pub id: String,
    /// Plugin display name.
    pub name: String,
    /// Semver string the plugin targets against the closure-core API.
    pub api_version: String,
    /// Free-form metadata.
    pub meta: HashMap<String, String>,
}

/// Plugin trait. A plugin's `init` runs once at load time; subsequent
/// invocations route through registered commands so the plugin can
/// only mutate `Document` through I8 channels.
pub trait Plugin {
    /// Manifest for this plugin.
    fn manifest(&self) -> &Manifest;
    /// One-time setup. Failure aborts the load.
    fn init(&mut self) -> Result<(), PluginError> {
        Ok(())
    }
}

/// Plugin host handle.
#[derive(Debug, Default)]
pub struct Host {
    plugins: Vec<Manifest>,
}

impl Host {
    /// Fresh host with no plugins.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Register a plugin's manifest.
    pub fn register(&mut self, m: Manifest) {
        self.plugins.push(m);
    }

    /// All registered manifests.
    #[must_use]
    pub fn manifests(&self) -> &[Manifest] {
        &self.plugins
    }
}

/// Parse a manifest from a `key = value` block.
///
/// Required keys: `id`, `name`, `api_version`. Other keys go into
/// `meta` verbatim.
#[allow(clippy::missing_errors_doc)]
pub fn parse_manifest(content: &str) -> Result<Manifest, PluginError> {
    let mut id: Option<String> = None;
    let mut name: Option<String> = None;
    let mut api_version: Option<String> = None;
    let mut meta: HashMap<String, String> = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            return Err(PluginError::Load(format!("expected `key = value`: {line}")));
        };
        let k = k.trim().to_owned();
        let v = v.trim().trim_matches('"').to_owned();
        match k.as_str() {
            "id" => id = Some(v),
            "name" => name = Some(v),
            "api_version" => api_version = Some(v),
            _ => {
                meta.insert(k, v);
            }
        }
    }
    Ok(Manifest {
        id: id.ok_or_else(|| PluginError::Load("missing `id`".into()))?,
        name: name.ok_or_else(|| PluginError::Load("missing `name`".into()))?,
        api_version: api_version
            .ok_or_else(|| PluginError::Load("missing `api_version`".into()))?,
        meta,
    })
}

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
