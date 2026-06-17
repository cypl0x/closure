//! Sync transports for closure vaults.
//!
//! Transports implement [`Transport`] and ship edits that a CRDT merger
//! consumes. Phase 1 transports: plain file copy and git. Later phases:
//! IPFS / iroh for P2P collaboration.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::Command;

use closure_core::{BlockId, Document};
use closure_crdt::{CrdtError, Replica, VectorClock};
use thiserror::Error;

/// A peer's CRDT sync state: an accumulated [`Replica`] + [`VectorClock`].
///
/// Local edits are folded in via [`Self::record_local`]; a peer's
/// replica is merged in via [`Self::receive`]. Merges are commutative +
/// idempotent (LWW per block field), so any exchange order converges.
/// Transport-agnostic — the loopback and iroh transports both just move
/// the [`Replica`].
#[derive(Debug, Clone, Default)]
pub struct SyncSession {
    replica: Replica,
    clock: VectorClock,
    name: String,
}

impl SyncSession {
    /// New session for replica `name` (the clock's replica id).
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            replica: Replica::default(),
            clock: VectorClock::new(name),
            name: name.to_owned(),
        }
    }

    /// Fold a local snapshot of `doc` into this session. Bumps the
    /// clock and snapshots `doc` *against* the accumulated replica at
    /// the new logical time: only fields that actually changed advance
    /// their timestamp, so an untouched field never outranks a
    /// concurrent edit on another peer (the key to convergence).
    pub fn record_local(&mut self, doc: &Document) {
        self.clock.bump(&self.name);
        let ts = self.clock.logical_time();
        let snap = Replica::snapshot_against(&self.replica, doc, ts);
        self.replica.merge(&snap);
    }

    /// The replica to ship to a peer.
    #[must_use]
    pub const fn outgoing(&self) -> &Replica {
        &self.replica
    }

    /// Merge a peer's replica into this session (LWW per block field).
    pub fn receive(&mut self, other: &Replica) {
        self.replica.merge(other);
    }

    /// Block ids known to this session.
    pub fn block_ids(&self) -> impl Iterator<Item = &BlockId> {
        self.replica.block_ids()
    }

    /// Winning title for `id`.
    #[must_use]
    pub fn title_of(&self, id: &BlockId) -> Option<&str> {
        self.replica.title_of(id)
    }

    /// Winning body for `id`.
    #[must_use]
    pub fn body_of(&self, id: &BlockId) -> Option<&str> {
        self.replica.body_of(id)
    }

    /// Reconcile `doc` to this session's winning registers through
    /// kernel commands (I8). Returns the number of edits applied.
    ///
    /// # Errors
    ///
    /// [`CrdtError`] when a kernel command refuses an edit.
    pub fn apply_to(&self, doc: &mut Document) -> Result<usize, CrdtError> {
        self.replica.apply_to(doc)
    }
}

/// In-memory loopback link between two [`SyncSession`] peers.
///
/// A shared pair of one-shot mailboxes carrying a [`Replica`] each way
/// — the hermetic stand-in for a network link, which the iroh transport
/// mirrors over the wire instead of in process.
#[derive(Debug, Default)]
pub struct LoopbackPair {
    a_to_b: Option<Replica>,
    b_to_a: Option<Replica>,
}

impl LoopbackPair {
    /// New empty link.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Peer A publishes its replica toward B.
    pub fn push_a(&mut self, a: &SyncSession) {
        self.a_to_b = Some(a.outgoing().clone());
    }
    /// Peer B publishes its replica toward A.
    pub fn push_b(&mut self, b: &SyncSession) {
        self.b_to_a = Some(b.outgoing().clone());
    }
    /// Peer A consumes + merges whatever B published (no-op if empty).
    pub fn pull_a(&mut self, a: &mut SyncSession) {
        if let Some(r) = self.b_to_a.take() {
            a.receive(&r);
        }
    }
    /// Peer B consumes + merges whatever A published (no-op if empty).
    pub fn pull_b(&mut self, b: &mut SyncSession) {
        if let Some(r) = self.a_to_b.take() {
            b.receive(&r);
        }
    }

    /// One full sync round: both peers publish, then both consume +
    /// merge. After this, the two sessions have converged.
    pub fn sync_round(&mut self, a: &mut SyncSession, b: &mut SyncSession) {
        self.push_a(a);
        self.push_b(b);
        self.pull_a(a);
        self.pull_b(b);
    }
}

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

/// P2P transport via external binary (iroh or IPFS CLI).
///
/// Decision (recorded per ROADMAP): use external binary (iroh/ipfs) so the core stays hermetic,
/// no heavy deps, and the presence is gated (test skips or errors if binary not in PATH, like git tests).
/// The binary is called for push/pull; for the stub, we check presence and error "not implemented / binary not found".
#[derive(Debug, Clone)]
pub struct IrohTransport {
    /// Vault directory.
    pub dir: PathBuf,
}

impl IrohTransport {
    /// Build an Iroh (or IPFS) P2P transport rooted at `dir` (external binary gated).
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    #[allow(clippy::unused_self)]
    fn binary_present(&self, name: &str) -> bool {
        Command::new(name).arg("--version").output().is_ok()
            || Command::new("which")
                .arg(name)
                .output()
                .is_ok_and(|o| o.status.success())
    }
}

impl Transport for IrohTransport {
    fn push(&mut self) -> Result<(), SyncError> {
        if !self.binary_present("iroh") && !self.binary_present("ipfs") {
            return Err(SyncError::Transport("binary not found: iroh or ipfs (decision: external for P2P, gated on presence to keep core hermetic)".to_owned()));
        }
        // Stub: the real would shell `iroh send ...` or `ipfs add ...` etc with the vault state.
        Err(SyncError::Transport("iroh/ipfs push not implemented in stub (binary present; real impl would do the P2P transfer)".to_owned()))
    }

    fn pull(&mut self) -> Result<(), SyncError> {
        if !self.binary_present("iroh") && !self.binary_present("ipfs") {
            return Err(SyncError::Transport("binary not found: iroh or ipfs (decision: external for P2P, gated on presence to keep core hermetic)".to_owned()));
        }
        Err(SyncError::Transport("iroh/ipfs pull not implemented in stub (binary present; real impl would fetch and apply deltas)".to_owned()))
    }
}
