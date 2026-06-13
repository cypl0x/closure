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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_vault: None,
            input_mode: InputMode::Doom,
            theme: "default".into(),
            todo_keywords: vec!["TODO".into(), "DONE".into()],
            priority_levels: vec!['A', 'B', 'C'],
            tag_inheritance: true,
            agenda_files: Vec::new(),
            llm_provider: None,
            llm_model: None,
            llm_key_env: None,
            record_commands: false,
            search_backend: None,
            llm_tools: None,
            sniffer_blocklist: None,
        }
    }
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
                            reason: format!("{line_info}: priority_levels requires at least one letter"),
                        });
                    }
                    if !levels.iter().all(char::is_ascii_uppercase) {
                        return Err(ConfigError::BadValue {
                            key: key.into(),
                            reason: format!("{line_info}: priority_levels must be ASCII uppercase letters"),
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
                "llm_provider" => cfg.llm_provider = Some(value.into()),
                "llm_model" => cfg.llm_model = Some(value.into()),
                "llm_key_env" => cfg.llm_key_env = Some(value.into()),
                other => return Err(ConfigError::UnknownKey(other.into())),
            }
        }

        // Typed cross-key constraints (yesod/CUE principle from spec I9 and
        // ROADMAP: error at *load* time, not at first use of the feature).
        // Example: llm_provider set ⇒ llm_key_env must be present (BYOK).
        if cfg.llm_provider.is_some() && cfg.llm_key_env.is_none() {
            return Err(ConfigError::BadValue {
                key: "llm_key_env".into(),
                reason: "llm_provider is set but llm_key_env is required for BYOK".into(),
            });
        }

        Ok(cfg)
    }
}
