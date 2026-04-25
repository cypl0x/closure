//! Sync transports for closure vaults.
//!
//! Transports implement [`Transport`] and ship edits that a CRDT merger
//! consumes. Phase 1 transports: plain file copy and git. Later phases:
//! IPFS / iroh for P2P collaboration.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::Command;

use thiserror::Error;

/// Sync transport trait.
pub trait Transport {
    /// Send the vault state to the remote.
    fn push(&mut self) -> Result<(), SyncError>;
    /// Receive the vault state from the remote.
    fn pull(&mut self) -> Result<(), SyncError>;
}

/// Sync error.
#[derive(Debug, Error)]
pub enum SyncError {
    /// Transport-specific error.
    #[error("transport: {0}")]
    Transport(String),
    /// IO error.
    #[error("io: {0}")]
    Io(String),
}

/// No-op transport. Useful for tests and single-host setups.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopTransport;

impl Transport for NoopTransport {
    fn push(&mut self) -> Result<(), SyncError> {
        Ok(())
    }
    fn pull(&mut self) -> Result<(), SyncError> {
        Ok(())
    }
}

/// Git-based transport. The vault directory must already be a git
/// working tree with a configured remote. `push` runs
/// `git add -A && git commit -m <msg> && git push`. `pull` runs
/// `git pull --rebase`.
#[derive(Debug, Clone)]
pub struct GitTransport {
    /// Vault directory.
    pub dir: PathBuf,
    /// Branch to push (defaults to current `HEAD`).
    pub branch: Option<String>,
    /// Commit message used by [`Transport::push`].
    pub commit_message: String,
}

impl GitTransport {
    /// Build a git transport rooted at `dir`.
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            branch: None,
            commit_message: "closure: sync".into(),
        }
    }

    fn run(&self, args: &[&str]) -> Result<(), SyncError> {
        let status = Command::new("git")
            .current_dir(&self.dir)
            .args(args)
            .status()
            .map_err(|e| SyncError::Io(e.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            Err(SyncError::Transport(format!(
                "git {:?} exited {}",
                args,
                status.code().unwrap_or(-1)
            )))
        }
    }
}

impl Transport for GitTransport {
    fn push(&mut self) -> Result<(), SyncError> {
        self.run(&["add", "-A"])?;
        // Allow empty commits to fail silently — nothing to sync is fine.
        let _ = self.run(&["commit", "-m", &self.commit_message]);
        let branch = self.branch.clone().unwrap_or_else(|| "HEAD".into());
        self.run(&["push", "origin", &branch])
    }

    fn pull(&mut self) -> Result<(), SyncError> {
        self.run(&["pull", "--rebase"])
    }
}
