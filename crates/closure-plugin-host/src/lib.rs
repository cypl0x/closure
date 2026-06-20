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

    /// Load + run a wasm plugin described by `manifest` (its module
    /// `bytes`, binary or WAT), gated by the manifest's `api_version`
    /// against [`SUPPORTED_API_VERSION`]: an incompatible plugin is
    /// rejected *before* it runs. On success the manifest is registered
    /// and the command names the guest requested (through the I8 host
    /// import, see [`WasmRuntime::run_with_commands`]) are returned.
    ///
    /// # Errors
    ///
    /// [`PluginError::Load`] when the manifest's `api_version` is
    /// incompatible; otherwise the errors of
    /// [`WasmRuntime::run_with_commands`].
    #[cfg(feature = "wasmtime")]
    pub fn run_wasm_manifest(
        &mut self,
        manifest: &Manifest,
        bytes: &[u8],
        entry: &str,
        registry: &closure_core::Registry,
    ) -> Result<Vec<String>, PluginError> {
        if !api_compatible(SUPPORTED_API_VERSION, &manifest.api_version) {
            return Err(PluginError::Load(format!(
                "plugin `{}` targets API {} incompatible with host {SUPPORTED_API_VERSION}",
                manifest.id, manifest.api_version
            )));
        }
        let cmds = WasmRuntime::new().run_with_commands(bytes, entry, registry)?;
        self.plugins.push(manifest.clone());
        Ok(cmds)
    }
}

/// The closure-core plugin API version this host supports. Plugins
/// declare the `api_version` they target in their manifest; the host
/// loads them only when [`api_compatible`] holds.
pub const SUPPORTED_API_VERSION: &str = "0.1.0";

/// Parse a dotted `MAJOR.MINOR.PATCH` into its three numbers.
fn parse_semver(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Whether a `plugin` API version is compatible with the `host`.
///
/// Same major version, and the plugin's minor is not newer than the
/// host's (patch ignored). Unparseable versions are incompatible.
#[must_use]
pub fn api_compatible(host: &str, plugin: &str) -> bool {
    match (parse_semver(host), parse_semver(plugin)) {
        (Some((hmaj, hmin, _)), Some((pmaj, pmin, _))) => hmaj == pmaj && pmin <= hmin,
        _ => false,
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
    /// A package manifest / lockfile was malformed.
    #[error("package: {0}")]
    Package(String),
}

/// A package: name + version + dependencies + provided commands (V4a).
///
/// On-disk format is a `#+BEGIN_SRC closure-package` block of `key =
/// value` lines (plain text, no JSON/YAML), with repeatable `dep` and
/// `command` keys. Round-trips byte-exact via [`render_package`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// Package name (kebab-case).
    pub name: String,
    /// Semver version.
    pub version: String,
    /// Dependencies as `(name, version requirement)`, in declared order.
    pub deps: Vec<(String, String)>,
    /// Commands this package provides, in declared order.
    pub commands: Vec<String>,
}

/// Extract the content of a `#+BEGIN_SRC closure-package` block from
/// `src` (the `key = value` lines between `BEGIN`/`END`), if present.
#[must_use]
pub fn extract_package_block(src: &str) -> Option<&str> {
    let begin = src.lines().scan(0usize, |off, l| {
        let start = *off;
        *off += l.len() + 1;
        Some((start, l))
    });
    let mut content_start = None;
    for (off, line) in begin {
        let t = line.trim_start().to_ascii_lowercase();
        if content_start.is_none() && t.starts_with("#+begin_src closure-package") {
            content_start = Some(off + line.len() + 1);
        } else if let Some(cs) = content_start
            && t.starts_with("#+end_src")
        {
            return src.get(cs..off);
        }
    }
    None
}

/// Load every package defined in a local registry directory (V4c): each
/// `*.org` file containing a `closure-package` block contributes one
/// package, keyed by name. Files without a block are skipped.
///
/// # Errors
///
/// [`PluginError::Package`] if the directory cannot be read or a package
/// block is malformed.
pub fn load_packages(
    dir: &std::path::Path,
) -> Result<std::collections::BTreeMap<String, Package>, PluginError> {
    let mut out = std::collections::BTreeMap::new();
    let entries =
        std::fs::read_dir(dir).map_err(|e| PluginError::Package(format!("read dir: {e}")))?;
    let mut paths: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "org"))
        .collect();
    paths.sort();
    for path in paths {
        let src = std::fs::read_to_string(&path)
            .map_err(|e| PluginError::Package(format!("read {}: {e}", path.display())))?;
        if let Some(content) = extract_package_block(&src) {
            let pkg = parse_package(content)?;
            out.insert(pkg.name.clone(), pkg);
        }
    }
    Ok(out)
}

/// Parse a [`Package`] from a `closure-package` `key = value` block.
///
/// # Errors
///
/// [`PluginError::Package`] when `name` or `version` is missing, or a
/// `dep` line lacks a version requirement.
pub fn parse_package(content: &str) -> Result<Package, PluginError> {
    let mut name = None;
    let mut version = None;
    let mut deps = Vec::new();
    let mut commands = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| PluginError::Package(format!("expected `key = value`: {line}")))?;
        let (key, value) = (key.trim(), value.trim());
        match key {
            "name" => name = Some(value.to_owned()),
            "version" => version = Some(value.to_owned()),
            "dep" => {
                let (dep, req) = value
                    .split_once(char::is_whitespace)
                    .ok_or_else(|| PluginError::Package(format!("dep needs a version: {value}")))?;
                deps.push((dep.trim().to_owned(), req.trim().to_owned()));
            }
            "command" => commands.push(value.to_owned()),
            other => return Err(PluginError::Package(format!("unknown key: {other}"))),
        }
    }
    Ok(Package {
        name: name.ok_or_else(|| PluginError::Package("missing name".into()))?,
        version: version.ok_or_else(|| PluginError::Package("missing version".into()))?,
        deps,
        commands,
    })
}

/// Render a [`Package`] to its canonical block content (byte-exact
/// inverse of [`parse_package`] for canonical input).
#[must_use]
pub fn render_package(pkg: &Package) -> String {
    use std::fmt::Write as _;
    let mut out = format!("name = {}\nversion = {}\n", pkg.name, pkg.version);
    for (dep, req) in &pkg.deps {
        let _ = writeln!(out, "dep = {dep} {req}");
    }
    for cmd in &pkg.commands {
        let _ = writeln!(out, "command = {cmd}");
    }
    out
}

/// A resolved lockfile entry: a package pinned to an exact version + a
/// content hash (V4a).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LockEntry {
    /// Package name.
    pub name: String,
    /// Exact resolved version.
    pub version: String,
    /// Content hash (e.g. `sha256:...`).
    pub hash: String,
}

/// Parse a lockfile: one `name version hash` per line (blank/`#` lines
/// skipped).
///
/// # Errors
///
/// [`PluginError::Package`] for a line without three fields.
pub fn parse_lockfile(content: &str) -> Result<Vec<LockEntry>, PluginError> {
    let mut out = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        match (it.next(), it.next(), it.next()) {
            (Some(name), Some(version), Some(hash)) => out.push(LockEntry {
                name: name.to_owned(),
                version: version.to_owned(),
                hash: hash.to_owned(),
            }),
            _ => return Err(PluginError::Package(format!("bad lock line: {line}"))),
        }
    }
    Ok(out)
}

/// Parse `major.minor.patch`; `None` if not three numeric fields.
fn semver(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.split('.');
    let parts = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() {
        return None;
    }
    Some((
        parts.0.parse().ok()?,
        parts.1.parse().ok()?,
        parts.2.parse().ok()?,
    ))
}

/// Whether `version` satisfies `req`: `>=X.Y.Z` (at least) or an exact
/// `X.Y.Z`. Unparseable versions never satisfy.
fn version_satisfies(req: &str, version: &str) -> bool {
    let Some(have) = semver(version) else {
        return false;
    };
    req.strip_prefix(">=").map_or_else(
        || semver(req) == Some(have),
        |min| semver(min.trim()).is_some_and(|min| have >= min),
    )
}

/// FNV-1a content hash (dep-free, hermetic), matching the vault's index
/// hashing style.
fn content_hash(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a:{h:016x}")
}

/// Resolve `root`'s transitive dependencies over a local package set
/// `available` (no network, V4b), producing a lockfile that pins each
/// dependency to its exact version + a content hash.
///
/// Deterministic (I6): the result is sorted and independent of dependency
/// declaration order. Cycle- and version-checked.
///
/// # Errors
///
/// [`PluginError::Package`] when a dependency is missing, its version
/// does not satisfy the requirement, or a dependency cycle exists.
pub fn resolve(
    root: &Package,
    available: &std::collections::BTreeMap<String, Package>,
) -> Result<Vec<LockEntry>, PluginError> {
    let mut resolved: std::collections::BTreeMap<String, LockEntry> =
        std::collections::BTreeMap::new();
    let mut stack: Vec<String> = Vec::new();
    visit(root, available, &mut resolved, &mut stack)?;
    Ok(resolved.into_values().collect())
}

/// DFS one package's deps, recording lock entries; `stack` carries the
/// active chain for cycle detection.
fn visit(
    pkg: &Package,
    available: &std::collections::BTreeMap<String, Package>,
    resolved: &mut std::collections::BTreeMap<String, LockEntry>,
    stack: &mut Vec<String>,
) -> Result<(), PluginError> {
    stack.push(pkg.name.clone());
    for (dep, req) in &pkg.deps {
        let dep_pkg = available
            .get(dep)
            .ok_or_else(|| PluginError::Package(format!("unknown dependency `{dep}`")))?;
        if !version_satisfies(req, &dep_pkg.version) {
            return Err(PluginError::Package(format!(
                "`{dep}` {} does not satisfy `{req}`",
                dep_pkg.version
            )));
        }
        if stack.iter().any(|n| n == dep) {
            return Err(PluginError::Package(format!(
                "dependency cycle through `{dep}`"
            )));
        }
        if !resolved.contains_key(dep) {
            resolved.insert(
                dep.clone(),
                LockEntry {
                    name: dep.clone(),
                    version: dep_pkg.version.clone(),
                    hash: content_hash(&render_package(dep_pkg)),
                },
            );
            visit(dep_pkg, available, resolved, stack)?;
        }
    }
    stack.pop();
    Ok(())
}

/// Render a lockfile, sorted by `(name, version)` for a deterministic,
/// byte-exact result (I6).
#[must_use]
pub fn render_lockfile(entries: &[LockEntry]) -> String {
    use std::fmt::Write as _;
    let mut sorted = entries.to_vec();
    sorted.sort();
    let mut out = String::new();
    for e in &sorted {
        let _ = writeln!(out, "{} {} {}", e.name, e.version, e.hash);
    }
    out
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
