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
}

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
