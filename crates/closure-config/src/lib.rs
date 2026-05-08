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
    /// with the default for any field the user didn't specify.
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
                                reason: format!("unknown input_mode `{other}`"),
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
                            reason: "priority_levels requires at least one letter".into(),
                        });
                    }
                    if !levels.iter().all(char::is_ascii_uppercase) {
                        return Err(ConfigError::BadValue {
                            key: key.into(),
                            reason: "priority_levels must be ASCII uppercase letters".into(),
                        });
                    }
                    cfg.priority_levels = levels;
                }
                "tag_inheritance" => {
                    cfg.tag_inheritance = match value {
                        "true" | "yes" | "1" => true,
                        "false" | "no" | "0" => false,
                        other => {
                            return Err(ConfigError::BadValue {
                                key: key.into(),
                                reason: format!("expected boolean, got `{other}`"),
                            });
                        }
                    };
                }
                "agenda_files" => {
                    cfg.agenda_files = value
                        .split(',')
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .map(PathBuf::from)
                        .collect();
                }
                other => return Err(ConfigError::UnknownKey(other.into())),
            }
        }
        Ok(cfg)
    }
}
