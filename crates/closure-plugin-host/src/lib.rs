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
    commands: Vec<PluginCommand>,
}

impl Host {
    /// Fresh host with no plugins.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            plugins: Vec::new(),
            commands: Vec::new(),
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

/// Embedded wasm runtime over [`wasmtime`] (opt-in `wasmtime` feature).
///
/// Loads + runs sandboxed modules in-process. Accepts both binary wasm
/// and WAT text (wasmtime auto-detects), so tests run hand-written WAT
/// fixtures with no external toolchain. The default workspace build
/// stays hermetic without this feature; the [process path](runner_for)
/// is the feature-off fallback.
#[cfg(feature = "wasmtime")]
#[derive(Debug)]
pub struct WasmRuntime {
    engine: wasmtime::Engine,
}

#[cfg(feature = "wasmtime")]
impl Default for WasmRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "wasmtime")]
impl WasmRuntime {
    /// Fresh runtime with a default [`wasmtime::Engine`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            engine: wasmtime::Engine::default(),
        }
    }

    /// Load and instantiate `bytes` (binary wasm or WAT text) with no
    /// imports, proving the module is well-formed and self-contained.
    ///
    /// # Errors
    ///
    /// [`PluginError::Load`] on a malformed module or a failed
    /// instantiation (e.g. unsatisfied imports).
    pub fn instantiate(&self, bytes: &[u8]) -> Result<(), PluginError> {
        let module = wasmtime::Module::new(&self.engine, bytes)
            .map_err(|e| PluginError::Load(e.to_string()))?;
        let mut store = wasmtime::Store::new(&self.engine, ());
        wasmtime::Instance::new(&mut store, &module, &[])
            .map_err(|e| PluginError::Load(e.to_string()))?;
        Ok(())
    }

    /// Instantiate `bytes` and call the exported, no-argument function
    /// `export` returning an `i32`.
    ///
    /// # Errors
    ///
    /// [`PluginError::Load`] on a malformed module or instantiation
    /// failure; [`PluginError::UndefinedImport`] when `export` is
    /// missing or not an `() -> i32` function, or it traps.
    pub fn call_i32(&self, bytes: &[u8], export: &str) -> Result<i32, PluginError> {
        let module = wasmtime::Module::new(&self.engine, bytes)
            .map_err(|e| PluginError::Load(e.to_string()))?;
        let mut store = wasmtime::Store::new(&self.engine, ());
        let instance = wasmtime::Instance::new(&mut store, &module, &[])
            .map_err(|e| PluginError::Load(e.to_string()))?;
        let func = instance
            .get_typed_func::<(), i32>(&mut store, export)
            .map_err(|e| PluginError::UndefinedImport(format!("{export}: {e}")))?;
        func.call(&mut store, ())
            .map_err(|e| PluginError::UndefinedImport(format!("{export} trapped: {e}")))
    }

    /// Run a plugin module's `entry` export, capturing the command names
    /// it requests through the one host import it is given —
    /// `closure.run_command(ptr, len)`, which reads a UTF-8 command name
    /// from the guest's own exported `memory`. This is the I8 boundary:
    /// the guest has no `Document` import and no shared host state, so it
    /// can only *request* registered commands by name — never mutate the
    /// document directly. Each requested name is gated against
    /// `registry`; an unknown name is rejected.
    ///
    /// Returns the requested (and registry-valid) command names in order.
    ///
    /// # Errors
    ///
    /// [`PluginError::Load`] on a malformed module / instantiation / trap;
    /// [`PluginError::UndefinedImport`] when the guest requests a command
    /// the `registry` does not know.
    pub fn run_with_commands(
        &self,
        bytes: &[u8],
        entry: &str,
        registry: &closure_core::Registry,
    ) -> Result<Vec<String>, PluginError> {
        let module = wasmtime::Module::new(&self.engine, bytes)
            .map_err(|e| PluginError::Load(e.to_string()))?;
        let mut store = wasmtime::Store::new(&self.engine, Vec::<String>::new());
        let mut linker = wasmtime::Linker::new(&self.engine);
        linker
            .func_wrap(
                "closure",
                "run_command",
                |mut caller: wasmtime::Caller<'_, Vec<String>>, ptr: i32, len: i32| {
                    let Some(wasmtime::Extern::Memory(mem)) = caller.get_export("memory") else {
                        return;
                    };
                    let data = mem.data(&caller);
                    let start = usize::try_from(ptr).unwrap_or(usize::MAX);
                    let end = start.saturating_add(usize::try_from(len).unwrap_or(0));
                    let name = data
                        .get(start..end)
                        .and_then(|b| std::str::from_utf8(b).ok())
                        .map(ToOwned::to_owned);
                    if let Some(name) = name {
                        caller.data_mut().push(name);
                    }
                },
            )
            .map_err(|e| PluginError::Load(e.to_string()))?;
        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| PluginError::Load(e.to_string()))?;
        let func = instance
            .get_typed_func::<(), ()>(&mut store, entry)
            .map_err(|e| PluginError::UndefinedImport(format!("{entry}: {e}")))?;
        func.call(&mut store, ())
            .map_err(|e| PluginError::Load(format!("{entry} trapped: {e}")))?;
        let requested = store.into_data();
        for name in &requested {
            if registry.get(name).is_none() {
                return Err(PluginError::UndefinedImport(format!(
                    "plugin requested unknown command `{name}`"
                )));
            }
        }
        Ok(requested)
    }
}

/// The argv used to run a plugin executable: `.wasm` files run under
/// an external `wasmtime` (sandboxed, no embedded runtime dep);
/// anything else executes directly.
#[must_use]
pub fn runner_for(path: &std::path::Path) -> Vec<String> {
    let p = path.display().to_string();
    if path.extension().is_some_and(|e| e == "wasm") {
        vec!["wasmtime".to_owned(), p]
    } else {
        vec![p]
    }
}

/// A command contributed by a plugin executable.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PluginCommand {
    name: String,
    executable: std::path::PathBuf,
}

impl Host {
    /// Register the command declared by `manifest` (its `command`
    /// meta key), backed by `executable`. Invocation appends the
    /// caller's args to [`runner_for`]'s argv.
    ///
    /// # Errors
    ///
    /// [`PluginError::Load`] when the manifest declares no `command`.
    pub fn register_command(
        &mut self,
        manifest: &Manifest,
        executable: &std::path::Path,
    ) -> Result<(), PluginError> {
        let name = manifest
            .meta
            .get("command")
            .ok_or_else(|| PluginError::Load("manifest missing `command`".into()))?;
        self.commands.push(PluginCommand {
            name: name.clone(),
            executable: executable.to_path_buf(),
        });
        self.plugins.push(manifest.clone());
        Ok(())
    }

    /// Names of every plugin-contributed command.
    #[must_use]
    pub fn commands(&self) -> Vec<String> {
        self.commands.iter().map(|c| c.name.clone()).collect()
    }

    /// Invoke a plugin command with `args`, returning its stdout.
    ///
    /// # Errors
    ///
    /// [`PluginError::Load`] for unknown commands or spawn/exit
    /// failures.
    pub fn invoke(&mut self, command: &str, args: &[&str]) -> Result<String, PluginError> {
        let cmd = self
            .commands
            .iter()
            .find(|c| c.name == command)
            .ok_or_else(|| PluginError::Load(format!("unknown command `{command}`")))?;
        let runner = runner_for(&cmd.executable);
        let mut proc = std::process::Command::new(&runner[0]);
        proc.args(&runner[1..]).args(args);
        let out = proc
            .output()
            .map_err(|e| PluginError::Load(e.to_string()))?;
        if !out.status.success() {
            return Err(PluginError::Load(format!(
                "plugin `{command}` exited {}",
                out.status.code().unwrap_or(-1)
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}
