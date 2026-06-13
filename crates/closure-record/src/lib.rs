//! Append-only command journal.
//!
//! When enabled (config `record_commands = true`), every executed
//! command is appended as a level-1 org headline to `journal.org` in
//! the vault. Off by default; the journal is itself plain org, so it
//! is searchable and queryable like any other vault data.

#![forbid(unsafe_code)]

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Journal error.
#[derive(Debug, Error)]
pub enum RecordError {
    /// IO failure appending to the journal file.
    #[error("io: {0}")]
    Io(String),
}

/// Format one journal entry as a level-1 org headline. Newlines in
/// `detail` are flattened to spaces so each entry stays a single
/// headline line.
#[must_use]
pub fn format_entry(unix_secs: u64, kind: &str, detail: &str) -> String {
    let flat: String = detail
        .split(['\n', '\r'])
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    format!("* [{unix_secs}] {kind}: {flat}\n")
}

/// A vault command journal. Cheap to construct; does nothing unless
/// enabled.
#[derive(Debug, Clone)]
pub struct Journal {
    path: PathBuf,
    enabled: bool,
}

impl Journal {
    /// Build a journal targeting `<vault>/journal.org`.
    #[must_use]
    pub fn new(vault: &Path, enabled: bool) -> Self {
        Self {
            path: vault.join("journal.org"),
            enabled,
        }
    }

    /// Whether recording is on.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Read back the journal entries (one per recorded headline), in
    /// order. Empty when the journal does not exist yet.
    ///
    /// # Errors
    ///
    /// [`RecordError::Io`] when the file exists but cannot be read.
    pub fn entries(&self) -> Result<Vec<String>, RecordError> {
        match std::fs::read_to_string(&self.path) {
            Ok(s) => Ok(s
                .lines()
                .filter(|l| l.starts_with("* "))
                .map(str::to_owned)
                .collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(RecordError::Io(e.to_string())),
        }
    }

    /// Entries containing `needle` (case-insensitive).
    ///
    /// # Errors
    ///
    /// Propagates [`Self::entries`] failures.
    pub fn filtered(&self, needle: &str) -> Result<Vec<String>, RecordError> {
        let lower = needle.to_lowercase();
        Ok(self
            .entries()?
            .into_iter()
            .filter(|l| l.to_lowercase().contains(&lower))
            .collect())
    }

    /// Append a command entry. No-op when disabled.
    ///
    /// # Errors
    ///
    /// [`RecordError::Io`] when the append fails.
    pub fn record(&self, unix_secs: u64, kind: &str, detail: &str) -> Result<(), RecordError> {
        if !self.enabled {
            return Ok(());
        }
        let entry = format_entry(unix_secs, kind, detail);
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| RecordError::Io(e.to_string()))?;
        f.write_all(entry.as_bytes())
            .map_err(|e| RecordError::Io(e.to_string()))
    }
}
