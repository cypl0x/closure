//! Typed configuration with load-time validation (spec invariant I9).
//!
//! Config is a typed schema that fails to load on invalid input, rather
//! than deferring errors to the feature that uses the value. All crates
//! read config only through typed [`Config`] handles, never raw
//! key/value strings.
//!
//! M4 scope: parse a minimal config from an org `#+BEGIN_SRC closure-config`
//! code block, with a small number of strongly-typed fields. CUE-style
//! dependent validation lands in a later milestone.

#![forbid(unsafe_code)]

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use closure_org::{NodeKind, parse};
use thiserror::Error;

/// User-facing configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Default vault directory, if the user pinned one.
    pub default_vault: Option<PathBuf>,
    /// Input mode (`emacs`, `vim`, `doom`, `helix`, `notion`).
    pub input_mode: InputMode,
    /// Theme name.
    pub theme: String,
    /// Which shape the GUI opens in: the clickable outline, or the file
    /// as one full-window buffer (`clickable` / `editor`).
    pub view: String,
    /// Custom TODO keywords (in order).
    pub todo_keywords: Vec<String>,
    /// Priority letters in order of decreasing severity (default `A`,
    /// `B`, `C`).
    pub priority_levels: Vec<char>,
    /// Whether tags are inherited from parent headlines.
    pub tag_inheritance: bool,
    /// Per-vault override files included in the agenda view.
    pub agenda_files: Vec<PathBuf>,
    /// LLM provider name (`anthropic`, `openai`, `echo`, …). BYOK.
    pub llm_provider: Option<String>,
    /// Model identifier passed to the provider.
    pub llm_model: Option<String>,
    /// Name of the environment variable holding the API key — the
    /// key itself never lives in the org file.
    pub llm_key_env: Option<String>,
    /// Explicit endpoint URL, for anything speaking a known wire
    /// format at an address of its own: vLLM, llama.cpp's server,
    /// LM Studio, `OpenRouter`, a company proxy. The built-in providers
    /// know their own URL and leave this unset.
    pub llm_endpoint: Option<String>,
    /// Record every executed command to `journal.org` (off by
    /// default).
    pub record_commands: bool,
    /// Full-text search engine (`builtin`, `ripgrep`/`rg`, `fd`).
    pub search_backend: Option<String>,
    /// Allowed tools for LLM (comma list; e.g. read,search,capture,rename,set-property,view-state).
    /// Per-tool permission model (live configurable via llm-allow/deny later).
    pub llm_tools: Option<Vec<String>>,
    /// Blocklist globs for the network sniffer (comma patterns; * wildcards).
    /// Used by =closure sniff= and block events.
    pub sniffer_blocklist: Option<Vec<String>>,
    /// Languages allowed to execute via the evaluator (comma list, e.g.
    /// `shell, python`). Empty = default-deny (the security default,
    /// C1a): no code block runs unless its language is listed here.
    pub eval_trust: Vec<String>,
    /// Pairing tickets for peers already paired with, so a peer pasted
    /// once is still there tomorrow. A ticket is an address and a
    /// public key — nothing secret, and readable in the file like
    /// everything else.
    pub sync_peers: Vec<String>,
    /// Soft-wrap long body lines instead of scrolling sideways. Off by
    /// default: wrapping costs the one-number gutter and the fixed row
    /// height, and prose people want wrapped is not the only thing in
    /// an org file.
    pub wrap: bool,
    /// Socket the pairing listener binds (`0.0.0.0:7420` to accept from
    /// the network, `127.0.0.1:7420` to stay machine-local). Unset
    /// leaves the choice to the shell.
    pub sync_bind: Option<SocketAddr>,
    /// Which of this host's addresses a peer should dial — what the
    /// ticket names. Only the operator knows whether the peer is on the
    /// LAN or the VPN, so a multi-homed host says here. Unset means
    /// "detect it"; the port always comes from the bound socket.
    pub sync_advertise: Option<IpAddr>,
    /// Directory inside the vault that pasted images are filed in
    /// (default `assets`). Relative to the vault root, because a link
    /// in an org file is relative to the file.
    pub assets_dir: Option<PathBuf>,
    /// A folder both machines can see — a Syncthing share, a Dropbox, a
    /// mounted drive, a USB stick — where `sync-export` leaves a signed
    /// bundle and `sync-import` picks up the peers'.
    ///
    /// The socket path needs both machines up at once and a route
    /// between them; a shared folder needs neither. Same replica, same
    /// trust anchor: a bundle is merged only when a paired peer signed
    /// it, because a folder is exactly as trustworthy as whoever can
    /// write to it.
    pub sync_dir: Option<PathBuf>,
    /// Block id of the headline the last session was in — the outline
    /// opens on it rather than on row zero.
    ///
    /// Written by the window when it closes, not on every keystroke: a
    /// config file rewritten at cursor speed is a config file at war
    /// with the editor you have it open in. An id that no longer
    /// resolves is ignored, because a vault is edited elsewhere too.
    pub last_place: Option<String>,
    /// Files the last sessions opened, most recent first — what the
    /// file picker offers before it offers the rest of the vault.
    ///
    /// Same reasoning as [`Self::last_place`]: durable view state
    /// belongs in the plain file the user can read, not in a hidden
    /// state directory. Paths that no longer exist are skipped when the
    /// picker is built rather than treated as an error, because a vault
    /// is edited elsewhere too.
    pub recent_files: Vec<PathBuf>,
}

/// The tail of the generated `config.org`: the keys that have no
/// default worth writing, shown commented out so the file says what
/// *can* be set without claiming any of it is set.
///
/// Split out of [`Config::default_org`] because it is the half with no
/// interpolation in it — and because that function is otherwise one
/// long string that keeps growing a key at a time.
const OPTIONAL_KEYS_DOC: &str = "\
\n\
# Pairing. `sync_bind` is the socket to open; `sync_advertise` is which of\n\
# this host's addresses goes in the ticket you hand over — set it when the\n\
# machine has more than one route to it (a LAN *and* a mesh VPN).\n\
# sync_bind = 0.0.0.0:7420\n\
# sync_advertise = 100.101.102.103\n\
# Peers you have paired with. Written by the pairing pane\n\
# when you paste a ticket; a ticket is an address and a\n\
# public key, so there is nothing secret in it.\n\
# sync_peers = closure-sync:100.1.2.3:7420|abc123…\n\
# A folder both machines can see — Syncthing, a Dropbox, a USB stick.\n\
# `sync-export` leaves a signed bundle in it and `sync-import` picks up\n\
# the peers', so neither machine has to be up when the other is. Still\n\
# only merges bundles signed by a peer you have paired with.\n\
# sync_dir = /home/you/Sync/closure\n\
\n\
# The assistant. The key itself never lives here: `llm_key_env` names an\n\
# environment variable, so this file can be committed and synced.\n\
# llm_provider = anthropic\n\
# llm_model = claude-sonnet-4-5\n\
# llm_key_env = ANTHROPIC_API_KEY\n\
# llm_endpoint = http://localhost:8080/v1/chat/completions\n\
# llm_tools = read, search, capture\n\
\n\
# Network sniffer blocklist (`*` wildcards).\n\
# sniffer_blocklist = *.doubleclick.net, telemetry.*\n";

impl Default for Config {
    fn default() -> Self {
        Self {
            default_vault: None,
            input_mode: InputMode::Doom,
            theme: "default".into(),
            // The outline is where the rail and every affordance is;
            // the editor view is one chord (`g v`) away, and this key
            // is for the users who want to start there every time.
            view: "clickable".into(),
            todo_keywords: vec!["TODO".into(), "DONE".into()],
            priority_levels: vec!['A', 'B', 'C'],
            tag_inheritance: true,
            agenda_files: Vec::new(),
            llm_provider: None,
            llm_model: None,
            llm_key_env: None,
            llm_endpoint: None,
            record_commands: false,
            search_backend: None,
            llm_tools: None,
            sniffer_blocklist: None,
            eval_trust: Vec::new(),
            sync_peers: Vec::new(),
            wrap: false,
            sync_bind: None,
            sync_advertise: None,
            assets_dir: None,
            sync_dir: None,
            last_place: None,
            recent_files: Vec::new(),
        }
    }
}

/// Coarse visual theme a shell applies. The free-form [`Config::theme`]
/// string maps to one of these (default [`ThemeKind::Dark`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    /// Dark visuals (default).
    Dark,
    /// Light visuals.
    Light,
}

/// Selected input binding scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Plain Emacs (C-x C-s, etc.)
    Emacs,
    /// Vim modal.
    Vim,
    /// Doom Emacs (SPC leader).
    Doom,
    /// Helix modal.
    Helix,
    /// Notion mouse + slash commands.
    Notion,
}

/// Config load / validation failure.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The config source didn't parse as an org document.
    #[error("config is not valid org: {0}")]
    ParseOrg(String),
    /// The config block is missing or empty.
    #[error("no #+BEGIN_SRC closure-config block found")]
    Missing,
    /// An unknown key was set.
    #[error("unknown config key: {0}")]
    UnknownKey(String),
    /// A value failed to parse for its type.
    #[error("invalid value for `{key}`: {reason}")]
    BadValue {
        /// Key whose value failed.
        key: String,
        /// Human-readable reason.
        reason: String,
    },
}

/// Set one key in a `config.org`, leaving everything else exactly as it
/// was.
///
/// The app writes settings back — a pasted pairing ticket should still
/// be there tomorrow — and the file it writes into is one a person
/// reads and edits. So this replaces a single line, or adds one inside
/// the block, and never reformats: the prose around the block, the
/// comments inside it, the order of the keys and any key this build has
/// never heard of all survive. A file with no block at all grows one at
/// the end, which is what an empty or prose-only config.org needs.
///
/// # Errors
///
/// [`ConfigError::ParseOrg`] if the *result* would not parse as org —
/// checked rather than assumed, because this writes a file the user
/// owns.
pub fn set_config_key(source: &str, key: &str, value: &str) -> Result<String, ConfigError> {
    let line = format!("{key} = {value}");
    let mut out = String::with_capacity(source.len() + line.len() + 32);
    let mut in_block = false;
    let mut replaced = false;
    let mut inserted = false;
    for raw in source.split_inclusive('\n') {
        let trimmed = raw.trim();
        if trimmed.eq_ignore_ascii_case("#+BEGIN_SRC closure-config") {
            in_block = true;
            out.push_str(raw);
            continue;
        }
        if in_block && trimmed.eq_ignore_ascii_case("#+END_SRC") {
            // The key was not in the block: add it just before the end,
            // so it lands with its siblings rather than after them.
            if !replaced {
                out.push_str(&line);
                out.push('\n');
                inserted = true;
            }
            in_block = false;
            out.push_str(raw);
            continue;
        }
        let is_this_key = in_block
            && trimmed
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == key);
        if is_this_key && !replaced {
            out.push_str(&line);
            out.push('\n');
            replaced = true;
            continue;
        }
        out.push_str(raw);
    }
    if !replaced && !inserted {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("\n#+BEGIN_SRC closure-config\n");
        out.push_str(&line);
        out.push_str("\n#+END_SRC\n");
    }
    // What we wrote has to be readable again, including by the org
    // parser — this is somebody's file.
    parse(&out).map_err(|e| ConfigError::ParseOrg(e.to_string()))?;
    Ok(out)
}

/// Parse an org-config boolean (`true`/`yes`/`1` or `false`/`no`/`0`).
fn parse_bool(key: &str, value: &str) -> Result<bool, ConfigError> {
    match value {
        "true" | "yes" | "1" => Ok(true),
        "false" | "no" | "0" => Ok(false),
        other => Err(ConfigError::BadValue {
            key: key.into(),
            reason: format!("expected boolean, got `{other}`"),
        }),
    }
}

impl Config {
    /// The visual theme to apply, mapped from the free-form [`Self::theme`]
    /// string: `light` (case-insensitive) → [`ThemeKind::Light`], anything
    /// else (including the default) → [`ThemeKind::Dark`].
    #[must_use]
    pub fn theme_kind(&self) -> ThemeKind {
        if self.theme.eq_ignore_ascii_case("light") {
            ThemeKind::Light
        } else {
            ThemeKind::Dark
        }
    }

    /// The default configuration as a `config.org` a person can read
    /// and edit.
    ///
    /// Generated from [`Self::default`] rather than written by hand:
    /// a sample file drifts from the schema the moment a key is added,
    /// and then every vault carries a wrong one. Keys with no default
    /// value are shown commented out — the file's job is to say what
    /// *can* be set, and an empty `llm_provider =` would be a lie about
    /// what is configured.
    #[must_use]
    pub fn default_org() -> String {
        let d = Self::default();
        let todo = d.todo_keywords.join(", ");
        let priorities = d
            .priority_levels
            .iter()
            .map(char::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "#+TITLE: closure configuration\n\
             #+FILETAGS: :closure:\n\
             \n\
             This file *is* the configuration: closure reads the block below \
             every time it starts, and\nnothing else. Delete a line to get \
             its default back — the defaults are what is written\nhere.\n\
             \n\
             #+BEGIN_SRC closure-config\n\
             # How you type. emacs | vim | doom | helix | notion\n\
             input_mode = {input_mode}\n\
             \n\
             # Colours. doom-vibrant | dark | light | high-contrast\n\
             theme = {theme}\n\
             \n\
             # Which shape the window opens in. clickable = the outline \
             (where the rail\n# and every affordance live); editor = the file \
             as one full-window buffer.\n\
             # `g v` toggles it either way.\n\
             view = {view}\n\
             \n\
             # Org basics.\n\
             todo_keywords = {todo}\n\
             priority_levels = {priorities}\n\
             tag_inheritance = {tags}\n\
             # agenda_files = work.org, home.org\n\
             # default_vault = /home/you/vault\n\
             \n\
             # Write every executed command to journal.org.\n\
             record_commands = {record}\n\
             \n\
             # Soft-wrap long body lines instead of scrolling sideways.\n\
             wrap = {wrap}\n\
             \n\
             # Full-text search engine. builtin | ripgrep | fd\n\
             # search_backend = ripgrep\n\
             \n\
             # Where an image pasted into a note is filed, relative to \
             the vault.\n# The link written into the file is relative to \
             it too, so the note\n# still resolves in Emacs.\n\
             # assets_dir = assets\n\
             \n\
             # Where the last session was. Written when the window \
             closes, so the\n# outline opens on the note you were in \
             rather than on the first one.\n\
             # last_place = 01HQ…\n\
             \n\
             # Which languages a code block — or `:!` — may actually run.\n\
             # Empty is default-deny: nothing runs, whatever the file says, \
             because a\n# vault is something people can send you. Add \
             `shell, python` to opt in.\n\
             eval_trust = {eval}\n\
             {optional}\
             #+END_SRC\n",
            input_mode = match d.input_mode {
                InputMode::Emacs => "emacs",
                InputMode::Vim => "vim",
                InputMode::Doom => "doom",
                InputMode::Helix => "helix",
                InputMode::Notion => "notion",
            },
            theme = d.theme,
            view = d.view,
            tags = d.tag_inheritance,
            record = d.record_commands,
            wrap = d.wrap,
            eval = d.eval_trust.join(", "),
            optional = OPTIONAL_KEYS_DOC,
        )
    }

    /// Load a config from a `*.org` file on disk.
    #[allow(clippy::missing_errors_doc)]
    pub fn from_path(path: &std::path::Path) -> Result<Self, ConfigError> {
        let src =
            std::fs::read_to_string(path).map_err(|e| ConfigError::ParseOrg(e.to_string()))?;
        Self::from_org_source(&src)
    }

    /// Load a config from an org document containing a
    /// `#+BEGIN_SRC closure-config` block with `key = value` lines.
    pub fn from_org_source(src: &str) -> Result<Self, ConfigError> {
        let doc = parse(src).map_err(|e| ConfigError::ParseOrg(e.to_string()))?;
        for n in doc.preamble() {
            if n.kind() == NodeKind::CodeBlock
                && let Some(cb) = n.as_code_block()
                && cb.language == Some("closure-config")
            {
                return Self::from_kv_block(cb.content);
            }
        }
        Err(ConfigError::Missing)
    }

    /// Parse `key = value` lines, returning a fully-typed [`Config`]
    /// with the default for any field the user didn't specified.
    #[allow(clippy::too_many_lines)]
    pub fn from_kv_block(content: &str) -> Result<Self, ConfigError> {
        let mut cfg = Self::default();
        for (line_no, raw) in content.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(ConfigError::BadValue {
                    key: format!("line {}", line_no + 1),
                    reason: format!("expected `key = value`, got `{line}`"),
                });
            };
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            let line_info = format!("line {}", line_no + 1);
            match key {
                "default_vault" => {
                    cfg.default_vault = Some(PathBuf::from(value));
                }
                "input_mode" => {
                    cfg.input_mode = match value {
                        "emacs" => InputMode::Emacs,
                        "vim" => InputMode::Vim,
                        "doom" => InputMode::Doom,
                        "helix" => InputMode::Helix,
                        "notion" => InputMode::Notion,
                        other => {
                            return Err(ConfigError::BadValue {
                                key: key.into(),
                                reason: format!("{line_info}: unknown input_mode `{other}`"),
                            });
                        }
                    };
                }
                "view" => match value {
                    "clickable" | "outline" => cfg.view = "clickable".into(),
                    "editor" | "file" => cfg.view = "editor".into(),
                    other => {
                        return Err(ConfigError::BadValue {
                            key: key.into(),
                            reason: format!("{line_info}: unknown view `{other}`"),
                        });
                    }
                },
                "theme" => {
                    cfg.theme = value.into();
                }
                "todo_keywords" => {
                    cfg.todo_keywords = value
                        .split(',')
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                "priority_levels" => {
                    let levels: Vec<char> = value
                        .split(',')
                        .filter_map(|s| s.trim().chars().next())
                        .collect();
                    if levels.is_empty() {
                        return Err(ConfigError::BadValue {
                            key: key.into(),
                            reason: format!(
                                "{line_info}: priority_levels requires at least one letter"
                            ),
                        });
                    }
                    if !levels.iter().all(char::is_ascii_uppercase) {
                        return Err(ConfigError::BadValue {
                            key: key.into(),
                            reason: format!(
                                "{line_info}: priority_levels must be ASCII uppercase letters"
                            ),
                        });
                    }
                    cfg.priority_levels = levels;
                }
                "tag_inheritance" => cfg.tag_inheritance = parse_bool(key, value)?,
                "agenda_files" => {
                    cfg.agenda_files = value
                        .split(',')
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .map(PathBuf::from)
                        .collect();
                }
                "record_commands" => cfg.record_commands = parse_bool(key, value)?,
                "last_place" => {
                    cfg.last_place = (!value.trim().is_empty()).then(|| value.trim().to_owned());
                }
                "recent_files" => {
                    cfg.recent_files = value
                        .split(',')
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .map(PathBuf::from)
                        .collect();
                }
                "assets_dir" => {
                    cfg.assets_dir =
                        (!value.trim().is_empty()).then(|| PathBuf::from(value.trim()));
                }
                "sync_dir" => {
                    cfg.sync_dir = (!value.trim().is_empty()).then(|| PathBuf::from(value.trim()));
                }
                "wrap" => cfg.wrap = parse_bool(key, value)?,
                "sync_peers" => {
                    cfg.sync_peers = value
                        .split(',')
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                "sync_bind" => {
                    cfg.sync_bind =
                        Some(
                            value
                                .parse::<SocketAddr>()
                                .map_err(|e| ConfigError::BadValue {
                                    key: key.into(),
                                    reason: format!(
                                        "{line_info}: expected `host:port` (e.g. `0.0.0.0:7420`), \
                                 got `{value}` — {e}"
                                    ),
                                })?,
                        );
                }
                "sync_advertise" => {
                    cfg.sync_advertise =
                        Some(value.parse::<IpAddr>().map_err(|e| ConfigError::BadValue {
                            key: key.into(),
                            reason: format!(
                                "{line_info}: expected a bare IP address (the port comes \
                                 from the bound socket), got `{value}` — {e}"
                            ),
                        })?);
                }
                "search_backend" => cfg.search_backend = Some(value.into()),
                "llm_tools" => {
                    cfg.llm_tools = Some(
                        value
                            .split(',')
                            .map(|s| s.trim().to_owned())
                            .filter(|s| !s.is_empty())
                            .collect(),
                    );
                }
                "sniffer_blocklist" => {
                    cfg.sniffer_blocklist = Some(
                        value
                            .split(',')
                            .map(|s| s.trim().to_owned())
                            .filter(|s| !s.is_empty())
                            .collect(),
                    );
                }
                "eval_trust" => {
                    cfg.eval_trust = value
                        .split(',')
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                "llm_provider" => cfg.llm_provider = Some(value.into()),
                "llm_model" => cfg.llm_model = Some(value.into()),
                "llm_key_env" => cfg.llm_key_env = Some(value.into()),
                "llm_endpoint" => cfg.llm_endpoint = Some(value.into()),
                other => return Err(ConfigError::UnknownKey(other.into())),
            }
        }

        // Typed cross-key constraints (yesod/CUE principle from spec I9 and
        // ROADMAP: error at *load* time, not at first use of the feature).
        // A remote BYOK provider needs llm_key_env; keyless providers
        // (echo for tests, ollama on localhost) do not.
        let keyless = matches!(cfg.llm_provider.as_deref(), Some("echo" | "ollama"));
        if cfg.llm_provider.is_some() && !keyless && cfg.llm_key_env.is_none() {
            return Err(ConfigError::BadValue {
                key: "llm_key_env".into(),
                reason: "llm_provider is set but llm_key_env is required for BYOK".into(),
            });
        }

        // An endpoint with nothing to use it is a typo, not a setting.
        if cfg.llm_endpoint.is_some() && cfg.llm_provider.is_none() {
            return Err(ConfigError::BadValue {
                key: "llm_provider".into(),
                reason: "llm_endpoint is set but no llm_provider says what speaks to it".into(),
            });
        }
        // …and naming a wire format says nothing about *where*.
        if cfg.llm_provider.as_deref() == Some("openai-compatible") && cfg.llm_endpoint.is_none() {
            return Err(ConfigError::BadValue {
                key: "llm_endpoint".into(),
                reason: "provider `openai-compatible` needs llm_endpoint = <url of the \
                         /v1/chat/completions route>"
                    .into(),
            });
        }
        if let Some(url) = &cfg.llm_endpoint
            && !(url.starts_with("http://") || url.starts_with("https://"))
        {
            // Plain http stays allowed: it is how every local runner
            // ships, and refusing it would just mean nobody can point
            // this at llama.cpp.
            return Err(ConfigError::BadValue {
                key: "llm_endpoint".into(),
                reason: format!("expected an http:// or https:// URL, got `{url}`"),
            });
        }

        Ok(cfg)
    }
}
